//! Contains the functions to manage the IO tasks.

use crate::{
    frame::{Frame, FrameType},
    frame_handlers::*,
    window::{SafeCond, SafeWindow, Window},
};
use log::{debug, error, info, warn};
use std::{process::exit, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::mpsc::{self, error::TryRecvError},
    task::JoinHandle,
    time::{self, sleep},
};

/// The number of message the channel can hold.
/// That channel is used for communications between threads.
pub const CHANNEL_CAPACITY: usize = 1024;

/// Receives frames from the server and handles them.
/// The function reads from the stream and constructs frames from the received bytes.
/// It then calls `handle_frame()` to process the frames.
/// If a connection end frame is received, the function returns.
///
/// # Arguments
/// - `stream` - The read half of the TCP stream.
/// - `window` - The window to manage the frames.
/// - `writer_tx` - The sender to send frames to the writer.
/// - `assembler_tx` - The sender to send frames to the assembler.
///
/// # Returns
/// The function returns a join handle to the spawned task.
/// The task will return a string indicating the reason for ending.
///
/// # Panics
/// The function will panic if:
/// - The lock of the window fails
pub fn reader(
    mut stream: OwnedReadHalf,
    window: SafeWindow,
    writer_tx: mpsc::Sender<Vec<u8>>,
    assembler_tx: Option<mpsc::Sender<Vec<u8>>>,
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
                Ok(0) => return Ok("Connection ended"),
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
                        debug!("Empty frame buffer");
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
                    let do_disconnect = handle_reception(
                        frame,
                        window.clone(),
                        writer_tx.clone(),
                        assembler_tx.clone(),
                        &mut next_info_frame_num,
                    )
                    .await;

                    if do_disconnect {
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
/// - The `assembler_tx` is not provided when handling an information frame
pub async fn handle_reception(
    frame: Frame,
    safe_window: SafeWindow,
    writer_tx: mpsc::Sender<Vec<u8>>,
    assembler_tx: Option<mpsc::Sender<Vec<u8>>>,
    expected_info_num: &mut u8,
) -> bool {
    match frame.frame_type.into() {
        FrameType::ReceiveReady => handle_receive_ready(safe_window, &frame),
        FrameType::ConnectionEnd => handle_connection_end(safe_window, writer_tx).await,
        FrameType::ConnectionStart => handle_connection_start(safe_window, &frame, writer_tx).await,
        FrameType::Reject => handle_reject(safe_window, &frame, writer_tx).await,
        FrameType::Information => {
            let assembler_tx = assembler_tx.expect("No sender provided to handle frame reassembly");
            handle_information(
                safe_window,
                frame,
                writer_tx,
                assembler_tx,
                expected_info_num,
            )
            .await
        }
        FrameType::P => handle_p(writer_tx, *expected_info_num).await,
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
/// - `drop_probability` - The probability of dropping a frame.
/// - `flip_probability` - The probability of flipping a bit in a frame.
///
/// # Returns
/// The function returns a join handle to the spawned task.
/// The task will return a string indicating the reason for ending.
pub fn writer(
    mut stream: OwnedWriteHalf,
    mut rx: mpsc::Receiver<Vec<u8>>,
    drop_probability: f32,
    flip_probability: f32,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        // Receive frames until all tx are dropped
        loop {
            match rx.try_recv() {
                Ok(mut frame) => {
                    debug!("Writer task received frame {:X?}", frame);

                    // Drop the frame with a probability
                    if rand::random::<f32>() < drop_probability {
                        warn!("Dropping frame");
                        continue;
                    }

                    // Bit flip the frame with a probability
                    if rand::random::<f32>() < flip_probability {
                        warn!("Flipping bit in frame");
                        let bit = rand::random::<usize>() % frame.len() * 8;
                        frame[bit / 8] ^= 1 << (bit % 8);
                    }

                    debug!("Sending frame {:X?}", frame);
                    // Send the file contents to the server
                    if stream.write_all(&frame).await.is_err() {
                        return Err("Failed to write to stream");
                    };

                    // Flush the stream to ensure the data is sent immediately
                    if (stream.flush().await).is_err() {
                        return Err("Failed to flush stream");
                    };
                    info!("Sent frame");
                }
                Err(TryRecvError::Empty) => sleep(Duration::from_millis(10)).await,
                Err(TryRecvError::Disconnected) => break,
            };
        }

        // BUG: Sometimes, the "await" never wakes up...
        // The code higher seems to fix it.
        //
        // while let Some(mut frame) = rx.recv().await {
        //     debug!("Writer task received frame {:X?}", frame);
        //
        //     // Drop the frame with a probability
        //     if rand::random::<f32>() < drop_probability {
        //         warn!("Dropping frame");
        //         continue;
        //     }
        //
        //     // Bit flip the frame with a probability
        //     if rand::random::<f32>() < flip_probability {
        //         warn!("Flipping bit in frame");
        //         let bit = rand::random::<usize>() % frame.len() * 8;
        //         frame[bit / 8] ^= 1 << (bit % 8);
        //     }
        //
        //     debug!("Sending frame {:X?}", frame);
        //     // Send the file contents to the server
        //     if stream.write_all(&frame).await.is_err() {
        //         return Err("Failed to write to stream");
        //     };
        //
        //     // Flush the stream to ensure the data is sent immediately
        //     if (stream.flush().await).is_err() {
        //         return Err("Failed to flush stream");
        //     };
        //     info!("Sent frame");
        // }

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
/// # Returns
/// The function returns a join handle to the spawned task.
pub fn create_frame_timer(frame_bytes: Vec<u8>, tx: mpsc::Sender<Vec<u8>>) -> JoinHandle<()> {
    debug!("Starting frame timer for frame {}", frame_bytes[2]);
    tokio::spawn(async move {
        debug!("Frame timer started for frame {}", frame_bytes[2]);

        let mut interval = time::interval(time::Duration::from_secs(Window::FRAME_TIMEOUT));
        loop {
            interval.tick().await;

            info!("Timeout expired, resending frame {}", frame_bytes[2]);
            if tx.send(frame_bytes.clone()).await.is_err() {
                error!("Failed to send frame to writer task. Channel probably dropped");
                return;
            }
            debug!("Resent frame {}", frame_bytes[2]);
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
/// The function will panic if:
/// - The window lock fails
/// - The frame fails to be pushed to the window
/// - The frame fails to be sent to the writer task
/// - The condition fails to wait
pub async fn connection_request(
    safe_window: SafeWindow,
    connection_start: bool,
    srej: Option<u8>,
    tx: mpsc::Sender<Vec<u8>>,
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
        .await
        .expect("Failed to send frame to writer task");
    info!("Sent connection request frame");

    // Run a timer to resend the request if it is not acknowledged
    let mut window = safe_window.lock().expect("Failed to lock window");

    // Set a flag to indicate that a disconnect request was sent or not
    window.sent_disconnect_request = !connection_start;

    // Only if we start a new connection
    if connection_start {
        window.srej = srej == 1;

        debug!("Waiting for acknowledgment of connection request");

        // Create a frame timer for the connection request frame
        window
            .push(request_frame, tx.clone())
            .expect("Failed to push frame to window");

        // Wait for the connection to be established
        window = condition
            .wait_while(window, |window| !window.is_empty())
            .expect("Failed to wait for window");
    }
    window.is_connected = connection_start;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;
    use tokio::try_join;

    /// Creates a pair of TCP streams.
    ///
    /// The function binds a listener to an ephemeral port and connects to it.
    ///
    /// # Returns
    /// A tuple containing the read and write halves of the client and server streams.
    async fn create_tcp_streams() -> (
        (OwnedReadHalf, OwnedWriteHalf),
        (OwnedReadHalf, OwnedWriteHalf),
    ) {
        // Start a TCP listener on an ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind listener");
        let addr = listener.local_addr().expect("Failed to get local address");

        // Spawn a task to accept the connection
        let server = tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("Failed to accept connection");
            stream.into_split()
        });

        // Connect to the listener
        let client_stream = TcpStream::connect(addr)
            .await
            .expect("Failed to connect to server");
        let (client_read_half, client_write_half) = client_stream.into_split();

        let (server_read_half, server_write_half) = server.await.expect("Failed to get server");

        (
            (client_read_half, client_write_half),
            (server_read_half, server_write_half),
        )
    }

    #[tokio::test]
    async fn test_writer() {
        let ((_, client_write), (mut server_read, _)) = create_tcp_streams().await;
        let (tx, rx) = mpsc::channel(10);

        // Spawn a task to read from the stream
        let reader_handle = tokio::spawn(async move {
            let mut buf = vec![0; 64];
            let n = server_read
                .read(&mut buf)
                .await
                .expect("Failed to read from stream");
            assert_eq!(&buf[..n], &[1, 2, 3, 4]);
        });

        // Spawn a task to write to the stream
        let writer_handle = writer(client_write, rx, 0f32, 0f32);

        // Send a mock message
        tx.send(vec![1, 2, 3, 4])
            .await
            .expect("Failed to send message");
        // Close sender to allow writer to complete
        drop(tx);

        assert!(writer_handle.await.is_ok(), "Writer task failed");
        reader_handle.await.expect("Reader task failed");
    }

    #[tokio::test]
    async fn test_reader() {
        let ((client_read, _), (_, mut server_write)) = create_tcp_streams().await;
        let (tx, _rx) = mpsc::channel(10);

        let window = SafeWindow::default();
        {
            let mut window = window.lock().expect("Failed to lock window");
            window.is_connected = true;
            window.sent_disconnect_request = false;
        }
        let reader_handle = reader(client_read, SafeWindow::default(), tx, None);

        // Mock the input to the reader by writing data to the server's write half
        server_write
            .write_all(&[
                Frame::BOUNDARY_FLAG,
                FrameType::ConnectionEnd.into(),
                0,
                0xA7,
                0x6A,
                Frame::BOUNDARY_FLAG,
            ])
            .await
            .expect("Failed to write to stream");

        assert!(try_join!(reader_handle).is_ok());
    }
}
