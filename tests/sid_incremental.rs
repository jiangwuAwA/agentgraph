//! Incremental reindex must not leave dangling `resolved_symbol_id` values.
//!
//! Scenario: file A defines `helper`; file B calls `helper`. After resolve, B's ref
//! has resolved_symbol_id → A's helper. Replacing A (DELETE symbols + INSERT new)
//! used to leave that old id dangling because resolve only filled NULL rows.
//! Fix: resolve_symbol_ids always clears all sids then fully relinks.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::PathBuf;

mod common;

fn temp_db(name: &str) -> PathBuf {
    common::temp_db(&format!("agentgraph-sid-test-{name}"))
}

#[test]
fn sid_cleared_and_relinked_after_incremental_reindex() {
    let db = temp_db("sid-incremental");
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();

    // File A: defines helper
    let a_src = r#"
pub fn helper() -> i32 { 1 }
"#;
    // File B: calls helper
    let b_src = r#"
pub fn run() { helper(); }
"#;
    let a = extract_file(a_src, Language::Rust, "src/a.rs", &known).unwrap();
    let b = extract_file(b_src, Language::Rust, "src/b.rs", &known).unwrap();

    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha1", "rust", &a).unwrap();
    store.replace_file("src/b.rs", "hb1", "rust", &b).unwrap();
    store.commit_batch().unwrap();

    let linked1 = store.resolve_symbol_ids().unwrap();
    assert!(
        linked1 > 0,
        "expected at least one linked ref, got {linked1}"
    );

    let callers_before = store.callers("helper", 50).unwrap();
    assert!(callers_before.iter().any(|r| r.path == "src/b.rs"));

    // Incremental reindex of A only: helper is REMOVED (renamed away).
    let a2_src = r#"
pub fn helper2() -> i32 { 2 }
"#;
    let a2 = extract_file(a2_src, Language::Rust, "src/a.rs", &known).unwrap();
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha2", "rust", &a2).unwrap();
    store.commit_batch().unwrap();

    // Old bug: resolve only filled WHERE resolved_symbol_id IS NULL, so B kept
    // the old id pointing at a deleted symbols row. After the fix, full clear+relink
    // drops that stale link — linked count must fall.
    let linked2 = store.resolve_symbol_ids().unwrap();
    assert!(
        linked2 < linked1,
        "stale sids were not cleared on incremental reindex: linked2={linked2} linked1={linked1}"
    );

    // Restore helper — links recover.
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha3", "rust", &a).unwrap();
    store.commit_batch().unwrap();
    let linked3 = store.resolve_symbol_ids().unwrap();
    assert!(
        linked3 >= linked1,
        "after restoring helper, links should be at least as many as before: {linked3} vs {linked1}"
    );

    // Remove helper again — links must drop again (no accumulation of stale ids).
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha4", "rust", &a2).unwrap();
    store.commit_batch().unwrap();
    let linked4 = store.resolve_symbol_ids().unwrap();
    assert!(
        linked4 < linked3,
        "stale sids were not cleared on second incremental reindex: linked4={linked4} linked3={linked3}"
    );
}

#[test]
fn sid_resolve_is_idempotent_full_relink() {
    let db = temp_db("sid-idempotent");
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();

    let src = r#"
fn a() { b(); }
fn b() { c(); }
fn c() {}
"#;
    let parsed = extract_file(src, Language::Rust, "src/mod.rs", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/mod.rs", "h1", "rust", &parsed)
        .unwrap();
    store.commit_batch().unwrap();

    let n1 = store.resolve_symbol_ids().unwrap();
    let n2 = store.resolve_symbol_ids().unwrap();
    assert_eq!(n1, n2, "resolve must be idempotent (clear + full relink)");

    // Reindex same content: resolve again, count stable.
    store.begin_batch().unwrap();
    store
        .replace_file("src/mod.rs", "h2", "rust", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    let n3 = store.resolve_symbol_ids().unwrap();
    assert_eq!(n1, n3, "same content reindex must preserve link count");
}

#[test]
fn savepoint_rolls_back_failed_file_without_poisoning_batch() {
    let db = temp_db("savepoint");
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();

    let ok_src = r#"
fn ok_fn() {}
"#;
    let ok = extract_file(ok_src, Language::Rust, "src/ok.rs", &known).unwrap();

    store.begin_batch().unwrap();
    // Successful file via savepoint
    store.begin_savepoint("file_sp").unwrap();
    store.replace_file("src/ok.rs", "hok", "rust", &ok).unwrap();
    store.release_savepoint("file_sp").unwrap();

    // A second file: open savepoint, write, then roll it back (simulates Err path)
    let bad_src = r#"
fn bad_fn() {}
"#;
    let bad = extract_file(bad_src, Language::Rust, "src/bad.rs", &known).unwrap();
    store.begin_savepoint("file_sp").unwrap();
    store
        .replace_file("src/bad.rs", "hbad", "rust", &bad)
        .unwrap();
    store.rollback_savepoint("file_sp").unwrap();

    store.commit_batch().unwrap();

    // ok.rs must be present; bad.rs must not.
    assert!(store.file_hash("src/ok.rs").unwrap().is_some());
    assert!(store.file_hash("src/bad.rs").unwrap().is_none());
    let ok_syms = store.find_symbol_exact("ok_fn", 5).unwrap();
    assert_eq!(ok_syms.len(), 1);
    let bad_syms = store.find_symbol_exact("bad_fn", 5).unwrap();
    assert!(
        bad_syms.is_empty(),
        "rollback should drop bad_fn: {bad_syms:?}"
    );
}
