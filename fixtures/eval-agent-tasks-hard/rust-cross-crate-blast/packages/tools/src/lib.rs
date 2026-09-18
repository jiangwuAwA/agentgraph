//! Unrelated package: local normalize_id name collision (not a core consumer).
pub mod audit;

pub fn local_normalize_id(s: &str) -> String {
    s.replace('_', "-")
}

/// Name collision with core::normalize_id — different semantics, no dependency.
pub fn normalize_id(s: &str) -> String {
    local_normalize_id(s)
}

pub fn audit_label(s: &str) -> String {
    audit::label(normalize_id(s))
}
