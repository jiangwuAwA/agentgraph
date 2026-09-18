//! TDD: same-line multi-ref SCIP export — distinct ranges + Document.symbols placement.
//!
//! Issue 1: `name_cols_on_line` used `text.find(name)` (first occurrence only),
//! so two refs to the same name on one line shared columns → scip lint duplicate.
//!
//! Issue 5: definition symbols must live in `Document.symbols`, not `external_symbols`.

use agentgraph::index::export::{export_scip, export_scip_json};
use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod common;

fn temp_dir(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-sameline-{name}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
    dir
}

/// `function a(){}; export function b(){ return a(1)+a(2); }`
/// Two refs to `a` on the same line must get DISTINCT column ranges.
const SAME_LINE_SRC: &str = "function a(){}; export function b(){ return a(1)+a(2); }\n";

fn seed_same_line(root: &Path) -> Store {
    let db = root.join(".agentgraph").join("index.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();
    let parsed = extract_file(SAME_LINE_SRC, Language::JavaScript, "src/mod.js", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/mod.js", "h1", "javascript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    // Write the source file so name_cols_on_line can read it.
    std::fs::write(root.join("src/mod.js"), SAME_LINE_SRC).unwrap();
    store
}

#[test]
fn same_line_multi_ref_gets_distinct_ranges() {
    let dir = temp_dir("ranges");
    let store = seed_same_line(&dir);
    let out = dir.join("index.scip.json");
    export_scip_json(&store, &dir, &out).unwrap();

    let text = std::fs::read_to_string(&out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let docs = v["documents"].as_array().unwrap();
    let doc = docs
        .iter()
        .find(|d| d["relativePath"] == "src/mod.js")
        .expect("src/mod.js document");

    // Collect reference occurrence ranges (symbolRoles == 0 or absent) for symbol `a.`
    let mut ref_ranges: Vec<(i64, i64, i64)> = Vec::new();
    for occ in doc["occurrences"].as_array().unwrap() {
        let sym = occ["symbol"].as_str().unwrap_or("");
        let roles = occ.get("symbolRoles").and_then(|r| r.as_i64()).unwrap_or(0);
        if sym.contains(" a.") && roles & 1 == 0 {
            let range = occ["range"].as_array().unwrap();
            ref_ranges.push((
                range[0].as_i64().unwrap(),
                range[1].as_i64().unwrap(),
                range[2].as_i64().unwrap(),
            ));
        }
    }
    assert!(
        ref_ranges.len() >= 2,
        "expected >=2 reference occurrences for `a`, got {ref_ranges:?}"
    );
    // First two ref ranges must differ in start column.
    assert_ne!(
        ref_ranges[0].1, ref_ranges[1].1,
        "two refs to `a` on same line must have distinct start columns: {ref_ranges:?}"
    );
}

#[test]
fn same_line_scip_binary_lints_clean() {
    let dir = temp_dir("lint");
    let store = seed_same_line(&dir);
    let out = dir.join("index.scip");
    export_scip(&store, &dir, &out).unwrap();

    // If scip CLI is available, lint must exit 0.
    let scip = which_scip();
    let Some(scip) = scip else {
        eprintln!("skip: scip CLI not found");
        return;
    };
    let lint = std::process::Command::new(&scip)
        .arg("lint")
        .arg(&out)
        .output()
        .expect("run scip lint");
    assert!(
        lint.status.success(),
        "scip lint failed: {}",
        String::from_utf8_lossy(&lint.stderr)
    );
}

#[test]
fn definitions_live_in_document_symbols_not_external() {
    let dir = temp_dir("docsyms");
    let store = seed_same_line(&dir);
    let out = dir.join("index.scip.json");
    export_scip_json(&store, &dir, &out).unwrap();

    let text = std::fs::read_to_string(&out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();

    // external_symbols must be empty (or absent) — only true externals go there.
    let ext = v
        .get("externalSymbols")
        .and_then(|s| s.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(
        ext, 0,
        "external_symbols must be empty for local definitions, got {ext}"
    );

    // Each document must have symbols matching its definition occurrences.
    let docs = v["documents"].as_array().unwrap();
    assert!(!docs.is_empty());
    for doc in docs {
        let def_count = doc["occurrences"]
            .as_array()
            .map(|occs| {
                occs.iter()
                    .filter(|o| o.get("symbolRoles").and_then(|r| r.as_i64()).unwrap_or(0) & 1 == 1)
                    .count()
            })
            .unwrap_or(0);
        let sym_count = doc["symbols"].as_array().map(|s| s.len()).unwrap_or(0);
        assert!(
            sym_count >= def_count,
            "document {} has {def_count} def occurrences but only {sym_count} symbols",
            doc["relativePath"].as_str().unwrap_or("?")
        );
        assert!(
            sym_count > 0,
            "document {} must have at least one SymbolInformation",
            doc["relativePath"].as_str().unwrap_or("?")
        );
    }
}

fn which_scip() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["scip.exe", "scip"] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
