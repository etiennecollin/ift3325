//! A simple TCP client for sending files to a server.
//!
//! This client connects to a specified TCP server and sends the contents
//! of a file. It uses the `log` crate for logging errors and information
//! during the execution of the program.
//!
//! ## Usage
//! To run the client, specify the server address, port number, file path, and a placeholder argument:
//!
//! ```bash
//! cargo run -- <address> <port> <file_path> <go_back_n>
//! ```
//!
//! Replace `<address>` with the server's IP address (e.g., 127.0.0.1), `<port>` with the desired port number,
//! and `<file_path>` with the path to the file you want to send.

use log::{error, info};
use std::{
    env,
    fs::File,
    io::Read,
    net::SocketAddr,
    process::exit,
    sync::{Arc, Condvar, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};
use utils::{
    frame::{Frame, FrameType},
    window::Window,
};

type SafeWindow = Arc<Mutex<Window>>;
type SafeCond = Arc<Condvar>;

/// Handles the received frames from the server.
///
/// If the type of frame is an acknowledgment, the function pops the
/// acknowledged frames from the window. If the type is a rejection,
/// it pops the implicitly acknowledged frames from the window and
/// signals the `send()` task to resends the frames in the window.
///
/// # Arguments
/// * `frame` - The frame received from the server.
/// * `window` - The window to manage the frames.
///
/// # Returns
/// If the function returns `true`, the connection should be terminated.
///
/// # Panics
/// This function will panic if it fails to lock the window.
fn handle_frame(frame: Frame, window: &SafeWindow, condition: &SafeCond) -> bool {
    let mut window = window.lock().expect("Failed to lock window");

    // Check if the frame is an acknowledgment or a rejection
    let mut end_connection = false;
    match frame.frame_type.into() {
        // If it is an acknowledgment, pop the acknowledged frames from the window
        FrameType::ReceiveReady => {
            // Pop the acknowledged frames from the window
            window.pop_until((frame.num - 1) % Window::SIZE as u8);

            // Notify the send task that space was created in the window
            condition.notify_one();

            info!("Received acknowledgement up to frame {}", frame.num);
        }
        // If it is a rejection, pop the implicitly acknowledged frames from the window
        FrameType::Reject => {
            // Pop the implicitly acknowledged frames from the window
            window.pop_until(frame.num);
            window.resend = true;

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
async fn receive(
    mut stream: OwnedReadHalf,
    window: SafeWindow,
    condition: SafeCond,
) -> Result<(), ()> {
    let mut frame_buf = [0; Frame::MAX_SIZE];
    let mut frame_position = 0;
    let mut in_frame = false;
    loop {
        let mut buf = [0; Frame::MAX_SIZE];
        let read_length = match stream.read(&mut buf).await {
            Ok(read_length) => read_length,
            Err(e) => {
                error!("Failed to read from stream: {}", e);
                return Err(());
            }
        };

        // TODO: Move this to a separate function in utils?
        for byte in buf[..read_length].iter() {
            if *byte == Frame::BOUNDARY_FLAG {
                frame_buf[frame_position] = *byte;
                frame_position += 1;

                // If the frame is complete, handle it
                if in_frame {
                    frame_position = 0;

                    // Create frame from buffer
                    let frame = match Frame::from_bytes(&frame_buf) {
                        Ok(frame) => frame,
                        Err(e) => {
                            // TODO: Do we return or simply ignore errors here as there will be a retransmission?
                            error!("Failed to create frame from buffer: {:?}", e);
                            return Err(());
                        }
                    };

                    // Handle the frame
                    let status = handle_frame(frame, &window, &condition);

                    // We received a connection end frame, stop the connection
                    if status {
                        return Ok(());
                    }

                    // Reset buffer
                    frame_buf = [0; Frame::MAX_SIZE];
                }

                in_frame = !in_frame;
            } else if in_frame {
                // Add byte to frame buffer
                frame_buf[frame_position] = *byte;
                frame_position += 1;
            }
        }
    }
}

/// Sends a file to the specified server address.
///
/// This function opens the specified file, reads it and sends it over the
/// established TCP connection.
///
/// # Arguments
///
/// * `addr` - The socket address of the server to connect to.
/// * `window` - The window to manage the frames.
/// * `file_path` - The path to the file to be sent.
///
/// # Panics
///
/// This function will exit the process with an error message if any of the following fails:
/// - Opening the file
/// - Reading the file contents
/// - Sending the file contents to the server
async fn send(
    mut stream: OwnedWriteHalf,
    safe_window: SafeWindow,
    condition: SafeCond,
    file_path: String,
) -> Result<(), ()> {
    // Open the file
    let mut file = match File::open(&file_path) {
        Ok(file) => {
            info!("Opened file: {}", file_path);
            file
        }
        Err(e) => {
            error!("Failed to open file: {}", e);
            return Err(());
        }
    };

    // Read the file contents into the buffer
    let mut buf = Vec::new();
    if let Err(e) = file.read_to_end(&mut buf) {
        error!("Failed to read file contents: {}", e);
        return Err(());
    }

    // Read the file in chunks and create the frames to be sent
    let mut num: u8 = 0;
    for (i, chunk) in buf.chunks(Frame::MAX_SIZE_DATA).enumerate() {
        // Create a new frame with the chunk data
        // Get the frame number based on the chunk index and the window size
        num = (i % Window::SIZE) as u8;
        let frame = Frame::new(FrameType::Information, num, chunk.to_vec());
        let frame_bytes = frame.to_bytes();

        let mut resend_frames: Option<Vec<(u8, Vec<u8>)>> = None;

        // Create a scope to make sure the window is unlocked as soon as possible when the MutexGuard is dropped
        {
            // Lock the window to access the frames
            let mut window = safe_window.lock().expect("Failed to lock window");

            // Wait for the window to have space
            while window.isfull() {
                window = condition
                    .wait(window)
                    .expect("Failed to wait for condition");
            }

            // Check if we need to resend the frames in the window
            if window.resend {
                // Collect the frames to be resent
                let frames = window
                    .frames
                    .iter()
                    .map(|frame| (frame.num, frame.to_bytes()))
                    .collect();

                resend_frames = Some(frames);
                window.resend = false;
            }

            // Push the frame to the window
            window
                .push(frame)
                .expect("Failed to push frame to window, this should never happen");
        }

        // Resend the frames in the window if needed
        if let Some(frames) = resend_frames {
            for (num, frame) in frames {
                // Send the file contents to the server
                match stream.write_all(&frame).await {
                    Ok(_) => info!("Resent frame number {}", num),
                    Err(e) => {
                        error!("Failed to resend frame number {}: {}", num, e);
                    }
                };
                // Flush the stream to ensure the data is sent immediately
                stream.flush().await.expect("Failed to flush stream");
            }
        };

        // Send the file contents to the server
        match stream.write_all(&frame_bytes).await {
            Ok(_) => info!("Sent chunk {} with frame number {} to the server", i, {
                num
            }),
            Err(e) => {
                error!("Failed to send file contents of chunk {}: {}", i, e);
                exit(1);
            }
        }

        // Flush the stream to ensure the data is sent immediately
        stream.flush().await.expect("Failed to flush stream");
    }

    // Create a scope to make sure the window is unlocked as soon as possible when the MutexGuard is dropped
    info!("Finished sending file contents, waiting for window to be empty");
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        while !window.is_empty() {
            window = condition
                .wait(window)
                .expect("Failed to wait for condition");
        }
    }

    // Send disconnect frame
    let disconnection_frame_num = match num {
        0 => 0,
        _ => (num + 1) % Window::SIZE as u8,
    };
    let disconnection_frame =
        Frame::new(FrameType::ConnexionEnd, disconnection_frame_num, Vec::new()).to_bytes();
    match stream.write_all(&disconnection_frame).await {
        Ok(_) => info!("Sent disconnection frame to the server"),
        Err(e) => error!("Failed to send disconnection frame: {}", e),
    };

    // Close the connection
    if let Err(e) = stream.shutdown().await {
        error!("Failed to close connection: {}", e);
        return Err(());
    };

    Ok(())
}

/// The main function that initializes the client.
///
/// This function sets up logging, processes command-line arguments to extract the server address,
/// port number, and file path, and then calls `send_file` to send the specified file to the server.
///
/// # Panics
///
/// This function will exit the process with an error message if the arguments are incorrect,
/// if the port number is invalid, or if the address format is invalid.
#[tokio::main]
async fn main() {
    // Initialize the logger
    env_logger::init();

    // Collect command-line arguments
    let args: Vec<String> = env::args().collect();

    // Check if the right number of arguments were provided
    if args.len() != 5 {
        error!("Usage: {} <address> <port> <file> <0>", args[0]);
        exit(1);
    }

    // Parse the port number
    let port: u16 = match args[2].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid port number: {}", args[2]);
            exit(1);
        }
    };

    // Parse and validate the address format
    let addr = &*format!("{}:{}", &args[1], port);
    let socket_addr = match addr.parse::<SocketAddr>() {
        Ok(socket_addr) => socket_addr,
        Err(_) => {
            error!("Invalid address format: {}", addr);
            exit(1);
        }
    };

    let file_path = args[3].clone();

    // Connect to server
    let stream = match TcpStream::connect(socket_addr).await {
        Ok(stream) => {
            info!("Connected to server at {}", addr);
            stream
        }
        Err(e) => {
            error!("Failed to connect to server: {}", e);
            exit(1);
        }
    };

    // Split the stream into read and write halves
    let (read, write) = stream.into_split();

    // Create a window to manage the frames
    let window = Arc::new(Mutex::new(Window::new()));

    // Create a condition to signal the send task that space is available in the window
    let condition = Arc::new(Condvar::new());

    // Receive the frames from the server
    let window_clone = window.clone();
    let condition_clone = condition.clone();
    let reader = tokio::spawn(async move {
        match receive(read, window_clone, condition_clone).await {
            Ok(_) => info!("Connection ended by server"),
            Err(_) => error!("Connection ended with an error"),
        };
    });

    // Send the file to the server
    let window_clone = window.clone();
    let condition_clone = condition.clone();
    let sender = tokio::spawn(async move {
        match send(write, window_clone, condition_clone, file_path).await {
            Ok(_) => info!("Connection ended by client"),
            Err(_) => error!("Failed to send file"),
        };
    });

    // Wait for all data to be transmitted
    let _ = tokio::try_join!(sender, reader);
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task,
    };

    use super::*;

    const TEST_FILE_CONTENTS: &[u8] = b"Test file contents";
    const FILE_PATH: &str = "_cargo_client_test.txt";
    const PORT: u16 = 8081;

    async fn start_test_server() {
        // Bind the server to the specified port
        let listener = TcpListener::bind(format!("127.0.0.1:{}", PORT))
            .await
            .expect("Failed to bind to address");

        task::spawn(async move {
            // Accept incoming connections
            let (mut stream, _) = listener
                .accept()
                .await
                .expect("Failed to accept connection");

            // Read the data from the stream
            let mut buf = Vec::new();
            let _ = stream
                .read_to_end(&mut buf)
                .await
                .expect("Failed to read data");

            // Verify the data received
            assert_eq!(buf, TEST_FILE_CONTENTS);

            // Close the connection
            stream.shutdown().await.expect("Failed to close connection");
        });
    }

    // /// Tests the functionality of sending a file to the server.
    // ///
    // /// This test starts a mock TCP server that listens for incoming connections.
    // /// It creates a test file with known contents ("Test file contents"),
    // /// then connects to the server and sends the file. The test verifies that
    // /// the server receives the correct data by checking the contents received.
    // ///
    // /// After the test, the created file is removed to clean up.
    // #[tokio::test]
    // async fn test_send_file() {
    //     // Start a test server
    //     start_test_server().await;
    //
    //     // Create a test file and write to it
    //     let mut file = File::create(FILE_PATH).expect("Failed to create test file");
    //     writeln!(file, "{:?}", TEST_FILE_CONTENTS).expect("Failed to write to test file");
    //
    //     // Send the test file
    //     let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    //     send(addr, FILE_PATH);
    //
    //     // Clean up test file
    //     std::fs::remove_file(FILE_PATH).expect("Failed to remove test file");
    // }
}
