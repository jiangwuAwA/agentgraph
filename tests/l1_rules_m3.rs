//! TDD: Track M3 L1 default-recall rule packages (A–E).
//!
//! Each rule must produce a ref with the right confidence + evidence.
//! Dyn-trait implementor candidates are **not** sound-eligible (open dispatch).

use agentgraph::index::extract::extract_file;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

fn extract(src: &str, lang: Language, path: &str) -> agentgraph::index::extract::ExtractedFile {
    extract_file(src, lang, path, &HashSet::new()).expect("extract")
}

fn rule_hits<'a>(
    out: &'a agentgraph::index::extract::ExtractedFile,
    rule: &str,
) -> Vec<&'a agentgraph::index::extract::ExtractedRef> {
    out.references
        .iter()
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == rule)
                .unwrap_or(false)
        })
        .collect()
}

fn has_rule_name(out: &agentgraph::index::extract::ExtractedFile, rule: &str, name: &str) -> bool {
    rule_hits(out, rule).iter().any(|r| r.name == name)
}

// ── M3-A: Rust dyn Trait method → implementors ──────────────────────

const RUST_DYN: &str = r#"
trait Shape {
    fn area(&self) -> f64;
}

struct Circle { r: f64 }
struct Rect { w: f64, h: f64 }

impl Shape for Circle {
    fn area(&self) -> f64 { std::f64::consts::PI * self.r * self.r }
}

impl Shape for Rect {
    fn area(&self) -> f64 { self.w * self.h }
}

fn total_area(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}
"#;

#[test]
fn m3a_dyn_trait_method_call_emits_implementor_candidates() {
    let out = extract(RUST_DYN, Language::Rust, "src/shapes.rs");
    let hits = rule_hits(&out, "rs.di.dyn_trait_method");
    assert!(
        !hits.is_empty(),
        "dyn Shape method call must yield rs.di.dyn_trait_method; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.qualifier.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
    // Only already-indexed implementors — never invent types.
    assert!(
        has_rule_name(&out, "rs.di.dyn_trait_method", "area"),
        "dyn method name must appear; hits={:?}",
        hits.iter()
            .map(|r| (r.name.clone(), r.qualifier.clone()))
            .collect::<Vec<_>>()
    );
    for h in &hits {
        assert_eq!(h.confidence, Confidence::Heuristic);
        let q = h.qualifier.clone().unwrap_or_default();
        assert!(
            q == "Circle" || q == "Rect" || q.is_empty(),
            "qualifier must be an indexed implementor, got {q:?}"
        );
        let ev = h.evidence.as_ref().expect("evidence");
        assert!(!ev.snippet.is_empty(), "evidence snippet required");
    }
}

#[test]
fn m3a_dyn_trait_no_invented_implementors() {
    // Adversarial: dyn type present but no impl of that trait in file.
    let src = r#"
trait Handler {}
struct App;
fn takes(h: Box<dyn Handler>) { let _ = h; }
"#;
    let out = extract(src, Language::Rust, "a.rs");
    let hits = rule_hits(&out, "rs.di.dyn_trait_method");
    assert!(
        hits.iter().all(|r| r.name != "App"),
        "must not invent App as Handler implementor; hits={:?}",
        hits.iter()
            .map(|r| (r.name.clone(), r.qualifier.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn m3a_dyn_trait_not_sound_eligible() {
    use agentgraph::index::subset::is_sound_eligible;
    assert!(
        !is_sound_eligible(Confidence::Heuristic, Some("rs.di.dyn_trait_method")),
        "dyn-trait implementor candidates must stay Unsound (open dispatch, not finite registration)"
    );
}

// ── M3-B: Go interface impl v2 ───────────────────────────────────────

const GO_IFACE: &str = r#"
package store

type Store interface {
	Get(id string) string
	Put(id string, v string)
}

type Cache interface {
	Get(id string) string
}

type MemStore struct{}

func (m *MemStore) Get(id string) string { return id }
func (m *MemStore) Put(id string, v string) {}

var _ Store = (*MemStore)(nil)

type RedisCache struct{}

func (r *RedisCache) Get(id string) string { return id }
"#;

#[test]
fn m3b_go_assertion_links_interface_methods() {
    let out = extract(GO_IFACE, Language::Go, "store.go");
    let hits = rule_hits(&out, "go.di.interface_impl_v2");
    assert!(
        !hits.is_empty(),
        "var _ Store = (*MemStore)(nil) + method-set must yield go.di.interface_impl_v2; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.qualifier.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
    assert!(
        has_rule_name(&out, "go.di.interface_impl_v2", "Get"),
        "assertion site must link Get; hits={:?}",
        hits.iter().map(|r| r.name.clone()).collect::<Vec<_>>()
    );
    assert!(
        has_rule_name(&out, "go.di.interface_impl_v2", "Put"),
        "assertion site must link Put"
    );
    // Evidence must mention the interface or method-set proof.
    for h in &hits {
        assert_eq!(h.confidence, Confidence::Heuristic);
        let ev = h.evidence.as_ref().expect("evidence");
        assert!(
            ev.snippet.contains("Store")
                || ev.snippet.contains("Cache")
                || ev.snippet.contains("MemStore")
                || ev.snippet.contains("RedisCache")
                || ev.snippet.contains("method-set"),
            "evidence must name interface/impl; got {}",
            ev.snippet
        );
    }
}

#[test]
fn m3b_go_method_set_match_without_assertion() {
    let out = extract(GO_IFACE, Language::Go, "store.go");
    // RedisCache implements Cache by method-set name match only.
    let redis_hits: Vec<_> = rule_hits(&out, "go.di.interface_impl_v2")
        .into_iter()
        .filter(|r| {
            r.name == "Get"
                && r.evidence
                    .as_ref()
                    .map(|e| e.snippet.contains("RedisCache") || e.snippet.contains("Cache"))
                    .unwrap_or(false)
        })
        .collect();
    assert!(
        !redis_hits.is_empty(),
        "method-set match RedisCache→Cache must emit v2 edge; refs={:?}",
        out.references
            .iter()
            .filter(|r| r.name == "Get")
            .map(|r| (
                r.qualifier.clone(),
                r.evidence.as_ref().map(|e| e.rule_id.clone()),
                r.evidence.as_ref().map(|e| e.snippet.clone())
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn m3b_go_interface_impl_v2_sound_eligibility_documented() {
    use agentgraph::index::subset::is_sound_eligible;
    // Finite method-set within indexed types — same class as go.di.interface_impl.
    assert!(
        is_sound_eligible(Confidence::Heuristic, Some("go.di.interface_impl_v2")),
        "v2 is finite method-set over indexed types — allowlisted"
    );
    assert!(
        is_sound_eligible(Confidence::Heuristic, Some("ts.framework.register")),
        "framework register handlers at call site — allowlisted"
    );
    assert!(
        is_sound_eligible(
            Confidence::Heuristic,
            Some("rs.di.linkme_distributed_slice")
        ),
        "linkme attribute-site identifiers — allowlisted"
    );
}

// ── M3-C: Python entry points + Security ─────────────────────────────

const PY_PLUGINS: &str = r#"
from importlib.metadata import entry_points
from fastapi import Depends, Security
from typing import Annotated

class Plugin:
    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)

class AuthPlugin(Plugin):
    pass

def get_current_user():
    return {}

def get_user_service():
    return object()

def read_user(
    svc: Annotated[object, Depends(get_user_service)],
    user=Security(get_current_user),
):
    return user

def load_plugins():
    return list(entry_points(group="myapp.plugins"))
"#;

#[test]
fn m3c_py_entry_points_group_is_heuristic() {
    let out = extract(PY_PLUGINS, Language::Python, "app/plugins.py");
    assert!(
        has_rule_name(&out, "py.di.entry_points", "myapp.plugins"),
        "entry_points(group=...) must yield Heuristic for the group; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
    for h in rule_hits(&out, "py.di.entry_points") {
        assert_eq!(h.confidence, Confidence::Heuristic);
        assert!(!h.evidence.as_ref().unwrap().snippet.is_empty());
    }
}

#[test]
fn m3c_py_security_depends_is_heuristic() {
    let out = extract(PY_PLUGINS, Language::Python, "app/plugins.py");
    assert!(
        has_rule_name(&out, "py.di.depends", "get_current_user"),
        "Security(get_current_user) must yield Heuristic; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn m3c_py_annotated_depends_is_heuristic() {
    let out = extract(PY_PLUGINS, Language::Python, "app/plugins.py");
    assert!(
        has_rule_name(&out, "py.di.depends", "get_user_service"),
        "Annotated[..., Depends(fn)] must yield Heuristic"
    );
}

#[test]
fn m3c_py_entry_points_not_sound_eligible() {
    use agentgraph::index::subset::is_sound_eligible;
    assert!(
        !is_sound_eligible(Confidence::Heuristic, Some("py.di.entry_points")),
        "entry-point plugins are not enumerated at the call site — not finite registration"
    );
}

// ── M3-D: TS router / framework registration ─────────────────────────

const TS_ROUTER: &str = r#"
export function getUsers() { return []; }
export function createUser() { return {}; }
export function authMiddleware() { return true; }
export function metricsHandler() { return 1; }
export function bootstrap(app: any, router: any) {
  router.get('/users', getUsers);
  router.post('/users', createUser);
  app.use(authMiddleware);
  app.register('/metrics', metricsHandler);
}
"#;

#[test]
fn m3d_ts_express_router_verbs_are_heuristic() {
    let out = extract(TS_ROUTER, Language::TypeScript, "src/routes.ts");
    assert!(
        has_rule_name(&out, "ts.framework.register", "getUsers")
            || out.references.iter().any(|r| {
                r.name == "getUsers"
                    && r.confidence == Confidence::Heuristic
                    && r.evidence
                        .as_ref()
                        .map(|e| e.rule_id.contains("route") || e.rule_id.contains("register"))
                        .unwrap_or(false)
            }),
        "router.get(..., getUsers) must yield Heuristic; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
    assert!(
        has_rule_name(&out, "ts.framework.register", "authMiddleware")
            || out
                .references
                .iter()
                .any(|r| { r.name == "authMiddleware" && r.confidence == Confidence::Heuristic }),
        "app.use(authMiddleware) must yield Heuristic"
    );
    assert!(
        has_rule_name(&out, "ts.framework.register", "metricsHandler")
            || out
                .references
                .iter()
                .any(|r| { r.name == "metricsHandler" && r.confidence == Confidence::Heuristic }),
        "app.register(..., metricsHandler) must yield Heuristic"
    );
}

// ── M3-E: Rust linkme / distributed_slice ────────────────────────────

const RUST_LINKME: &str = r#"
use linkme::distributed_slice;

pub struct StrategyRegistration {
    pub factory: fn() -> u32,
}

pub struct DemoStrategy;

impl DemoStrategy {
    pub fn new() -> u32 { 1 }
}

#[distributed_slice(STRATEGIES)]
static DEMO: StrategyRegistration = StrategyRegistration {
    factory: DemoStrategy::new,
};
"#;

#[test]
fn m3e_linkme_distributed_slice_is_heuristic() {
    let out = extract(RUST_LINKME, Language::Rust, "src/plugins.rs");
    let hits = rule_hits(&out, "rs.di.linkme_distributed_slice");
    assert!(
        !hits.is_empty(),
        "#[distributed_slice] must yield rs.di.linkme_distributed_slice; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
    let names: Vec<_> = hits.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"StrategyRegistration") || names.contains(&"DemoStrategy"),
        "linkme site must yield registration/factory type identifiers; got {names:?}"
    );
    for h in &hits {
        assert_eq!(h.confidence, Confidence::Heuristic);
        assert!(!h.evidence.as_ref().unwrap().snippet.is_empty());
    }
}

#[test]
fn m3e_inventory_already_covered_no_duplicate_rule_required() {
    // Inventory source pattern is covered by rs.di.inventory_submit (existing).
    let src = r#"
struct StrategyRegistration { name: &'static str }
struct DemoStrategy;
impl DemoStrategy { fn new() -> Self { Self } }
inventory::submit! {
    StrategyRegistration {
        name: "demo",
        factory: || Box::new(DemoStrategy::new()),
    }
}
"#;
    let out = extract(src, Language::Rust, "src/inv.rs");
    assert!(
        has_rule_name(&out, "rs.di.inventory_submit", "DemoStrategy")
            || has_rule_name(&out, "rs.di.inventory_submit", "StrategyRegistration"),
        "inventory::submit! must stay covered by rs.di.inventory_submit"
    );
    // M3-E only adds the linkme gap; inventory is not re-minted under a new id.
    assert!(
        rule_hits(&out, "rs.di.linkme_distributed_slice").is_empty(),
        "inventory submit must not mint linkme rule id"
    );
}
