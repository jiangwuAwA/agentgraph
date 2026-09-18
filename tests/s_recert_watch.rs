//! TDD Track M4: S re-certification after watch / `index_paths`.
//!
//! Contract:
//! - Dirty-file reindex refreshes subset violations for those paths.
//! - `subset` / `impact --sound` / `callers --sound` reflect **current disk**.
//! - Recovered (fixed) files clear stale violations.
//! - Deleted violating files do not leave permanent S debt (cascade/prune).

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentgraph::index::Indexer;

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-s-recert-{name}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
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

fn subset_json(root: &Path) -> serde_json::Value {
    let out = run(root, &["subset"]);
    // subset exits 2 when violations present; still prints JSON.
    let text = stdout(&out);
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "subset json parse failed ({e}): stdout={text} stderr={}",
            stderr(&out)
        )
    })
}

/// Write eval into a clean JS file → `index_paths` → subset shows violation +
/// promise_tier disabled; fix file → violation cleared.
#[test]
fn index_paths_recertifies_subset_on_dirty_file() {
    let root = temp_root("eval-toggle");
    let js = root.join("src/app.js");
    std::fs::write(
        &js,
        "export function safe(x) { return x + 1; }\nexport function main() { return safe(1); }\n",
    )
    .unwrap();

    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let before = subset_json(&root);
    assert_eq!(
        before["in_subset"],
        serde_json::json!(true),
        "clean fixture must start in S: {before}"
    );
    assert_eq!(
        before["promise_tier"].as_str(),
        Some("ast_modeled"),
        "clean corpus selects ast_modeled: {before}"
    );

    // Dirty the file with eval (leaves S_js).
    std::fs::write(
        &js,
        "export function evil(x) { return eval(x); }\nexport function main() { return evil('1+1'); }\n",
    )
    .unwrap();
    indexer
        .index_paths(std::slice::from_ref(&js))
        .expect("index_paths after eval");

    let dirty = subset_json(&root);
    assert_eq!(
        dirty["in_subset"],
        serde_json::json!(false),
        "eval must produce a stored S violation after index_paths: {dirty}"
    );
    assert!(
        dirty["violation_count"].as_u64().unwrap_or(0) >= 1,
        "expected >=1 violation: {dirty}"
    );
    assert_eq!(
        dirty["promise_tier"].as_str(),
        Some("disabled"),
        "promise_tier must be disabled when S violated: {dirty}"
    );
    let violations = dirty["violations"].as_array().cloned().unwrap_or_default();
    assert!(
        violations.iter().any(|v| {
            v["path"]
                .as_str()
                .map(|p| p.contains("app.js"))
                .unwrap_or(false)
                && v["kind"]
                    .as_str()
                    .map(|k| k.contains("eval"))
                    .unwrap_or(false)
        }),
        "expected eval violation on app.js: {dirty}"
    );

    // impact --sound must also see current-disk S state (not stale full-index OK).
    let sound = run(&root, &["impact", "main", "--sound"]);
    // Sound queries still succeed as JSON even when subset_ok=false.
    let sound_text = stdout(&sound);
    let sound_json: serde_json::Value = serde_json::from_str(&sound_text)
        .unwrap_or_else(|e| panic!("impact --sound json ({e}): {sound_text}"));
    assert_eq!(
        sound_json["subset_ok"],
        serde_json::json!(false),
        "impact --sound must reflect current disk S state: {sound_json}"
    );
    assert_eq!(
        sound_json["promise_tier"].as_str(),
        Some("disabled"),
        "sound promise must be disabled: {sound_json}"
    );

    // Fix the file → violation cleared after next index_paths.
    std::fs::write(
        &js,
        "export function safe(x) { return x + 1; }\nexport function main() { return safe(1); }\n",
    )
    .unwrap();
    indexer
        .index_paths(std::slice::from_ref(&js))
        .expect("index_paths after fix");

    let fixed = subset_json(&root);
    assert_eq!(
        fixed["in_subset"],
        serde_json::json!(true),
        "recovered file must clear S violation: {fixed}"
    );
    assert_eq!(
        fixed["promise_tier"].as_str(),
        Some("ast_modeled"),
        "promise restored after recovery: {fixed}"
    );
    let fixed_violations = fixed["violations"].as_array().cloned().unwrap_or_default();
    assert!(
        !fixed_violations.iter().any(|v| v["path"]
            .as_str()
            .map(|p| p.contains("app.js"))
            .unwrap_or(false)),
        "no stale app.js violations after fix: {fixed}"
    );
}

/// Deleting a violating source file must not leave permanent subset debt.
#[test]
fn deleted_violating_file_clears_subset_debt() {
    let root = temp_root("delete-violation");
    std::fs::write(
        root.join("src/clean.ts"),
        "export function ok() { return 1; }\n",
    )
    .unwrap();
    let bad = root.join("src/bad.ts");
    std::fs::write(&bad, "export function evil(x) { return eval(x); }\n").unwrap();

    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();
    let dirty = subset_json(&root);
    assert_eq!(dirty["in_subset"], serde_json::json!(false), "{dirty}");

    std::fs::remove_file(&bad).unwrap();
    // Full index prune path.
    indexer.index(false).unwrap();
    let after = subset_json(&root);
    assert_eq!(
        after["in_subset"],
        serde_json::json!(true),
        "deleted violating file must not leave S debt: {after}"
    );
}

/// Store-level API: refresh_subset_for_paths re-scans dirty paths from disk.
#[test]
fn store_refresh_subset_for_paths_updates_violations() {
    use agentgraph::index::subset::scan_subset;
    use agentgraph::model::Language;

    let root = temp_root("store-api");
    let js = root.join("src/x.js");
    std::fs::write(&js, "export function f() { return 1; }\n").unwrap();
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let mut store = indexer.open_store().unwrap();
    assert_eq!(store.subset_violation_count().unwrap(), 0);

    // Dirty on disk without going through extract — refresh API must re-scan.
    std::fs::write(&js, "export function f() { return eval('1'); }\n").unwrap();
    let path = "src/x.js";
    let n = store
        .refresh_subset_for_paths(std::slice::from_ref(&path.to_string()))
        .expect("refresh_subset_for_paths");
    assert!(n >= 1, "refresh should mint at least one violation row");
    let viols = store.subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.path == path && v.kind.contains("eval")),
        "refresh must store eval violation: {viols:?}"
    );

    // Direct scanner sanity (same source the refresh used).
    let src = std::fs::read_to_string(&js).unwrap();
    let report = scan_subset(&src, Language::JavaScript, path);
    assert!(!report.in_subset);

    // Fix + refresh → cleared.
    std::fs::write(&js, "export function f() { return 1; }\n").unwrap();
    store
        .refresh_subset_for_paths(std::slice::from_ref(&path.to_string()))
        .expect("refresh after fix");
    assert_eq!(
        store.subset_violations().unwrap().len(),
        0,
        "fix + refresh must clear violations"
    );
}
