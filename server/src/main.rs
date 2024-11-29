//! A simple asynchronous TCP server that listens for client connections and handles incoming data.
//!
//! This server uses the `tokio` asynchronous runtime and the `log` crate for logging.
//! Clients can connect to the server, send data, and if they send the message "shutdown",
//! the server will shut down.
//!
//! ## Usage
//! To run the server, specify a port number as a command-line argument:
//!
//! ```bash
//! cargo run -- <port_number> <prob_frame_drop> <prob_bit_flip>
//! ```
//!
//! Replace `<port_number>` with the desired port number for the server, `<prob_frame_drop>`
//! with the probability of dropping a frame, and `<prob_bit_flip>` with the probability of
//! flipping a bit in a frame.
//!
//! The probabilities are given as floating point numbers in the range [0, 1]
//! and are independent.

use env_logger::TimestampPrecision;
use log::{error, info};
use std::{env, net::SocketAddr, process::exit};
use tokio::{
    fs::{create_dir_all, File},
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task,
};
use utils::{
    io::{reader, writer, CHANNEL_CAPACITY},
    misc::flatten,
    window::{SafeCond, SafeWindow},
};

const OUTPUT_DIR: &str = "./output";

/// The main function that initializes the server.
///
/// This function sets up the logging, parses the command-line arguments,
/// binds to the specified address and port, and enters a loop to accept client
/// connections.
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
    if args.len() != 4 {
        error!(
            "Usage: {} <port_number> <prob_frame_drop> <prob_bit_flip>",
            args[0]
        );
        exit(1);
    }

    // Parse the port number
    let port: u16 = match args[1].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid port number: {}", args[1]);
            exit(1);
        }
    };

    // Parse the drop probability
    let drop_probability: f32 = match args[2].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid drop probability: {}", args[2]);
            exit(1);
        }
    };

    // Parse the flip probability
    let flip_probability: f32 = match args[3].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid flip probability: {}", args[3]);
            exit(1);
        }
    };

    // Bind to the address and port
    let full_addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match TcpListener::bind(full_addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to address: {}", e);
            exit(1);
        }
    };

    // Accept connections indefinitely
    info!("Server listening on {}", full_addr);
    loop {
        match listener.accept().await {
            Ok((stream, client_addr)) => {
                // Spawn a new task to handle the client
                info!("New client: {:?}", client_addr);
                task::spawn(async move {
                    // Handle the client
                    let status = match handle_client(
                        stream,
                        client_addr,
                        drop_probability,
                        flip_probability,
                    )
                    .await
                    {
                        Ok(status) => status,
                        Err(e) => {
                            error!("Failed to handle client: {}", e);
                            false
                        }
                    };

                    // Check if the client requested server shutdown
                    if status {
                        info!("Shutting down server");
                        exit(0);
                    }
                    info!("Client connection ended: {:?}", client_addr);
                });
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }
}

/// Handles incoming client connections.
///
/// This function reads data sent by the client, logs the received data,
/// and checks if the client has requested to shut down the server.
///
/// # Arguments
/// - `stream` - The TCP stream associated with the connected client.
/// - `addr` - The socket address of the client.
///
/// # Returns
/// Returns `Ok(true)` if the client sent "shutdown", indicating the server should shut down,
/// `Ok(false)` if the connection was closed by the client, or an `Err` if an error occurred.
async fn handle_client(
    stream: TcpStream,
    addr: SocketAddr,
    drop_probability: f32,
    flip_probability: f32,
) -> Result<bool, &'static str> {
    // Split the stream into read and write halves
    let (read, write) = stream.into_split();

    // Create channel for tasks to send data to write to writer task
    let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);
    // Create channel to reassemble frames
    let (assembler_tx, assembler_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);

    // Create a window to manage the frames
    let window = SafeWindow::default();

    // Create a condition to signal the send task that space is available in the window
    let condition = SafeCond::default();

    // Spawn reader task which receives frames from the server
    let reader = reader(
        read,
        window.clone(),
        condition.clone(),
        Some(write_tx.clone()),
        Some(assembler_tx.clone()),
    );

    // Spawn the writer task which sends frames to the server
    let writer = writer(write, write_rx, drop_probability, flip_probability);

    // Spawn the assembler task which reassembles frames
    let assembler = tokio::spawn(async move { assembler(assembler_rx, addr).await });

    // Drop the main transmit channel to allow the writer task to stop when
    // all data is sent
    drop(write_tx);
    drop(assembler_tx);

    // Wait for all data to be transmitted
    match tokio::try_join!(flatten(reader), flatten(writer), flatten(assembler)) {
        Ok((v1, v2, v3)) => {
            info!("{}", v1);
            info!("{}", v2);
            let do_disconnect = v3.parse::<bool>().expect("Could not parse status returned by assembler as boolean. This should never happen.");
            if do_disconnect {
                info!("Client requested server shutdown");
            }
            Ok(do_disconnect)
        }
        Err(e) => Err(e),
    }
}

async fn assembler(
    mut assembler_rx: mpsc::Receiver<Vec<u8>>,
    addr: SocketAddr,
) -> Result<&'static str, &'static str> {
    // Get all the data until the connection is closed
    let mut data: Vec<u8> = Vec::new();
    while let Some(frame_data) = assembler_rx.recv().await {
        data.extend_from_slice(&frame_data);
    }

    // Close the receiver channel
    assembler_rx.close();

    // Parse the data as a UTF-8 string
    let data_str = String::from_utf8_lossy(&data);
    let data_trimmed = data_str.trim();
    // info!("Received data from: {:?}:\r\n{}", addr, data_trimmed);

    // Create the output directory if it does not exist
    create_dir_all(OUTPUT_DIR)
        .await
        .expect("Failed to create output directory");

    // Save the data to a file
    let filepath = format!("{}/out_{}.txt", OUTPUT_DIR, addr.port());
    let mut file = File::create(&filepath)
        .await
        .expect("Failed to create test file");
    file.write_all(&data)
        .await
        .expect("Failed to write to file");

    info!("Data saved to: {}", filepath);

    // Check if the client requested server shutdown
    if data_trimmed == "shutdown" {
        Ok("true")
    } else {
        Ok("false")
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use tokio::io::AsyncWriteExt;
//
//     async fn test_server(data: &[u8]) -> bool {
//         // Setup the server
//         let mut server_addr = SocketAddr::from(([127, 0, 0, 1], 0));
//         let listener = TcpListener::bind(server_addr)
//             .await
//             .expect("Failed to bind to address");
//         server_addr = listener.local_addr().expect("Failed to get local address");
//
//         // Spawn a task to accept a connection
//         let status = task::spawn(async move {
//             // Accept incoming connections
//             let (stream, client_addr) = listener
//                 .accept()
//                 .await
//                 .expect("Failed to accept connection");
//
//             // Get the status from the connection
//             handle_client(stream, client_addr)
//                 .await
//                 .expect("Failed to handle client")
//         });
//
//         // Client connects to the server
//         let mut client = TcpStream::connect(server_addr)
//             .await
//             .expect("Failed to connect to server");
//
//         // Client sends data
//         client.write_all(data).await.expect("Failed to write data");
//
//         // Client closes the connection
//         client
//             .shutdown()
//             .await
//             .expect("Failed to shutdown connection");
//
//         // Return the status
//         status.await.expect("Failed to get status")
//     }
//
//     /// Tests the handling of a client connection that sends normal data.
//     ///
//     /// This test simulates a client connecting to the server and sending a
//     /// simple message. It verifies that the server can handle the message
//     /// without requesting a shutdown.
//     #[tokio::test]
//     async fn test_handle_client() {
//         assert!(!test_server(b"Hello").await);
//     }
//
//     /// Tests the handling of the "exit" command from a client.
//     ///
//     /// This test simulates a client connecting to the server and sending
//     /// the "exit" command. It verifies that the server correctly recognizes
//     /// this command and indicates that it should shut down.
//     #[tokio::test]
//     async fn test_exit_command() {
//         assert!(test_server(b"shutdown").await);
//     }
// }
