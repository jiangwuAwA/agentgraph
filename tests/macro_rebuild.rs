//! Track M1 — fingerprint / stale / `macro rebuild` (spec §1.5).
//!
//! Synthetic fixtures only. Nested roots stay hard-reject (R26/R27).

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_pair(tag: &str) -> (PathBuf, PathBuf) {
    let base = common::temp_root(&format!("agentgraph-rebuild-{tag}"));
    let root = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    (root, expanded)
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

fn parse_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).unwrap_or_else(|e| {
        panic!(
            "invalid JSON ({e}): stdout={} stderr={}",
            stdout(out),
            stderr(out)
        )
    })
}

fn write_main(root: &Path) {
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
}

fn write_expanded(expanded: &Path) {
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn fmt() -> i32 { helper() }
pub fn clone() -> i32 { helper() }
"#,
    )
    .unwrap();
}

fn build(root: &Path, expanded: &Path) {
    let idx = run(root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let exp = expanded.to_string_lossy().into_owned();
    let side = run(root, &["index", "--force", "--macro-expanded-root", &exp]);
    assert!(side.status.success(), "{}", stderr(&side));
}

fn status(root: &Path) -> serde_json::Value {
    let st = run(root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    parse_json(&st)
}

fn union_rows(payload: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = payload.as_array() {
        return arr.clone();
    }
    if let Some(arr) = payload.get("callers").and_then(|x| x.as_array()) {
        return arr.clone();
    }
    if let Some(arr) = payload.get("impact").and_then(|x| x.as_array()) {
        return arr.clone();
    }
    panic!("no rows in payload: {payload}");
}

/// After build, fingerprint is recorded and status.stale is false.
#[test]
fn fingerprint_recorded_and_not_stale_after_build() {
    let (root, expanded) = temp_pair("fresh");
    write_main(&root);
    write_expanded(&expanded);
    build(&root, &expanded);

    let st = status(&root);
    assert_eq!(st["exists"], true, "{st}");
    assert_eq!(st["stale"], false, "fresh sidecar must not be stale: {st}");
    assert!(
        st["source_fingerprint"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "source_fingerprint recorded at build: {st}"
    );
    assert_eq!(st["rebuild_policy"], "manual", "{st}");
    assert!(st.get("path_map_present").is_some(), "{st}");
    assert!(st.get("dedup_stats").is_some(), "{st}");
}

/// Changing main source after sidecar build → stale=true; --with-macro warns
/// + still unions + stale:true in payload.
#[test]
fn stale_after_main_change_warns_and_unions() {
    let (root, expanded) = temp_pair("stale");
    write_main(&root);
    write_expanded(&expanded);
    build(&root, &expanded);

    // Mutate main source (mtime+size change → new fingerprint).
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn extra() -> i32 { helper() + 2 }
"#,
    )
    .unwrap();
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let st = status(&root);
    assert_eq!(
        st["stale"], true,
        "main change must mark sidecar stale: {st}"
    );

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(
        with.status.success(),
        "stale sidecar must still union: {}",
        stderr(&with)
    );
    let err = stderr(&with).to_lowercase();
    assert!(
        err.contains("stale") || err.contains("rebuild"),
        "stderr must warn about stale sidecar: {err}"
    );
    let payload = parse_json(&with);
    if payload.is_object() {
        assert_eq!(
            payload["stale"], true,
            "payload must carry stale:true: {payload}"
        );
    }
    let rows = union_rows(&payload);
    let enc: Vec<&str> = rows
        .iter()
        .filter_map(|r| r["enclosing"].as_str())
        .collect();
    assert!(
        enc.iter().any(|e| *e == "fmt" || *e == "clone"),
        "stale union must still include sidecar candidates: {enc:?}"
    );
}

/// `macro rebuild` is idempotent and clears stale after main change.
#[test]
fn macro_rebuild_idempotent_and_clears_stale() {
    let (root, expanded) = temp_pair("rebuild");
    write_main(&root);
    write_expanded(&expanded);
    build(&root, &expanded);

    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn extra() -> i32 { 99 }
"#,
    )
    .unwrap();
    assert!(run(&root, &["index", "--force"]).status.success());
    assert_eq!(status(&root)["stale"], true);

    let rb1 = run(&root, &["macro", "rebuild"]);
    assert!(rb1.status.success(), "rebuild: {}", stderr(&rb1));
    let payload1 = parse_json(&rb1);
    assert!(
        payload1["macro_sidecar"]["origin"] == "macro_expanded"
            || payload1["status"]["origin"] == "macro_expanded",
        "rebuild payload: {payload1}"
    );
    let st1 = status(&root);
    assert_eq!(st1["stale"], false, "rebuild clears stale: {st1}");
    assert_eq!(st1["exists"], true, "{st1}");
    let files1 = st1["files"].as_u64().unwrap();
    let symbols1 = st1["symbols"].as_u64().unwrap();

    // Second rebuild: idempotent counts.
    let rb2 = run(&root, &["macro", "rebuild"]);
    assert!(rb2.status.success(), "{}", stderr(&rb2));
    let st2 = status(&root);
    assert_eq!(st2["stale"], false, "{st2}");
    assert_eq!(st2["files"].as_u64().unwrap(), files1, "idempotent files");
    assert_eq!(
        st2["symbols"].as_u64().unwrap(),
        symbols1,
        "idempotent symbols"
    );

    // Queries still work after rebuild.
    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    let rows = union_rows(&parse_json(&with));
    let enc: Vec<&str> = rows
        .iter()
        .filter_map(|r| r["enclosing"].as_str())
        .collect();
    assert!(
        enc.iter().any(|e| *e == "clone" || *e == "fmt"),
        "post-rebuild union: {enc:?}"
    );
}

/// rebuild without a sidecar / recorded expanded_root fails closed.
#[test]
fn rebuild_without_sidecar_fails() {
    let (root, _expanded) = temp_pair("no-side");
    write_main(&root);
    assert!(run(&root, &["index", "--force"]).status.success());
    let rb = run(&root, &["macro", "rebuild"]);
    assert!(
        !rb.status.success(),
        "rebuild without sidecar must fail: {}",
        stdout(&rb)
    );
    let err = stderr(&rb).to_lowercase();
    assert!(
        err.contains("sidecar") || err.contains("expanded_root") || err.contains("macro"),
        "err={err}"
    );
}

/// Nested expanded root still hard-rejects on index (R26/R27 not softened).
#[test]
fn nested_expanded_root_still_hard_reject() {
    let (root, _expanded) = temp_pair("nested");
    write_main(&root);
    let nested = root.join("expand-shadow");
    std::fs::create_dir_all(nested.join("src")).unwrap();
    std::fs::write(nested.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
    let out = run(
        &root,
        &[
            "index",
            "--force",
            "--macro-expanded-root",
            &nested.to_string_lossy(),
        ],
    );
    assert!(
        !out.status.success(),
        "nested expanded root must reject: {}",
        stdout(&out) + &stderr(&out)
    );
    let err = stderr(&out).to_lowercase();
    assert!(
        err.contains("under project root")
            || err.contains("nested")
            || err.contains("sibling")
            || err.contains("pollute"),
        "err={err}"
    );
    // Main index must not have been dirtied by a rejected nested sidecar path
    // in a way that creates the sidecar under nested root as main content —
    // at minimum the command failed closed.
}

/// Fingerprint is stable when main files are unchanged (rebuild not required).
#[test]
fn fingerprint_stable_without_main_changes() {
    let (root, expanded) = temp_pair("stable");
    write_main(&root);
    write_expanded(&expanded);
    build(&root, &expanded);
    let fp1 = status(&root)["source_fingerprint"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    // Re-index main without content change — fingerprint must stay stable.
    assert!(run(&root, &["index", "--force"]).status.success());
    let st = status(&root);
    assert_eq!(st["stale"], false, "{st}");
    assert_eq!(
        st["source_fingerprint"].as_str().unwrap_or_default(),
        fp1,
        "fingerprint stable when sources unchanged: {st}"
    );
}
