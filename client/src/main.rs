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
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    process::exit,
    str::FromStr,
};

/// Sends a file to the specified server address.
///
/// This function connects to a TCP server at the given address, opens the specified file,
/// reads its contents, and sends those contents over the established TCP connection.
///
/// # Arguments
///
/// * `addr` - The socket address of the server to connect to.
/// * `file_path` - The path to the file to be sent.
///
/// # Panics
///
/// This function will exit the process with an error message if any of the following fails:
/// - Connecting to the server
/// - Opening the file
/// - Reading the file contents
/// - Sending the file contents to the server
fn send_file(addr: SocketAddr, file_path: &str) {
    // Connect to server
    let mut stream = match TcpStream::connect(addr) {
        Ok(stream) => {
            info!("Connected to server at {}", addr);
            stream
        }
        Err(e) => {
            error!("Failed to connect to server: {}", e);
            exit(1);
        }
    };

    // Open the file
    let mut file = match File::open(file_path) {
        Ok(file) => {
            info!("Opened file: {}", file_path);
            file
        }
        Err(e) => {
            error!("Failed to open file: {}", e);
            exit(1);
        }
    };

    // Read the file contents into the buffer
    let mut buf = Vec::new();
    if let Err(e) = file.read_to_end(&mut buf) {
        error!("Failed to read file contents: {}", e);
        exit(1);
    }

    // Send the file contents to the server
    match stream.write_all(&buf) {
        Ok(_) => info!("Sent file contents to the server"),
        Err(e) => {
            error!("Failed to send file contents: {}", e);
            exit(1);
        }
    }

    // Close the connection
    match stream.shutdown(Shutdown::Both) {
        Ok(_) => info!("Connection closed"),
        Err(e) => {
            error!("Failed to close connection: {}", e);
            exit(1);
        }
    }
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
fn main() {
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
    let addr = &args[1];
    let full_addr = match SocketAddr::from_str(&format!("{}:{}", addr, port)) {
        Ok(addr) => addr,
        Err(_) => {
            error!("Invalid address format: {}:{}", addr, port);
            exit(1);
        }
    };

    let file_path = &args[3];

    // Send the file to the server
    send_file(full_addr, file_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task,
    };

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

    /// Tests the functionality of sending a file to the server.
    ///
    /// This test starts a mock TCP server that listens for incoming connections.
    /// It creates a test file with known contents ("Test file contents"),
    /// then connects to the server and sends the file. The test verifies that
    /// the server receives the correct data by checking the contents received.
    ///
    /// After the test, the created file is removed to clean up.
    #[tokio::test]
    async fn test_send_file() {
        // Start a test server
        start_test_server().await;

        // Create a test file and write to it
        let mut file = File::create(FILE_PATH).expect("Failed to create test file");
        writeln!(file, "{:?}", TEST_FILE_CONTENTS).expect("Failed to write to test file");

        // Send the test file
        let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
        send_file(addr, FILE_PATH);

        // Clean up test file
        std::fs::remove_file(FILE_PATH).expect("Failed to remove test file");
    }
}
