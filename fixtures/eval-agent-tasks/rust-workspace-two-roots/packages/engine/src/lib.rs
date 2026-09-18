pub mod compute;

pub fn run_batch(xs: &[i32]) -> i32 {
    compute::compute(xs)
}
