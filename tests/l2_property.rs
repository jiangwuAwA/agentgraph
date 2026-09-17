//! TDD (L2 hardening): property-style checks that S_js / S_py / S_go
//! programs keep the "every direct call has an Exact edge" invariant, and
//! that impact --sound contains the golden caller set for generated programs.
//!
//! Generators are intentionally small and deterministic (no proptest dep):
//! random call graphs of N functions with only static calls — always in S.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{Confidence, ConfidenceFilter, Language};
use std::collections::HashSet;
use std::path::PathBuf;

/// xorshift64* — tiny deterministic PRNG for reproducible programs.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Generate an S_js program: N exported functions, each calling a subset of others.
fn gen_s_js(seed: u64, n_funcs: usize) -> (String, Vec<(String, String)>) {
    let mut rng = Rng(seed | 1);
    let names: Vec<String> = (0..n_funcs).map(|i| format!("fn{i}")).collect();
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut src = String::new();
    for (i, name) in names.iter().enumerate() {
        src.push_str(&format!("export function {name}() {{\n"));
        // each function calls 0–3 others with lower index (DAG → no cycles)
        let n_calls = if i == 0 { 0 } else { rng.below(3.min(i) + 1) };
        for _ in 0..n_calls {
            let j = rng.below(i);
            let callee = &names[j];
            src.push_str(&format!("  {callee}();\n"));
            edges.push((name.clone(), callee.clone()));
        }
        src.push_str("}\n");
    }
    (src, edges)
}

fn temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-l2-prop-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("index.db")
}

#[test]
fn every_direct_call_in_s_program_has_exact_edge() {
    for seed in 1..=40u64 {
        let (src, edges) = gen_s_js(seed, 6);
        let known = HashSet::from(["src/p.js".to_string()]);
        let out = extract_file(&src, Language::JavaScript, "src/p.js", &known)
            .expect("parse generated S program");
        for (from, to) in &edges {
            let hit = out.references.iter().any(|r| {
                r.name == *to
                    && r.confidence == Confidence::Exact
                    && r.enclosing.as_deref() == Some(from.as_str())
            });
            assert!(
                hit,
                "seed={seed}: missing Exact edge {from}->{to}\nsrc:\n{src}\nrefs={:?}",
                out.references
                    .iter()
                    .map(|r| (r.name.clone(), r.enclosing.clone(), r.confidence.as_str()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn impact_sound_contains_all_transitive_callers_on_s_program() {
    for seed in 1..=20u64 {
        let (src, edges) = gen_s_js(seed, 5);
        let db = temp_db(&format!("imp-{seed}"));
        let mut store = Store::open(&db).unwrap();
        let known = HashSet::new();
        let parsed = extract_file(&src, Language::JavaScript, "src/p.js", &known).unwrap();
        store.begin_batch().unwrap();
        store
            .replace_file("src/p.js", "h", "javascript", &parsed)
            .unwrap();
        store.commit_batch().unwrap();

        // For each edge from->to, impact(to) must include from (depth 1).
        for (from, to) in &edges {
            let (nodes, viols) = store.impact_sound(to, 1, 200).unwrap();
            assert!(
                viols.is_empty(),
                "seed={seed}: generated S program must have no violations"
            );
            let found = nodes
                .iter()
                .any(|n| n.enclosing.as_deref() == Some(from.as_str()));
            assert!(
                found,
                "seed={seed}: impact_sound({to}) must contain caller {from}; nodes={nodes:?}"
            );
        }
    }
}

#[test]
fn generated_programs_stay_in_subset_s() {
    use agentgraph::index::subset::scan_subset;
    for seed in 1..=30u64 {
        let (src, _) = gen_s_js(seed, 7);
        let r = scan_subset(&src, Language::JavaScript, "src/p.js");
        assert!(r.in_subset, "seed={seed} left S: {:?}", r.violations);
    }
}

#[test]
fn exact_only_filter_still_shows_all_direct_calls() {
    let (src, edges) = gen_s_js(99, 5);
    let db = temp_db("exact");
    let mut store = Store::open(&db).unwrap();
    let parsed = extract_file(&src, Language::JavaScript, "src/p.js", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/p.js", "h", "javascript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    for (_from, to) in &edges {
        let hits = store
            .callers_filtered(to, 50, ConfidenceFilter::ExactOnly)
            .unwrap();
        assert!(
            !hits.is_empty(),
            "ExactOnly must still find callers of {to}"
        );
    }
}

// ─── S_py ───────────────────────────────────────────────────────────────

/// Generate an S_py program: N top-level functions, each calling a subset of
/// lower-index functions. Stays in S_py (no eval/exec/getattr/import_module/
/// setattr/vars/globals/locals/dunder builtins).
fn gen_s_py(seed: u64, n_funcs: usize) -> (String, Vec<(String, String)>) {
    let mut rng = Rng(seed | 1);
    let names: Vec<String> = (0..n_funcs).map(|i| format!("fn{i}")).collect();
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut src = String::new();
    for (i, name) in names.iter().enumerate() {
        src.push_str(&format!("def {name}():\n"));
        let n_calls = if i == 0 { 0 } else { rng.below(3.min(i) + 1) };
        if n_calls == 0 {
            src.push_str("    pass\n");
        } else {
            for _ in 0..n_calls {
                let j = rng.below(i);
                let callee = &names[j];
                src.push_str(&format!("    {callee}()\n"));
                edges.push((name.clone(), callee.clone()));
            }
        }
    }
    (src, edges)
}

fn index_store_py(src: &str, tag: &str) -> Store {
    let db = temp_db(tag);
    let mut store = Store::open(&db).unwrap();
    let parsed = extract_file(src, Language::Python, "src/p.py", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/p.py", "h", "python", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store
}

#[test]
fn every_direct_call_in_s_py_program_has_exact_edge() {
    for seed in 1..=40u64 {
        let (src, edges) = gen_s_py(seed, 6);
        let known = HashSet::from(["src/p.py".to_string()]);
        let out = extract_file(&src, Language::Python, "src/p.py", &known)
            .expect("parse generated S_py program");
        for (from, to) in &edges {
            let hit = out.references.iter().any(|r| {
                r.name == *to
                    && r.confidence == Confidence::Exact
                    && r.enclosing.as_deref() == Some(from.as_str())
            });
            assert!(
                hit,
                "seed={seed}: missing Exact edge {from}->{to}\nsrc:\n{src}\nrefs={:?}",
                out.references
                    .iter()
                    .map(|r| (r.name.clone(), r.enclosing.clone(), r.confidence.as_str()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn impact_sound_contains_all_transitive_callers_on_s_py_program() {
    for seed in 1..=20u64 {
        let (src, edges) = gen_s_py(seed, 5);
        let store = index_store_py(&src, &format!("imp-py-{seed}"));
        for (from, to) in &edges {
            let (nodes, viols) = store.impact_sound(to, 1, 200).unwrap();
            assert!(
                viols.is_empty(),
                "seed={seed}: generated S_py program must have no violations"
            );
            let found = nodes
                .iter()
                .any(|n| n.enclosing.as_deref() == Some(from.as_str()));
            assert!(
                found,
                "seed={seed}: impact_sound({to}) must contain caller {from}; nodes={nodes:?}"
            );
        }
    }
}

#[test]
fn generated_py_programs_stay_in_subset_s() {
    use agentgraph::index::subset::scan_subset;
    for seed in 1..=30u64 {
        let (src, _) = gen_s_py(seed, 7);
        let r = scan_subset(&src, Language::Python, "src/p.py");
        assert!(r.in_subset, "seed={seed} left S: {:?}", r.violations);
    }
}

/// First edge whose callee also appears as a caller (mid-chain node).
fn find_mid_chain_edge(edges: &[(String, String)]) -> Option<(String, String)> {
    let callees: HashSet<&str> = edges.iter().map(|(_, t)| t.as_str()).collect();
    edges
        .iter()
        .find(|(from, _)| callees.contains(from.as_str()))
        .cloned()
}

#[test]
fn callers_sound_finds_mid_chain_caller_on_s_py_program() {
    let (src, from, to) = (1..=20u64)
        .find_map(|seed| {
            let (src, edges) = gen_s_py(seed, 6);
            find_mid_chain_edge(&edges).map(|(from, to)| (src, from, to))
        })
        .expect("generator must produce a mid-chain edge");
    let store = index_store_py(&src, "callers-py-mid");
    let (hits, viols) = store.callers_sound(&to, 50).unwrap();
    assert!(viols.is_empty(), "S_py program must have no violations");
    assert!(
        hits.iter()
            .any(|r| r.enclosing.as_deref() == Some(from.as_str())),
        "callers_sound({to}) must include mid-chain caller {from}; hits={hits:?}"
    );
}

// ─── S_go ───────────────────────────────────────────────────────────────

/// Generate an S_go program: `package main` with N funcs, each calling a
/// subset of lower-index funcs. Stays in S_go (no unsafe/reflect/cgo/plugin).
fn gen_s_go(seed: u64, n_funcs: usize) -> (String, Vec<(String, String)>) {
    let mut rng = Rng(seed | 1);
    let names: Vec<String> = (0..n_funcs).map(|i| format!("fn{i}")).collect();
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut src = String::from("package main\n\n");
    for (i, name) in names.iter().enumerate() {
        let n_calls = if i == 0 { 0 } else { rng.below(3.min(i) + 1) };
        if n_calls == 0 {
            src.push_str(&format!("func {name}() {{}}\n"));
        } else {
            src.push_str(&format!("func {name}() {{\n"));
            for _ in 0..n_calls {
                let j = rng.below(i);
                let callee = &names[j];
                src.push_str(&format!("\t{callee}()\n"));
                edges.push((name.clone(), callee.clone()));
            }
            src.push_str("}\n");
        }
    }
    (src, edges)
}

fn index_store_go(src: &str, tag: &str) -> Store {
    let db = temp_db(tag);
    let mut store = Store::open(&db).unwrap();
    let parsed = extract_file(src, Language::Go, "main.go", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store.replace_file("main.go", "h", "go", &parsed).unwrap();
    store.commit_batch().unwrap();
    store
}

#[test]
fn every_direct_call_in_s_go_program_has_exact_edge() {
    for seed in 1..=40u64 {
        let (src, edges) = gen_s_go(seed, 6);
        let known = HashSet::from(["main.go".to_string()]);
        let out = extract_file(&src, Language::Go, "main.go", &known)
            .expect("parse generated S_go program");
        for (from, to) in &edges {
            let hit = out.references.iter().any(|r| {
                r.name == *to
                    && r.confidence == Confidence::Exact
                    && r.enclosing.as_deref() == Some(from.as_str())
            });
            assert!(
                hit,
                "seed={seed}: missing Exact edge {from}->{to}\nsrc:\n{src}\nrefs={:?}",
                out.references
                    .iter()
                    .map(|r| (r.name.clone(), r.enclosing.clone(), r.confidence.as_str()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn impact_sound_contains_all_transitive_callers_on_s_go_program() {
    for seed in 1..=20u64 {
        let (src, edges) = gen_s_go(seed, 5);
        let store = index_store_go(&src, &format!("imp-go-{seed}"));
        for (from, to) in &edges {
            let (nodes, viols) = store.impact_sound(to, 1, 200).unwrap();
            assert!(
                viols.is_empty(),
                "seed={seed}: generated S_go program must have no violations"
            );
            let found = nodes
                .iter()
                .any(|n| n.enclosing.as_deref() == Some(from.as_str()));
            assert!(
                found,
                "seed={seed}: impact_sound({to}) must contain caller {from}; nodes={nodes:?}"
            );
        }
    }
}

#[test]
fn generated_go_programs_stay_in_subset_s() {
    use agentgraph::index::subset::scan_subset;
    for seed in 1..=30u64 {
        let (src, _) = gen_s_go(seed, 7);
        let r = scan_subset(&src, Language::Go, "main.go");
        assert!(r.in_subset, "seed={seed} left S: {:?}", r.violations);
    }
}

#[test]
fn callers_sound_finds_mid_chain_caller_on_s_go_program() {
    let (src, from, to) = (1..=20u64)
        .find_map(|seed| {
            let (src, edges) = gen_s_go(seed, 6);
            find_mid_chain_edge(&edges).map(|(from, to)| (src, from, to))
        })
        .expect("generator must produce a mid-chain edge");
    let store = index_store_go(&src, "callers-go-mid");
    let (hits, viols) = store.callers_sound(&to, 50).unwrap();
    assert!(viols.is_empty(), "S_go program must have no violations");
    assert!(
        hits.iter()
            .any(|r| r.enclosing.as_deref() == Some(from.as_str())),
        "callers_sound({to}) must include mid-chain caller {from}; hits={hits:?}"
    );
}
