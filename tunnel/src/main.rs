//! A pair of TCP streams that represent a client-server connection.
//!
//! The tunnel reads frames from the client and sends them to the server. I then
//! reads frames from the server and sends them to the client. It introduces
//! errors in the communication based on the drop and flip probabilities.
//!
//! ## Usage
//! To run the tunnel, simply execute the following command in the terminal:
//!
//! ```bash
//! cargo run -- <in_port> <out_address> <out_port> <prob_frame_drop> <prob_bit_flip>"
//! ```
//!
//! Replace `<in_port>` by the port number the client will connect to,
//! `<out_address>` by the address of the server to connect to, `<out_port>`
//! by the port number of the server to connect to, `<prob_frame_drop>` by the
//! probability of dropping a frame, and `<prob_bit_flip>` by the probability
//! of flipping a bit in a frame.
//!
//! The frobabilities are givven as floating point numbers between 0 and 1.

use env_logger::TimestampPrecision;
use log::{debug, error, info, warn};
use std::{env, net::SocketAddr, process::exit};
use tokio::{
    io::AsyncReadExt,
    net::{tcp::OwnedReadHalf, TcpListener, TcpStream},
    sync::mpsc,
    task::{self, JoinHandle},
};
use utils::{
    frame::Frame,
    io::{writer, CHANNEL_CAPACITY},
    misc::flatten,
};

/// This is a simple tunnel that takes frames from a client and sends them to a server.
/// It is used to introduce errors in the communication for testing purposes.
#[tokio::main(flavor = "multi_thread", worker_threads = 3)]
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
    if args.len() != 6 {
        error!(
            "Usage: {} <in_port> <out_address> <out_port> <prob_frame_drop> <prob_bit_flip>",
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
            Ok((client_stream, client_addr)) => {
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
                    handle_connection(
                        client_stream,
                        server_stream,
                        drop_probability,
                        flip_probability,
                    )
                    .await;
                });
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }
}

/// Handles a single client connection.
/// This function reads frames from the client and sends them to the server.
/// It also reads frames from the server and sends them to the client.
/// It introduces errors in the communication based on the drop and flip probabilities.
/// The function returns when the client or server closes the connection.
async fn handle_connection(
    client_stream: TcpStream,
    server_stream: TcpStream,
    drop_probability: f32,
    flip_probability: f32,
) {
    let (client_tx, client_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);
    let (server_tx, server_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);

    // Split streams
    let (client_read, client_write) = client_stream.into_split();
    let (server_read, server_write) = server_stream.into_split();

    // Generate writer tasks
    let client_writer = writer(client_write, client_rx);
    let server_writer = writer(server_write, server_rx);

    let client_handler = handle_client(
        client_read,
        server_tx.clone(),
        drop_probability,
        flip_probability,
    );
    let server_handler = handle_server(
        server_read,
        client_tx.clone(),
        drop_probability,
        flip_probability,
    );

    // Drop our own copies of the sender channels
    // This will allow the writer tasks to
    // terminate when the sender channels are dropped
    drop(client_tx);
    drop(server_tx);

    match tokio::try_join!(
        flatten(client_writer),
        flatten(server_writer),
        flatten(client_handler),
        flatten(server_handler),
    ) {
        Ok(_) => info!("Connection closed"),
        Err(e) => error!("Error: {:?}", e),
    };
}

/// Handles the client stream.
/// This function reads frames from the client and sends them to the server.
/// It introduces errors in the communication based on the drop and flip probabilities.
fn handle_client(
    mut client_read: OwnedReadHalf,
    server_tx: mpsc::Sender<Vec<u8>>,
    drop_probability: f32,
    flip_probability: f32,
) -> JoinHandle<Result<(), &'static str>> {
    tokio::spawn(async move {
        loop {
            // =====================================================================
            // Read client stream
            // =====================================================================
            debug!("Reading from client");
            let mut from_client = [0; Frame::MAX_SIZE];
            let read_length = match client_read.read(&mut from_client).await {
                Ok(v) => v,
                Err(_) => {
                    return Err("Failed to read from client stream");
                }
            };

            if read_length == 0 {
                warn!("Client closed the connection");
                return Ok(());
            }

            // =====================================================================
            // Perturb the frame
            // =====================================================================
            // Drop the frame with a probability
            if rand::random::<f32>() < drop_probability {
                warn!("Dropping client frame");
                continue;
            }

            // Bit flip the frame with a probability
            if rand::random::<f32>() < flip_probability {
                warn!("Flipping bit in client frame");
                let bit = rand::random::<usize>() % read_length * 8;
                from_client[bit / 8] ^= 1 << (bit % 8);
            }

            // =====================================================================
            // Send the frame to server
            // =====================================================================
            debug!("Sending frame to server");
            // Send the file contents to the server
            server_tx
                .send(from_client[..read_length].to_vec())
                .await
                .unwrap();
        }
    })
}

/// Handles the server stream.
/// This function reads frames from the server and sends them to the client.
/// It introduces errors in the communication based on the drop and flip probabilities.
fn handle_server(
    mut server_read: OwnedReadHalf,
    client_tx: mpsc::Sender<Vec<u8>>,
    drop_probability: f32,
    flip_probability: f32,
) -> JoinHandle<Result<(), &'static str>> {
    tokio::spawn(async move {
        loop {
            // =====================================================================
            // Read server stream
            // =====================================================================
            debug!("Reading from server");
            let mut from_server = [0; Frame::MAX_SIZE];
            let read_length = match server_read.read(&mut from_server).await {
                Ok(read_length) => read_length,
                Err(_) => {
                    return Err("Failed to read from server stream");
                }
            };

            if read_length == 0 {
                warn!("Server closed the connection");
                return Ok(());
            }

            // =====================================================================
            // Perturb the frame
            // =====================================================================
            // Drop the frame with a probability
            if rand::random::<f32>() < drop_probability {
                warn!("Dropping server frame");
                continue;
            }

            // Bit flip the frame with a probability
            if rand::random::<f32>() < flip_probability {
                warn!("Flipping bit in server frame");
                let bit = rand::random::<usize>() % read_length * 8;
                from_server[bit / 8] ^= 1 << (bit % 8);
            }

            // =====================================================================
            // Send the frame to client
            // =====================================================================
            debug!("Sending frame to client");
            client_tx
                .send(from_server[..read_length].to_vec())
                .await
                .unwrap();
        }
    })
}
