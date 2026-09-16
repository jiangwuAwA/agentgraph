//! L3 invariant tests (PLAN §5.2 I1–I3) — executable companion to formal/.
//!
//! These are *not* machine-checked proofs. They pin the same properties the
//! TLA+ model reasons about, so CI fails if an implementation change breaks
//! an invariant the formal model assumes.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{Confidence, EdgeKind, Language};
use std::collections::HashSet;
use std::path::PathBuf;

fn temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-l3-inv-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("index.db")
}

/// I1: every call_expression AST node yields ≥1 Exact call ref (strengthened R4).
#[test]
fn i1_every_direct_call_yields_exact_ref() {
    let src = r#"
export function a() { return 1; }
export function b() { return a(); }
export function c() { b(); a(); }
export class S {
  m() { return a(); }
  n() { return this.m(); }
}
"#;
    let out = extract_file(src, Language::TypeScript, "src/i1.ts", &HashSet::new()).unwrap();
    let exact_calls: Vec<_> = out
        .references
        .iter()
        .filter(|r| r.kind == EdgeKind::Call && r.confidence == Confidence::Exact)
        .collect();
    assert!(
        exact_calls.len() >= 4,
        "I1: expected ≥4 Exact call refs, got {exact_calls:?}"
    );
    use agentgraph::index::parser;
    let tree = parser::parse(src, Language::TypeScript).unwrap();
    fn collect_callees(n: tree_sitter::Node, src: &str, out: &mut Vec<String>) {
        if n.kind() == "call_expression" {
            if let Some(f) = n.child_by_field_name("function") {
                let t = src.get(f.byte_range()).unwrap_or("");
                let name = t.rsplit(['.', ':']).next().unwrap_or(t).trim().to_string();
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            collect_callees(ch, src, out);
        }
    }
    let mut callees = Vec::new();
    collect_callees(tree.root_node(), src, &mut callees);
    assert!(!callees.is_empty());
    for callee in &callees {
        assert!(
            exact_calls.iter().any(|r| &r.name == callee),
            "I1: call_expression {callee} has no Exact ref among {exact_calls:?}"
        );
    }
}

/// I2: after file content changes, DB rows for that path match extract(C').
#[test]
fn i2_incremental_replace_is_isomorphic_to_extract() {
    let db = temp_db("i2");
    let mut store = Store::open(&db).unwrap();
    let v1 = "export function a() { return 1; }\nexport function b() { return a(); }\n";
    let p1 = extract_file(v1, Language::TypeScript, "src/f.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/f.ts", "h1", "typescript", &p1)
        .unwrap();
    store.commit_batch().unwrap();
    let before = store.callers("a", 10).unwrap();
    assert_eq!(before.len(), 1);

    let v2 = "export function a() { return 2; }\nexport function c() { return a(); }\n";
    let p2 = extract_file(v2, Language::TypeScript, "src/f.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/f.ts", "h2", "typescript", &p2)
        .unwrap();
    store.commit_batch().unwrap();

    let after = store.callers("a", 10).unwrap();
    assert_eq!(
        after.len(),
        p2.references
            .iter()
            .filter(|r| r.name == "a" && r.kind == EdgeKind::Call)
            .count(),
        "I2: DB callers(a) must match extract(C')"
    );
    // b's call must be gone
    let b_callers = store.callers("b", 10).unwrap();
    assert!(
        b_callers.is_empty(),
        "I2: stale b() call must not remain after replace"
    );
}

/// I3: impact BFS depth-1 = direct callers; depth-2 includes callers of those enclosings.
#[test]
fn i3_impact_bfs_depth_semantics() {
    let db = temp_db("i3");
    let mut store = Store::open(&db).unwrap();
    let src = r#"
export function leaf() { return 1; }
export function mid() { return leaf(); }
export function top() { return mid(); }
"#;
    let parsed = extract_file(src, Language::TypeScript, "src/g.ts", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/g.ts", "h", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();

    let d1 = store.impact("leaf", 1, 50).unwrap();
    assert!(
        d1.iter().any(|n| n.enclosing.as_deref() == Some("mid")),
        "depth1 impact(leaf) must include mid: {d1:?}"
    );
    assert!(
        !d1.iter().any(|n| n.enclosing.as_deref() == Some("top")),
        "depth1 must not include top: {d1:?}"
    );

    let d2 = store.impact("leaf", 2, 50).unwrap();
    assert!(
        d2.iter().any(|n| n.enclosing.as_deref() == Some("top")),
        "depth2 impact(leaf) must include top: {d2:?}"
    );
}

/// formal/ artifacts exist (PLAN §5.4) — CI does not require TLC/Apalache.
#[test]
fn formal_track_artifacts_present() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        root.join("formal/README.md").is_file(),
        "formal/README.md required"
    );
    assert!(
        root.join("formal/IncrementalIndex.tla").is_file(),
        "formal/IncrementalIndex.tla required"
    );
}
