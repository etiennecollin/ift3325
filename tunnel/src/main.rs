use log::{error, info, warn};
use std::{env, net::SocketAddr, process::exit};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task,
};
use utils::frame::Frame;

/// This is a simple tunnel that takes frames from a client and sends them to a server.
/// It is used to introduce errors in the communication for testing purposes.

#[tokio::main]
async fn main() {
    // Initialize the logger
    env_logger::init();

    // Collect command-line arguments
    let args: Vec<String> = env::args().collect();

    // Check if the right number of arguments were provided
    if args.len() != 6 {
        error!(
            "Usage: {} <port_number> <server_address> <server_port_number> <P(drop)> <P(flip)>",
            args[0]
        );
        exit(1);
    }

    // Parse the port number
    let local_port: u16 = match args[1].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid port number: {}", args[1]);
            exit(1);
        }
    };

    // Parse the port number argument
    let server_port: u16 = match args[3].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid port number: {}", args[3]);
            exit(1);
        }
    };

    // Parse and validate the address format from the arguments
    let server_addr = &*format!("{}:{}", &args[2], server_port);
    let server_addr = match server_addr.parse::<SocketAddr>() {
        Ok(socket_addr) => socket_addr,
        Err(_) => {
            error!("Invalid address format: {}", server_addr);
            exit(1);
        }
    };

    // Parse the drop probability
    let drop_probability: f32 = match args[4].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid drop probability: {}", args[4]);
            exit(1);
        }
    };

    // Parse the flip probability
    let flip_probability: f32 = match args[5].parse() {
        Ok(num) => num,
        Err(_) => {
            error!("Invalid flip probability: {}", args[5]);
            exit(1);
        }
    };

    // Bind to the address and port
    let local_addr = SocketAddr::from(([127, 0, 0, 1], local_port));
    let listener = match TcpListener::bind(local_addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to address: {}", e);
            exit(1);
        }
    };

    // Accept connections indefinitely
    info!(
        "Server listening on {} and forwarding to {}",
        local_addr, server_addr
    );
    loop {
        match listener.accept().await {
            Ok((stream, client_addr)) => {
                // Spawn a new task to handle the client
                info!("New client: {:?}", client_addr);

                // Connect to server
                let server_stream = match TcpStream::connect(server_addr).await {
                    Ok(stream) => {
                        info!("Connected to server at {}", server_addr);
                        stream
                    }
                    Err(e) => {
                        error!("Failed to connect to server: {}", e);
                        exit(1);
                    }
                };

                // Handle the client
                task::spawn(async move {
                    handle_client(stream, server_stream, drop_probability, flip_probability).await;
                });
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }
}

async fn handle_client(
    mut client_stream: TcpStream,
    mut server_stream: TcpStream,
    drop_probability: f32,
    flip_probability: f32,
) {
    loop {
        // =====================================================================
        // Read client stream
        // =====================================================================
        let mut from_client = [0; Frame::MAX_SIZE];
        let read_length = match client_stream.read(&mut from_client).await {
            Ok(read_length) => read_length,
            Err(e) => {
                error!("Failed to read from client stream: {}", e);
                break;
            }
        };

        if read_length == 0 {
            warn!("Client closed the connection");
            break;
        }

        // =====================================================================
        // Perturb the frame
        // =====================================================================

        // Drop the frame with a probability
        if rand::random::<f32>() < drop_probability {
            // Drop the frame
            warn!("Dropping client frame");
            continue;
        }

        // Bit flip the frame with a probability
        if rand::random::<f32>() < flip_probability {
            // Flip a random bit
            warn!("Flipping bit in client frame");
            let bit = rand::random::<usize>() % read_length * 8;
            from_client[bit / 8] ^= 1 << (bit % 8);
        }

        // =====================================================================
        // Send the frame to server
        // =====================================================================

        // Send the file contents to the server
        match server_stream.write_all(&from_client).await {
            Ok(it) => it,
            Err(_) => {
                error!("Failed to write to server stream");
                break;
            }
        };

        // Flush the stream to ensure the data is sent immediately
        match server_stream.flush().await {
            Ok(it) => it,
            Err(_) => {
                error!("Failed to flush server stream");
                break;
            }
        };

        // =====================================================================
        // Read server stream
        // =====================================================================
        let mut from_server = [0; Frame::MAX_SIZE];
        let read_length = match server_stream.read(&mut from_server).await {
            Ok(read_length) => read_length,
            Err(e) => {
                error!("Failed to read from server stream: {}", e);
                break;
            }
        };

        if read_length == 0 {
            warn!("Server closed the connection");
            break;
        }

        // =====================================================================
        // Perturb the frame
        // =====================================================================

        // Drop the frame with a probability
        if rand::random::<f32>() < drop_probability {
            // Drop the frame
            warn!("Dropping server frame");
            continue;
        }

        // Bit flip the frame with a probability
        if rand::random::<f32>() < flip_probability {
            // Flip a random bit
            warn!("Flipping bit in server frame");
            let bit = rand::random::<usize>() % read_length * 8;
            from_server[bit / 8] ^= 1 << (bit % 8);
        }

        // =====================================================================
        // Send the frame to client
        // =====================================================================

        // Send the file contents to the server
        match client_stream.write_all(&from_server).await {
            Ok(it) => it,
            Err(_) => {
                error!("Failed to write to client stream");
                break;
            }
        };

        // Flush the stream to ensure the data is sent immediately
        match client_stream.flush().await {
            Ok(it) => it,
            Err(_) => {
                error!("Failed to flush client stream");
                break;
            }
        };
    }

    if server_stream.shutdown().await.is_err() {
        error!("Failed to shutdown server stream");
    };
    if client_stream.shutdown().await.is_err() {
        error!("Failed to shutdown client stream");
    };
}
