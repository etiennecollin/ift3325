#[derive(Debug)]
pub struct Frame {
    pub address: u8,
    pub control: u8,
    pub data: Vec<u8>,
    pub fcs: u16, // le crc est sur 2 octets
}

impl Frame {
    pub const FLAG: u8 = 0x7E;

    pub fn new(address: u8, control: u8, data: Vec<u8>, fcs: u16) -> Frame {
        Frame {
            address,
            control,
            data,
            fcs,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![Frame::FLAG, self.address, self.control];
        bytes.append(&mut self.data.clone());
        bytes.append(&mut self.fcs.to_be_bytes().to_vec());
        bytes.push(Frame::FLAG);
        bytes
    }

    pub fn new_from_bytes(bytes: Vec<u8>) -> Frame {
        let address = bytes[1];
        let control = bytes[2];
        let data = bytes[3..bytes.len() - 3].to_vec();
        let fcs = u16::from_be_bytes([bytes[bytes.len() - 3], bytes[bytes.len() - 2]]);

        Frame::new(address, control, data, fcs)
    }
}
