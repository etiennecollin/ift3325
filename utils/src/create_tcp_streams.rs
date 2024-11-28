use tokio::io::{AsyncReadExt, AsyncWriteExt}; 
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream}; 


pub async fn create_tcp_streams() -> (OwnedReadHalf, OwnedWriteHalf, OwnedReadHalf, OwnedWriteHalf) {
    // Start a TCP listener on an ephemeral port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Spawn a task to accept the connection
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        stream.into_split()
    });

    // Connect to the listener
    let client_stream = TcpStream::connect(addr).await.unwrap();
    let (client_read_half, client_write_half) = client_stream.into_split();

    let (server_read_half, server_write_half) = server.await.unwrap();

    (
        client_read_half,
        client_write_half,
        server_read_half,
        server_write_half,
    )
}
