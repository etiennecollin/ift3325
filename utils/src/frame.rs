//! The frame module contains the Frame struct, the FrameType enum and the
//! FrameError enum.
//!
//! The Frame struct represents a frame in the HDLC protocol.
//! The FrameType enum represents the type of a frame.
//! The FrameError enum represents an error that may occur when working with frames.

use crate::{
    byte_stuffing::{byte_destuffing, byte_stuffing},
    crc::{crc_16_ccitt, PolynomialSize},
};

/// An error that may occur when working with frames.
/// The following errors are possible:
/// - InvalidFrameType: The frame type is invalid
/// - InvalidFCS: The FCS does not match the CRC
/// - InvalidLength: The frame is too short
/// - MissingBoundaryFlag: The frame does not start and end with a boundary flag
/// - AbortSequenceReceived: An abort sequence was received during byte destuffing
/// - DestuffingError: An error occurred during byte destuffing
#[derive(Debug)]
pub enum FrameError {
    InvalidFrameType(u8),
    InvalidFCS,
    InvalidLength(usize),
    MissingBoundaryFlag,
    AbortSequenceReceived,
    DestuffingError,
}

/// The type of a frame.
/// The frame type is encoded as a single byte.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FrameType {
    /// Contains data to be transmitted
    Information = b'I',
    /// Indicates that the sender wants to establish a connection
    ConnectionStart = b'C',
    /// Indicates that the sender is ready to receive more data
    ReceiveReady = b'A',
    /// Requests immediate retransmission of data
    Reject = b'R',
    /// Indicates that the sender has finished sending data
    ConnectionEnd = b'F',
    /// Forces the receiver to send a response
    P = b'P',
    /// Unknown frame type
    Unknown = 0,
}

/// Convert a byte to a frame type.
impl From<u8> for FrameType {
    fn from(byte: u8) -> Self {
        match byte {
            b'I' => FrameType::Information,
            b'C' => FrameType::ConnectionStart,
            b'A' => FrameType::ReceiveReady,
            b'R' => FrameType::Reject,
            b'F' => FrameType::ConnectionEnd,
            b'P' => FrameType::P,
            _ => FrameType::Unknown,
        }
    }
}

/// Convert a frame type to a byte.
impl From<FrameType> for u8 {
    fn from(frame_type: FrameType) -> Self {
        frame_type as u8
    }
}

/// A frame for the HDLC protocol.
#[derive(Debug)]
pub struct Frame {
    /// The frame_type of the frame
    pub frame_type: u8,

    /// The num byte of the frame
    pub num: u8,

    /// The data of the frame
    pub data: Vec<u8>,

    /// The CRC-16 checksum stored in native endianess
    fcs: Option<PolynomialSize>,

    /// The raw bytes of the frame before byte stuffing
    /// The frame is encoded as follows:
    /// - 1 byte: frame_type
    /// - 1 byte: num
    /// - n bytes: data
    /// - 2 bytes: fcs stored as big-endian
    content: Option<Vec<u8>>,

    /// The byte-stuffed version of the frame content
    content_stuffed: Option<Vec<u8>>,
}

impl Frame {
    /// Byte stuffing boundary flag
    pub const BOUNDARY_FLAG: u8 = 0x7E;
    /// Byte stuffing escape flag
    pub const ESCAPE_FLAG: u8 = 0x7D;
    /// Replace an escaped byte by computing the XOR between the byte and this byte
    pub const REPLACEMENT_POSITION: u8 = 0x20;
    /// The maximum size of the data field in bytes
    // pub const MAX_SIZE_DATA: usize = 1024; // 1kB
    pub const MAX_SIZE_DATA: usize = 64 * 1024; // 64kB

    /// The size of a frame in bytes
    ///
    /// The frame is encoded as follows:
    /// - 1 byte: boundary flag
    /// - Content (2x since each byte could be stuffed and escaped)
    ///     - 1 byte: frame_type
    ///     - 1 byte: num
    ///     - n bytes: data
    ///     - 2 bytes: fcs stored as big-endian
    /// - 1 byte: boundary flag
    pub const MAX_SIZE: usize = 2 + 2 * (4 + Self::MAX_SIZE_DATA);

    /// Create a new frame
    /// # Parameters
    /// - `frame_type`: The type of the frame
    /// - `num`: The number identifying the frame or associated with a ReceiveReady or Reject frame
    /// - `data`: The data of the frame. It may be empty
    pub fn new(frame_type: FrameType, num: u8, data: Vec<u8>) -> Frame {
        let mut frame = Frame {
            frame_type: frame_type as u8,
            num,
            data,
            fcs: None,
            content: None,
            content_stuffed: None,
        };
        frame.generate_content();
        frame
    }

    /// Initialize the content fields of the frame.
    ///
    /// The following fields are initialized:
    /// - The frame content
    /// - The bit-stuffed version of the frame content
    ///
    /// The frame content is encoded as follows:
    /// - 1 byte: boundary flag
    /// - Content
    ///     - 1 byte: frame_type
    ///     - 1 byte: num
    ///     - n bytes: data
    ///     - 2 bytes: fcs stored as big-endian
    /// - 1 byte: boundary flag
    ///
    /// The size of the content may vary due to byte stuffing.
    ///
    /// # Panics
    /// This function panics if the frame content is not set.
    /// This should never happen since the content is set during the frame
    /// creation.
    fn generate_content(&mut self) {
        // Create the frame content
        let mut frame_content = vec![self.frame_type, self.num];
        frame_content.extend_from_slice(&self.data);

        // Compute the FCS and store it
        let fcs = crc_16_ccitt(&frame_content);
        self.fcs = Some(fcs);

        // Append the FCS to the frame content and store it
        frame_content.extend_from_slice(&fcs.to_be_bytes());

        // Store content and stuffed content
        self.content = Some(frame_content);
        self.content_stuffed =
            Some(byte_stuffing(self.content.as_ref().expect(
                "The frame content is not set, this should never happen",
            )));
    }

    /// Convert the frame to a vector of bytes encoded with byte stuffing.
    ///
    /// The frame is encoded as follows:
    /// - 1 byte: boundary flag
    /// - Content
    ///     - 1 byte: frame_type
    ///     - 1 byte: num
    ///     - n bytes: data
    ///     - 2 bytes: fcs stored as big-endian
    /// - 1 byte: boundary flag
    ///
    /// The size of the content may vary due to byte stuffing.
    ///
    /// # Panics
    /// This function panics if the frame content is not set.
    /// This should never happen since the content is set during the frame
    /// creation.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.content_stuffed
            .as_ref()
            .expect("The stuffed frame content is not set, this should never happen")
            .clone()
    }

    /// Create a new frame from the given bytes.
    ///
    /// The frame is decoded as follows:
    /// - Content
    ///    - 1 byte: frame_type
    ///    - 1 byte: num
    ///    - n bytes: data
    ///    - 2 bytes: fcs stored as big-endian
    pub fn from_bytes(bytes: &[u8]) -> Result<Frame, FrameError> {
        // The frame should contain at least 4 bytes: 1 frame_type, 1 num, 2 FCS
        if bytes.len() < 4 {
            return Err(FrameError::InvalidLength(bytes.len()));
        }

        // Destuff the frame content
        let content = byte_destuffing(bytes)?;

        // Check that the FCS matches the CRC
        let checksum = crc_16_ccitt(&content);
        if checksum != 0 {
            return Err(FrameError::InvalidFCS);
        }

        // Extract the frame information
        let frame_type = match FrameType::from(content[0]) {
            FrameType::Unknown => return Err(FrameError::InvalidFrameType(content[0])),
            frame_type => frame_type,
        };

        let num = content[1];
        let data = content[2..content.len() - 2].to_vec();
        let fcs = u16::from_be_bytes([content[content.len() - 2], content[content.len() - 1]]);

        // Create the frame content
        let mut frame = Frame::new(frame_type, num, data);
        frame.fcs = Some(fcs);
        frame.content = Some(content);
        frame.content_stuffed = Some(bytes.to_vec());

        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_abort_error_test() {
        let bytes: &[u8] = &[
            0x7E,
            FrameType::ConnectionStart.into(),
            0x02,
            0x03,
            0x04,
            0x8E,
            0xF7,
            Frame::ESCAPE_FLAG,
            Frame::BOUNDARY_FLAG,
            0x7E,
        ];
        let frame = Frame::from_bytes(bytes);
        assert!(frame.is_err());
    }

    #[test]
    fn frame_length_error_test() {
        let bytes: &[u8] = &[0xFF];
        let frame = Frame::from_bytes(bytes);
        assert!(frame.is_err());
    }

    #[test]
    fn frame_flags_error_test() {
        let bytes: &[u8] = &[
            FrameType::ConnectionStart.into(),
            0x02,
            0x03,
            0x04,
            0x8E,
            0xF7,
            0x7E,
        ];
        let frame = Frame::from_bytes(bytes);
        assert!(frame.is_err());

        let bytes: &[u8] = &[
            0x7E,
            FrameType::ConnectionStart.into(),
            0x02,
            0x03,
            0x04,
            0x8E,
            0xF7,
        ];
        let frame = Frame::from_bytes(bytes);
        assert!(frame.is_err());
    }

    #[test]
    fn frame_type_error_test() {
        let bytes: &[u8] = &[0x7E, 0x00, 0x02, 0x03, 0x04, 0x8E, 0xF7, 0x7E];
        let frame = Frame::from_bytes(bytes);
        assert!(frame.is_err());
    }

    #[test]
    fn frame_crc_error_test() {
        let bytes: &[u8] = &[
            0x7E,
            FrameType::ConnectionStart.into(),
            0x02,
            0x03,
            0x04,
            0x8E,
            0xFF, // Should be 0xF7
            0x7E,
        ];
        let frame = Frame::from_bytes(bytes);
        assert!(frame.is_err());
    }

    #[test]
    fn frame_get_bytes_structure_test() {
        let frame = Frame::new(FrameType::ConnectionStart, 0x02, vec![0x03, 0x04]);

        // Set the expected values
        let expected_fcs = 0x8EF7u16;
        let mut expected_content = vec![FrameType::ConnectionStart.into(), 0x02, 0x03, 0x04];
        expected_content.extend_from_slice(&expected_fcs.to_be_bytes());
        let expected_stuffed_content = byte_stuffing(&expected_content);

        // Check that the bytes are correct
        assert_eq!(frame.fcs.unwrap(), expected_fcs);
        assert_eq!(frame.content.unwrap(), expected_content);
        assert_eq!(frame.content_stuffed.unwrap(), expected_stuffed_content);
    }

    #[test]
    fn bytes_to_frame_conversion_test() {
        let bytes: &[u8] = &[
            FrameType::ConnectionStart.into(),
            0x00,
            0x03,
            0x04,
            0xE0,
            0x97,
        ];
        let frame = Frame::from_bytes(bytes);

        // Check that the frame is correct
        assert!(frame.is_ok());
        let frame = frame.unwrap();
        assert_eq!(frame.frame_type, bytes[0]);
        assert_eq!(frame.num, bytes[1]);
        assert_eq!(frame.data, bytes[2..bytes.len() - 2].to_vec());
        assert_eq!(frame.fcs.unwrap(), 0xE097);
        assert_eq!(frame.content.unwrap(), bytes.to_vec());
        assert_eq!(frame.content_stuffed.unwrap(), bytes);
    }

    #[test]
    fn frame_empty_data_test() {
        let frame = Frame::new(FrameType::ConnectionStart, 0x00, vec![]);
        let bytes = frame.to_bytes();

        // Set the expected values
        let expected_fcs = 0x589Fu16;
        let mut expected_bytes = vec![
            Frame::BOUNDARY_FLAG,
            FrameType::ConnectionStart.into(),
            0x00,
        ];
        expected_bytes.extend_from_slice(&expected_fcs.to_be_bytes());
        expected_bytes.push(Frame::BOUNDARY_FLAG);

        // Check that the bytes are correct
        assert_eq!(frame.fcs.unwrap(), expected_fcs);
        assert_eq!(
            frame.content.unwrap(),
            expected_bytes[1..expected_bytes.len() - 1].to_vec()
        );
        assert_eq!(bytes, expected_bytes);

        // Decode the frame from the bytes
        let frame = Frame::from_bytes(&bytes[1..expected_bytes.len() - 1]);

        // Check that the frame is correct
        assert!(frame.is_ok());
        let frame = frame.unwrap();
        assert_eq!(frame.frame_type, expected_bytes[1]);
        assert_eq!(frame.num, expected_bytes[2]);
        assert_eq!(frame.data, vec![]);
        assert_eq!(frame.fcs.unwrap(), expected_fcs);
        assert_eq!(
            frame.content.unwrap(),
            expected_bytes[1..expected_bytes.len() - 1]
        );
        assert_eq!(
            frame.content_stuffed.unwrap(),
            bytes[1..expected_bytes.len() - 1]
        );
    }
}
