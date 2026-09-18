//! Noise: legacy encode helpers + comments — not a Packet Encode implementor.
pub fn encode_legacy(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF];
    out.extend_from_slice(data);
    out
}

// Noise comment: old encode path used by nothing in packet/wire.
// Mentions encode many times: encode encode encode.
pub fn note() -> &'static str {
    "encode was different before"
}
