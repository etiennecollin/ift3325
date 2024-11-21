use crate::{
    frame::{Frame, FrameType},
    window::{SafeCond, SafeWindow, Window},
};
use log::{error, info};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::mpsc,
    task::JoinHandle,
    time,
};

/// Handles the received frames from the server.
///
/// If the type of frame is an acknowledgment, the function pops the
/// acknowledged frames from the window. If the type is a rejection,
/// it pops the implicitly acknowledged frames from the window and
/// signals other tasks that space is available in the window.
///
/// # Arguments
/// * `frame` - The frame received from the server.
/// * `window` - The window to manage the frames.
/// * `condition` - The condition variable to signaling that space was created in the window.
/// * `tx` - The sender to send the frames in case of a rejection. If set to `None`, then
///         the function will panic if the frame is a rejection.
///
/// # Returns
/// If the function returns `true`, the connection should be terminated.
///
/// # Panics
/// The functino will panic if:
/// - The lock of the window fails
/// - The `tx` is not provided and the frame is a rejection
/// - The sender fails to send the frames
pub async fn handle_reception(
    frame: Frame,
    window: &SafeWindow,
    condition: &SafeCond,
    tx: Option<&mpsc::Sender<Vec<u8>>>,
) -> bool {
    // Check if the frame is an acknowledgment or a rejection
    let mut end_connection = false;
    match frame.frame_type.into() {
        // If it is an acknowledgment, pop the acknowledged frames from the window
        FrameType::ReceiveReady => {
            let mut window = window.lock().expect("Failed to lock window");
            // Pop the acknowledged frames from the window
            window.pop_until((frame.num - 1) % Window::SIZE as u8);

            // Notify the send task that space was created in the window
            condition.notify_one();

            info!("Received acknowledgement up to frame {}", frame.num);
        }
        // If it is a rejection, pop the implicitly acknowledged frames from the window
        FrameType::Reject => {
            let tx = tx.expect("No sender provided to handle frame");

            let frames: Vec<Vec<u8>>;
            {
                let mut window = window.lock().expect("Failed to lock window");
                // Pop the implicitly acknowledged frames from the window
                window.pop_until(frame.num);
                frames = window.frames.iter().map(|frame| frame.to_bytes()).collect()
            }

            // Resend the frames in the window
            for frame in frames {
                tx.send(frame).await.expect("Failed to resend frame");
            }

            // Notify the send task that space was created in the window
            condition.notify_one();

            info!("Received reject for frame {}", frame.num);
        }
        // If it is a connection end frame, return true to stop the connection
        FrameType::ConnexionEnd => {
            end_connection = true;
        }
        _ => {}
    }

    end_connection
}

/// Receives frames from the server and handles them.
/// The function reads from the stream and constructs frames from the received bytes.
/// It then calls `handle_frame()` to process the frames.
/// If a connection end frame is received, the function returns.
///
/// # Arguments
/// * `stream` - The read half of the TCP stream.
/// * `window` - The window to manage the frames.
/// * `pop_condition` - The condition to signal the sen task that space is available in the window.
pub fn reader(
    mut stream: OwnedReadHalf,
    window: SafeWindow,
    pop_condition: SafeCond,
    tx: Option<mpsc::Sender<Vec<u8>>>,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        let mut frame_buf = Vec::with_capacity(Frame::MAX_SIZE);
        let mut in_frame = false;
        loop {
            let mut buf = [0; Frame::MAX_SIZE];
            let read_length = match stream.read(&mut buf).await {
                Ok(read_length) => read_length,
                Err(e) => {
                    error!("Failed to read from stream: {}", e);
                    return Err("Connection ended with an error");
                }
            };

            // TODO: Move this to a separate function in utils
            for byte in buf[..read_length].iter() {
                if *byte == Frame::BOUNDARY_FLAG {
                    frame_buf.push(*byte);

                    // If the frame is complete, handle it
                    if in_frame {
                        // Create frame from buffer
                        let frame = match Frame::from_bytes(&frame_buf) {
                            Ok(frame) => frame,
                            Err(e) => {
                                // TODO: Do we return or simply ignore errors here as there will be a retransmission?
                                error!("Failed to create frame from buffer: {:?}", e);
                                return Err("Connection ended with an error");
                            }
                        };

                        // Handle the frame and check if the connection should be terminated
                        if handle_reception(frame, &window, &pop_condition, tx.as_ref()).await {
                            return Ok("Connection ended by server");
                        }

                        // Reset buffer
                        frame_buf.clear();
                    }

                    in_frame = !in_frame;
                } else if in_frame {
                    // Add byte to frame buffer
                    frame_buf.push(*byte);
                }
            }
        }
    })
}

/// Sends a frame to the server.
///
/// The function writes the frame bytes to the stream and flushes the stream.
/// If an error occurs while sending the frame, the function returns an error.
pub fn writer(
    mut stream: OwnedWriteHalf,
    mut rx: mpsc::Receiver<Vec<u8>>,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        // Receive frames until all tx are dropped
        while let Some(frame) = rx.recv().await {
            // Send the file contents to the server
            match stream.write_all(&frame).await {
                Ok(it) => it,
                Err(_) => return Err("Failed to write to stream"),
            };
            // Flush the stream to ensure the data is sent immediately
            match stream.flush().await {
                Ok(it) => it,
                Err(_) => return Err("Failet to flush stream"),
            };
        }

        // Close the connection
        if stream.shutdown().await.is_err() {
            return Err("Failed to close connection");
        };
        Ok("Closed writer")
    })
}

/// Flattens the result of a join handle.
/// The function awaits the result of the join handle and returns the inner result or error.
pub async fn flatten<T>(handle: JoinHandle<Result<T, &'static str>>) -> Result<T, &'static str> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(_) => Err("Handling failed"),
    }
}

/// Creates a frame timer task.
///
/// The task keeps checking if a frame was acknowledged and sends it if it was not.
/// If the frame is still in the window, the task sends the frame to the sender.
/// If the frame is not in the window, the task stops.
///
/// # Arguments
/// * `safe_window` - The window to check for the frame.
/// * `num` - The number of the frame to check in the window.
/// * `tx` - The sender to send the frame if it was not acknowledged.
pub async fn create_frame_timer(safe_window: SafeWindow, num: u8, tx: mpsc::Sender<Vec<u8>>) {
    tokio::spawn(async move {
        let mut interval = time::interval(time::Duration::from_secs(Window::FRAME_TIMEOUT));
        loop {
            interval.tick().await;
            let frame;

            // Lock the window for as short a time as possible
            {
                let window = safe_window.lock().expect("Failed to lock window");

                // Check if the frame is still in the window and get its bytes
                frame = window
                    .frames
                    .iter()
                    .find(|frame| frame.num == num)
                    .map(|frame| frame.to_bytes());
            }

            // Send the frame if it is still in the window
            // If the frame is not in the window, it has been acknowledged and the task can stop
            if let Some(frame) = frame {
                tx.send(frame).await.unwrap();
            } else {
                break;
            }
        }
    });
}
