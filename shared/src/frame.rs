#[derive(Debug)]
pub struct Frame {
    pub flag: u8,
    pub frame_type: u8,
    pub num: u8,
    pub data: Vec<u8>,
    pub crc: u16, // le crc est sur 2 octets
}

impl Frame {
    pub fn new(frame_type: u8, num: u8, data: Vec<u8>, crc: u16) -> Frame {
        Frame {
            flag: 0x7E, // 01111110
            frame_type,
            num,
            data,
            crc,
        }
    }
}
