/// Canonical id normalizer used across package roots.
pub fn normalize_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}
