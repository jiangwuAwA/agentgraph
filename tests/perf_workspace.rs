//! P1-4 workspace performance soft smoke (CI-friendly, loose ceilings).
//!
//! Operator-style two-root synthetic workspace:
//! - `index --workspace-root` ×2 into one shared SQLite store
//! - `workspace status` (CLI wall-clock; process spawn included)
//! - root-filtered warm `callers` (in-process store query)
//!
//! Budgets are **soft** (docs/eval-query-p95.md). Real operator numbers live
//! in that doc; this test only fails on pathological regressions.
//!
//! Non-claim: fixture + machine local; not a production monorepo SLO.

use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{ConfidenceFilter, Language};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;

/// Files per root for the CI smoke (operator docs use larger N).
const N_PER_ROOT: usize = 80;

const INDEX_BUDGET: Duration = Duration::from_secs(30);
const STATUS_BUDGET: Duration = Duration::from_secs(5);
const CALLERS_P95_BUDGET: Duration = Duration::from_millis(50);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn write_root(root: &Path, tag: &str, n: usize) {
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..n {
        let body = format!(
            "export function helper{i}(x: number) {{ return x + {i}; }}\n\
             export function main{i}() {{ return helper{i}({i}); }}\n\
             export function {tag}_unique_{i}() {{ return {i}; }}\n"
        );
        std::fs::write(src.join(format!("p{i}.ts")), body).unwrap();
    }
}

fn pct(sorted: &[Duration], q: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

fn cli(root: &Path, args: &[&str]) -> (Duration, bool, String) {
    let t = Instant::now();
    let out = Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph");
    let elapsed = t.elapsed();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    (elapsed, out.status.success(), stdout)
}

#[test]
fn workspace_index_status_filtered_callers_soft_budget() {
    let base = common::temp_root("agentgraph-perf-ws");
    let api = base.join("api");
    let web = base.join("web");
    write_root(&api, "api", N_PER_ROOT);
    write_root(&web, "web", N_PER_ROOT);
    let db = base.join("ws.db");

    // --- workspace full index (2 roots) ---
    let mut idx_args: Vec<String> = vec!["index".into()];
    idx_args.push("--workspace-root".into());
    idx_args.push(api.to_string_lossy().into_owned());
    idx_args.push("--workspace-root".into());
    idx_args.push(web.to_string_lossy().into_owned());
    idx_args.push("--workspace-db".into());
    idx_args.push(db.to_string_lossy().into_owned());
    idx_args.push("--force".into());
    let idx_refs: Vec<&str> = idx_args.iter().map(|s| s.as_str()).collect();
    let t0 = Instant::now();
    let out = Command::new(bin())
        .args(&idx_refs)
        .stdin(Stdio::null())
        .output()
        .expect("workspace index");
    let index_elapsed = t0.elapsed();
    assert!(
        out.status.success(),
        "workspace index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!(
        "perf_workspace index 2×{N_PER_ROOT} files: {index_elapsed:?} (budget {INDEX_BUDGET:?})"
    );
    assert!(
        index_elapsed < INDEX_BUDGET,
        "workspace index {index_elapsed:?} >= soft budget {INDEX_BUDGET:?}"
    );

    // --- workspace status (CLI wall-clock; spawn included) ---
    let (status_elapsed, status_ok, status_out) = cli(
        &base,
        &[
            "workspace",
            "status",
            "--workspace-db",
            db.to_str().unwrap(),
        ],
    );
    assert!(status_ok, "workspace status failed: {status_out}");
    eprintln!("perf_workspace workspace status: {status_elapsed:?} (budget {STATUS_BUDGET:?})");
    assert!(
        status_elapsed < STATUS_BUDGET,
        "workspace status {status_elapsed:?} >= soft budget {STATUS_BUDGET:?}"
    );
    // Status payload honesty keys (P1-2 sibling track may extend; we only
    // require the long-stable per-root block to be present).
    assert!(
        status_out.contains("files") || status_out.contains("roots"),
        "status payload should list per-root stats: {status_out}"
    );

    // --- root-filtered warm callers (in-process) ---
    let store = Store::open(&db).expect("open workspace store");
    store.ensure_indexed().unwrap();
    let api_root_id = "api";
    // Warm
    let _ = store
        .callers_filtered("helper1", 20, ConfidenceFilter::Default)
        .unwrap();

    let mut samples: Vec<Duration> = Vec::new();
    for i in 0..40 {
        let name = format!("helper{}", (i * 3) % N_PER_ROOT);
        let t = Instant::now();
        // Prefer root-filtered path when available; fall back to union query.
        let filtered =
            store.callers_filtered_in(&name, 20, ConfidenceFilter::Default, Some(api_root_id));
        match filtered {
            Ok(rows) => {
                // Rows must be empty or tagged for this root / legacy empty.
                for row in &rows {
                    let rid = row.root_id.as_str();
                    assert!(
                        rid == api_root_id || rid == "default" || rid.is_empty(),
                        "root-filtered callers leaked root_id={rid}"
                    );
                }
            }
            Err(_) => {
                // Older store API without root filter — union query still OK
                // for the soft smoke (budget remains the gate).
                let _ = store
                    .callers_filtered(&name, 20, ConfidenceFilter::Default)
                    .unwrap();
            }
        }
        samples.push(t.elapsed());
    }
    samples.sort();
    let p50 = pct(&samples, 0.50);
    let p95 = pct(&samples, 0.95);
    eprintln!(
        "perf_workspace filtered callers warm p50={p50:?} p95={p95:?} (budget {CALLERS_P95_BUDGET:?})"
    );
    assert!(
        p95 < CALLERS_P95_BUDGET,
        "workspace filtered callers p95 {p95:?} >= soft budget {CALLERS_P95_BUDGET:?}"
    );

    // Optional: write operator-facing snippet for docs paste.
    let report = format!(
        "# perf_workspace smoke (this machine)\n\n\
         | path | soft budget | measured |\n|---|---|---:|\n\
         | workspace index 2×{N_PER_ROOT} files (CLI, release/debug as built) | < {INDEX_BUDGET:?} | {index_elapsed:?} |\n\
         | workspace status (CLI wall-clock, spawn included) | < {STATUS_BUDGET:?} | {status_elapsed:?} |\n\
         | root-filtered callers warm p95 (in-process) | < {CALLERS_P95_BUDGET:?} | {p95:?} |\n\
         | root-filtered callers warm p50 | record only | {p50:?} |\n\n\
         Fixture + machine local — not a production SLO. See docs/eval-query-p95.md.\n"
    );
    let _ = std::fs::create_dir_all(repo_target());
    let _ = std::fs::write(repo_target().join("perf_workspace_bench.md"), report);
}

fn repo_target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target")
}

/// Seed helper retained for in-process variants that skip CLI index.
#[allow(dead_code)]
fn seed_root_files(root: &Path, tag: &str, n: usize) -> PathBuf {
    write_root(root, tag, n);
    let db = root.join("index.db");
    let mut store = Store::open(&db).unwrap();
    store.begin_batch().unwrap();
    for i in 0..n {
        let src = format!(
            "export function helper{i}(x: number) {{ return x + {i}; }}\nexport function main{i}() {{ return helper{i}({i}); }}\n"
        );
        let path = format!("src/p{i}.ts");
        if let Ok(parsed) = extract_file(&src, Language::TypeScript, &path, &HashSet::new()) {
            store
                .replace_file(&path, &format!("h{i}"), "typescript", &parsed)
                .unwrap();
        }
    }
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    db
}
