//! Track M1 — `--with-macro` de-dup table (docs/product-boundary-migration.md §1.4).
//!
//! Synthetic fixtures only (do not commit operator expand corpora).

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_pair(tag: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("agentgraph-dedup-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
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

/// Extract the callers/impact row array from either a plain array or the
/// M1 wrapped with-macro payload.
fn result_rows(v: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = v.as_array() {
        return arr.clone();
    }
    if let Some(arr) = v.get("callers").and_then(|x| x.as_array()) {
        return arr.clone();
    }
    if let Some(arr) = v.get("impact").and_then(|x| x.as_array()) {
        return arr.clone();
    }
    panic!("expected array or wrapped callers/impact payload: {v}");
}

fn enclosings(rows: &[serde_json::Value]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| r["enclosing"].as_str().map(|s| s.to_string()))
        .collect()
}

fn rows_with_origin(rows: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    rows.iter()
        .filter(|r| r["origin"] == "macro_expanded")
        .collect()
}

fn build_sidecar(root: &Path, expanded: &Path) {
    let idx = run(root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let exp = expanded.to_string_lossy().into_owned();
    let side = run(root, &["index", "--force", "--macro-expanded-root", &exp]);
    assert!(side.status.success(), "sidecar index: {}", stderr(&side));
}

/// Sidecar mapped path + name + enclosing match main Exact → keep main only;
/// `dedup_stats.merged_exact >= 1`.
#[test]
fn merged_exact_same_path_name_enclosing() {
    let (root, expanded) = temp_pair("exact");
    // Same logical edge on both sides: process → helper at src/core.rs.
    let main = r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#;
    let expanded_src = r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn fmt() -> i32 { helper() }
pub fn clone() -> i32 { helper() }
"#;
    std::fs::write(root.join("src/core.rs"), main).unwrap();
    std::fs::write(expanded.join("src/core.rs"), expanded_src).unwrap();
    build_sidecar(&root, &expanded);

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    let payload = parse_json(&with);
    let rows = result_rows(&payload);

    // Wrapped payload must carry dedup_stats.
    if payload.is_object() {
        let stats = &payload["dedup_stats"];
        assert!(
            stats["merged_exact"].as_u64().unwrap_or(0) >= 1,
            "merged_exact expected for same path+name+enclosing Exact: {payload}"
        );
    }

    // No dual row: at most one row per (enclosing) for process.
    let enc = enclosings(&rows);
    let process_count = enc.iter().filter(|e| e.as_str() == "process").count();
    assert_eq!(
        process_count,
        1,
        "source path + expanded path must not double-count process: {enc:?} raw={}",
        stdout(&with)
    );

    // Sidecar-only candidates still present.
    assert!(enc.iter().any(|e| e == "fmt"), "fmt kept: {enc:?}");
    assert!(enc.iter().any(|e| e == "clone"), "clone kept: {enc:?}");

    // Mapped rows: process (if sidecar-origin remains) or main — either way
    // fmt/clone should be mapped=true when identity path map works.
    for r in rows_with_origin(&rows) {
        assert_eq!(r["origin"], "macro_expanded");
        // Identity layout maps src/core.rs.
        if r["mapped"].is_boolean() {
            // fmt/clone live in mapped src/core.rs
            assert!(
                r["path"].as_str().unwrap_or("").contains("src/core.rs")
                    || r["path"].as_str().unwrap_or("").contains("core"),
                "mapped sidecar path expected source-relative: {r}"
            );
        }
    }
}

/// Sidecar path cannot be mapped → keep row, origin=macro_expanded, mapped=false.
#[test]
fn unmappable_path_keeps_sidecar_row() {
    let (root, expanded) = temp_pair("unmap");
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
    // Expanded tree: helper defined in mystery path that does not map.
    std::fs::create_dir_all(expanded.join("mystery")).unwrap();
    std::fs::write(
        expanded.join("mystery/gen.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn clone() -> i32 { helper() }
"#,
    )
    .unwrap();
    build_sidecar(&root, &expanded);

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    let payload = parse_json(&with);
    let rows = result_rows(&payload);
    let side = rows_with_origin(&rows);
    assert!(
        !side.is_empty(),
        "unmappable sidecar row must be kept: {payload}"
    );
    for r in &side {
        assert_eq!(r["origin"], "macro_expanded");
        assert_eq!(
            r["mapped"], false,
            "unmappable path must set mapped=false: {r}"
        );
        // Original expanded path preserved when unmapped.
        let p = r["path"].as_str().unwrap_or("");
        assert!(
            p.contains("mystery") || p.contains("gen.rs"),
            "unmappable row keeps expanded path: {r}"
        );
    }
    if payload.is_object() {
        assert!(
            payload["dedup_stats"]["unmapped"].as_u64().unwrap_or(0) >= 1,
            "unmapped counter: {payload}"
        );
    }
}

/// Sidecar-only symbols (fmt/clone) are kept — not dropped by default noise filters.
#[test]
fn sidecar_only_symbols_are_kept_candidates() {
    let (root, expanded) = temp_pair("keep-only");
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn fmt() -> i32 { helper() }
pub fn clone() -> i32 { helper() }
pub fn eq() -> bool { helper() > 0 }
"#,
    )
    .unwrap();
    build_sidecar(&root, &expanded);

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    let enc = enclosings(&result_rows(&parse_json(&with)));
    for name in ["fmt", "clone", "eq"] {
        assert!(
            enc.iter().any(|e| e == name),
            "sidecar-only {name} must be kept: {enc:?}"
        );
    }
}

/// `--exact-only --with-macro` ignores the sidecar entirely (spec §1.4 lock).
#[test]
fn exact_only_with_macro_ignores_sidecar() {
    let (root, expanded) = temp_pair("exact-only");
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn clone() -> i32 { helper() }
"#,
    )
    .unwrap();
    build_sidecar(&root, &expanded);

    let with = run(
        &root,
        &["callers", "helper", "--with-macro", "--exact-only"],
    );
    assert!(with.status.success(), "{}", stderr(&with));
    let payload = parse_json(&with);
    let rows = result_rows(&payload);
    let enc = enclosings(&rows);
    assert!(
        enc.iter().any(|e| e == "process"),
        "main exact hit expected: {enc:?}"
    );
    assert!(
        !enc.iter().any(|e| e == "clone"),
        "exact-only must ignore sidecar (no clone): {enc:?} raw={}",
        stdout(&with)
    );
    assert!(
        rows_with_origin(&rows).is_empty(),
        "exact-only + with-macro must not tag sidecar rows: {}",
        stdout(&with)
    );
}

/// Main Heuristic wins over a sidecar row with the same logical key.
#[test]
fn main_heuristic_wins_over_sidecar_same_key() {
    let (root, expanded) = temp_pair("heur-win");
    // TS DI: container.register(helper) → Heuristic edge from wire.
    std::fs::write(
        root.join("src/core.ts"),
        r#"
export function helper() { return 1; }
export function wire(c: any) {
  c.register(helper);
}
"#,
    )
    .unwrap();
    // Expanded: same wire + exact helper() call in wire + sidecar-only clone.
    std::fs::write(
        expanded.join("src/core.ts"),
        r#"
export function helper() { return 1; }
export function wire(c: any) {
  c.register(helper);
  helper();
}
export function clone() { helper(); }
"#,
    )
    .unwrap();
    build_sidecar(&root, &expanded);

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    let payload = parse_json(&with);
    let rows = result_rows(&payload);

    // Main wire row must remain Heuristic (source evidence), not replaced by
    // a sidecar Exact-only row.
    let wire_rows: Vec<_> = rows.iter().filter(|r| r["enclosing"] == "wire").collect();
    assert!(!wire_rows.is_empty(), "wire caller expected: {payload}");
    // If any wire row is tagged macro_expanded, de-dup failed for same key.
    for r in &wire_rows {
        if r["origin"] == "macro_expanded" && r["mapped"] == true {
            panic!("sidecar duplicate of main wire edge survived de-dup: {r}");
        }
    }
    // At least one merge counted when wrapped.
    if payload.is_object() {
        let stats = &payload["dedup_stats"];
        let merged = stats["merged_exact"].as_u64().unwrap_or(0)
            + stats["merged_heuristic"].as_u64().unwrap_or(0);
        assert!(
            merged >= 1,
            "sidecar merge count expected for same logical edge: {payload}"
        );
    }
    // Sidecar-only clone still kept.
    let enc = enclosings(&rows);
    assert!(enc.iter().any(|e| e == "clone"), "clone kept: {enc:?}");
}

/// `--no-macro-dedup` keeps duplicate sidecar rows (debug flag).
#[test]
fn no_macro_dedup_keeps_duplicates() {
    let (root, expanded) = temp_pair("nodedup");
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn clone() -> i32 { helper() }
"#,
    )
    .unwrap();
    build_sidecar(&root, &expanded);

    let with = run(
        &root,
        &["callers", "helper", "--with-macro", "--no-macro-dedup"],
    );
    assert!(with.status.success(), "{}", stderr(&with));
    let payload = parse_json(&with);
    let rows = result_rows(&payload);
    let enc = enclosings(&rows);
    let process_count = enc.iter().filter(|e| e.as_str() == "process").count();
    assert!(
        process_count >= 2,
        "--no-macro-dedup must keep sidecar duplicate process rows: {enc:?} raw={}",
        stdout(&with)
    );
    if payload.is_object() {
        assert_eq!(
            payload["dedup_stats"]["merged_exact"].as_u64().unwrap_or(0),
            0,
            "no-dedup must not merge: {payload}"
        );
    }
}

/// `macro status` exposes dedup_stats after a with-macro query.
#[test]
fn macro_status_exposes_dedup_stats() {
    let (root, expanded) = temp_pair("status-dedup");
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn clone() -> i32 { helper() }
"#,
    )
    .unwrap();
    build_sidecar(&root, &expanded);

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));

    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let status = parse_json(&st);
    assert_eq!(status["exists"], true);
    assert!(
        status.get("dedup_stats").is_some(),
        "status must expose dedup_stats: {status}"
    );
    assert!(
        status.get("stale").is_some(),
        "status must expose stale: {status}"
    );
    assert!(
        status.get("path_map_present").is_some(),
        "status must expose path_map_present: {status}"
    );
}
