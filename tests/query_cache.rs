//! TDD: L0.4 — repeated identical queries on the same Store hit a cache
//! (second call does not re-run the full SQL path; observable via timing or
//! an explicit cache-stat counter).
use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

mod common;

fn seed_db(tag: &str) -> (PathBuf, Store) {
    let dir = common::temp_root(&format!("agentgraph-cache-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("index.db");
    let mut store = Store::open(&db).unwrap();
    let known = HashSet::new();
    let src = r#"
export function a() { return 1; }
export function b() { return a() + a(); }
export function c() { return b(); }
"#;
    let parsed = extract_file(src, Language::TypeScript, "src/m.ts", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/m.ts", "h", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    (db, store)
}

#[test]
fn repeated_callers_query_hits_cache() {
    let (_db, store) = seed_db("hit");
    // Warm
    let _ = store.callers("a", 50).unwrap();
    let hits0 = store.cache_hits();
    let misses0 = store.cache_misses();

    let _ = store.callers("a", 50).unwrap();
    let hits1 = store.cache_hits();
    let misses1 = store.cache_misses();

    assert!(
        hits1 > hits0,
        "second identical callers() must increment cache hits (hits {hits0}->{hits1})"
    );
    assert_eq!(
        misses1, misses0,
        "second identical callers() must not miss again"
    );
}

#[test]
fn cache_distinguishes_different_queries() {
    let (_db, store) = seed_db("keys");
    let _ = store.callers("a", 50).unwrap();
    let _ = store.callers("b", 50).unwrap();
    let _ = store.callers("a", 50).unwrap();
    // a, b, a → 2 misses + 1 hit
    assert!(store.cache_hits() >= 1);
    assert!(store.cache_misses() >= 2);
}

#[test]
fn replace_file_invalidates_callers_cache() {
    let (_db, mut store) = seed_db("inv");
    let before = store.callers("a", 50).unwrap().len();
    assert!(before >= 1);

    // Rewrite file without calls to a
    let known = HashSet::new();
    let src = "export function a() { return 1; }\n";
    let parsed = extract_file(src, Language::TypeScript, "src/m.ts", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/m.ts", "h2", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();

    let after = store.callers("a", 50).unwrap().len();
    assert!(
        after < before,
        "cache must be invalidated on write (before={before}, after={after})"
    );
}

#[test]
fn prune_missing_clears_query_cache() {
    // m8: prune_missing deletes files/refs but must also clear the in-memory
    // callers/impact cache or callers() returns stale rows.
    let (_db, mut store) = seed_db("prune");
    let before = store.callers("a", 50).unwrap();
    assert!(!before.is_empty(), "seed must produce callers of a");
    store.prune_missing(&[]).unwrap();
    let after = store.callers("a", 50).unwrap();
    assert!(
        after.is_empty(),
        "cache must not return stale callers after prune: {after:?}"
    );
}

#[test]
fn cached_query_not_pathologically_slower_first_time() {
    let (_db, store) = seed_db("perf");
    // First cold query
    let t0 = Instant::now();
    let _ = store.impact("a", 3, 100).unwrap();
    let cold = t0.elapsed();
    // Repeat — should not be slower on average (allow noise)
    let t1 = Instant::now();
    for _ in 0..20 {
        let _ = store.impact("a", 3, 100).unwrap();
    }
    let warm_each = t1.elapsed() / 20;
    // Soft check: warm path should be <= cold * 2 (cache helps or at least doesn't hurt badly)
    assert!(
        warm_each <= cold.saturating_mul(2) + std::time::Duration::from_millis(5),
        "warm={warm_each:?} cold={cold:?}"
    );
}
