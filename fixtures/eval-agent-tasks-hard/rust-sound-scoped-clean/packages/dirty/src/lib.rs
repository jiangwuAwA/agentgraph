//! Dirty sibling: unsafe leaves S — union sound must stay off.
pub mod shadow;

pub fn dirty_entry(x: u32) -> i32 {
    unsafe { std::mem::transmute(x) }
}

// Name collision comment: "batch_write lives here too" — it does not.
// Noise: batch_write appears only in comments / unrelated helper names.
pub fn batch_write_shadow(n: usize) -> usize {
    shadow::shadow_write(n)
}
