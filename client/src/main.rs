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
    sync::mpsc,
    time,
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
async fn handle_frame(
    tx: &mpsc::Sender<Vec<u8>>,
    frame: Frame,
    window: &SafeWindow,
    condition: &SafeCond,
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
async fn reader(
    mut stream: OwnedReadHalf,
    tx: mpsc::Sender<Vec<u8>>,
    window: SafeWindow,
    pop_condition: SafeCond,
) -> Result<(), ()> {
    let mut frame_buf = Vec::with_capacity(Frame::MAX_SIZE);
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
                            return Err(());
                        }
                    };

                    // Handle the frame and check if the connection should be terminated
                    if handle_frame(&tx, frame, &window, &pop_condition).await {
                        return Ok(());
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
}

/// Sends a frame to the server.
///
/// The function writes the frame bytes to the stream and flushes the stream.
/// If an error occurs while sending the frame, the function returns an error.
async fn writer(mut stream: OwnedWriteHalf, mut rx: mpsc::Receiver<Vec<u8>>) -> Result<(), ()> {
    // Receive frames until all tx are dropped
    while let Some(frame) = rx.recv().await {
        // Send the file contents to the server
        match stream.write_all(&frame).await {
            Ok(it) => it,
            Err(_) => return Err(()),
        };
        // Flush the stream to ensure the data is sent immediately
        match stream.flush().await {
            Ok(it) => it,
            Err(_) => return Err(()),
        };
    }

    // Close the connection
    if let Err(e) = stream.shutdown().await {
        error!("Failed to close connection: {}", e);
        return Err(());
    };
    Ok(())
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
async fn send_file(
    tx: mpsc::Sender<Vec<u8>>,
    safe_window: SafeWindow,
    pop_condition: SafeCond,
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
        // Get the frame number based on the chunk index and the window size
        num = (i % Window::SIZE) as u8;

        // Create a new frame with the chunk data
        let frame = Frame::new(FrameType::Information, num, chunk.to_vec());
        let frame_bytes = frame.to_bytes();

        // Create a scope to make sure the window is unlocked as soon as possible when the MutexGuard is dropped
        {
            // Lock the window to access the frames
            let mut window = safe_window.lock().expect("Failed to lock window");

            // Wait for the window to have space
            while window.isfull() {
                window = pop_condition
                    .wait(window)
                    .expect("Failed to wait for condition");
            }

            // Push the frame to the window
            window
                .push(frame)
                .expect("Failed to push frame to window, this should never happen");
        }

        tx.send(frame_bytes).await.unwrap();

        // Run a timer to resend the frame if it is not acknowledged
        let tx_clone = tx.clone();
        let safe_window_clone = safe_window.clone();
        create_timer(safe_window_clone, num, tx_clone).await;
    }

    // Create a scope to make sure the window is unlocked as soon as possible when the MutexGuard is dropped
    info!("Finished sending file contents, waiting for window to be empty");
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        while !window.is_empty() {
            window = pop_condition
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

    tx.send(disconnection_frame).await.unwrap();

    Ok(())
}

async fn create_timer(safe_window: SafeWindow, num: u8, tx: mpsc::Sender<Vec<u8>>) {
    tokio::spawn(async move {
        let mut interval = time::interval(time::Duration::from_secs(Window::FRAME_TIMEOUT));
        loop {
            interval.tick().await;
            let frame;

            // Lock the window for as short a time as possible
            {
                let window = safe_window.lock().expect("Failed to lock window");

                // Check if the frame is still in the window and get its bytes
                frame = match window.frames.iter().find(|frame| frame.num == num) {
                    Some(frame) => Some(frame.to_bytes()),
                    None => None,
                };
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

    // Create channel for tasks to send data to write to writer task
    let (tx, rx) = mpsc::channel::<Vec<u8>>(100);

    // Create a window to manage the frames
    let window = Arc::new(Mutex::new(Window::new()));

    // Create a condition to signal the send task that space is available in the window
    let condition = Arc::new(Condvar::new());

    // Receive the frames from the server
    let tx_clone = tx.clone();
    let window_clone = window.clone();
    let condition_clone = condition.clone();
    let reader = tokio::spawn(async move {
        match reader(read, tx_clone, window_clone, condition_clone).await {
            Ok(_) => info!("Connection ended by server"),
            Err(_) => error!("Connection ended with an error"),
        };
    });

    let writer = tokio::spawn(async move {
        match writer(write, rx).await {
            Ok(_) => info!("Closed writer"),
            Err(_) => error!("Error in writer"),
        };
    });

    // Send the file to the server
    let tx_clone = tx.clone();
    let window_clone = window.clone();
    let condition_clone = condition.clone();
    let sender = tokio::spawn(async move {
        match send_file(tx_clone, window_clone, condition_clone, file_path).await {
            Ok(_) => info!("Connection ended by client"),
            Err(_) => error!("Failed to send file"),
        };
    });

    // Drop the main transmit channel to allow the writer task to stop when
    // all data is sent
    drop(tx);

    // Wait for all data to be transmitted
    let _ = tokio::try_join!(reader, writer, sender);
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
