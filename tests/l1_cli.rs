//! TDD: CLI confidence flags for callers/impact.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agentgraph"))
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-l1-cli-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
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
