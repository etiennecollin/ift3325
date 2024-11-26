use std::process::exit;

use crate::{
    frame::{Frame, FrameType},
    window::{SafeCond, SafeWindow, Window},
};
use log::{debug, error, info, warn};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::mpsc,
    task::JoinHandle,
    time,
};

/// Receives frames from the server and handles them.
/// The function reads from the stream and constructs frames from the received bytes.
/// It then calls `handle_frame()` to process the frames.
/// If a connection end frame is received, the function returns.
///
/// # Arguments
/// * `stream` - The read half of the TCP stream.
/// * `window` - The window to manage the frames.
/// * `condition` - The condition to signal tasks that space is available in the window.
/// * `writer_tx` - The sender to send frames to the writer.
/// * `assembler_tx` - The sender to send frames to the assembler.
pub fn reader(
    mut stream: OwnedReadHalf,
    window: SafeWindow,
    condition: SafeCond,
    writer_tx: Option<mpsc::Sender<Vec<u8>>>,
    assembler_tx: Option<mpsc::Sender<Vec<u8>>>,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        let mut frame_buf = Vec::with_capacity(Frame::MAX_SIZE);
        let mut next_info_frame_num: u8 = 0;

        loop {
            if window
                .lock()
                .expect("Failed to lock window")
                .sent_disconnect_request
            {
                return Ok("Reader task ended, sent disconnect request");
            }

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
                    // Create frame from buffer
                    let frame = match Frame::from_bytes(&frame_buf) {
                        Ok(frame) => frame,
                        Err(e) => {
                            warn!("Received bad frame: {:X?}", e);
                            frame_buf.clear();
                            continue;
                        }
                    };

                    // Handle the frame and check if the connection should be terminated
                    if handle_reception(
                        frame,
                        &window,
                        &condition,
                        writer_tx.as_ref(),
                        assembler_tx.as_ref(),
                        &mut next_info_frame_num,
                    )
                    .await
                    {
                        return Ok("Connection ended by server");
                    }

                    // Reset buffer
                    frame_buf.clear();
                } else {
                    // Add byte to frame buffer
                    frame_buf.push(*byte);
                }
            }
        }
    })
}

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
    expected_info_num: &mut u8,
) -> bool {
    // Check if the frame is an acknowledgment or a rejection
    let mut end_connection = false;
    let writer_tx = writer_tx.expect("No sender provided to handle frame rejection");

    match frame.frame_type.into() {
        // If it is an acknowledgment, pop the acknowledged frames from the window
        FrameType::ReceiveReady => {
            {
                let mut window = safe_window.lock().expect("Failed to lock window");

                // Ignore the frame if the connection is not established
                if !window.is_connected {
                    return false;
                }

                // Pop the acknowledged frames from the window
                window.pop_until(
                    (frame.num + (Window::MAX_FRAME_NUM - 1)) % Window::MAX_FRAME_NUM,
                    true,
                    condition,
                );
            }

            info!("Received RR {}", frame.num);
        }

        // If it is a connection end frame, return true to stop the connection
        FrameType::ConnexionEnd => {
            let is_waiting_disconnect: bool;
            {
                let mut window = safe_window.lock().expect("Failed to lock window");
                window.is_connected = false;
                is_waiting_disconnect = window.sent_disconnect_request;
            }
            end_connection = true;
            info!("Received connection end frame");

            // If the window is empty, then we are not the ones initiating the connection
            // and we should send an acknowledgment. Else, we should pop the connection request
            // frame from our window as it was acknowledged.
            //
            // This works as the connection request is always the last frame to be sent.
            if is_waiting_disconnect {
                let mut window = safe_window.lock().expect("Failed to lock window");
                window.pop_front(condition);
            } else {
                // Send an acknowledgment for the connection request frame
                let ack_frame = Frame::new(FrameType::ConnexionEnd, 0, Vec::new()).to_bytes();
                writer_tx
                    .send(ack_frame)
                    .await
                    .expect("Failed to send acknowledgment frame");

                info!("Sent connection end acknowledgment");
            }
        }

        FrameType::ConnectionStart => {
            // Initialize the window
            let is_window_empty: bool;
            {
                let mut window = safe_window.lock().expect("Failed to lock window");
                window.is_connected = true;
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
                let ack_frame =
                    Frame::new(FrameType::ConnectionStart, frame.num, Vec::new()).to_bytes();
                writer_tx
                    .send(ack_frame)
                    .await
                    .expect("Failed to send acknowledgment frame");

                info!("Received connection request and sent acknowledgment");
            } else {
                let mut window = safe_window.lock().expect("Failed to lock window");
                window.pop_front(condition);
                info!("Received acknowledgment for connection request frame");
            }
        }

        FrameType::Reject => {
            let frames: Vec<(u8, Vec<u8>)>;
            {
                let mut window = safe_window.lock().expect("Failed to lock window");

                // Ignore the frame if the connection is not established
                if !window.is_connected {
                    return false;
                }
                let srej = window.srej;

                // Pop the implicitly acknowledged frames from the window
                window.pop_until(
                    (frame.num + (Window::MAX_FRAME_NUM - 1)) % Window::MAX_FRAME_NUM,
                    true,
                    condition,
                );

                // Select the frames to be resent
                if srej {
                    // Select the frame to resend
                    let selected_frame = window
                        .frames
                        .iter()
                        .find(|f| f.num == frame.num)
                        .expect("Frame not found in window, this should never happen")
                        .to_bytes();

                    frames = vec![(frame.num, selected_frame)];
                    warn!("Received SREJ for frame {}", frame.num);
                } else {
                    // Select all frames to be resent
                    frames = window
                        .frames
                        .iter()
                        .map(|frame| (frame.num, frame.to_bytes()))
                        .collect();
                    warn!("Received REJ for frame {}", frame.num);
                }
            }

            // Resend the frames
            info!(
                "Resending reject frames {:?}",
                frames.iter().map(|(num, _)| num).collect::<Vec<&u8>>()
            );
            for (_, frame) in frames {
                writer_tx.send(frame).await.expect("Failed to resend frame");
            }
        }

        FrameType::Information => {
            // Ignore the frame if the connection is not established
            if !safe_window
                .lock()
                .expect("Failed to lock window")
                .is_connected
            {
                return false;
            }

            let assembler_tx = assembler_tx.expect("No sender provided to handle frame reassembly");

            // Check if a frame was dropped
            if *expected_info_num != frame.num {
                warn!(
                    "Dropped frame detected - Received: {}, Expected: {}",
                    frame.num, expected_info_num
                );

                let rej_frame = Frame::new(FrameType::Reject, *expected_info_num, Vec::new());
                let rej_frame_bytes = rej_frame.to_bytes();

                // Create a scope for the mutex lock
                {
                    let mut window = safe_window.lock().expect("Failed to lock window");
                    // If window is full, ignore frame
                    if window.is_full() || window.contains(*expected_info_num) {
                        return false;
                    }
                    window
                        .push(rej_frame)
                        .expect("Failed to push frame to window");
                }

                // Send a reject frame
                writer_tx
                    .send(rej_frame_bytes)
                    .await
                    .expect("Failed to send reject frame");

                // Run a timer to resend the frame if it is not received
                create_frame_timer(safe_window.clone(), *expected_info_num, writer_tx.clone())
                    .await;

                info!("Sent reject for frame {}", expected_info_num);
                return false;
            }

            // Pop old reject frames from window
            {
                let mut window = safe_window.lock().expect("Failed to lock window");
                window.pop_until(frame.num, true, condition);
            }

            // Update the expected next information frame number
            *expected_info_num = (*expected_info_num + 1) % Window::MAX_FRAME_NUM;

            // Send the frame data to the assembler
            assembler_tx
                .send(frame.data)
                .await
                .expect("Failed to send frame data to assembler");

            // Send an acknowledgment for the information frame
            let ack_frame =
                Frame::new(FrameType::ReceiveReady, *expected_info_num, Vec::new()).to_bytes();
            writer_tx
                .send(ack_frame)
                .await
                .expect("Failed to send acknowledgment frame");

            info!("Received I {}", frame.num,);
            info!("Sent RR {}", *expected_info_num)
        }

        FrameType::P => {
            // Send an acknowledgment for the P frame
            let ack_frame =
                Frame::new(FrameType::ReceiveReady, *expected_info_num, Vec::new()).to_bytes();
            writer_tx
                .send(ack_frame)
                .await
                .expect("Failed to send acknowledgment frame");

            info!("Received P frame");
            info!("Sent RR {}", *expected_info_num)
        }

        FrameType::Unknown => {}
    }

    end_connection
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
                Err(_) => return Err("Failed to flush stream"),
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
            let frame_bytes;

            // Lock the window for as short a time as possible
            {
                let window = safe_window.lock().expect("Failed to lock window");

                // If the connection is ending, stop the task
                if window.sent_disconnect_request {
                    return;
                }

                // Check if the frame is still in the window and get its bytes
                let frame = match window.frames.iter().find(|frame| frame.num == num) {
                    Some(frame) => frame,
                    None => {
                        // If the frame is not in the window, it has been acknowledged and the task can stop
                        debug!("Frame {} was acked", num);
                        debug!(
                            "Window: {:?}",
                            window.frames.iter().map(|f| f.num).collect::<Vec<u8>>()
                        );
                        return;
                    }
                };

                frame_bytes = frame.to_bytes();
            }

            info!("Timeout expired, resending frame {}", num);
            tx.send(frame_bytes).await.expect("Failed to send frame");
        }
    });
}

/// Sends a connection request frame to the server.
///
/// This function sends a connection request frame to the server and waits for the acknowledgment.
/// It also handles resending the frame in case of a timeout.
///
/// The function returns when the connection is established.
///
/// # Arguments
/// * `window` - The window to manage the frames.
/// * `connection_start` - A boolean indicating if the connection is starting or ending.
/// * `srej` - A boolean indicating if the connection uses REJ or SREJ.
/// * `tx` - The sender channel to send the frame to the writer task.
/// * `condition` - The condition variable to signal the window state.
pub async fn connection_request(
    window: &SafeWindow,
    connection_start: bool,
    srej: Option<u8>,
    tx: mpsc::Sender<Vec<u8>>,
    condition: &SafeCond,
) {
    let frame_type = match connection_start {
        true => FrameType::ConnectionStart,
        false => FrameType::ConnexionEnd,
    };

    if connection_start && srej.is_none() {
        error!("SREJ value is required when starting a connection");
        exit(1);
    }

    let srej = srej.unwrap_or(0);
    let request_frame = Frame::new(frame_type, srej, Vec::new());
    let request_frame_bytes = request_frame.to_bytes();

    {
        let mut window = window.lock().expect("Failed to lock window");

        window.sent_disconnect_request = !connection_start;

        // Only if we start a new connection
        if connection_start {
            window.srej = srej == 1;
            window
                .push(request_frame)
                .expect("Failed to push frame to window");
        }
    }

    // Send the connection request frame
    tx.send(request_frame_bytes)
        .await
        .expect("Failed to send frame to writer task");
    info!("Sent connection request frame");

    // Wait for the request to be acknowledged
    if connection_start {
        // Run a timer to resend the request if it is not acknowledged
        let tx_clone = tx.clone();
        let window_clone = window.clone();
        create_frame_timer(window_clone, srej, tx_clone).await;

        // Wait for the connection to be established
        {
            let mut window = window.lock().expect("Failed to lock window");
            while !window.is_empty() {
                window = condition
                    .wait(window)
                    .expect("Failed to wait for condition");
            }
        }
    }
    window.lock().expect("Failed to lock window").is_connected = connection_start;
}
