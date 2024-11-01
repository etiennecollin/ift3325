use log::{error, info};
use std::{
    env,
    fs::File,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    process::exit,
    str::FromStr,
};

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
}

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
