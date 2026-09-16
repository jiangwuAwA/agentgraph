//! In-process query latency soft gate (CI-friendly).
//! Hard 5k-file p95 is measured by `scripts/bench_query_p95.ps1` on release.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::ConfidenceFilter;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn seed(n: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-q-slo-{n}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("index.db");
    let mut store = Store::open(&db).unwrap();
    store.begin_batch().unwrap();
    for i in 0..n {
        let src = format!(
            "export function helper{i}(x: number) {{ return x + {i}; }}\nexport function main{i}() {{ return helper{i}({i}); }}\n"
        );
        let path = format!("src/p{i}.ts");
        let parsed = extract_file(&src, Language::TypeScript, &path, &HashSet::new()).unwrap();
        store
            .replace_file(&path, &format!("h{i}"), "typescript", &parsed)
            .unwrap();
    }
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    db
}

fn pct(sorted: &[Duration], q: f64) -> Duration {
    let idx = ((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

#[test]
fn callers_impact_p95_under_50ms_on_400_file_db() {
    let db = seed(400);
    let store = Store::open(&db).unwrap();
    // warm
    let _ = store
        .callers_filtered("helper1", 20, ConfidenceFilter::Default)
        .unwrap();

    let mut callers_t = Vec::new();
    let mut impact_t = Vec::new();
    for i in 0..80 {
        let name = format!("helper{}", i * 3 % 400);
        let t = Instant::now();
        let _ = store
            .callers_filtered(&name, 20, ConfidenceFilter::Default)
            .unwrap();
        callers_t.push(t.elapsed());
        let t = Instant::now();
        let _ = store
            .impact_filtered(&name, 2, 50, ConfidenceFilter::Default)
            .unwrap();
        impact_t.push(t.elapsed());
    }
    callers_t.sort();
    impact_t.sort();
    let c95 = pct(&callers_t, 0.95);
    let i95 = pct(&impact_t, 0.95);
    eprintln!("callers p95={c95:?} impact p95={i95:?}");
    // Soft CI budget (debug builds); release hard SLO is scripts/bench_query_p95.ps1
    assert!(
        c95 < Duration::from_millis(50),
        "callers p95 {c95:?} >= 50ms"
    );
    assert!(
        i95 < Duration::from_millis(50),
        "impact p95 {i95:?} >= 50ms"
    );
}
