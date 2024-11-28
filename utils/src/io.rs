//! Contains the functions to manage the IO tasks.

use crate::{
    frame::{Frame, FrameType},
    frame_handlers::*,
    window::{SafeCond, SafeWindow, Window},
};
use log::{debug, error, info, warn};
use std::process::exit;
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
/// - `stream` - The read half of the TCP stream.
/// - `window` - The window to manage the frames.
/// - `condition` - The condition to signal tasks that space is available in the window.
/// - `writer_tx` - The sender to send frames to the writer.
/// - `assembler_tx` - The sender to send frames to the assembler.
///
/// # Panics
/// The function will panic if:
/// - The lock of the window fails
pub fn reader(
    mut stream: OwnedReadHalf,
    window: SafeWindow,
    condition: SafeCond,
    writer_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    assembler_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        let mut frame_buf = Vec::with_capacity(Frame::MAX_SIZE);
        let mut next_info_frame_num: u8 = 0;

        loop {
            debug!("Reader locking window");
            if window
                .lock()
                .expect("Failed to lock window")
                .sent_disconnect_request
            {
                return Ok("Reader task ended, sent disconnect request");
            }

            debug!("Reader waiting for data");
            // Read from the stream into the buffer
            let mut buf = [0; Frame::MAX_SIZE];
            let read_length = match stream.read(&mut buf).await {
                Ok(read_length) => read_length,
                Err(e) => {
                    error!("Failed to read from stream: {}", e);
                    return Err("Connection ended with an error");
                }
            };
            debug!("Reader received {} bytes", read_length);

            // Iterate over the received bytes
            for byte in buf[..read_length].iter() {
                // Check if the byte starts or ends a frame
                if *byte == Frame::BOUNDARY_FLAG {
                    if frame_buf.is_empty() {
                        frame_buf.clear();
                        continue;
                    }

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
                        window.clone(),
                        condition.clone(),
                        writer_tx.as_ref(),
                        assembler_tx.as_ref(),
                        &mut next_info_frame_num,
                    ) {
                        return Ok("Connection ended by server");
                    }
                    debug!(
                        "Window after handling: {:?}",
                        window
                            .lock()
                            .expect("Failed to lock window")
                            .frames
                            .iter()
                            .map(|(frame, handle)| (frame.num, handle))
                            .collect::<Vec<(u8, &JoinHandle<()>)>>()
                    );

                    debug!("Reader finished handling frame");

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
/// - `frame` - The frame received from the server.
/// - `safe_window` - The window to manage the frames.
/// - `condition` - The condition variable to signaling that space was created in the window.
/// - `writer_tx` - The sender to send the frames in case of a rejection. If set to `None`, then
///   the function will panic if the frame is a rejection.
/// - `assembler_tx` - The sender to send the frame to the assembler. The assembler will
///   reconstruct the file from the frames.
/// - `expected_info_num` - The expected number of the next information frame.
///
/// # Returns
/// If the function returns `true`, the connection should be terminated.
///
/// # Panics
/// The functino will panic if:
/// - The lock of the window fails
/// - The `tx` is not provided and the frame is a rejection
/// - The sender fails to send the frames
pub fn handle_reception(
    frame: Frame,
    safe_window: SafeWindow,
    condition: SafeCond,
    writer_tx: Option<&mpsc::UnboundedSender<Vec<u8>>>,
    assembler_tx: Option<&mpsc::UnboundedSender<Vec<u8>>>,
    expected_info_num: &mut u8,
) -> bool {
    let writer_tx = writer_tx.expect("No sender provided to handle frame rejection");

    match frame.frame_type.into() {
        FrameType::ReceiveReady => handle_receive_ready(safe_window, &frame, condition),
        FrameType::ConnectionEnd => handle_connection_end(safe_window, writer_tx, condition),
        FrameType::ConnectionStart => {
            handle_connection_start(safe_window, &frame, writer_tx, condition)
        }
        FrameType::Reject => handle_reject(safe_window, &frame, writer_tx, condition),
        FrameType::Information => {
            let assembler_tx = assembler_tx.expect("No sender provided to handle frame reassembly");
            handle_information(
                safe_window,
                frame,
                writer_tx,
                condition,
                assembler_tx,
                expected_info_num,
            )
        }
        FrameType::P => handle_p(writer_tx, expected_info_num),
        FrameType::Unknown => false,
    }
}

/// Sends a frame to the server.
///
/// The function writes the frame bytes to the stream and flushes the stream.
/// If an error occurs while sending the frame, the function returns an error.
///
/// # Arguments
/// - `stream` - The write half of the TCP stream.
/// - `rx` - The receiver channel to receive the frames to send.
pub fn writer(
    mut stream: OwnedWriteHalf,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        // FIXME:: The writer gets overwhelmed and stops sending frames. The
        // tunnel should be able to send all the frames.

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

        rx.close();
        Ok("Closed writer")
    })
}

/// Creates a frame timer task.
///
/// The task keeps checking if a frame was acknowledged and sends it if it was not.
/// If the frame is still in the window, the task sends the frame to the sender.
/// If the frame is not in the window, the task stops.
///
/// # Arguments
/// - `frame_bytes` - The bytes of the frame to send.
/// - `tx` - The sender to send the frame if it was not acknowledged.
///
/// # Panics
/// The function will panic if:
/// - The sender fails to send the frame
pub fn create_frame_timer(
    frame_bytes: Vec<u8>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
) -> JoinHandle<()> {
    debug!("Starting frame timer for frame {}", frame_bytes[2]);
    tokio::spawn(async move {
        debug!("Frame timer started for frame {}", frame_bytes[2]);

        let sleep_duration = time::Duration::from_secs(Window::FRAME_TIMEOUT);
        let mut interval = time::interval(sleep_duration);
        loop {
            interval.tick().await;

            info!("Timeout expired, resending frame {}", frame_bytes[2]);
            tx.send(frame_bytes.clone()).expect("Failed to send frame");
        }
    })
}

/// Sends a connection request frame to the server.
///
/// This function sends a connection request frame to the server and waits for the acknowledgment.
/// It also handles resending the frame in case of a timeout.
///
/// The function returns when the connection is established.
///
/// # Arguments
/// - `safe_window` - The window to manage the frames.
/// - `connection_start` - A boolean indicating if the connection is starting or ending.
/// - `srej` - A boolean indicating if the connection uses REJ or SREJ.
/// - `tx` - The sender channel to send the frame to the writer task.
/// - `condition` - The condition variable to signal the window state.
///
/// # Panics
/// - The window lock fails
/// - The frame fails to be pushed to the window
/// - The frame fails to be sent to the writer task
/// - The condition fails to wait
pub async fn connection_request(
    safe_window: SafeWindow,
    connection_start: bool,
    srej: Option<u8>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    condition: SafeCond,
) {
    let frame_type = match connection_start {
        true => FrameType::ConnectionStart,
        false => FrameType::ConnectionEnd,
    };

    if connection_start && srej.is_none() {
        error!("SREJ value is required when starting a connection");
        exit(1);
    }

    let srej = srej.unwrap_or(0);
    let request_frame = Frame::new(frame_type, srej, Vec::new());
    let request_frame_bytes = request_frame.to_bytes();

    // Send the connection request frame
    tx.send(request_frame_bytes.clone())
        .expect("Failed to send frame to writer task");
    info!("Sent connection request frame");

    // Run a timer to resend the request if it is not acknowledged
    let mut window = safe_window.lock().expect("Failed to lock window");

    // Set a flag to indicate that a disconnect request was sent or not
    window.sent_disconnect_request = !connection_start;

    // Only if we start a new connection
    if connection_start {
        window.srej = srej == 1;

        // Create a frame timer for the connection request frame
        window
            .push(request_frame, tx.clone())
            .expect("Failed to push frame to window");

        // Wait for the connection to be established
        window = condition
            .wait_while(window, |window| !window.is_empty())
            .expect("Failed to wait for window");
        window.pop(srej, condition);
    }

    window.is_connected = connection_start;
}
