//! Entry for linkme plugins corpus. Slice decl + call site.

pub mod registry;

pub static PLUGINS: [registry::PluginRegistration];

pub fn run_metrics() -> i32 {
    let f = registry::MetricsPlugin::new;
    f()()
}
