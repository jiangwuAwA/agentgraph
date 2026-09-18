//! Web package: comment-only mention of normalize_id — no call edge.
// Noise: docs say "always call normalize_id before display" but this package
// does not import or call core::normalize_id.
pub fn render_title(name: &str) -> String {
    format!("<h1>{name}</h1>")
}
