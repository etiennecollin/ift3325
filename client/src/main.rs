//! A simple TCP client for sending files to a server.
//!
//! This client connects to a specified TCP server and sends the contents
//! of a file. It uses the `log` crate for logging errors and information
//! during the execution of the program.
//!
//! ## Usage
//! To run the client, specify the required arguments:
//!
//! ```bash
//! cargo run -- <server_address> <server_port> <file> <go_back_n> <prob_frame_drop> <prob_bit_flip>
//! ```
//!
//! Replace `<server_address>` with the server's IP address (e.g., 127.0.0.1),
//! `<server_port>` with the server's port number, `<file_path>` with the path to
//! the file to be sent, and `<go_back_n>` with 0 to use the go-back-N
//! protocol, `prob_frame_drop` with the probability of dropping a frame, and
//! `prob_bit_flip` with the probability of flipping a bit in a frame.
//!
//! The probabilities are given as floating point numbers in the range [0, 1]
//! and are independent.

use env_logger::TimestampPrecision;
use log::{debug, error, info};
use std::{env, fs::File, io::Read, net::SocketAddr, process::exit};
use tokio::{net::TcpStream, sync::mpsc, task::JoinHandle};
use utils::{
    frame::{Frame, FrameType},
    io::{connection_request, reader, writer, CHANNEL_CAPACITY},
    misc::flatten,
    window::{SafeCond, SafeWindow, Window},
};

/// The main function that initializes the client.
///
/// This function sets up logging, processes command-line arguments and sends
/// the specified file to the server.
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // Initialize the logger
    env_logger::builder()
        .format_module_path(false)
        .format_timestamp(Some(TimestampPrecision::Nanos))
        .format_level(true)
        .format_target(true)
        .init();

    // Collect command-line arguments
    let args: Vec<String> = env::args().collect();

    // Check if the right number of arguments were provided
    if args.len() != 7 {
        error!(
            "Usage: {} <server_address> <server_port> <file> <go_back_n> <prob_frame_drop> <prob_bit_flip>",
            args[0]
        );
        exit(1);
    }

    // Parse the port number argument
    let port: u16 = match args[2].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid port number: {}", args[2]);
            exit(1);
        }
    };

    // Parse and validate the address format from the arguments
    let addr = &*format!("{}:{}", &args[1], port);
    let socket_addr = match addr.parse::<SocketAddr>() {
        Ok(socket_addr) => socket_addr,
        Err(_) => {
            error!("Invalid address format: {}", addr);
            exit(1);
        }
    };

    // Get the file path argument
    let file_path = args[3].clone();

    // Get the srej argument
    let srej: u8 = match args[4].parse() {
        Ok(0) => 0,
        Ok(1) => 1,
        _ => {
            error!("Invalid srej value: {}", args[4]);
            exit(1);
        }
    };

    // Parse the drop probability
    let drop_probability: f32 = match args[5].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid drop probability: {}", args[5]);
            exit(1);
        }
    };

    // Parse the flip probability
    let flip_probability: f32 = match args[6].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid flip probability: {}", args[6]);
            exit(1);
        }
    };

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

    setup_connection(stream, srej, file_path, drop_probability, flip_probability).await;
}

/// Sets up the connection with the server.
///
/// This function initializes the reader and writer tasks, sends the connection request frame,
/// waits for the acknowledgment, and then sends the file to the server.
/// It also handles resending frames in case of a timeout.
///
/// The function returns when all data has been sent.
///
/// # Arguments
/// - `stream` - The TCP stream to communicate with the server.
/// - `srej` - The selective reject flag to use for the connection.
/// - `file_path` - The path to the file to be sent.
/// - `drop_probability` - The probability of dropping a frame.
/// - `flip_probability` - The probability of flipping a bit in a frame.
async fn setup_connection(
    stream: TcpStream,
    srej: u8,
    file_path: String,
    drop_probability: f32,
    flip_probability: f32,
) {
    // =========================================================================
    // Setup the reader and writer tasks
    // =========================================================================
    // Split the stream into read and write halves
    let (read, write) = stream.into_split();

    // Create channel for tasks to send data to write to writer task
    let (tx, rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);

    // Create a window to manage the frames
    let window = SafeWindow::default();
    let condition = window
        .lock()
        .expect("Failed to lock window")
        .condition
        .clone();

    // Spawn reader task which receives frames from the server
    let reader = reader(read, window.clone(), tx.clone(), None);

    // Spawn the writer task which sends frames to the server
    let writer = writer(write, rx, drop_probability, flip_probability);

    // =========================================================================
    // Send connection start request frame
    // =========================================================================
    connection_request(
        window.clone(),
        true,
        Some(srej),
        tx.clone(),
        condition.clone(),
    )
    .await;

    // =========================================================================
    // Send the file to the server
    // =========================================================================
    let sender = send_file(tx.clone(), window.clone(), condition, file_path);

    // Drop the main transmit channel to allow the writer task to stop when
    // all data is sent
    drop(tx);

    // Wait for all data to be transmitted
    match tokio::try_join!(flatten(reader), flatten(writer), flatten(sender)) {
        Ok((v1, v2, v3)) => {
            info!("{}", v1);
            info!("{}", v2);
            info!("{}", v3);
        }
        Err(e) => error!("Error: {:?}", e),
    };
}

/// Sends a file to the specified server address.
///
/// This function opens the specified file, reads it and sends it over the
/// established TCP connection chunk by chunk. It creates a new frame for each
/// chunk and sends it to the server. The function also handles resending frames
/// in case of a timeout.
///
/// # Arguments
/// - `tx` - The sender channel to send the frame to the writer task.
/// - `safe_window` - The window to manage the frames.
/// - `condition` - The condition to signal tasks that space is available in the window.
/// - `file_path` - The path to the file to be sent.
///
/// # Panics
/// This function will panic if:
/// - The window mutex cannot be locked.
/// - The condition cannot be waited on.
/// - The frame cannot be pushed to the window.
/// - The frame cannot be sent to the writer task.
///
/// # Returns
/// The function returns a `JoinHandle` corresponding to the async task launched
/// that can be awaited to get the result of the operation.
fn send_file(
    tx: mpsc::Sender<Vec<u8>>,
    safe_window: SafeWindow,
    condition: SafeCond,
    file_path: String,
) -> JoinHandle<Result<&'static str, &'static str>> {
    tokio::spawn(async move {
        // Open the file
        let mut file = match File::open(&file_path) {
            Ok(file) => {
                info!("Opened file: {}", file_path);
                file
            }
            Err(e) => {
                error!("Failed to open file: {}", e);
                return Err("Failed to send file");
            }
        };

        // Read the file contents into the buffer
        let mut buf = Vec::new();
        if let Err(e) = file.read_to_end(&mut buf) {
            error!("Failed to read file contents: {}", e);
            return Err("Failed to send file");
        }

        // Read the file in chunks and create the frames to be sent
        let mut num: u8;
        for (i, chunk) in buf.chunks(Frame::MAX_SIZE_DATA).enumerate() {
            let frame_bytes: Vec<u8>;

            // Create a scope to make sure the window is unlocked as soon as possible when the MutexGuard is dropped
            {
                // Lock the window to access the frames
                let mut window = safe_window.lock().expect("Failed to lock window");

                // Get the frame number based on the chunk index and the window size
                num = (i % Window::MAX_FRAME_NUM as usize) as u8;

                // Create a new frame with the chunk data
                let frame = Frame::new(FrameType::Information, num, chunk.to_vec());
                frame_bytes = frame.to_bytes();

                // Wait for the window to have space
                window = condition
                    .wait_while(window, |window| window.is_full())
                    .expect("Failed to wait for condition");

                // Push the frame to the window
                window
                    .push(frame, tx.clone())
                    .expect("Failed to push frame to window, this should never happen");
            }

            // Send the frame to the writer task
            tx.send(frame_bytes)
                .await
                .expect("Failed to send frame to writer task");

            info!("Sent frame {}", num);
        }

        // Create a scope to make sure the window is unlocked as soon as possible when the MutexGuard is dropped
        info!("Finished sending file contents, waiting for window to be empty");
        {
            let mut window = safe_window.lock().expect("Failed to lock window");
            while !window.is_empty() {
                debug!(
                    "Window still not empty: {:X?}",
                    window
                        .frames
                        .iter()
                        .map(|(f, t)| (f.num, t))
                        .collect::<Vec<(u8, &JoinHandle<()>)>>()
                );
                window = condition
                    .wait(window)
                    .expect("Failed to wait for condition");
            }
        }
        info!("Window is empty, all data sent");

        // Send disconnect frame
        connection_request(safe_window, false, None, tx, condition).await;
        Ok("Connection ended by client")
    })
}

// #[cfg(test)]
// mod tests {
//     use std::io::Write;
//
//     use tokio::{
//         io::{AsyncReadExt, AsyncWriteExt},
//         net::TcpListener,
//         task,
//     };
//
//     use super::*;
//
//     const TEST_FILE_CONTENTS: &[u8] = b"Test file contents";
//     const FILE_PATH: &str = "_cargo_client_test.txt";
//     const PORT: u16 = 8081;
//
//     async fn start_test_server() {
//         // Bind the server to the specified port
//         let listener = TcpListener::bind(format!("127.0.0.1:{}", PORT))
//             .await
//             .expect("Failed to bind to address");
//
//         task::spawn(async move {
//             // Accept incoming connections
//             let (mut stream, _) = listener
//                 .accept()
//                 .await
//                 .expect("Failed to accept connection");
//
//             // Read the data from the stream
//             let mut buf = Vec::new();
//             let _ = stream
//                 .read_to_end(&mut buf)
//                 .await
//                 .expect("Failed to read data");
//
//             // Verify the data received
//             assert_eq!(buf, TEST_FILE_CONTENTS);
//
//             // Close the connection
//             stream.shutdown().await.expect("Failed to close connection");
//         });
//     }
//
//     /// Tests the functionality of sending a file to the server.
//     ///
//     /// This test starts a mock TCP server that listens for incoming connections.
//     /// It creates a test file with known contents ("Test file contents"),
//     /// then connects to the server and sends the file. The test verifies that
//     /// the server receives the correct data by checking the contents received.
//     ///
//     /// After the test, the created file is removed to clean up.
//     #[tokio::test]
//     async fn test_send_file() {
//         // Start a test server
//         start_test_server().await;
//
//         // Create a test file and write to it
//         let mut file = File::create(FILE_PATH).expect("Failed to create test file");
//         file.write_all(TEST_FILE_CONTENTS)
//             .expect("Failed to write to test file");
//
//         // Send the test file
//         let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
//         send_file(addr, FILE_PATH);
//
//         // Clean up test file
//         std::fs::remove_file(FILE_PATH).expect("Failed to remove test file");
//     }
// }
