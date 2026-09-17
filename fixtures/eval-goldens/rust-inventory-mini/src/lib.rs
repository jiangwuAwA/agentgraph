//! Synthetic Rust inventory-shaped registry (public domain shape).
//! Not derived from any private trading / monorepo source.
//!
//! Expected L1 rule: `rs.di.inventory_submit` (Heuristic registration/factory
//! candidates). Not sound; not a complete runtime graph.

/// Registration record stored in the inventory registry.
pub struct StrategyRegistration {
    pub name: &'static str,
    pub factory: fn() -> Box<dyn Send>,
}

/// Minimal strategy stand-in for factory-type golden edges.
pub struct DemoStrategy;

impl DemoStrategy {
    pub fn new(_name: &str) -> Self {
        Self
    }
}

pub trait Strategy {
    fn name(&self) -> &'static str;
}

impl Strategy for DemoStrategy {
    fn name(&self) -> &'static str {
        "demo"
    }
}

inventory::submit! {
    StrategyRegistration {
        name: "demo",
        factory: || Box::new(DemoStrategy::new("demo")),
    }
}

/// Exact call site: `registry` fan-in via trait method.
pub fn run_registered(s: &dyn Strategy) -> &'static str {
    s.name()
}

pub fn bootstrap() -> &'static str {
    run_registered(&DemoStrategy::new("demo"))
}
