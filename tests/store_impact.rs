use agentgraph::index::store::Store;
use agentgraph::model::Language;
use agentgraph::index::extract::extract_file;
use std::collections::HashSet;
use std::path::PathBuf;

fn temp_db(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-test-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("index.db")
}

#[test]
fn impact_is_bfs_and_preserves_descriptions() {
    let db = temp_db("impact");
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();

    // a -> b -> c call chain in one fake file
    let src = r#"
fn a() { b(); }
fn b() { c(); }
fn c() {}
"#;
    let parsed = extract_file(src, Language::Rust, "src/mod.rs", &known).unwrap();
    store.begin_batch().unwrap();
    store.replace_file("src/mod.rs", "h1", "rust", &parsed).unwrap();
    store.commit_batch().unwrap();

    // set description on `a`
    let syms = store.find_symbol("a", 10).unwrap();
    let a = syms.iter().find(|s| s.name == "a").expect("symbol a");
    store.set_description(a.id, "entrypoint").unwrap();

    // reindex same file — description must survive
    store.begin_batch().unwrap();
    store.replace_file("src/mod.rs", "h2", "rust", &parsed).unwrap();
    store.commit_batch().unwrap();
    let syms2 = store.find_symbol("a", 10).unwrap();
    let a2 = syms2.iter().find(|s| s.name == "a").unwrap();
    assert_eq!(a2.description.as_deref(), Some("entrypoint"));

    // impact from c: call sites of c (depth1, enclosing b) then of b (depth2, enclosing a)
    let impact = store.impact("c", 3, 50).unwrap();
    assert!(!impact.is_empty(), "impact empty: {:?}", impact);
    let d1 = impact.iter().find(|i| i.depth == 1).expect("has depth 1");
    assert_eq!(d1.name, "c");
    assert_eq!(d1.enclosing.as_deref(), Some("b"));
    let d2 = impact.iter().find(|i| i.depth == 2).expect("has depth 2");
    assert_eq!(d2.name, "b");
    assert_eq!(d2.enclosing.as_deref(), Some("a"));
}
