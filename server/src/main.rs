// use colored::*;
use log::{error, info};
use std::{env, net::SocketAddr, process::exit};
use tokio::io::{AsyncReadExt, Result};
use tokio::net::{TcpListener, TcpStream};

async fn handle_client(mut stream: TcpStream) -> Result<()> {
    let addr = stream.peer_addr()?;
    info!("New client: {:?}", addr);

    // Print received data
    let mut buf = [0; 1024];
    let bytes_read = stream.read(&mut buf).await?;

    // Check if connection was closed
    if bytes_read == 0 {
        return Ok(());
    }

    // Convert the bytes to a string and print it
    let data = String::from_utf8_lossy(&buf[..bytes_read]);
    info!("Received data: {}", data);

    info!("Closing connection with: {:?}", addr);
    Ok(())
}

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
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to address: {}", e);
            exit(1);
        }
    };

    // Accept connections
    info!("Server listening on {}", addr);
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                info!("Client connected");

                // Spawn a new task to handle the client
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream).await {
                        error!("Failed to handle client: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }
}
