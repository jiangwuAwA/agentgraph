/// Clean package implementation of batch_write.
pub fn batch_write(n: usize) -> usize {
    n.saturating_mul(2)
}

pub fn batch_write_verbose(n: usize) -> String {
    format!("wrote {}", batch_write(n))
}
