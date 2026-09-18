//! Real-idiom multi-file: linkme distributed_slice registry (M3-E).

use linkme::distributed_slice;

pub struct PluginRegistration {
    pub name: &'static str,
    pub factory: fn() -> Box<dyn Fn() -> i32>,
}

pub struct MetricsPlugin;

impl MetricsPlugin {
    pub fn new() -> Box<dyn Fn() -> i32> {
        Box::new(|| 42)
    }
}

#[distributed_slice(PLUGINS)]
static METRICS: PluginRegistration = PluginRegistration {
    name: "metrics",
    factory: MetricsPlugin::new,
};
