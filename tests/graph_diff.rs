//! TDD Track M4: indexed-edge snapshot diff (`agentgraph diff`).
//!
//! Contract:
//! - Semantics = **indexed-edge set difference**, not runtime semantics.
//! - Snapshot written at successful `index` time (dual meta: current + previous).
//! - No baseline → fail-loud with hint to `index` first.
//! - `--exact-only` filters to Exact confidence only.
//! - Honesty note present in JSON payload.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-graph-diff-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn write_base(root: &Path) {
    std::fs::write(
        root.join("src/a.ts"),
        r#"
export function helper(x: number): number { return x + 1; }
export function createUser(email: string) {
  helper(1);
  return { email };
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/b.ts"),
        r#"
import { createUser } from "./a";
export function loginHandler(email: string) {
  return createUser(email);
}
"#,
    )
    .unwrap();
}

/// After a second `index` following an added call site, diff reports the new edge.
#[test]
fn diff_reports_added_call_site_after_second_index() {
    let root = temp_root("added");
    write_base(&root);
    let idx1 = run(&root, &["index", "--force"]);
    assert!(idx1.status.success(), "index1: {}", stderr(&idx1));

    // Snapshot must exist after first index.
    assert!(
        root.join(".agentgraph/refs.snapshot.json").exists(),
        "index must write refs.snapshot.json"
    );

    // Add a new call site: adminHandler → createUser (new edge into createUser).
    std::fs::write(
        root.join("src/c.ts"),
        r#"
import { createUser } from "./a";
export function adminHandler(email: string) {
  createUser(email);
  createUser(email);
}
"#,
    )
    .unwrap();

    let idx2 = run(&root, &["index", "--force"]);
    assert!(idx2.status.success(), "index2: {}", stderr(&idx2));

    let d = run(&root, &["diff"]);
    assert!(
        d.status.success(),
        "diff: stdout={} stderr={}",
        stdout(&d),
        stderr(&d)
    );
    let payload: serde_json::Value = serde_json::from_str(&stdout(&d)).expect("diff json");
    assert!(
        payload["summary"]["added"].as_u64().unwrap_or(0) >= 1,
        "expected added edges: {}",
        stdout(&d)
    );
    let added = payload["added"].as_array().expect("added array");
    assert!(
        added.iter().any(|e| {
            e["name"].as_str() == Some("createUser")
                && e["path"]
                    .as_str()
                    .map(|p| p.contains("c.ts"))
                    .unwrap_or(false)
        }),
        "expected createUser call site from c.ts in added: {}",
        stdout(&d)
    );
    // Honesty string required.
    let note = payload["note"].as_str().unwrap_or("");
    assert!(
        note.contains("indexed edges") && note.to_lowercase().contains("not a runtime"),
        "honesty note required: {note}"
    );
    assert_eq!(payload["exact_only"], serde_json::json!(false));
}

/// After removing a call site + reindex, the old edge appears in `removed`.
#[test]
fn diff_reports_removed_call_site_after_second_index() {
    let root = temp_root("removed");
    write_base(&root);
    let idx1 = run(&root, &["index", "--force"]);
    assert!(idx1.status.success(), "index1: {}", stderr(&idx1));

    // Drop the b.ts import/call of createUser.
    std::fs::write(
        root.join("src/b.ts"),
        r#"
export function loginHandler(email: string) {
  return { email };
}
"#,
    )
    .unwrap();

    let idx2 = run(&root, &["index", "--force"]);
    assert!(idx2.status.success(), "index2: {}", stderr(&idx2));

    let d = run(&root, &["diff"]);
    assert!(d.status.success(), "diff: {}", stderr(&d));
    let payload: serde_json::Value = serde_json::from_str(&stdout(&d)).unwrap();
    assert!(
        payload["summary"]["removed"].as_u64().unwrap_or(0) >= 1,
        "expected removed edges: {}",
        stdout(&d)
    );
    let removed = payload["removed"].as_array().unwrap();
    assert!(
        removed.iter().any(|e| {
            e["name"].as_str() == Some("createUser")
                && e["path"]
                    .as_str()
                    .map(|p| p.contains("b.ts"))
                    .unwrap_or(false)
        }),
        "expected removed createUser edge from b.ts: {}",
        stdout(&d)
    );
}

/// No snapshot → fail-loud with index hint (exit non-zero).
#[test]
fn diff_without_snapshot_fails_loud() {
    let root = temp_root("nosnap");
    write_base(&root);
    // Index the store but delete the snapshot sidecar to simulate missing baseline.
    // (Or: never index — both must fail-loud.)
    let d0 = run(&root, &["diff"]);
    assert!(
        !d0.status.success(),
        "diff before index must fail: stdout={} stderr={}",
        stdout(&d0),
        stderr(&d0)
    );
    let err = format!("{}{}", stdout(&d0), stderr(&d0)).to_lowercase();
    assert!(
        err.contains("snapshot") || err.contains("index"),
        "fail-loud must mention snapshot/index: {err}"
    );

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success());
    std::fs::remove_file(root.join(".agentgraph/refs.snapshot.json")).ok();
    std::fs::remove_file(root.join(".agentgraph/refs.snapshot.prev.json")).ok();
    let d1 = run(&root, &["diff"]);
    assert!(
        !d1.status.success(),
        "diff without snapshot files must fail even after index"
    );
    let err1 = format!("{}{}", stdout(&d1), stderr(&d1)).to_lowercase();
    assert!(
        err1.contains("index") || err1.contains("snapshot"),
        "hint required: {err1}"
    );
}

/// `--exact-only` filters added/removed to Exact confidence edges only.
#[test]
fn diff_exact_only_filters_heuristic_edges() {
    let root = temp_root("exact");
    // Base: exact call helper ← createUser.
    std::fs::write(
        root.join("src/auth.ts"),
        r#"
export function helper(x: number): number { return x + 1; }
export function createUser(email: string) {
  helper(1);
  return { email };
}
export class UserService {
  load() { return helper(2); }
}
export function bootstrap(c: any) {
  c.register(UserService);
}
"#,
    )
    .unwrap();
    let idx1 = run(&root, &["index", "--force"]);
    assert!(idx1.status.success(), "{}", stderr(&idx1));

    // Add a DI heuristic edge (ts.di.register — Heuristic, not Exact).
    std::fs::write(
        root.join("src/di.ts"),
        r#"
import { UserService } from "./auth";
export function wire(c: any) {
  c.register(UserService);
}
"#,
    )
    .unwrap();
    let idx2 = run(&root, &["index", "--force"]);
    assert!(idx2.status.success(), "{}", stderr(&idx2));

    let all = run(&root, &["diff"]);
    assert!(all.status.success(), "{}", stderr(&all));
    let all_payload: serde_json::Value = serde_json::from_str(&stdout(&all)).unwrap();
    assert!(
        all_payload["summary"]["added"].as_u64().unwrap_or(0) >= 1,
        "default diff should include new edges: {}",
        stdout(&all)
    );

    let ex = run(&root, &["diff", "--exact-only"]);
    assert!(ex.status.success(), "{}", stderr(&ex));
    let ex_payload: serde_json::Value = serde_json::from_str(&stdout(&ex)).unwrap();
    assert_eq!(ex_payload["exact_only"], serde_json::json!(true));
    // All returned edges (added/removed) must be Exact when --exact-only.
    for side in ["added", "removed"] {
        if let Some(arr) = ex_payload[side].as_array() {
            for e in arr {
                assert_eq!(
                    e["confidence"].as_str(),
                    Some("exact"),
                    "--exact-only must not return non-exact edges in {side}: {e}"
                );
            }
        }
    }
    // Heuristic-only new edge (register of UserService from di.ts) must NOT appear
    // under --exact-only added (or if Exact edges also appeared, that's fine).
    if let Some(arr) = ex_payload["added"].as_array() {
        let has_heuristic_register = arr.iter().any(|e| {
            e["confidence"].as_str() != Some("exact")
                && e["path"]
                    .as_str()
                    .map(|p| p.contains("di.ts"))
                    .unwrap_or(false)
        });
        assert!(
            !has_heuristic_register,
            "heuristic di.ts edge leaked into --exact-only: {}",
            stdout(&ex)
        );
    }
}

/// `--limit N` caps rows returned per side.
#[test]
fn diff_limit_caps_output_rows() {
    let root = temp_root("limit");
    write_base(&root);
    let idx1 = run(&root, &["index", "--force"]);
    assert!(idx1.status.success());

    // Many new call sites across files.
    for i in 0..8 {
        std::fs::write(
            root.join(format!("src/gen{i}.ts")),
            format!(
                r#"
import {{ createUser }} from "./a";
export function gen{i}(e: string) {{
  createUser(e);
}}
"#
            ),
        )
        .unwrap();
    }
    let idx2 = run(&root, &["index", "--force"]);
    assert!(idx2.status.success(), "{}", stderr(&idx2));

    let d = run(&root, &["diff", "--limit", "2"]);
    assert!(d.status.success(), "{}", stderr(&d));
    let payload: serde_json::Value = serde_json::from_str(&stdout(&d)).unwrap();
    let added_len = payload["added"].as_array().map(|a| a.len()).unwrap_or(0);
    let removed_len = payload["removed"].as_array().map(|a| a.len()).unwrap_or(0);
    assert!(added_len <= 2, "limit 2 must cap added, got {added_len}");
    assert!(
        removed_len <= 2,
        "limit 2 must cap removed, got {removed_len}"
    );
    // Summary still reflects full counts (not truncated).
    assert!(
        payload["summary"]["added"].as_u64().unwrap_or(0) >= 3,
        "summary should count all added even when limited: {}",
        stdout(&d)
    );
}

/// Incremental `index_paths` (watch path) updates live refs; diff vs baseline
/// shows the new edge without requiring a second full index.
#[test]
fn diff_sees_index_paths_changes_against_baseline() {
    use agentgraph::index::Indexer;
    let root = temp_root("watchdiff");
    write_base(&root);
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let new_file = root.join("src/watch_add.ts");
    std::fs::write(
        &new_file,
        r#"
import { createUser } from "./a";
export function watchCaller(e: string) {
  createUser(e);
}
"#,
    )
    .unwrap();
    indexer
        .index_paths(std::slice::from_ref(&new_file))
        .expect("index_paths");

    let d = run(&root, &["diff"]);
    assert!(d.status.success(), "diff after index_paths: {}", stderr(&d));
    let payload: serde_json::Value = serde_json::from_str(&stdout(&d)).unwrap();
    let added = payload["added"].as_array().cloned().unwrap_or_default();
    assert!(
        added.iter().any(|e| e["path"]
            .as_str()
            .map(|p| p.contains("watch_add.ts"))
            .unwrap_or(false)),
        "index_paths-added edge must show in diff: {}",
        stdout(&d)
    );
}

/// Unit-level: edge key and set-diff helper.
#[test]
fn unit_edge_key_and_set_diff() {
    use agentgraph::index::diff::{diff_edges, edge_key, SnapshotEdge};

    let a = SnapshotEdge {
        name: "helper".into(),
        path: "src/a.ts".into(),
        line: 2,
        confidence: "exact".into(),
        enclosing: Some("createUser".into()),
        root_id: String::new(),
    };
    let mut b = a.clone();
    b.line = 10;
    assert_ne!(edge_key(&a), edge_key(&b));

    let baseline = vec![a.clone()];
    let current = vec![b.clone()];
    let d = diff_edges(&baseline, &current, false, None);
    assert_eq!(d.added.len(), 1);
    assert_eq!(d.removed.len(), 1);
    assert_eq!(d.summary.added, 1);
    assert_eq!(d.summary.removed, 1);
    assert!(d.note.contains("indexed edges"));
}
