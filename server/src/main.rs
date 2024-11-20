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
//! cargo run -- <port_number>
//! ```
//!
//! Replace `<port_number>` with the desired port number (e.g., 8080).

use log::{error, info};
use std::{env, net::SocketAddr, process::exit};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Result},
    net::{TcpListener, TcpStream},
    task,
};
use utils::frame::{Frame, FrameType};

/// Handles incoming client connections.
///
/// This function reads data sent by the client, logs the received data,
/// and checks if the client has requested to shut down the server.
///
/// # Arguments
///
/// * `stream` - The TCP stream associated with the connected client.
/// * `addr` - The socket address of the client.
///
/// # Returns
///
/// Returns `Ok(true)` if the client sent "shutdown", indicating the server should shut down,
/// `Ok(false)` if the connection was closed by the client, or an `Err` if an error occurred.
async fn handle_client(mut stream: TcpStream, addr: SocketAddr) -> Result<bool> {
    info!("New client: {:?}", addr);

    // FIXME: This is really bad. It's temporary code. The entire loop should be a util as it is
    // used in the client as well.
    let (mut read, mut write) = stream.split();

    let mut frame_buf = [0; Frame::MAX_SIZE];
    let mut frame_position = 0;
    let mut in_frame = false;
    loop {
        let mut buf = [0; Frame::MAX_SIZE];
        let read_length = read.read(&mut buf).await.unwrap();

        for byte in buf[..read_length].iter() {
            if *byte == Frame::BOUNDARY_FLAG {
                frame_buf[frame_position] = *byte;
                frame_position += 1;

                // If the frame is complete, handle it
                if in_frame {
                    frame_position = 0;

                    // Create frame from buffer
                    let frame = Frame::from_bytes(&frame_buf).unwrap();

                    // Handle the frame
                    let mut end_connection = false;
                    match frame.frame_type.into() {
                        // If it is an acknowledgment, pop the acknowledged frames from the window
                        FrameType::ReceiveReady => {
                            info!("Received acknowledgement for frame {}", frame.num);
                        }
                        // If it is a rejection, pop the implicitly acknowledged frames from the window
                        FrameType::Reject => {
                            info!("Received reject for frame {}", frame.num);
                        }
                        // If it is a connection end frame, return true to stop the connection
                        FrameType::ConnexionEnd => {
                            end_connection = true;
                        }
                        _ => {}
                    }

                    // Convert the bytes to a string and print it
                    let data_str = String::from_utf8_lossy(&frame.data);
                    let data_trimmed = data_str.trim();
                    info!("Received data from: {:?}: {}", addr, data_trimmed);

                    // Send an acknowledgment and close the connection
                    info!("Sending acknowledgment frame to: {:?}", addr);
                    let ack_frame = Frame::new(FrameType::ReceiveReady, 1, Vec::new()).to_bytes();
                    write.write_all(&ack_frame).await?;
                    write.flush().await?;

                    // Send a disconnection frame and close the connection
                    info!("Sending disconnection frame to: {:?}", addr);
                    let disconnection_frame =
                        Frame::new(FrameType::ConnexionEnd, 0, Vec::new()).to_bytes();
                    write.write_all(&disconnection_frame).await?;
                    write.flush().await?;
                    write.shutdown().await?;

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
    //
    // // Check if the client requested server shutdown
    // if data_trimmed == "shutdown" {
    //     Ok(true)
    // } else {
    //     Ok(false)
    // }
}

/// The main function that initializes the server.
///
/// This function sets up the logging, parses the command-line arguments to get the port number,
/// binds to the specified address and port, and enters a loop to accept client connections.
///
/// # Panics
///
/// This function will exit the process with an error message if the port number is invalid
/// or if binding to the address fails.
#[tokio::main]
async fn main() {
    // Initialize the logger
    env_logger::init();

    // Collect command-line arguments
    let args: Vec<String> = env::args().collect();

    // Check if the right number of arguments were provided
    if args.len() != 2 {
        error!("Usage: {} <port_number>", args[0]);
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
                task::spawn(async move {
                    // Handle the client
                    let status = match handle_client(stream, client_addr).await {
                        Ok(status) => status,
                        Err(e) => {
                            error!("Failed to handle client: {}", e);
                            return;
                        }
                    };

                    // Check if the client requested server shutdown
                    if status {
                        info!("Shutting down server");
                        exit(0);
                    }
                });
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    async fn test_server(data: &[u8]) -> bool {
        // Setup the server
        let mut server_addr = SocketAddr::from(([127, 0, 0, 1], 0));
        let listener = TcpListener::bind(server_addr)
            .await
            .expect("Failed to bind to address");
        server_addr = listener.local_addr().expect("Failed to get local address");

        // Spawn a task to accept a connection
        let status = task::spawn(async move {
            // Accept incoming connections
            let (stream, client_addr) = listener
                .accept()
                .await
                .expect("Failed to accept connection");

            // Get the status from the connection
            handle_client(stream, client_addr)
                .await
                .expect("Failed to handle client")
        });

        // Client connects to the server
        let mut client = TcpStream::connect(server_addr)
            .await
            .expect("Failed to connect to server");

        // Client sends data
        client.write_all(data).await.expect("Failed to write data");

        // Client closes the connection
        client
            .shutdown()
            .await
            .expect("Failed to shutdown connection");

        // Return the status
        status.await.expect("Failed to get status")
    }

    /// Tests the handling of a client connection that sends normal data.
    ///
    /// This test simulates a client connecting to the server and sending a
    /// simple message. It verifies that the server can handle the message
    /// without requesting a shutdown.
    #[tokio::test]
    async fn test_handle_client() {
        assert!(!test_server(b"Hello").await);
    }

    /// Tests the handling of the "exit" command from a client.
    ///
    /// This test simulates a client connecting to the server and sending
    /// the "exit" command. It verifies that the server correctly recognizes
    /// this command and indicates that it should shut down.
    #[tokio::test]
    async fn test_exit_command() {
        assert!(test_server(b"shutdown").await);
    }
}
