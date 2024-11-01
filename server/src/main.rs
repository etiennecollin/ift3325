//! A simple asynchronous TCP server that listens for client connections and handles incoming data.
//!
//! This server uses the `tokio` asynchronous runtime and the `log` crate for logging.
//! Clients can connect to the server, send data, and if they send the message "exit",
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
/// Returns `Ok(true)` if the client sent "exit", indicating the server should shut down,
/// `Ok(false)` if the connection was closed by the client, or an `Err` if an error occurred.
async fn handle_client(mut stream: TcpStream, addr: SocketAddr) -> Result<bool> {
    info!("New client: {:?}", addr);

    // Print received data
    let mut buf = [0; 1024];
    let bytes_read = match stream.read(&mut buf).await {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to read stream from: {}", addr);
            return Err(e);
        }
    };

    // Check if connection was closed
    if bytes_read == 0 {
        info!("Connection closed by: {:?}", addr);
        return Ok(false);
    }

    // Convert the bytes to a string and print it
    let data = String::from_utf8_lossy(&buf[..bytes_read]);
    let data_trimmed = data.trim();
    info!("Received data from: {:?}: {}", addr, data_trimmed);

    match stream.shutdown().await {
        Ok(_) => info!("Connection closed with: {:?}", addr),
        Err(e) => {
            error!("Failed to close connection with: {:?}", addr);
            return Err(e);
        }
    }

    // Check if the client requested server shutdown
    if data_trimmed == "exit" {
        Ok(true)
    } else {
        Ok(false)
    }
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

    const PORT: u16 = 8080;

    async fn test_server(port: u16, data: &[u8], expected_status: bool) {
        // Setup the server
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = TcpListener::bind(addr)
            .await
            .expect("Failed to bind to address");

        // Spawn a task to accept a connection
        task::spawn(async move {
            // Accept incoming connections
            let (stream, _) = listener
                .accept()
                .await
                .expect("Failed to accept connection");

            // Get the status from the connection
            let result = handle_client(stream, addr)
                .await
                .expect("Failed to handle client");

            // Check that connection status
            assert!(result == expected_status);
        });

        // Client connects to the server
        let mut client = TcpStream::connect(addr)
            .await
            .expect("Failed to connect to server");

        // Client sends data
        client.write_all(data).await.expect("Failed to write data");

        // Client closes the connection
        client
            .shutdown()
            .await
            .expect("Failed to shutdown connection");
    }

    /// Tests the handling of a client connection that sends normal data.
    ///
    /// This test simulates a client connecting to the server and sending a
    /// simple message. It verifies that the server can handle the message
    /// without requesting a shutdown.
    #[tokio::test]
    async fn test_handle_client() {
        test_server(PORT + 1, b"Hello", false).await;
    }

    /// Tests the handling of the "exit" command from a client.
    ///
    /// This test simulates a client connecting to the server and sending
    /// the "exit" command. It verifies that the server correctly recognizes
    /// this command and indicates that it should shut down.
    #[tokio::test]
    async fn test_exit_command() {
        test_server(PORT + 2, b"exit", false).await;
    }
}
