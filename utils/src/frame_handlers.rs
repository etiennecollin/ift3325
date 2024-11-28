//! Frame handlers for the receiver.
//!
//! Each handler is responsible for handling a specific frame type and
//! returning a boolean indicating if the connection should be terminated.

use crate::{
    frame::{Frame, FrameType},
    window::{SafeCond, SafeWindow, Window},
};
use log::{debug, info, warn};
use tokio::sync::mpsc::Sender;

/// Handles the receive ready frames.
///
/// Pop the acknowledged frames from the window.
pub fn handle_receive_ready(safe_window: SafeWindow, frame: &Frame, condition: SafeCond) -> bool {
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

    info!("Received RR {}", frame.num);
    false
}

/// Handles the connection end frames.
///
/// If we are not the ones initiating the disconnection, we should send an
/// acknowledgment. Else, we should pop the connection request frame
/// from our window as it was acknowledged.
pub async fn handle_connection_end(
    safe_window: SafeWindow,
    writer_tx: Sender<Vec<u8>>,
    condition: SafeCond,
) -> bool {
    let sent_disconnect_request;
    {
        let mut window = safe_window.lock().expect("Failed to lock window");
        window.is_connected = false;
        sent_disconnect_request = window.sent_disconnect_request;
        info!("Received connection end frame");

        // If we sent the request, we should pop the connection request frame
        // from our window as it was acknowledged.
        if sent_disconnect_request {
            window.pop_front(condition);
        }
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
pub async fn handle_connection_start(
    safe_window: SafeWindow,
    frame: &Frame,
    writer_tx: Sender<Vec<u8>>,
    condition: SafeCond,
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
            window.pop_front(condition);
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
pub async fn handle_reject(
    safe_window: SafeWindow,
    frame: &Frame,
    writer_tx: Sender<Vec<u8>>,
    condition: SafeCond,
) -> bool {
    let frames: Vec<(u8, Vec<u8>)>;
    {
        let mut window = safe_window.lock().expect("Failed to lock window");

        // Ignore the frame if the connection is not established
        if !window.is_connected {
            debug!(
                "Received REJ for frame {} but connection is not established",
                frame.num
            );
            return false;
        }
        let srej = window.srej;

        // Pop the implicitly acknowledged frames from the window
        let popped_num = window.pop_until(
            (frame.num + (Window::MAX_FRAME_NUM - 1)) % Window::MAX_FRAME_NUM,
            true,
            condition,
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
        //FIXME: Seems like there is a deadlock here. I never reach the debug
        // statement after the loop. It also prevents the timers from sending
        // data to the writer.
        writer_tx.send(frame).await.expect("Failed to resend frame");
    }

    debug!("Resent reject frames");

    false
}

/// Handles the I frame.
///
/// The receiver sends an acknowledgment frame for the information frame and sends the data to the
/// assembler.
pub async fn handle_information(
    safe_window: SafeWindow,
    frame: Frame,
    writer_tx: Sender<Vec<u8>>,
    condition: SafeCond,
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
        return handle_dropped_frame(&frame, safe_window, writer_tx, *expected_info_num).await;
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
    safe_window: SafeWindow,
    writer_tx: Sender<Vec<u8>>,
    expected_info_num: u8,
) -> bool {
    warn!(
        "Dropped frame detected - Received: {}, Expected: {}",
        frame.num, expected_info_num
    );

    {
        // If window is full, ignore frame
        let window = safe_window.lock().expect("Failed to lock window");
        if window.is_full() || window.contains(expected_info_num) {
            debug!("Window is full or contains frame, ignoring");
            return false;
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
        .push(rej_frame, writer_tx.clone())
        .expect("Window is full, this should probably never happen");

    false
}

/// Handles the P frame.
///
/// Send an acknowledgment for the P frame telling the sender that the receiver is ready to
/// receive.
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
