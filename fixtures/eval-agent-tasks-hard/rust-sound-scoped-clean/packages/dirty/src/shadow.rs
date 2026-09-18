//! Unrelated dirty helpers — no true batch_write definition/call edge.
pub fn shadow_write(n: usize) -> usize {
    n.wrapping_add(1)
}

// Noise comment mentioning batch_write for greps.
pub fn note() -> &'static str {
    "batch_write is not implemented in dirty"
}
