//! Dirty sibling: unsafe leaves S (sound-disabled union).
pub fn unsafe_cast(x: u32) -> i32 {
    unsafe { std::mem::transmute(x) }
}

pub fn legacy_touch() -> i32 {
    unsafe_cast(1)
}
