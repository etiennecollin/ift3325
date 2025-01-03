//! Provides functions to perform byte stuffing on frame bytes.
//!
//! Byte stuffing is done to avoid the byte being interpreted as a flag.
//!
//! How it works:
//! - If a byte is equal to a flag, the escape flag is added before the byte
//!   and the byte has its 5th bit flipped.
//! - Boundary flags are added at the start and end of the frame.

use crate::frame::{Frame, FrameError};

/// Perform byte stuffing on the given frame bytes.
///
/// This is done to avoid the byte being interpreted as a flag.
/// How it works:
/// - If a byte is equal to a flag, the escape flag is added before the byte
///   and the byte has its 5th bit flipped.
/// - Boundary flags are added at the start and end of the frame.
///
/// # Arguments
/// - `frame_bytes`: The frame bytes to stuff.
///
/// # Returns
/// The stuffed frame bytes.
pub fn byte_stuffing(frame_bytes: &[u8]) -> Vec<u8> {
    let mut stuffed_frame: Vec<u8> = Vec::new();

    stuffed_frame.push(Frame::BOUNDARY_FLAG);
    frame_bytes.iter().for_each(|byte| {
        if *byte == Frame::BOUNDARY_FLAG || *byte == Frame::ESCAPE_FLAG {
            stuffed_frame.push(Frame::ESCAPE_FLAG);
            stuffed_frame.push(*byte ^ Frame::REPLACEMENT_POSITION);
        } else {
            stuffed_frame.push(*byte);
        }
    });
    stuffed_frame.push(Frame::BOUNDARY_FLAG);

    stuffed_frame
}

/// Destuff the given frame bytes.
///
/// This function removes the byte stuffing from the given frame bytes and
/// returns the original frame bytes.
///
/// # Arguments
/// - `frame_bytes`: The frame bytes to destuff.
///
/// # Returns
/// The destuffed frame bytes or an error if an abort sequence is received.
pub fn byte_destuffing(frame_bytes: &[u8]) -> Result<Vec<u8>, FrameError> {
    let mut destuffed_frame: Vec<u8> = Vec::with_capacity(Frame::MAX_SIZE);

    let mut escape: bool = false;
    for byte in frame_bytes {
        // Handle the escape flag
        if escape {
            match *byte {
                // Check if the byte is a boundary flag and if so, abort the sequence
                Frame::BOUNDARY_FLAG => return Err(FrameError::AbortSequenceReceived),
                _ => {
                    // Add the byte to the frame and flip the 5th bit
                    destuffed_frame.push(*byte ^ Frame::REPLACEMENT_POSITION);
                    escape = false;
                }
            }
        } else {
            match *byte {
                Frame::ESCAPE_FLAG => escape = true,
                _ => destuffed_frame.push(*byte),
            }
        }
    }

    Ok(destuffed_frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_stuffing_escape_test() {
        let data = [Frame::BOUNDARY_FLAG; 2];
        let stuffed_data = byte_stuffing(&data);
        assert_eq!(
            stuffed_data,
            [
                Frame::BOUNDARY_FLAG,
                Frame::ESCAPE_FLAG,
                Frame::BOUNDARY_FLAG ^ Frame::REPLACEMENT_POSITION,
                Frame::ESCAPE_FLAG,
                Frame::BOUNDARY_FLAG ^ Frame::REPLACEMENT_POSITION,
                Frame::BOUNDARY_FLAG,
            ]
        );
        let destuffed_data = byte_destuffing(&stuffed_data[1..stuffed_data.len() - 1]).unwrap();
        assert_eq!(destuffed_data, data);
    }

    #[test]
    fn bit_stuffing_abort_test() {
        let stuffed_data = [Frame::ESCAPE_FLAG, Frame::BOUNDARY_FLAG];
        let destuffed_data = byte_destuffing(&stuffed_data);
        assert!(destuffed_data.is_err());
    }

    #[test]
    fn bit_stuffing_simple_test() {
        let data = [0xFF];
        let stuffed_data = byte_stuffing(&data);
        let expected_stuffed_data = [Frame::BOUNDARY_FLAG, 0xFF, Frame::BOUNDARY_FLAG];
        assert_eq!(stuffed_data, expected_stuffed_data);
        let destuffed_data = byte_destuffing(&stuffed_data[1..stuffed_data.len() - 1]).unwrap();
        assert_eq!(destuffed_data, data);
    }

    #[test]
    fn byte_stuffing_complex_test() {
        let data = [
            Frame::BOUNDARY_FLAG,
            Frame::ESCAPE_FLAG,
            0xFF,
            0x00,
            0x01,
            Frame::ESCAPE_FLAG,
            Frame::BOUNDARY_FLAG,
        ];
        let stuffed_data = byte_stuffing(&data);
        let destuffed_data = byte_destuffing(&stuffed_data[1..stuffed_data.len() - 1]).unwrap();
        assert_eq!(destuffed_data, data);
    }

    #[test]
    fn byte_stuffing_empty_test() {
        let data = [];
        let stuffed_data = byte_stuffing(&data);
        let destuffed_data = byte_destuffing(&stuffed_data[1..stuffed_data.len() - 1]).unwrap();
        assert_eq!(destuffed_data, data);
    }
}
