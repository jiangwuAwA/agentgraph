//! R-track fixture gate: covered-path runtime edges ⊆ static graph ∪ gap.
//!
//! MVP: small Rust fixture with `rr_edge` probes; tracer + `eval_runtime_recall.py`.
//! Honesty: covered paths zero-miss only — not production absolute zero-miss.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn py() -> String {
    std::env::var("MIMO_PYTHON").unwrap_or_else(|_| "python".to_string())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(cmd: &str, args: &[&str], cwd: &Path) -> (bool, String, String) {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .expect("run");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn runtime_recall_fixture_zero_miss() {
    let root = repo_root();
    let fixture = root.join("fixtures/eval-runtime-recall");
    let trace = root.join("target").join("rr_trace_test.jsonl");
    if trace.exists() {
        let _ = std::fs::remove_file(&trace);
    }

    // 1) tracer
    let (ok, so, se) = run(
        &py(),
        &[
            "scripts/rs_trace.py",
            fixture.to_str().unwrap(),
            "--out",
            trace.to_str().unwrap(),
        ],
        &root,
    );
    assert!(ok, "rs_trace failed: {se}\n{so}");
    assert!(trace.is_file(), "trace missing");

    // 2) index fixture
    let bin = env!("CARGO_BIN_EXE_agentgraph");
    let (ok, _so, se) = run(
        bin,
        &["--root", fixture.to_str().unwrap(), "index", "--force"],
        &root,
    );
    assert!(ok, "index failed: {se}");

    // 3) gate
    let report = root.join("target").join("runtime_recall_test.json");
    let (ok, so, se) = run(
        &py(),
        &[
            "scripts/eval_runtime_recall.py",
            "--trace",
            trace.to_str().unwrap(),
            "--db",
            fixture
                .join(".agentgraph")
                .join("index.db")
                .to_str()
                .unwrap(),
            "--gap",
            fixture.join("gap_ledger.json").to_str().unwrap(),
            "--out",
            report.to_str().unwrap(),
        ],
        &root,
    );
    assert!(ok, "recall gate failed: {se}\n{so}");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(v["zero_miss"], true, "fixture must be zero-miss: {v}");
    assert!(v["trace_edges"].as_u64().unwrap() >= 3);
    assert_eq!(v["missed_count"].as_u64().unwrap(), 0);
    assert_eq!(v["recall_at_covered"].as_f64().unwrap(), 1.0);
}

#[test]
fn runtime_recall_gap_ledger_covers_miss() {
    // If a miss is listed in the gap ledger, zero_miss must not require static hit.
    let root = repo_root();
    let tmp = root.join("target").join("rr_gap_tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let trace = tmp.join("t.jsonl");
    std::fs::write(
        &trace,
        "{\"schema\":\"agentgraph.eval_runtime_recall.edge.v1\",\"from\":\"a::f\",\"to\":\"b::g\"}\n",
    )
    .unwrap();
    let gap = tmp.join("gap.json");
    std::fs::write(
        &gap,
        "{\"schema\":\"agentgraph.eval_runtime_recall.gap_ledger.v1\",\"edges\":[{\"from\":\"a::f\",\"to\":\"b::g\",\"reason\":\"function_pointer\",\"note\":\"synthetic\"}]}\n",
    )
    .unwrap();
    let empty_db = tmp.join("empty.db");
    let report = tmp.join("report.json");
    let py = py();
    let (ok, so, se) = run(
        &py,
        &[
            "scripts/eval_runtime_recall.py",
            "--trace",
            trace.to_str().unwrap(),
            "--db",
            empty_db.to_str().unwrap(),
            "--gap",
            gap.to_str().unwrap(),
            "--out",
            report.to_str().unwrap(),
        ],
        &root,
    );
    assert!(ok, "gate failed: {se}\n{so}");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(v["gap_used"].as_u64().unwrap(), 1);
    assert_eq!(v["missed_count"].as_u64().unwrap(), 0);
    assert_eq!(v["zero_miss"], true);
}
