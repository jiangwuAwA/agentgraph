//! TDD: L2 production S-sound — event emit↔on dispatch closure.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{Confidence, ConfidenceFilter, Language};
use std::collections::HashSet;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentgraph-l2-prod-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn emit_on_same_file_produces_dispatch_edge() {
    let db = temp_dir("evt").join("index.db");
    let mut store = Store::open(&db).unwrap();
    let src = r#"
export function handleClick() { return 1; }
export function wire(bus: any) {
  bus.on('click', handleClick);
}
export function fire(bus: any) {
  bus.emit('click');
}
"#;
    let parsed = extract_file(src, Language::JavaScript, "src/bus.js", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/bus.js", "h", "javascript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store.link_event_dispatch().expect("dispatch link");
    store.resolve_symbol_ids().unwrap();

    let (hits, viols) = store.callers_sound("handleClick", 20).unwrap();
    assert!(viols.is_empty());
    assert!(
        hits.iter()
            .any(|r| r.enclosing.as_deref() == Some("fire") && r.name == "handleClick"),
        "emit in fire must produce sound edge to handleClick; hits={hits:?}"
    );
}

#[test]
fn emit_without_on_is_harmless() {
    let db = temp_dir("evt2").join("index.db");
    let mut store = Store::open(&db).unwrap();
    let src = "export function fire(bus: any) { bus.emit('orphan'); }\n";
    let parsed = extract_file(src, Language::JavaScript, "src/o.js", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/o.js", "h", "javascript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store.link_event_dispatch().unwrap();
    store.resolve_symbol_ids().unwrap();
    let (hits, _) = store.callers_sound("orphan", 10).unwrap();
    // emit edge itself is DynamicCandidate (sound-eligible finite domain)
    assert!(hits.iter().any(|r| r.name == "orphan") || hits.is_empty());
}

#[test]
fn dispatch_edges_are_heuristic_and_sound_eligible() {
    let db = temp_dir("evt3").join("index.db");
    let mut store = Store::open(&db).unwrap();
    let src = r#"
export function handler() { return 2; }
export function setup(b: any) { b.on('trade', handler); }
export function pump(b: any) { b.emit('trade'); }
"#;
    let parsed = extract_file(src, Language::TypeScript, "src/t.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/t.ts", "h", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store.link_event_dispatch().unwrap();
    store.resolve_symbol_ids().unwrap();
    let hits = store
        .callers_filtered("handler", 20, ConfidenceFilter::Default)
        .unwrap();
    let disp = hits
        .iter()
        .find(|r| r.enclosing.as_deref() == Some("pump"))
        .expect("dispatch edge");
    assert_eq!(disp.confidence, Confidence::Heuristic);
    let ev = disp.evidence.as_ref().expect("evidence");
    assert!(ev.rule_id.contains("dispatch") || ev.rule_id.contains("event"));
}
