//! Noise: unrelated name collisions with encode / Encode — not packet consumers.
/// Label formatter — string helper, not Encode trait.
pub fn encode_label(name: &str) -> String {
    format!("m={name}")
}

/// Unrelated enum sharing the Encode type name.
pub enum Encode {
    Counter,
    Gauge,
}

pub fn encode_metric(e: Encode) -> &'static str {
    match e {
        Encode::Counter => "counter",
        Encode::Gauge => "gauge",
    }
}

// Comment noise: "we should encode metrics like packets" — no dependency.
