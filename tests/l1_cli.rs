//! TDD: CLI confidence flags for callers/impact.

use std::path::PathBuf;
use std::process::Command;

mod common;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agentgraph"))
}

fn temp_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-l1-cli-{name}"));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/app.ts"),
        r#"
export class UserService {
  load() { return 1; }
}
export function helper() { return 2; }
export function bootstrap(c: any) {
  c.register(UserService);
  helper();
}
export function dyn(obj: any) {
  obj['UserService']();
}
"#,
    )
    .unwrap();
    dir
}

fn run(root: &PathBuf, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run agentgraph");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn cli_callers_default_includes_heuristic() {
    let root = temp_root("default");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["callers", "UserService"]);
    assert!(ok, "callers failed: {err}");
    assert!(
        stdout.contains("\"heuristic\""),
        "default callers must include heuristic DI edge; got {stdout}"
    );
    assert!(
        !stdout.contains("\"dynamic_candidate\""),
        "default must exclude dynamic; got {stdout}"
    );
}

#[test]
fn cli_callers_exact_only_drops_heuristic() {
    let root = temp_root("exact");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["callers", "UserService", "--exact-only"]);
    assert!(ok, "callers failed: {err}");
    assert!(
        !stdout.contains("\"heuristic\""),
        "--exact-only must drop heuristic; got {stdout}"
    );
}

#[test]
fn cli_callers_recall_includes_dynamic() {
    let root = temp_root("recall");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["callers", "UserService", "--recall"]);
    assert!(ok, "callers failed: {err}");
    assert!(
        stdout.contains("\"dynamic_candidate\""),
        "--recall must surface DynamicCandidate; got {stdout}"
    );
}

#[test]
fn cli_callers_include_dynamic() {
    let root = temp_root("dyn");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["callers", "UserService", "--include-dynamic"]);
    assert!(ok, "callers failed: {err}");
    assert!(
        stdout.contains("\"dynamic_candidate\""),
        "--include-dynamic must surface DynamicCandidate; got {stdout}"
    );
}

#[test]
fn cli_impact_exact_only_has_no_heuristic() {
    let root = temp_root("impact");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["impact", "helper", "--exact-only"]);
    assert!(ok, "impact failed: {err}");
    assert!(
        !stdout.contains("\"heuristic\""),
        "impact --exact-only must not include heuristic; got {stdout}"
    );
}

#[test]
fn cli_impact_default_includes_heuristic_via_bfs() {
    let root = temp_root("impact-h");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    // bootstrap contains register(UserService) heuristic + helper() exact.
    // impact UserService default should include the bootstrap site as heuristic.
    let (ok, stdout, err) = run(&root, &["impact", "UserService"]);
    assert!(ok, "impact failed: {err}");
    assert!(
        stdout.contains("heuristic"),
        "impact default should walk Heuristic edges; got {stdout}"
    );
}

// ── Track M3: new Heuristic rule ids in default callers/impact ─────────

fn m3_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-l1-cli-m3-{name}"));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/routes.ts"),
        r#"
export function getUsers() { return []; }
export function metricsHandler() { return 1; }
export function bootstrap(app: any, router: any) {
  router.get('/users', getUsers);
  app.register('/metrics', metricsHandler);
}
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("rs")).unwrap();
    std::fs::write(
        dir.join("rs/shapes.rs"),
        r#"
trait Shape { fn area(&self) -> f64; }
struct Circle { r: f64 }
impl Shape for Circle { fn area(&self) -> f64 { 1.0 } }
fn total_area(s: &dyn Shape) -> f64 { s.area() }
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("rs/linkme.rs"),
        r#"
use linkme::distributed_slice;
pub struct StrategyRegistration { pub factory: fn() -> u32 }
pub struct DemoStrategy;
impl DemoStrategy { pub fn new() -> u32 { 1 } }
#[distributed_slice(STRATEGIES)]
static DEMO: StrategyRegistration = StrategyRegistration {
    factory: DemoStrategy::new,
};
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("go")).unwrap();
    std::fs::write(
        dir.join("go/store.go"),
        r#"
package store
type Store interface {
  Get(id string) string
  Put(id string, v string)
}
type MemStore struct{}
func (m *MemStore) Get(id string) string { return id }
func (m *MemStore) Put(id string, v string) {}
var _ Store = (*MemStore)(nil)
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("py")).unwrap();
    std::fs::write(
        dir.join("py/plugins.py"),
        r#"
from importlib.metadata import entry_points
def load_plugins():
    return list(entry_points(group="myapp.plugins"))
"#,
    )
    .unwrap();
    dir
}

fn stdout_has_rule_id(stdout: &str, rule_id: &str) -> bool {
    stdout.contains(rule_id)
}

/// Default callers/impact JSON must include shipped M3 Heuristic rule_ids
/// (Track M3 default-path productization). `--exact-only` must exclude them.
#[test]
fn cli_default_callers_include_m3_heuristic_rule_ids() {
    let root = m3_root("default-rules");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");

    let cases: &[(&str, &[&str])] = &[
        ("metricsHandler", &["ts.framework.register"]),
        ("area", &["rs.di.dyn_trait_method"]),
        ("StrategyRegistration", &["rs.di.linkme_distributed_slice"]),
        ("Get", &["go.di.interface_impl", "go.di.interface_impl_v2"]),
        ("myapp.plugins", &["py.di.entry_points"]),
    ];

    for (symbol, rules) in cases {
        let (ok, stdout, err) = run(&root, &["callers", symbol]);
        assert!(ok, "callers {symbol} failed: {err}\nstdout={stdout}");
        let found = rules.iter().any(|r| stdout_has_rule_id(&stdout, r));
        assert!(
            found,
            "default callers({symbol}) must include at least one of {rules:?}; got {stdout}"
        );
        assert!(
            stdout.contains("\"heuristic\""),
            "default callers({symbol}) must surface heuristic confidence; got {stdout}"
        );
    }
}

#[test]
fn cli_exact_only_excludes_m3_heuristic_rule_ids() {
    let root = m3_root("exact-rules");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");

    let cases: &[(&str, &[&str])] = &[
        ("metricsHandler", &["ts.framework.register"]),
        ("area", &["rs.di.dyn_trait_method"]),
        ("StrategyRegistration", &["rs.di.linkme_distributed_slice"]),
        ("Get", &["go.di.interface_impl_v2"]),
        ("myapp.plugins", &["py.di.entry_points"]),
    ];

    for (symbol, rules) in cases {
        let (ok, stdout, err) = run(&root, &["callers", symbol, "--exact-only"]);
        assert!(ok, "callers --exact-only {symbol} failed: {err}");
        for r in *rules {
            assert!(
                !stdout_has_rule_id(&stdout, r),
                "--exact-only callers({symbol}) must exclude rule_id {r}; got {stdout}"
            );
        }
    }
}

#[test]
fn cli_default_impact_includes_m3_heuristic_and_exact_only_excludes() {
    let root = m3_root("impact-rules");
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");

    // Default impact walks Heuristic registration / dyn-trait candidates.
    for (symbol, rule) in [
        ("metricsHandler", "ts.framework.register"),
        ("area", "rs.di.dyn_trait_method"),
        ("Get", "go.di.interface_impl"),
    ] {
        let (ok, stdout, err) = run(&root, &["impact", symbol]);
        assert!(ok, "impact {symbol} failed: {err}");
        assert!(
            stdout.contains("heuristic") || stdout_has_rule_id(&stdout, rule),
            "default impact({symbol}) should include heuristic M3 edge / {rule}; got {stdout}"
        );
    }

    // --exact-only impact must not report heuristic confidence rows for M3 symbols.
    for symbol in ["metricsHandler", "area"] {
        let (ok, stdout, err) = run(&root, &["impact", symbol, "--exact-only"]);
        assert!(ok, "impact --exact-only {symbol} failed: {err}");
        assert!(
            !stdout.contains("\"heuristic\""),
            "impact --exact-only({symbol}) must exclude heuristic; got {stdout}"
        );
        assert!(
            !stdout_has_rule_id(&stdout, "ts.framework.register")
                && !stdout_has_rule_id(&stdout, "rs.di.dyn_trait_method"),
            "impact --exact-only({symbol}) must exclude M3 rule ids; got {stdout}"
        );
    }
}
