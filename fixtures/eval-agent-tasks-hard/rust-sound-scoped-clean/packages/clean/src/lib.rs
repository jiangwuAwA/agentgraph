pub mod write;

pub use write::batch_write;

pub fn flush_all(n: usize) -> usize {
    batch_write(n)
}
