//! Frame handlers for the receiver.
//!
//! Each handler is responsible for handling a specific frame type and
//! returning a boolean indicating if the connection should be terminated.

use crate::{
    frame::{Frame, FrameType},
    io::create_frame_timer,
    window::{SafeCond, SafeWindow, Window},
};
use log::{info, warn};
use tokio::sync::mpsc::Sender;

/// Handles the receive ready frames.
///
/// Pop the acknowledged frames from the window.
pub fn handle_receive_ready(safe_window: &SafeWindow, frame: &Frame, condition: &SafeCond) -> bool {
    {
        let mut window = safe_window.lock().expect("Failed to lock window");

        // Ignore the frame if the connection is not established
        if !window.is_connected {
            return false;
        }

        // Pop the acknowledged frames from the window
        window.pop_until(
            (frame.num + (Window::MAX_FRAME_NUM - 1)) % Window::MAX_FRAME_NUM,
            true,
            condition,
        );
    }

    info!("Received RR {}", frame.num);
    false
}

/// Handles the connection end frames.
///
/// If we are not the ones initiating the disconnection, we should send an
/// acknowledgment. Else, we should pop the connection request frame
/// from our window as it was acknowledged.
pub async fn handle_connection_end(
    safe_window: &SafeWindow,
    writer_tx: &Sender<Vec<u8>>,
    condition: &SafeCond,
) -> bool {
    let is_waiting_disconnect: bool;
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.is_connected = false;
        is_waiting_disconnect = window.sent_disconnect_request;
    }
    info!("Received connection end frame");

    // If we are not the ones initiating the disconnection, we should send an
    // acknowledgment. Else, we should pop the connection request
    // frame from our window as it was acknowledged.
    if is_waiting_disconnect {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.pop_front(condition);
    } else {
        // Send an acknowledgment for the connection request frame
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
pub async fn handle_connection_start(
    safe_window: &SafeWindow,
    frame: &Frame,
    writer_tx: &Sender<Vec<u8>>,
    condition: &SafeCond,
) -> bool {
    // Initialize the window
    let is_window_empty: bool;
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.is_connected = true;
        is_window_empty = window.is_empty();

        // If the window is empty, then we are not the ones initiating the connection and
        // must set the SREJ flag based on the frame number. Else, we should check if the
        // SREJ flag is consistent with the frame number received.
        let frame_srej = match frame.num {
            0 => false,
            1 => true,
            _ => panic!("Invalid frame number for connection request"),
        };
        if is_window_empty {
            window.srej = frame_srej;
        } else {
            assert_eq!(frame_srej, window.srej);
        }
    }

    // If the window is empty, then we are not the ones initiating the connection
    // and we should send an acknowledgment. Else, we should pop the connection request
    // frame from our window as it was acknowledged.
    //
    // This works as the connection request is always the first frame to be sent.
    if is_window_empty {
        // Send an acknowledgment for the connection request frame
        let ack_frame = Frame::new(FrameType::ConnectionStart, frame.num, Vec::new()).to_bytes();
        writer_tx
            .send(ack_frame)
            .await
            .expect("Failed to send acknowledgment frame");

        info!("Received connection request and sent acknowledgment");
    } else {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.pop_front(condition);
        info!("Received acknowledgment for connection request frame");
    }

    false
}

/// Handles the REJ and SREJ frames.
///
/// Resend the rejected frames.
pub async fn handle_reject(
    safe_window: &SafeWindow,
    frame: &Frame,
    writer_tx: &Sender<Vec<u8>>,
    condition: &SafeCond,
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
        window.pop_until(
            (frame.num + (Window::MAX_FRAME_NUM - 1)) % Window::MAX_FRAME_NUM,
            true,
            condition,
        );

        // Select the frames to be resent
        if srej {
            // Select the frame to resend
            let selected_frame = window
                .frames
                .iter()
                .find(|f| f.num == frame.num)
                .expect("Frame not found in window, this should never happen")
                .to_bytes();

            frames = vec![(frame.num, selected_frame)];
            warn!("Received SREJ for frame {}", frame.num);
        } else {
            // Select all frames to be resent
            frames = window
                .frames
                .iter()
                .map(|frame| (frame.num, frame.to_bytes()))
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

    false
}

/// Handles the I frame.
///
/// The receiver sends an acknowledgment frame for the information frame and sends the data to the
/// assembler.
pub async fn handle_information(
    safe_window: &SafeWindow,
    frame: Frame,
    writer_tx: &Sender<Vec<u8>>,
    condition: &SafeCond,
    assembler_tx: &Sender<Vec<u8>>,
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
        return handle_dropped_frame(&frame, safe_window, writer_tx, expected_info_num).await;
    }

    // Pop old reject frames from window
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.pop_until(frame.num, true, condition);
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
pub async fn handle_dropped_frame(
    frame: &Frame,
    safe_window: &SafeWindow,
    writer_tx: &Sender<Vec<u8>>,
    expected_info_num: &u8,
) -> bool {
    warn!(
        "Dropped frame detected - Received: {}, Expected: {}",
        frame.num, expected_info_num
    );

    let rej_frame = Frame::new(FrameType::Reject, *expected_info_num, Vec::new());
    let rej_frame_bytes = rej_frame.to_bytes();

    // Create a scope for the mutex lock
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        // If window is full, ignore frame
        if window.is_full() || window.contains(*expected_info_num) {
            return false;
        }
        window
            .push(rej_frame)
            .expect("Failed to push frame to window");
    }

    // Send a reject frame
    writer_tx
        .send(rej_frame_bytes)
        .await
        .expect("Failed to send reject frame");

    // Run a timer to resend the frame if it is not received
    create_frame_timer(safe_window.clone(), *expected_info_num, writer_tx.clone()).await;

    info!("Sent reject for frame {}", expected_info_num);
    false
}

/// Handles the P frame.
///
/// Send an acknowledgment for the P frame telling the sender that the receiver is ready to
/// receive.
pub async fn handle_p(writer_tx: &Sender<Vec<u8>>, expected_info_num: &u8) -> bool {
    // Send an acknowledgment for the P frame
    let ack_frame = Frame::new(FrameType::ReceiveReady, *expected_info_num, Vec::new()).to_bytes();
    writer_tx
        .send(ack_frame)
        .await
        .expect("Failed to send acknowledgment frame");

    info!("Received P frame");
    info!("Sent RR {}", *expected_info_num);

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameType};
    use crate::window::{SafeCond, SafeWindow, Window};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_handle_connection_end() {
        let window = SafeWindow::default();
        let (tx, mut rx) = mpsc::channel(10);
        let cond = SafeCond::default();

        let should_terminate = handle_connection_end(&window, &tx, &cond).await;

        assert!(should_terminate);
        assert!(rx.try_recv().is_ok(), "Acknowledgment frame not sent");
    }

    #[tokio::test]
    async fn test_handle_receive_ready() {
        let window = SafeWindow::default();
        let cond = SafeCond::default();

        let frame = Frame::new(FrameType::ReceiveReady, 1, vec![]);
        let should_continue = handle_receive_ready(&window, &frame, &cond);

        assert!(!should_continue);
    }

    #[tokio::test]
    async fn test_handle_reject() {
        let window = SafeWindow::default();
        let cond = SafeCond::default();
        let (tx, mut rx) = mpsc::channel(10);

        let frame = Frame::new(FrameType::Reject, 1, vec![]);
        let should_continue = handle_reject(&window, &frame, &tx, &cond).await;

        assert!(!should_continue);
        assert!(rx.try_recv().is_ok(), "Rejection frame not resent");
    }
}


