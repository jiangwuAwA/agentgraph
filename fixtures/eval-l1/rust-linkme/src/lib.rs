//! M3-E: linkme `#[distributed_slice]` source registration + call site.
//! Heuristic `rs.di.linkme_distributed_slice`. Candidates only — not sound.
//! Not a sidecar/expand rule — pure source extract.

use linkme::distributed_slice;

pub struct StrategyRegistration {
    pub factory: fn() -> u32,
}

pub struct DemoStrategy;

impl DemoStrategy {
    pub fn new() -> u32 {
        1
    }
}

/// Declaration of the distributed slice (linkme registers here).
pub static STRATEGIES: [StrategyRegistration];

#[distributed_slice(STRATEGIES)]
static DEMO: StrategyRegistration = StrategyRegistration {
    factory: DemoStrategy::new,
};

/// Call site that consumes a registered strategy (Exact L0 call).
pub fn run_demo() -> u32 {
    DemoStrategy::new()
}
