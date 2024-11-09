use crate::{
    byte_stuffing::{byte_destuffing, byte_stuffing},
    crc::crc_16_ccitt,
};

#[derive(Debug)]
/// A frame for the HDLC protocol.
pub struct Frame {
    /// The address of the frame
    pub address: u8,

    /// The control byte of the frame
    pub control: u8,

    /// The data of the frame
    pub data: Vec<u8>,

    /// The CRC-16 checksum stored in native endianess
    fcs: Option<u16>,

    /// The raw bytes of the frame
    /// The frame is encoded as follows:
    /// - 1 byte: address
    /// - 1 byte: control
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

    /// Create a new frame with the given address, control, and data.
    pub fn new(address: u8, control: u8, data: Vec<u8>) -> Frame {
        let mut frame = Frame {
            address,
            control,
            data,
            fcs: None,
            content: None,
            content_stuffed: None,
        };
        frame.generate_content();
        frame
    }

    /// Initialize the content fields of the frame.
    /// The following fields are initialized:
    /// - The frame content
    /// - The bit-stuffed version of the frame content
    ///
    /// The frame content is encoded as follows:
    /// - 1 byte: boundary flag
    /// - Content
    ///     - 1 byte: address
    ///     - 1 byte: control
    ///     - n bytes: data
    ///     - 2 bytes: fcs stored as big-endian
    /// - 1 byte: boundary flag
    ///
    /// The size of the content may vary due to byte stuffing.
    fn generate_content(&mut self) {
        // Create the frame content
        let mut frame_content = vec![self.address, self.control];
        frame_content.append(&mut self.data.clone());

        // Compute the FCS and store it
        let fcs = crc_16_ccitt(&frame_content);
        self.fcs = Some(fcs);

        // Append the FCS to the frame content and store it
        frame_content.append(&mut fcs.to_be_bytes().to_vec());
        self.content = Some(frame_content);

        // Byte-stuff the frame content and store it
        let mut bytes = vec![Frame::BOUNDARY_FLAG];
        bytes.append(&mut byte_stuffing(
            self.content
                .as_ref()
                .expect("The frame content is not set, this should never happen"),
        ));
        bytes.push(Frame::BOUNDARY_FLAG);
        self.content_stuffed = Some(bytes);
    }

    /// Convert the frame to a vector of bytes encoded with byte stuffing.
    /// The frame is encoded as follows:
    /// - 1 byte: boundary flag
    /// - Content
    ///     - 1 byte: address
    ///     - 1 byte: control
    ///     - n bytes: data
    ///     - 2 bytes: fcs stored as big-endian
    /// - 1 byte: boundary flag
    ///
    /// The size of the content may vary due to byte stuffing.
    pub fn get_bytes(&mut self) -> Vec<u8> {
        self.content_stuffed
            .as_ref()
            .expect("The stuffed frame content is not set, this should never happen")
            .clone()
    }

    /// Create a new frame from the given bytes.
    /// The frame is decoded as follows:
    /// - 1 byte: boundary flag
    /// - Content
    ///    - 1 byte: address
    ///    - 1 byte: control
    ///    - n bytes: data
    ///    - 2 bytes: fcs stored as big-endian
    /// - 1 byte: boundary flag
    ///
    /// # Errors
    /// - If the frame is too short
    /// - If the frame does not start or end with a boundary flag
    /// - If a CRC error is detected
    /// - If a byte stuffing error is detected
    pub fn new_from_bytes(bytes: &[u8]) -> Result<Frame, &'static str> {
        // The frame should contain at least 6 bytes: 2 boundary flags, 1 address, 1 control, 2 FCS
        if bytes.len() < 6 {
            return Err("Invalid frame: too short");
        }

        // The frame should start and end with a boundary flag
        if bytes[0] != Frame::BOUNDARY_FLAG || bytes[bytes.len() - 1] != Frame::BOUNDARY_FLAG {
            return Err("Invalid frame: missing boundary flag");
        }

        // Destuff the frame content
        let content = byte_destuffing(&bytes[1..bytes.len() - 1])?;

        // Extract the frame information
        let address = content[0];
        let control = content[1];
        let data = content[2..content.len() - 2].to_vec();
        let fcs = u16::from_be_bytes([content[content.len() - 2], content[content.len() - 1]]);

        // Check that the FCS matches the CRC
        let mut frame_content = vec![address, control];
        frame_content.append(&mut data.clone());
        let expected_fcs = crc_16_ccitt(&frame_content);

        if fcs != expected_fcs {
            return Err("Invalid frame: a CRC error was detected");
        }

        // Create the frame content
        let mut frame = Frame::new(address, control, data);
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
    fn frame_get_bytes_structure_test() {
        let mut frame = Frame::new(0x01, 0x02, vec![0x03, 0x04]);
        let bytes = frame.get_bytes();

        // Set the expected values
        let expected_fcs = 0x0D03u16;
        let mut expected_bytes = vec![Frame::BOUNDARY_FLAG, 0x01, 0x02, 0x03, 0x04];
        expected_bytes.append(&mut expected_fcs.to_be_bytes().to_vec());
        expected_bytes.push(Frame::BOUNDARY_FLAG);

        // Check that the bytes are correct
        assert_eq!(frame.fcs.unwrap(), expected_fcs);
        assert_eq!(
            frame.content.unwrap(),
            expected_bytes[1..expected_bytes.len() - 1].to_vec()
        );
        assert_eq!(bytes, expected_bytes);
    }

    #[test]
    fn bytes_to_frame_conversion_test() {
        let bytes: &[u8] = &[0x7E, 0x01, 0x02, 0x03, 0x04, 0x0D, 0x03, 0x7E];
        let frame = Frame::new_from_bytes(bytes);

        // Check that the frame is correct
        assert!(frame.is_ok());
        let frame = frame.unwrap();
        assert_eq!(frame.address, 0x01);
        assert_eq!(frame.control, 0x02);
        assert_eq!(frame.data, vec![0x03, 0x04]);
        assert_eq!(frame.fcs.unwrap(), 0x0D03);
        assert_eq!(
            frame.content.unwrap(),
            vec![0x01, 0x02, 0x03, 0x04, 0x0D, 0x03]
        );
        assert_eq!(frame.content_stuffed.unwrap(), bytes);
    }

    #[test]
    fn frame_empty_data_test() {
        let mut frame = Frame::new(0x01, 0x02, vec![]);
        let bytes = frame.get_bytes();

        // Set the expected values
        let expected_fcs = 0x1373u16;
        let mut expected_bytes = vec![Frame::BOUNDARY_FLAG, 0x01, 0x02];
        expected_bytes.append(&mut expected_fcs.to_be_bytes().to_vec());
        expected_bytes.push(Frame::BOUNDARY_FLAG);

        // Check that the bytes are correct
        assert_eq!(frame.fcs.unwrap(), expected_fcs);
        assert_eq!(
            frame.content.unwrap(),
            expected_bytes[1..expected_bytes.len() - 1].to_vec()
        );
        assert_eq!(bytes, expected_bytes);

        // Decode the frame from the bytes
        let frame = Frame::new_from_bytes(&bytes);

        // Check that the frame is correct
        assert!(frame.is_ok());
        let frame = frame.unwrap();
        assert_eq!(frame.address, 0x01);
        assert_eq!(frame.control, 0x02);
        assert_eq!(frame.data, vec![]);
        assert_eq!(frame.fcs.unwrap(), 0x1373);
        assert_eq!(frame.content.unwrap(), vec![0x01, 0x02, 0x13, 0x73]);
        assert_eq!(frame.content_stuffed.unwrap(), bytes);
    }
}
