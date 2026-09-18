pub mod pages;

// Noise: web package does not call engine::compute.
pub fn title() -> &'static str {
    "web"
}
