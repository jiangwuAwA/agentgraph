//! TDD (L2 hardening): property-style checks that S_js programs keep
//! the "every direct call has an Exact edge" invariant, and that
//! impact --sound contains the golden caller set for generated programs.
//!
//! Generator is intentionally small and deterministic (no proptest dep):
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
