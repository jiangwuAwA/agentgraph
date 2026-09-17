//! TDD: Rust `inventory::submit!` registry registration → Heuristic edges.
//!
//! Real corpus pattern (stock-trading-app strategy-plugins):
//! `inventory::submit! { StrategyRegistration { name: "ai", factory: || Box::new(AiSignalStrategy::new("ai")), } }`
//!
//! Honest scope: registration / factory-candidate edges only (not runtime calls,
//! not full macro expansion). Mirrors Nest `module_providers` over-approx.

use agentgraph::index::extract::extract_file;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

fn extract(src: &str) -> agentgraph::index::extract::ExtractedFile {
    let known = HashSet::new();
    extract_file(src, Language::Rust, "src/plugins.rs", &known).expect("extract")
}

fn find_rule(out: &agentgraph::index::extract::ExtractedFile, rule: &str) -> Vec<(String, String)> {
    out.references
        .iter()
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == rule)
                .unwrap_or(false)
        })
        .map(|r| (r.name.clone(), r.confidence.as_str().to_string()))
        .collect()
}

#[test]
fn inventory_submit_factory_type_is_heuristic() {
    let src = r#"
struct AiSignalStrategy;
impl AiSignalStrategy {
    fn new(_name: &str) -> Self { Self }
}

struct StrategyRegistration {
    name: &'static str,
    factory: fn() -> Box<dyn Send>,
}

inventory::submit! {
    StrategyRegistration {
        name: "ai",
        factory: || Box::new(AiSignalStrategy::new("ai")),
    }
}
"#;
    let out = extract(src);
    let hits = find_rule(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|(n, _)| n == "AiSignalStrategy"),
        "factory Type::new must yield Heuristic candidate; got {hits:?}"
    );
    let conf = hits
        .iter()
        .find(|(n, _)| n == "AiSignalStrategy")
        .map(|(_, c)| c.clone())
        .unwrap();
    assert_eq!(conf, "heuristic");
}

#[test]
fn inventory_submit_registration_type_is_heuristic() {
    let src = r#"
struct StrategyRegistration { name: &'static str }
inventory::submit! { StrategyRegistration { name: "ai" } }
"#;
    let out = extract(src);
    let hits = find_rule(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|(n, _)| n == "StrategyRegistration"),
        "registration type name must yield Heuristic; got {hits:?}"
    );
}

#[test]
fn inventory_submit_does_not_claim_exact() {
    let src = r#"
struct VwapStrategy;
impl VwapStrategy { fn new() -> Self { Self } }
inventory::submit! { Registration { factory: || Box::new(VwapStrategy::new()) } }
"#;
    let out = extract(src);
    for r in &out.references {
        if r.evidence.as_ref().map(|e| e.rule_id.as_str()) == Some("rs.di.inventory_submit") {
            assert_eq!(
                r.confidence,
                Confidence::Heuristic,
                "inventory edges must stay Heuristic"
            );
        }
    }
}

#[test]
fn inventory_submit_is_sound_eligible_heuristic() {
    use agentgraph::index::subset::is_sound_eligible;
    assert!(is_sound_eligible(
        Confidence::Heuristic,
        Some("rs.di.inventory_submit")
    ));
}

#[test]
fn non_inventory_macro_does_not_emit_rule() {
    let src = r#"
struct Foo;
println!("hello {}", 1);
vec![Foo];
"#;
    let out = extract(src);
    assert!(
        find_rule(&out, "rs.di.inventory_submit").is_empty(),
        "ordinary macros must not mint inventory edges"
    );
}
