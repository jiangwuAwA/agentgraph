//! API routes depend on core::normalize_id via the package boundary.
pub fn lookup(raw: &str) -> String {
    // Cross-crate call: changing normalize_id contract affects this path.
    let key = core::normalize_id(raw);
    format!("route:{key}")
}
