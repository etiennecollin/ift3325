use crate::frame::Frame;

/// Perform byte stuffing on the given frame bytes.
/// This is done to avoid the byte being interpreted as a flag.
/// How it works:
/// - If a byte is equal to a flag, the escape flag is added before the byte and the byte has its 5th bit flipped.
pub fn byte_stuffing(frame_bytes: &[u8]) -> Vec<u8> {
    let mut stuffed_frame: Vec<u8> = Vec::new();

    frame_bytes.iter().for_each(|byte| {
        if *byte == Frame::BOUNDARY_FLAG || *byte == Frame::ESCAPE_FLAG {
            stuffed_frame.push(Frame::ESCAPE_FLAG);
            stuffed_frame.push(*byte ^ Frame::REPLACEMENT_POSITION);
        } else {
            stuffed_frame.push(*byte);
        }
    });
    stuffed_frame
}

/// Destuff the given frame bytes.
/// This function removes the byte stuffing from the given frame bytes.
///
/// # Errors
/// - If an abort sequence is detected
pub fn byte_destuffing(frame_bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut destuffed_frame: Vec<u8> = Vec::new();

    let mut escape: bool = false;
    for byte in frame_bytes {
        if escape {
            if *byte == Frame::BOUNDARY_FLAG {
                return Err("Invalid frame, abort sequence received");
            }
            destuffed_frame.push(*byte ^ Frame::REPLACEMENT_POSITION);
            escape = false;
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
                Frame::ESCAPE_FLAG,
                Frame::BOUNDARY_FLAG ^ Frame::REPLACEMENT_POSITION,
                Frame::ESCAPE_FLAG,
                Frame::BOUNDARY_FLAG ^ Frame::REPLACEMENT_POSITION
            ]
        );
        let destuffed_data = byte_destuffing(&stuffed_data).unwrap();
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
        assert_eq!(stuffed_data, data);
        let destuffed_data = byte_destuffing(&stuffed_data).unwrap();
        assert_eq!(destuffed_data, data);
    }
}
