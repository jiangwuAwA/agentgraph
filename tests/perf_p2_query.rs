//! P2: query latency smoke on a synthetic multi-file DB (budget, not strict CI gate).

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{ConfidenceFilter, Language};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

fn build_store(n_files: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-p2-q-{n_files}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("index.db");
    let mut store = Store::open(&db).unwrap();
    store.begin_batch().unwrap();
    for i in 0..n_files {
        let src = format!(
            "export class S{i} {{ save() {{ return {i}; }} }}\nexport function u{i}(s: S{i}) {{ return s.save(); }}\n"
        );
        let path = format!("src/f{i}.ts");
        let parsed = extract_file(&src, Language::TypeScript, &path, &HashSet::new()).unwrap();
        store
            .replace_file(&path, &format!("h{i}"), "typescript", &parsed)
            .unwrap();
    }
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    db
}

#[test]
fn callers_and_qualified_callers_complete_quickly() {
    let db = build_store(200);
    let store = Store::open(&db).unwrap();
    // Warm
    let _ = store
        .callers_filtered("save", 50, ConfidenceFilter::Default)
        .unwrap();
    let t = Instant::now();
    for i in 0..50 {
        let _ = store
            .callers_filtered(&format!("S{i}.save"), 20, ConfidenceFilter::Default)
            .unwrap();
        let _ = store
            .callers_filtered("save", 20, ConfidenceFilter::Default)
            .unwrap();
    }
    let elapsed = t.elapsed();
    // Soft budget: 100 qualified+bare queries on 200-file synthetic DB.
    assert!(
        elapsed.as_millis() < 2000,
        "query loop too slow: {elapsed:?}"
    );
}
