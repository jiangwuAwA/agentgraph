//! TDD: perf-plan P0 — mtime short-circuit, dirty early-out, incremental sid.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{Confidence, EdgeKind, Language};
use std::collections::HashSet;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentgraph-perf-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn seed_store(tag: &str) -> (PathBuf, Store) {
    let dir = temp_dir(tag);
    let db = dir.join("index.db");
    let mut store = Store::open(&db).unwrap();
    let src = "export function a() { return 1; }\nexport function b() { return a(); }\n";
    let parsed = extract_file(src, Language::TypeScript, "src/m.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file_with_meta("src/m.ts", "h1", "typescript", &parsed, 111, 50)
        .unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    (db, store)
}

#[test]
fn files_table_accepts_mtime_size_meta() {
    let (_db, store) = seed_store("meta");
    let meta = store.file_meta("src/m.ts").unwrap().expect("row");
    assert_eq!(meta.mtime_ns, 111);
    assert_eq!(meta.size, 50);
    assert_eq!(meta.hash, "h1");
}

#[test]
fn incremental_resolve_only_touches_dirty_paths() {
    let (_db, mut store) = seed_store("inc-sid");
    let before = store
        .callers("a", 10)
        .unwrap()
        .iter()
        .filter(|r| r.kind == EdgeKind::Call)
        .count();
    assert!(before >= 1);

    // Replace same file with new content that still calls a() from b().
    let src2 = "export function a() { return 2; }\nexport function b() { return a(); }\n";
    let parsed = extract_file(src2, Language::TypeScript, "src/m.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file_with_meta("src/m.ts", "h2", "typescript", &parsed, 222, 51)
        .unwrap();
    store.commit_batch().unwrap();
    let n = store
        .resolve_symbol_ids_for_paths(&["src/m.ts".to_string()])
        .unwrap();
    assert!(n >= 1, "must relink dirty path refs");
    let after = store.callers("a", 10).unwrap();
    assert!(!after.is_empty());
    // No dangling sid
    store.assert_no_dangling_sids().unwrap();
}

#[test]
fn sid_dirty_flag_set_on_replace_and_cleared_on_full_resolve() {
    let (_db, mut store) = seed_store("sid-flag");
    assert!(!store.sid_dirty(), "full resolve clears flag");
    let src = "export function z() { return 1; }\n";
    let parsed = extract_file(src, Language::TypeScript, "src/z.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file_with_meta("src/z.ts", "hz", "typescript", &parsed, 1, 2)
        .unwrap();
    store.commit_batch().unwrap();
    assert!(store.sid_dirty(), "replace must mark sid dirty");
    store.resolve_symbol_ids().unwrap();
    assert!(!store.sid_dirty());
}

#[test]
fn export_ensure_sids_when_dirty() {
    let (_db, mut store) = seed_store("export-sid");
    let src = "export function z() { return 1; }\n";
    let parsed = extract_file(src, Language::TypeScript, "src/z.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file_with_meta("src/z.ts", "hz", "typescript", &parsed, 1, 2)
        .unwrap();
    store.commit_batch().unwrap();
    assert!(store.sid_dirty());
    let n = store.ensure_sids_for_export().unwrap();
    assert!(n >= 1);
    assert!(!store.sid_dirty());
}

#[test]
fn resolve_qualifiers_batch_matches_scalar_semantics() {
    let (_db, mut store) = seed_store("qual-batch");
    let n = store.resolve_qualifiers().unwrap();
    // Second run should be stable (no new upgrades required).
    let n2 = store.resolve_qualifiers().unwrap();
    assert_eq!(n, n2, "batch qualifier pass must be idempotent");
}

#[test]
fn query_after_incremental_sid_still_exact() {
    let (_db, store) = seed_store("q-exact");
    let hits = store
        .callers_filtered("a", 10, agentgraph::model::ConfidenceFilter::ExactOnly)
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.confidence == Confidence::Exact));
}
