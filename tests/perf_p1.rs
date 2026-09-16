//! TDD: set-based full sid relink + watch path-scoped index + query p95 helper.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{ConfidenceFilter, Language};
use std::collections::HashSet;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentgraph-p1-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn set_based_full_resolve_links_refs() {
    let db = temp_dir("sid").join("index.db");
    let mut store = Store::open(&db).unwrap();
    let src = r#"
export class UserService {
  load() { return 1; }
}
export function boot(c: UserService) {
  return c.load();
}
"#;
    let parsed = extract_file(src, Language::TypeScript, "src/a.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/a.ts", "h", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    let n = store.resolve_symbol_ids().unwrap();
    assert!(n >= 1, "set-based resolve must link some refs");
    store.assert_no_dangling_sids().unwrap();
    let hits = store
        .callers_filtered("load", 10, ConfidenceFilter::ExactOnly)
        .unwrap();
    assert!(!hits.is_empty());
}

#[test]
fn path_scoped_index_updates_only_that_file() {
    let root = temp_dir("watch-scope");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.ts"), "export function a() { return 1; }\n").unwrap();
    std::fs::write(root.join("src/b.ts"), "export function b() { return 2; }\n").unwrap();
    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    let stats = indexer.index(false).unwrap();
    assert_eq!(stats.files, 2);

    // Change only a.ts
    std::fs::write(
        root.join("src/a.ts"),
        "export function a() { return 11; }\nexport function a2() { return a(); }\n",
    )
    .unwrap();
    let stats2 = indexer
        .index_paths(&[indexer.root.join("src/a.ts")])
        .expect("path-scoped index");
    assert!(
        stats2.files >= 2,
        "stats still count full tree files={}",
        stats2.files
    );
    let store = indexer.open_store().unwrap();
    let syms = store.find_symbol_exact("a2", 10).unwrap();
    assert!(
        !syms.is_empty(),
        "symbol a2 must appear after path-scoped reindex of a.ts"
    );
    let callers_a = store.callers("a", 10).unwrap();
    assert!(
        callers_a
            .iter()
            .any(|r| r.enclosing.as_deref() == Some("a2")),
        "callers(a) must include enclosing a2; got {callers_a:?}"
    );
}

#[test]
fn qualified_callers_use_indexable_column() {
    let db = temp_dir("qual").join("index.db");
    let mut store = Store::open(&db).unwrap();
    let src = r#"
export class Store {
  save() { return 1; }
}
export function useIt(s: Store) {
  return s.save();
}
"#;
    let parsed = extract_file(src, Language::TypeScript, "src/s.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/s.ts", "h", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    let hits = store
        .callers_filtered("Store.save", 10, ConfidenceFilter::Default)
        .unwrap();
    assert!(
        !hits.is_empty(),
        "qualified callers Store.save must hit; got {:?}",
        hits
    );
}
