pub mod id;

pub use id::normalize_id;

pub fn batch_key(raw: &str) -> String {
    normalize_id(raw)
}
