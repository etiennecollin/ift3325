use crate::{
    frame::{Frame, FrameError, FrameType},
    window::{SafeCond, SafeWindow, Window},
};
use log::{error, info, warn};
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
/// * `writer_tx` - The sender to send the frames in case of a rejection. If set to `None`, then
///   the function will panic if the frame is a rejection.
/// * `assembler_tx` - The sender to send the frame to the assembler. The assembler will
///   reconstruct the file from the frames.
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
    safe_window: &SafeWindow,
    condition: &SafeCond,
    writer_tx: Option<&mpsc::Sender<Vec<u8>>>,
    assembler_tx: Option<&mpsc::Sender<Vec<u8>>>,
) -> bool {
    // Check if the frame is an acknowledgment or a rejection
    let mut end_connection = false;
    match frame.frame_type.into() {
        // If it is an acknowledgment, pop the acknowledged frames from the window
        FrameType::ReceiveReady => {
            let mut window = safe_window.lock().expect("Failed to lock window");

            // Ignore the frame if the connection is not established
            if !window.connected {
                return false;
            }

            let size = window.get_size() as u8;

            // Pop the acknowledged frames from the window
            window.pop_until((frame.num - 1) % size, true);

            // Notify the send task that space was created in the window
            condition.notify_one();

            info!("Received acknowledgement up to frame {}", frame.num);
        }

        // If it is a rejection, pop the implicitly acknowledged frames from the window
        FrameType::Reject => {
            let writer_tx = writer_tx.expect("No sender provided to handle frame rejection");

            let frames: Vec<Vec<u8>>;
            let srej: bool;
            {
                let mut window = safe_window.lock().expect("Failed to lock window");

                // Ignore the frame if the connection is not established
                if !window.connected {
                    return false;
                }
                srej = window.srej;

                // Pop the implicitly acknowledged frames from the window
                window.pop_until(frame.num, false);

                // Select the frames to be resent
                if srej {
                    assert_eq!(window.frames.front().unwrap().num, frame.num);
                    // Select the frame to resend
                    frames = vec![window
                        .frames
                        .front()
                        .expect("The window is empty, this should never happen")
                        .to_bytes()];

                    info!("Received SREJ for frame {}", frame.num);
                    // // Select the frame to resend
                    // frames = vec![window
                    //     .frames
                    //     .iter()
                    //     .find(|f| f.num == frame.num)
                    //     .expect("Frame not found in window, this should never happen")
                    //     .to_bytes()];
                } else {
                    // Select all frames to be resent
                    frames = window.frames.iter().map(|frame| frame.to_bytes()).collect();
                    info!("Received REJ for frame {}", frame.num);
                }
            }

            // Resend the frames
            for frame in frames {
                writer_tx.send(frame).await.expect("Failed to resend frame");
            }

            // Notify the send task that space was created in the window
            condition.notify_one();
        }
        // If it is a connection end frame, return true to stop the connection
        FrameType::ConnexionEnd => {
            let mut window = safe_window.lock().expect("Failed to lock window");
            window.connected = false;
            end_connection = true;

            info!("Received connection end frame");
        }
        FrameType::Information => {
            // Ignore the frame if the connection is not established
            let is_connected = safe_window.lock().expect("Failed to lock window").connected;
            if !is_connected {
                return false;
            }

            let assembler_tx = assembler_tx.expect("No sender provided to handle frame reassembly");
            let writer_tx = writer_tx.expect("No sender provided to handle frame rejection");

            // Send the frame data to the assembler
            assembler_tx
                .send(frame.data)
                .await
                .expect("Failed to send frame data to assembler");

            // Send an acknowledgment for the information frame
            let ack_frame =
                Frame::new(FrameType::ReceiveReady, frame.num + 1, Vec::new()).to_bytes();
            writer_tx
                .send(ack_frame)
                .await
                .expect("Failed to send acknowledgment frame");

            info!(
                "Received information frame {} and sent acknowledgment",
                frame.num,
            );
        }
        FrameType::ConnexionRequest => {
            let is_window_empty: bool;
            {
                let mut window = safe_window.lock().expect("Failed to lock window");
                window.connected = true;
                is_window_empty = window.is_empty();

                // If the window is empty, then we are not the ones initiating the connection and
                // must set the SREJ flag based on the frame number. Else, we should check if the
                // SREJ flag is consistent with the frame number received.
                let frame_srej = match frame.num {
                    0 => false,
                    1 => true,
                    _ => panic!("Invalid frame number for connection request"),
                };
                if is_window_empty {
                    window.srej = frame_srej;
                } else {
                    assert_eq!(frame_srej, window.srej);
                }
            }

            // If the window is empty, then we are not the ones initiating the connection
            // and we should send an acknowledgment. Else, we should pop the connection request
            // frame from our window as it was acknowledged.
            //
            // This works as the connection request is always the first frame to be sent.
            if is_window_empty {
                // Send an acknowledgment for the connection request frame
                let writer_tx = writer_tx.expect("No sender provided to handle frame rejection");
                let ack_frame =
                    Frame::new(FrameType::ConnexionRequest, frame.num, Vec::new()).to_bytes();
                writer_tx
                    .send(ack_frame)
                    .await
                    .expect("Failed to send acknowledgment frame");

                info!("Received connection request frame and sent acknowledgment");
            } else {
                let mut window = safe_window.lock().expect("Failed to lock window");
                window.pop_front();
                condition.notify_one();
                info!("Received acknowledgment for connection request frame");
            }
        }
        FrameType::P => {
            todo!("What to do with P frames?");
        }
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
    writer_tx: Option<mpsc::Sender<Vec<u8>>>,
    assembler_tx: Option<mpsc::Sender<Vec<u8>>>,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        let mut frame_buf = Vec::with_capacity(Frame::MAX_SIZE);
        let mut in_frame = false;
        loop {
            // Read from the stream into the buffer
            let mut buf = [0; Frame::MAX_SIZE];
            let read_length = match stream.read(&mut buf).await {
                Ok(read_length) => read_length,
                Err(e) => {
                    error!("Failed to read from stream: {}", e);
                    return Err("Connection ended with an error");
                }
            };

            // Iterate over the received bytes
            for byte in buf[..read_length].iter() {
                // Check if the byte starts or ends a frame
                if *byte == Frame::BOUNDARY_FLAG {
                    frame_buf.push(*byte);

                    // If the frame is complete, handle it
                    if in_frame {
                        // Create frame from buffer
                        let frame = match Frame::from_bytes(&frame_buf) {
                            Ok(frame) => frame,
                            Err(FrameError::InvalidFCS(num)) => {
                                warn!("Received frame with invalid FCS");
                                // Send a rejection frame
                                let reject_frame =
                                    Frame::new(FrameType::Reject, num, Vec::new()).to_bytes();
                                let writer_tx = writer_tx
                                    .as_ref()
                                    .expect("No sender provided to handle frame rejection");
                                writer_tx
                                    .send(reject_frame)
                                    .await
                                    .expect("Failed to send rejection frame");
                                info!("Sent rejection frame for frame {}", num);
                                break;
                            }
                            Err(e) => {
                                error!("Failed to create frame from buffer: {:?}", e);
                                break;
                            }
                        };

                        // Handle the frame and check if the connection should be terminated
                        if handle_reception(
                            frame,
                            &window,
                            &pop_condition,
                            writer_tx.as_ref(),
                            assembler_tx.as_ref(),
                        )
                        .await
                        {
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
        time::sleep(time::Duration::from_secs(Window::FRAME_TIMEOUT)).await;
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
