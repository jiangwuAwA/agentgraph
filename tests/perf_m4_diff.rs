//! M4-P: latency budgets for `agentgraph diff` and S re-cert (dirty `index_paths`).
//!
//! Measures on a mid-size synthetic fixture (~200 TS files, same shape as
//! `scripts/gen_fixture.ps1 -N 200`):
//! - CLI `agentgraph diff` wall-clock cold / warm (process spawn included)
//! - In-process `run_diff` cold / warm (no CLI bias)
//! - S re-cert: `refresh_subset_for_paths` / `index_paths` on dirty files
//!   (1 / 5 / 10) vs full-corpus `scan_subset`
//!
//! **Budgets (loose CI ceilings — see docs/eval-query-p95.md):**
//! - CLI `diff` on the 200-file fixture: **< 2s** (includes process start)
//! - Dirty S re-cert (`refresh_subset_for_paths` on ≤10 paths): **< 500ms**
//! - `index_paths` on ≤10 dirty files: **< 2s** (extract + recert + prune walk)
//!
//! Actual p50/p95 are printed on stderr and written to
//! `target/perf_m4_diff_bench.md` for manual paste into docs. Numbers are
//! **fixture + machine local** — not a production SLO.

use agentgraph::index::diff::run_diff;
use agentgraph::index::subset::scan_subset;
use agentgraph::index::Indexer;
use agentgraph::model::Language;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FIXTURE_N: usize = 200;
const DIFF_CLI_BUDGET: Duration = Duration::from_secs(2);
const RECERT_REFRESH_BUDGET: Duration = Duration::from_millis(500);
const INDEX_PATHS_BUDGET: Duration = Duration::from_secs(2);
const WARM_SAMPLES: usize = 20;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-m4p-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_fixture(root: &Path, n: usize) {
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..n {
        let body = format!(
            "export function helper{i}(x: number) {{\n  return x + {i};\n}}\nexport function main{i}() {{\n  return helper{i}({i});\n}}\n"
        );
        std::fs::write(src.join(format!("p{i}.ts")), body).unwrap();
    }
}

fn cli(root: &Path, args: &[&str]) -> (Duration, bool, String, String) {
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
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (elapsed, out.status.success(), stdout, stderr)
}

fn pct(sorted: &[Duration], q: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

fn summarize(label: &str, samples: &[Duration]) -> String {
    let mut sorted = samples.to_vec();
    sorted.sort();
    let p50 = pct(&sorted, 0.50);
    let p95 = pct(&sorted, 0.95);
    let max = sorted.last().copied().unwrap_or(Duration::ZERO);
    let min = sorted.first().copied().unwrap_or(Duration::ZERO);
    format!(
        "| {label} | n={} | p50={p50:?} | p95={p95:?} | min={min:?} | max={max:?} |",
        samples.len()
    )
}

fn machine_note() -> String {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    format!(
        "os={} arch={} profile={profile} fixture_n={FIXTURE_N} warm_samples={WARM_SAMPLES}",
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn write_bench_snippet(path: &Path, lines: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = lines.join("\n");
    let _ = std::fs::write(path, body + "\n");
}

fn dirty_content(i: usize) -> String {
    format!(
        "export function helper{i}(x: number) {{ return x + {i}; }}\nexport function main{i}() {{ return helper{i}({i}); }}\nexport function extra{i}() {{ return helper{i}(1); }}\n"
    )
}

/// Mid-size fixture: CLI `diff` cold/warm + in-process run_diff; dirty S re-cert
/// vs full-corpus subset scan. Loose ceilings only.
#[test]
fn m4p_diff_and_s_recert_budgets() {
    let root = temp_root("bench");
    write_fixture(&root, FIXTURE_N);

    // Baseline full index (writes refs.snapshot.json for diff).
    let (idx_t, ok, _, idx_err) = cli(&root, &["index", "--force"]);
    assert!(ok, "index --force failed: {idx_err}");
    eprintln!("M4-P full index wall={idx_t:?} ({})", machine_note());

    // Small edge change + dirty-path reindex (baseline not refreshed) → live vs snapshot drift.
    let edited = root.join("src/p0.ts");
    std::fs::write(
        &edited,
        "export function helper0(x: number) { return x + 0; }\nexport function main0() { return helper0(0); }\nexport function extra0() { return helper0(1); }\n",
    )
    .unwrap();
    let indexer = Indexer::new(&root).unwrap();
    indexer
        .index_paths(std::slice::from_ref(&edited))
        .expect("index_paths after edit");

    // ---- CLI diff cold (first process after work; process spawn included) ----
    let (cold_t, ok, stdout, stderr) = cli(&root, &["diff"]);
    assert!(ok, "diff cold failed: stdout={stdout} stderr={stderr}");
    let payload: serde_json::Value = serde_json::from_str(&stdout).expect("diff json");
    assert!(
        payload["note"]
            .as_str()
            .unwrap_or("")
            .contains("indexed edges"),
        "honesty note missing: {stdout}"
    );
    eprintln!("M4-P CLI diff cold={cold_t:?}");

    // ---- CLI diff warm ----
    let mut warm = Vec::with_capacity(WARM_SAMPLES);
    for _ in 0..WARM_SAMPLES {
        let (t, ok, out, err) = cli(&root, &["diff"]);
        assert!(ok, "diff warm failed: {out} {err}");
        warm.push(t);
    }
    let warm_line = summarize("CLI diff warm", &warm);
    eprintln!("M4-P {warm_line}");
    let mut warm_sorted = warm.clone();
    warm_sorted.sort();
    let warm_p50 = pct(&warm_sorted, 0.50);
    let warm_p95 = pct(&warm_sorted, 0.95);

    // ---- In-process run_diff (no CLI spawn bias) ----
    let store = indexer.open_store().expect("open_store");
    let t = Instant::now();
    let d = run_diff(&indexer.root, &store, false, None, None).expect("run_diff cold");
    let proc_cold = t.elapsed();
    assert!(d.note.contains("indexed edges"));
    eprintln!(
        "M4-P in-process run_diff cold={proc_cold:?} added={} removed={}",
        d.summary.added, d.summary.removed
    );
    let mut proc_warm = Vec::with_capacity(WARM_SAMPLES);
    for _ in 0..WARM_SAMPLES {
        let t = Instant::now();
        let _ = run_diff(&indexer.root, &store, false, None, None).expect("run_diff warm");
        proc_warm.push(t.elapsed());
    }
    let proc_warm_line = summarize("in-process run_diff warm", &proc_warm);
    eprintln!("M4-P {proc_warm_line}");

    // ---- S re-cert: dirty refresh vs full-corpus scan_subset ----
    let dirty_counts = [1usize, 5, 10];
    let mut recert_refresh_lines = Vec::new();
    let mut index_paths_lines = Vec::new();
    let mut refresh_times: Vec<Duration> = Vec::new();
    let mut index_paths_times: Vec<Duration> = Vec::new();

    for &n in &dirty_counts {
        let mut paths = Vec::new();
        let mut path_bufs = Vec::new();
        for i in 0..n {
            let idx = 10 + i;
            let p = root.join(format!("src/p{idx}.ts"));
            std::fs::write(&p, dirty_content(idx)).unwrap();
            paths.push(format!("src/p{idx}.ts"));
            path_bufs.push(p);
        }

        // Store-level re-cert API (what index_paths hooks after dirty extract).
        let mut store = indexer.open_store().expect("open_store refresh");
        let t = Instant::now();
        let refreshed = store
            .refresh_subset_for_paths(&paths)
            .expect("refresh_subset_for_paths");
        let refresh_t = t.elapsed();
        assert_eq!(refreshed, n, "refresh must rewrite dirty paths");
        refresh_times.push(refresh_t);
        recert_refresh_lines.push(format!(
            "| refresh_subset_for_paths n={n} | {refresh_t:?} | refreshed={refreshed} |"
        ));
        eprintln!("M4-P S recert refresh n={n} t={refresh_t:?}");

        // End-to-end dirty index_paths (extract + subset meta + recert hook).
        let t = Instant::now();
        indexer.index_paths(&path_bufs).expect("index_paths dirty");
        let ip_t = t.elapsed();
        index_paths_times.push(ip_t);
        index_paths_lines.push(format!("| index_paths dirty n={n} | {ip_t:?} |"));
        eprintln!("M4-P index_paths dirty n={n} t={ip_t:?}");
    }

    // Full-corpus subset scan (in-process tree-sitter S walk over every fixture file).
    // This is the cost full `index` pays for corpus-wide certification — not the
    // dirty-path re-cert path.
    let src_dir = root.join("src");
    let t = Instant::now();
    let mut clean = 0usize;
    let mut dirty_viol = 0usize;
    for i in 0..FIXTURE_N {
        let rel = format!("src/p{i}.ts");
        let abs = src_dir.join(format!("p{i}.ts"));
        let src = std::fs::read_to_string(&abs).expect("read fixture");
        let report = scan_subset(&src, Language::TypeScript, &rel);
        if report.in_subset {
            clean += 1;
        } else {
            dirty_viol += 1;
        }
    }
    let full_scan_t = t.elapsed();
    eprintln!(
        "M4-P full-corpus scan_subset n={FIXTURE_N} t={full_scan_t:?} clean={clean} dirty={dirty_viol}"
    );

    // ---- Loose CI ceilings (slow runners must not flake) ----
    assert!(
        cold_t < DIFF_CLI_BUDGET,
        "CLI diff cold {cold_t:?} >= {DIFF_CLI_BUDGET:?} (fixture n={FIXTURE_N})"
    );
    assert!(
        warm_p95 < DIFF_CLI_BUDGET,
        "CLI diff warm p95 {warm_p95:?} >= {DIFF_CLI_BUDGET:?} (p50={warm_p50:?})"
    );
    let max_refresh = refresh_times
        .iter()
        .copied()
        .max()
        .unwrap_or(Duration::ZERO);
    assert!(
        max_refresh < RECERT_REFRESH_BUDGET,
        "dirty recert refresh max {max_refresh:?} >= {RECERT_REFRESH_BUDGET:?}"
    );
    let max_ip = index_paths_times
        .iter()
        .copied()
        .max()
        .unwrap_or(Duration::ZERO);
    assert!(
        max_ip < INDEX_PATHS_BUDGET,
        "index_paths dirty max {max_ip:?} >= {INDEX_PATHS_BUDGET:?}"
    );
    // Full scan is expected to dominate dirty re-cert; loose smoke only.
    assert!(
        full_scan_t < Duration::from_secs(30),
        "full scan_subset smoke ceiling exceeded: {full_scan_t:?}"
    );

    // ---- Snippet for docs (manual paste / local record) ----
    let out_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("perf_m4_diff_bench.md");
    let snippet = vec![
        format!("# M4-P local bench snippet ({})", machine_note()),
        format!("- fixture: {FIXTURE_N} synthetic TS files (gen_fixture.ps1 shape)"),
        format!("- full index --force wall: {idx_t:?}"),
        format!(
            "- CLI diff cold: {cold_t:?} (budget < {DIFF_CLI_BUDGET:?}, process spawn included)"
        ),
        format!("- CLI diff warm p50={warm_p50:?} p95={warm_p95:?}"),
        warm_line.clone(),
        format!("- in-process run_diff cold: {proc_cold:?}"),
        proc_warm_line.clone(),
        "### S re-cert (dirty paths)".into(),
        recert_refresh_lines.clone().join("\n"),
        index_paths_lines.clone().join("\n"),
        format!(
            "- full-corpus scan_subset n={FIXTURE_N}: {full_scan_t:?} (clean={clean} dirty={dirty_viol})"
        ),
        format!(
            "- budgets: CLI diff < {DIFF_CLI_BUDGET:?}; dirty refresh < {RECERT_REFRESH_BUDGET:?}; index_paths ≤10 dirty < {INDEX_PATHS_BUDGET:?}"
        ),
        "- honesty: fixture + machine local; not a production SLO; CLI timings include process spawn".into(),
    ];
    write_bench_snippet(&out_path, &snippet);
    eprintln!("M4-P wrote {}", out_path.display());
}
