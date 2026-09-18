//! Real call sites for Encode::encode.
use crate::packet::Encode;

pub fn send_packet(p: &dyn Encode) -> usize {
    p.encode().len()
}

pub fn dump_all(items: &[&dyn Encode]) -> usize {
    items.iter().map(|i| i.encode().len()).sum()
}

pub fn checksum(parts: &[&dyn Encode]) -> u8 {
    parts
        .iter()
        .flat_map(|p| p.encode())
        .fold(0u8, |a, b| a ^ b)
}
