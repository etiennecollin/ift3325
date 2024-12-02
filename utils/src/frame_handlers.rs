//! Frame handlers for the receiver.
//!
//! Each handler is responsible for handling a specific frame type and
//! returning a boolean indicating if the connection should be terminated.

use crate::{
    frame::{Frame, FrameType},
    window::{SafeWindow, Window},
};
use log::{debug, info, warn};
use tokio::sync::mpsc::Sender;

/// Handles the receive ready frames.
///
/// Pop the acknowledged frames from the window.
///
/// # Arguments
/// - `safe_window`: The window to handle the frame with.
/// - `frame`: The frame to handle.
///
/// # Returns
/// Always returns false as the connection should not be terminated.
///
/// # Panics
/// Panics if the window cannot be locked.
pub fn handle_receive_ready(safe_window: SafeWindow, frame: &Frame) -> bool {
    let mut window = safe_window.lock().expect("Failed to lock window");

    // Ignore the frame if the connection is not established
    if !window.is_connected {
        return false;
    }

    // Pop the acknowledged frames from the window
    window.pop_until(
        (frame.num + (Window::MAX_FRAME_NUM - 1)) % Window::MAX_FRAME_NUM,
        true,
    );

    info!("Received RR {}", frame.num);
    false
}

/// Handles the connection end frames.
///
/// If we are not the ones initiating the disconnection, we should send an
/// acknowledgment. Else, we should pop the connection request frame
/// from our window as it was acknowledged.
///
/// # Arguments
/// - `safe_window`: The window to handle the frame with.
/// - `writer_tx`: The channel to send data to the writer.
///
/// # Returns
/// Always returns true as the connection should be terminated.
///
/// # Panics
/// Panics if the window cannot be locked.
pub async fn handle_connection_end(safe_window: SafeWindow, writer_tx: Sender<Vec<u8>>) -> bool {
    info!("Received connection end frame");

    let sent_disconnect_request;
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.is_connected = false;
        sent_disconnect_request = window.sent_disconnect_request;

        // Empty the window
        window.clear();
    }

    // If we are not the ones initiating the disconnection, we should send an
    // acknowledgment.
    if !sent_disconnect_request {
        let ack_frame = Frame::new(FrameType::ConnectionEnd, 0, Vec::new()).to_bytes();
        writer_tx
            .send(ack_frame)
            .await
            .expect("Failed to send acknowledgment frame");
        info!("Sent connection end acknowledgment");
    }

    true
}

/// Handles the connection start frames.
///
/// If the window is empty, then we are not the ones initiating the connection and
/// must set the SREJ flag based on the frame number. Else, we should check if the
/// SREJ flag is consistent with the frame number received.
///
/// # Arguments
/// - `safe_window`: The window to handle the frame with.
/// - `frame`: The frame to handle.
/// - `writer_tx`: The channel to send data to the writer.
///
/// # Returns
/// Always returns false as the connection should not be terminated.
///
/// # Panics
/// Panics if:
/// - The window cannot be locked.
/// - The received frame number is invalid.
/// - The acknowledgment frame cannot be sent.
pub async fn handle_connection_start(
    safe_window: SafeWindow,
    frame: &Frame,
    writer_tx: Sender<Vec<u8>>,
) -> bool {
    let is_window_empty;
    {
        // Initialize the window
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.is_connected = true;
        is_window_empty = window.is_empty();

        let frame_srej = match frame.num {
            0 => false,
            1 => true,
            _ => panic!("Invalid frame number for connection request"),
        };

        // If the window is empty, then we are not the ones initiating the connection.
        // This works as the connection request is always the first frame to be sent.
        if is_window_empty {
            // We must set the SREJ flag based on the frame number
            window.srej = frame_srej;
        } else {
            // Check if the SREJ flag is consistent with the frame number
            assert_eq!(frame_srej, window.srej);
            // Pop the connection request frame from the window as it was acknowledged
            window.pop_front();
            info!("Received acknowledgment for connection request frame");
        }
    }

    if is_window_empty {
        // Send an acknowledgment for the connection request frame
        let ack_frame = Frame::new(FrameType::ConnectionStart, frame.num, Vec::new()).to_bytes();
        writer_tx
            .send(ack_frame)
            .await
            .expect("Failed to send acknowledgment frame");

        info!("Received connection request and sent acknowledgment");
    }

    false
}

/// Handles the REJ and SREJ frames.
///
/// Resend the rejected frames.
///
/// # Arguments
/// - `safe_window`: The window to handle the frame with.
/// - `frame`: The frame to handle.
/// - `writer_tx`: The channel to send data to the writer.
///
/// # Returns
/// Always returns false as the connection should not be terminated.
///
/// # Panics
/// Panics if:
/// - The window cannot be locked.
/// - The rejected frame cannot be found in the window.
pub async fn handle_reject(
    safe_window: SafeWindow,
    frame: &Frame,
    writer_tx: Sender<Vec<u8>>,
) -> bool {
    let frames: Vec<(u8, Vec<u8>)>;
    {
        let mut window = safe_window.lock().expect("Failed to lock window");

        // Ignore the frame if the connection is not established
        if !window.is_connected {
            return false;
        }
        let srej = window.srej;

        // Pop the implicitly acknowledged frames from the window
        let popped_num = window.pop_until(
            (frame.num + (Window::MAX_FRAME_NUM - 1)) % Window::MAX_FRAME_NUM,
            true,
        );

        debug!("Popped {} frames", popped_num);

        // Select the frames to be resent
        if srej {
            let selected_frame = window
                .frames
                .iter()
                .find(|(f, _)| f.num == frame.num)
                .expect("Frame not found in window, this should never happen")
                .0
                .to_bytes();

            frames = vec![(frame.num, selected_frame)];
            warn!("Received SREJ for frame {}", frame.num);
        } else {
            frames = window
                .frames
                .iter()
                .map(|(frame, _)| (frame.num, frame.to_bytes()))
                .collect();
            warn!("Received REJ for frame {}", frame.num);
        }
    }

    // Resend the frames
    info!(
        "Resending reject frames {:?}",
        frames.iter().map(|(num, _)| num).collect::<Vec<&u8>>()
    );

    for (_, frame) in frames {
        writer_tx.send(frame).await.expect("Failed to resend frame");
    }

    debug!("Resent reject frames");

    false
}

/// Handles the I frame.
///
/// The receiver sends an acknowledgment frame for the information frame and sends the data to the
/// assembler.
///
/// # Arguments
/// - `safe_window`: The window to handle the frame with.
/// - `frame`: The frame to handle.
/// - `writer_tx`: The channel to send data to the writer.
/// - `assembler_tx`: The channel to send data to the assembler.
/// - `expected_info_num`: The expected next information frame number.
///
/// # Returns
/// Always returns false as the connection should not be terminated.
///
/// # Panics
/// Panics if:
/// - The window cannot be locked.
/// - The acknowledgment frame cannot be sent.
/// - The frame data cannot be sent to the assembler.
pub async fn handle_information(
    safe_window: SafeWindow,
    frame: Frame,
    writer_tx: Sender<Vec<u8>>,
    assembler_tx: Sender<Vec<u8>>,
    expected_info_num: &mut u8,
) -> bool {
    // Ignore the frame if the connection is not established
    if !safe_window
        .lock()
        .expect("Failed to lock window")
        .is_connected
    {
        return false;
    }

    // Check if a frame was dropped
    if *expected_info_num != frame.num {
        handle_dropped_frame(&frame, safe_window, writer_tx, *expected_info_num).await;
        return false;
    }

    // Pop old reject frames from window
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.pop_until(frame.num, true);
    }

    // Update the expected next information frame number
    *expected_info_num = (*expected_info_num + 1) % Window::MAX_FRAME_NUM;

    // Send the frame data to the assembler
    assembler_tx
        .send(frame.data)
        .await
        .expect("Failed to send frame data to assembler");

    // Send an acknowledgment for the information frame
    let ack_frame = Frame::new(FrameType::ReceiveReady, *expected_info_num, Vec::new()).to_bytes();
    writer_tx
        .send(ack_frame)
        .await
        .expect("Failed to send acknowledgment frame");

    info!("Received I {}", frame.num,);
    info!("Sent RR {}", *expected_info_num);

    false
}

/// Handles the dropped frame.
///
/// Send a reject frame for the dropped frame and run a timer to resend the
/// frame if it is not received after a while.
///
/// # Arguments
/// - `frame`: The frame to handle.
/// - `safe_window`: The window to handle the frame with.
/// - `writer_tx`: The channel to send data to the writer.
/// - `expected_info_num`: The expected next information frame number.
///
/// # Panics
/// Panics if:
/// - The window cannot be locked.
/// - The reject frame cannot be sent.
/// - The reject frame cannot be added to the window as it is full.
async fn handle_dropped_frame(
    frame: &Frame,
    safe_window: SafeWindow,
    writer_tx: Sender<Vec<u8>>,
    expected_info_num: u8,
) {
    warn!(
        "Dropped frame detected - Received: {}, Expected: {}",
        frame.num, expected_info_num
    );

    {
        // If window is full, ignore frame
        let window = safe_window.lock().expect("Failed to lock window");
        if window.is_full() || window.contains(expected_info_num) {
            debug!("Window is full or contains frame, ignoring");
            return;
        }
    }

    // Create a reject frame for the dropped frame
    let rej_frame = Frame::new(FrameType::Reject, expected_info_num, Vec::new());

    // Send a reject frame
    writer_tx
        .send(rej_frame.to_bytes())
        .await
        .expect("Failed to send reject frame");
    info!("Sent reject for frame {}", expected_info_num);

    // Add the frame to the window
    safe_window
        .lock()
        .expect("Failed to lock window")
        .push(rej_frame, writer_tx)
        .expect("Window is full, this should probably never happen");
}

/// Handles the P frame.
///
/// Send an acknowledgment for the P frame telling the sender that the receiver is ready to
/// receive.
///
/// # Arguments
/// - `writer_tx`: The channel to send data to the writer.
/// - `expected_info_num`: The expected next information frame number.
///
/// # Returns
/// Always returns false as the connection should not be terminated.
///
/// # Panics
/// Panics if the acknowledgment frame cannot be sent.
pub async fn handle_p(writer_tx: Sender<Vec<u8>>, expected_info_num: u8) -> bool {
    // Send an acknowledgment for the P frame
    let ack_frame = Frame::new(FrameType::ReceiveReady, expected_info_num, Vec::new()).to_bytes();
    writer_tx
        .send(ack_frame)
        .await
        .expect("Failed to send acknowledgment frame");

    info!("Received P frame");
    info!("Sent RR {}", expected_info_num);

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameType};
    use crate::window::SafeWindow;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_handle_connection_end() {
        let window = SafeWindow::default();
        let (tx, mut rx) = mpsc::channel(10);

        let should_terminate = handle_connection_end(window, tx).await;

        assert!(should_terminate);
        assert!(rx.try_recv().is_ok(), "Acknowledgment frame not sent");
    }

    #[tokio::test]
    async fn test_handle_receive_ready() {
        let window = SafeWindow::default();

        let frame = Frame::new(FrameType::ReceiveReady, 1, vec![]);
        let should_continue = handle_receive_ready(window, &frame);

        assert!(!should_continue);
    }
    #[tokio::test]
    async fn test_handle_reject() {
        let window = SafeWindow::default();
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(10);

        {
            let mut window = window.lock().unwrap();
            window
                .push(Frame::new(FrameType::Information, 1, vec![]), tx.clone())
                .unwrap();
            window.is_connected = true;
        }

        let frame = Frame::new(FrameType::Reject, 1, vec![]);
        let should_continue = handle_reject(window, &frame, tx).await;

        assert!(
            !should_continue,
            "Handler should not terminate the connection"
        );
        assert!(rx.try_recv().is_ok(), "Rejection frame not resent");
    }

    #[tokio::test]
    async fn test_frame_drop() {
        let (writer_tx, _rx) = mpsc::channel::<Vec<u8>>(10);
        let (assembler_tx, _rx) = mpsc::channel::<Vec<u8>>(10);
        let window = SafeWindow::default();
        window.lock().expect("Failed to lock window").is_connected = true;

        // Handle the dropped frame
        handle_information(
            window.clone(),
            Frame::new(FrameType::Information, 1, Vec::new()),
            writer_tx,
            assembler_tx,
            &mut 0,
        )
        .await;

        // Check if a reject frame was sent
        let answer = window
            .lock()
            .expect("Failed to lock window")
            .pop_front()
            .expect("Window is empty")
            .to_bytes();
        let expected = Frame::new(FrameType::Reject, 0, Vec::new()).to_bytes();

        assert_eq!(answer, expected);
    }
}
