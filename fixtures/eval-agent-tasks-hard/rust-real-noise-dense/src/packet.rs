//! Packet Encode trait — dense implementors (real high-frequency shape).
pub trait Encode {
    fn encode(&self) -> Vec<u8>;
}

pub struct Header;
impl Encode for Header {
    fn encode(&self) -> Vec<u8> {
        b"HDR".to_vec()
    }
}

pub struct Body;
impl Encode for Body {
    fn encode(&self) -> Vec<u8> {
        b"BDY".to_vec()
    }
}

pub struct Footer;
impl Encode for Footer {
    fn encode(&self) -> Vec<u8> {
        b"FTR".to_vec()
    }
}

pub struct Ack;
impl Encode for Ack {
    fn encode(&self) -> Vec<u8> {
        b"ACK".to_vec()
    }
}

pub struct Nack;
impl Encode for Nack {
    fn encode(&self) -> Vec<u8> {
        b"NACK".to_vec()
    }
}

pub struct Ping;
impl Encode for Ping {
    fn encode(&self) -> Vec<u8> {
        b"PING".to_vec()
    }
}

pub struct Pong;
impl Encode for Pong {
    fn encode(&self) -> Vec<u8> {
        b"PONG".to_vec()
    }
}

pub struct Meta;
impl Encode for Meta {
    fn encode(&self) -> Vec<u8> {
        b"META".to_vec()
    }
}

pub struct Chunk0;
impl Encode for Chunk0 {
    fn encode(&self) -> Vec<u8> {
        vec![0]
    }
}

pub struct Chunk1;
impl Encode for Chunk1 {
    fn encode(&self) -> Vec<u8> {
        vec![1]
    }
}

pub struct Chunk2;
impl Encode for Chunk2 {
    fn encode(&self) -> Vec<u8> {
        vec![2]
    }
}

pub struct Chunk3;
impl Encode for Chunk3 {
    fn encode(&self) -> Vec<u8> {
        vec![3]
    }
}
