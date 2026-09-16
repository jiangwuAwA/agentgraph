//! TDD: L2 production S-sound — event emit↔on dispatch closure (Critical fixes).

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

fn seed_js(tag: &str, src: &str) -> Store {
    let db = temp_dir(tag).join("index.db");
    let mut store = Store::open(&db).unwrap();
    let parsed = extract_file(src, Language::JavaScript, "src/bus.js", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/bus.js", "h", "javascript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store.link_event_dispatch().unwrap();
    store.resolve_symbol_ids().unwrap();
    store
}

#[test]
fn emit_on_same_file_produces_dispatch_edge() {
    let store = seed_js(
        "evt",
        r#"
export function handleClick() { return 1; }
export function wire(bus: any) {
  bus.on('click', handleClick);
}
export function fire(bus: any) {
  bus.emit('click');
}
"#,
    );
    let (hits, viols) = store.callers_sound("handleClick", 20).unwrap();
    assert!(viols.is_empty());
    assert!(
        hits.iter()
            .any(|r| r.enclosing.as_deref() == Some("fire") && r.name == "handleClick"),
        "emit in fire must produce sound edge to handleClick; hits={hits:?}"
    );
}

#[test]
fn once_is_subscribe_and_pairs_with_emit() {
    let store = seed_js(
        "once",
        r#"
export function onTrade() { return 1; }
export function setup(bus: any) { bus.once('trade', onTrade); }
export function pump(bus: any) { bus.emit('trade'); }
"#,
    );
    let (hits, _) = store.callers_sound("onTrade", 20).unwrap();
    assert!(
        hits.iter().any(|r| r.enclosing.as_deref() == Some("pump")),
        "once must pair with emit; hits={hits:?}"
    );
}

#[test]
fn link_event_dispatch_is_idempotent() {
    let mut store = seed_js(
        "idem",
        r#"
export function h() { return 1; }
export function s(b: any) { b.on('e', h); }
export function f(b: any) { b.emit('e'); }
"#,
    );
    let n1 = store.event_dispatch_count().unwrap();
    assert!(n1 >= 1);
    store.link_event_dispatch().unwrap();
    store.link_event_dispatch().unwrap();
    let n2 = store.event_dispatch_count().unwrap();
    assert_eq!(n1, n2, "dispatch rebuild must not accumulate duplicates");
}

#[test]
fn arrow_handler_collects_all_call_targets() {
    let out = extract_file(
        r#"
export function handleA() { return 1; }
export function handleB() { return 2; }
export function wire(bus: any) {
  bus.on('e', () => { handleA(); handleB(); });
}
export function fire(bus: any) { bus.emit('e'); }
"#,
        Language::JavaScript,
        "src/arrow.js",
        &HashSet::new(),
    )
    .unwrap();
    let names: Vec<_> = out
        .references
        .iter()
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == "ts.event.subscribe")
                .unwrap_or(false)
        })
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        names.contains(&"handleA") && names.contains(&"handleB"),
        "arrow multi-call handler must record both callees; got {names:?}"
    );
}

#[test]
fn index_paths_rebuilds_dispatch() {
    let root = temp_dir("watch-disp");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/a.js"),
        "export function h() { return 1; }\nexport function s(b:any) { b.on('e', h); }\n",
    )
    .unwrap();
    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(false).unwrap();
    std::fs::write(
        root.join("src/a.js"),
        "export function h() { return 1; }\nexport function s(b:any) { b.on('e', h); }\nexport function f(b:any) { b.emit('e'); }\n",
    )
    .unwrap();
    indexer
        .index_paths(&[indexer.root.join("src/a.js")])
        .unwrap();
    let store = indexer.open_store().unwrap();
    let (hits, _) = store.callers_sound("h", 20).unwrap();
    assert!(
        hits.iter().any(|r| r.enclosing.as_deref() == Some("f")),
        "path-scoped index must rebuild dispatch; hits={hits:?}"
    );
}

#[test]
fn subscript_emit_and_on_pair() {
    let store = seed_js(
        "sub",
        r#"
export function h() { return 1; }
export function s(b: any) { b['on']('e', h); }
export function f(b: any) { b['emit']('e'); }
"#,
    );
    let (hits, _) = store.callers_sound("h", 20).unwrap();
    assert!(
        hits.iter().any(|r| r.enclosing.as_deref() == Some("f")),
        "bus['emit'] must pair with bus['on']; hits={hits:?}"
    );
}

#[test]
fn generator_function_handler_collects_calls() {
    let out = extract_file(
        r#"
export function handleY() { return 1; }
export function wire(bus: any) {
  bus.on('e', function* () { yield handleY(); });
}
"#,
        Language::JavaScript,
        "src/gen.js",
        &HashSet::new(),
    )
    .unwrap();
    let names: Vec<_> = out
        .references
        .iter()
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == "ts.event.subscribe")
                .unwrap_or(false)
        })
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        names.contains(&"handleY"),
        "generator handler must record handleY; got {names:?}"
    );
}

#[test]
fn dispatch_dirty_repairs_on_noop_index() {
    let root = temp_dir("dirty-repair");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/a.js"),
        "export function h() { return 1; }\nexport function s(b:any) { b.on('e', h); }\nexport function f(b:any) { b.emit('e'); }\n",
    )
    .unwrap();
    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(false).unwrap();
    {
        let mut store = indexer.open_store().unwrap();
        // Simulate failed dispatch rebuild after durable file commit.
        store.set_meta("dispatch_dirty", "1").unwrap();
        // Drop dispatch rows to simulate crash after DELETE.
        store.clear_dispatch_edges_for_test().unwrap();
        assert!(store.dispatch_dirty().unwrap());
    }
    // Noop index must repair dispatch.
    indexer.index(false).unwrap();
    let store = indexer.open_store().unwrap();
    assert!(!store.dispatch_dirty().unwrap());
    let (hits, _) = store.callers_sound("h", 20).unwrap();
    assert!(
        hits.iter().any(|r| r.enclosing.as_deref() == Some("f")),
        "noop index must repair dispatch edges; hits={hits:?}"
    );
}

#[test]
fn index_paths_early_out_repairs_dispatch_dirty() {
    let root = temp_dir("ip-dirty");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/a.js"),
        "export function h() { return 1; }\nexport function s(b:any) { b.on('e', h); }\nexport function f(b:any) { b.emit('e'); }\n",
    )
    .unwrap();
    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(false).unwrap();
    {
        let mut store = indexer.open_store().unwrap();
        store.set_meta("dispatch_dirty", "1").unwrap();
        store.clear_dispatch_edges_for_test().unwrap();
    }
    // index_paths on an unchanged file → hash-skip early-out; must still repair.
    indexer
        .index_paths(&[indexer.root.join("src/a.js")])
        .unwrap();
    let store = indexer.open_store().unwrap();
    assert!(!store.dispatch_dirty().unwrap());
    let (hits, _) = store.callers_sound("h", 20).unwrap();
    assert!(
        hits.iter().any(|r| r.enclosing.as_deref() == Some("f")),
        "index_paths early-out must repair dispatch; hits={hits:?}"
    );
}

#[test]
fn emit_without_on_has_no_dispatch_edge() {
    let store = seed_js(
        "evt2",
        r#"
export function orphan() { return 1; }
export function fire(bus: any) { bus.emit('orphan'); }
"#,
    );
    let (hits, _) = store.callers_sound("orphan", 10).unwrap();
    let dispatch = hits.iter().any(|r| {
        r.enclosing.as_deref() == Some("fire")
            && r.evidence
                .as_ref()
                .map(|e| e.rule_id == "ts.event.dispatch")
                .unwrap_or(false)
    });
    assert!(
        !dispatch,
        "must not invent dispatch without subscribe; hits={hits:?}"
    );
}

#[test]
fn dispatch_edges_are_heuristic_and_sound_eligible() {
    let store = seed_js(
        "evt3",
        r#"
export function handler() { return 2; }
export function setup(b: any) { b.on('trade', handler); }
export function pump(b: any) { b.emit('trade'); }
"#,
    );
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
