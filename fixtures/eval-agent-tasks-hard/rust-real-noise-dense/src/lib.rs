//! Dense real-noise crate: trait Encode + implementors + callers + name collisions.
pub mod packet;
pub mod wire;
pub mod metrics;
pub mod legacy;
pub mod clone_heavy;

pub use packet::Encode;
pub use wire::send_packet;
