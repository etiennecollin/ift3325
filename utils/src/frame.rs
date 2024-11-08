#[derive(Debug)]
pub struct Frame {
    pub frame_type: u8,
    pub num: u8,
    pub data: Vec<u8>,
    pub crc: u16, // le crc est sur 2 octets
}

impl Frame {
    pub const FLAG: u8 = 0x7E;

    pub fn new(frame_type: u8, num: u8, data: Vec<u8>, crc: u16) -> Frame {
        Frame {
            frame_type,
            num,
            data,
            crc,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![Frame::FLAG, self.frame_type, self.num];
        bytes.append(&mut self.data.clone());
        bytes.append(&mut self.crc.to_be_bytes().to_vec());
        bytes.push(Frame::FLAG);
        bytes
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Frame {
        let frame_type = bytes[1];
        let num = bytes[2];
        let data = bytes[3..bytes.len() - 3].to_vec();
        let crc = u16::from_be_bytes([bytes[bytes.len() - 3], bytes[bytes.len() - 2]]);

        Frame::new(frame_type, num, data, crc)
    }
}
