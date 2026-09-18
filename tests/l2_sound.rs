//! TDD: L2 CLI --sound + subset command + differential vs Node tracer.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agentgraph"))
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn run_ag(root: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("agentgraph");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn copy_fixture_to_temp(rel: &str, tag: &str) -> PathBuf {
    let src = fixture(rel);
    // Do NOT canonicalize here: on Windows that yields `\\?\` UNC paths which
    // Node (differential tracer) rejects. Indexer::new canonicalizes internally.
    common::copy_fixture_to_temp(&src, &format!("agentgraph-l2-sound-{tag}"))
}

fn index_clean() -> PathBuf {
    // Isolate per-test so parallel tests cannot race on one .agentgraph/ (C-fix race).
    let root = copy_fixture_to_temp("fixtures/eval-l2/s-js-auth", &unique_tag());
    let (ok, _, err) = run_ag(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    root
}

/// Process-unique tag: pid + monotonic counter + wall clock.
///
/// Wall clock alone is NOT unique when parallel tests in the same process call
/// this within one OS timer tick (observed on macOS CI: auth/evil fixtures
/// collided into one temp dir → evil `src/evil.js` violations landed in the
/// auth index). The AtomicU64 makes collisions impossible.
fn unique_tag() -> String {
    common::unique_tag()
}

#[test]
fn subset_clean_program_in_s() {
    let root = index_clean();
    let (ok, stdout, err) = run_ag(&root, &["subset"]);
    assert!(ok, "subset cmd failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(
        v["in_subset"], true,
        "S_js auth fixture must be in S: {stdout}"
    );
}

#[test]
fn subset_eval_program_leaves_s() {
    let root = copy_fixture_to_temp("fixtures/eval-l2/s-js-evil", &unique_tag());
    let (ok, _, err) = run_ag(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, _err) = run_ag(&root, &["subset"]);
    // exit code 2 when violations exist
    assert!(!ok, "subset must fail when eval present");
    let v: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["in_subset"], false, "{stdout}");
    assert!(
        v["violation_count"].as_u64().unwrap_or(0) >= 1,
        "expected eval violation: {stdout}"
    );
}

#[test]
fn impact_sound_on_clean_program() {
    let root = index_clean();
    let (ok, stdout, err) = run_ag(
        &root,
        &["impact", "authenticate", "--sound", "--depth", "2"],
    );
    assert!(ok, "impact --sound failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "{stdout}");
    assert!(
        v["impact"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "sound impact should find loginHandler/main callers: {stdout}"
    );
}

#[test]
fn impact_sound_disables_claim_when_eval() {
    let root = copy_fixture_to_temp("fixtures/eval-l2/s-js-evil", &unique_tag());
    let (ok, _, _) = run_ag(&root, &["index"]);
    assert!(ok);
    let (ok, stdout, err) = run_ag(&root, &["impact", "dangerous", "--sound"]);
    assert!(ok, "impact --sound still returns payload: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["subset_ok"], false, "{stdout}");
    assert!(
        v["promise"].as_str().unwrap_or("").contains("disabled"),
        "must not claim sound when S violated: {stdout}"
    );
}

#[test]
fn differential_runtime_edges_subset_of_sound_impact() {
    // Requires node. Skip soft-fail only if node missing.
    let node = which_node();
    let Some(node) = node else {
        eprintln!("skip: node not on PATH");
        return;
    };
    let root = index_clean();
    let mod_path = root.join("src/auth.js");
    let tracer = fixture("scripts/diff_trace.cjs");
    let out = Command::new(&node)
        .arg(&tracer)
        .arg(&mod_path)
        .arg("main")
        .output()
        .expect("run tracer");
    assert!(
        out.status.success(),
        "tracer failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let trace: Value = serde_json::from_slice(&out.stdout).expect("trace json");
    let edges = trace["edges"].as_array().expect("edges");
    assert!(
        !edges.is_empty(),
        "tracer must observe runtime edges: {trace}"
    );

    let (ok, stdout, err) = run_ag(
        &root,
        &[
            "impact", "main", "--sound", "--depth", "5", "--limit", "200",
        ],
    );
    assert!(ok, "impact --sound: {err}");
    let sound: Value = serde_json::from_str(&stdout).expect("sound json");
    assert_eq!(sound["subset_ok"], true);
    let impact = sound["impact"].as_array().expect("impact array");

    // C4: do NOT pre-insert `to` into the static set — that made the old
    // assertion vacuously true. Containment = callers(enclosing=from) OR
    // impact(to) names/enclosings contain `from`.
    for e in edges {
        let to = e["to"].as_str().unwrap();
        let from = e["from"].as_str().unwrap();
        let (ok2, stdout2, err2) = run_ag(&root, &["callers", to, "--sound"]);
        assert!(ok2, "callers --sound {to}: {err2}");
        let cv: Value = serde_json::from_str(&stdout2).unwrap();
        let callers = cv["callers"].as_array().cloned().unwrap_or_default();
        let found_in_callers = callers.iter().any(|c| {
            c["enclosing"]
                .as_str()
                .map(|s| s.contains(from))
                .unwrap_or(false)
        });
        let (ok4, stdout4, err4) = run_ag(&root, &["impact", to, "--sound", "--depth", "2"]);
        assert!(ok4, "{err4}");
        let iv: Value = serde_json::from_str(&stdout4).unwrap();
        let inodes = iv["impact"].as_array().cloned().unwrap_or_default();
        let found_in_impact = inodes.iter().any(|n| {
            n["name"].as_str() == Some(from)
                || n["enclosing"]
                    .as_str()
                    .map(|s| s.contains(from))
                    .unwrap_or(false)
        });
        assert!(
            found_in_callers || found_in_impact,
            "L2 containment fail: runtime {from}→{to}; callers={callers:?} impact={inodes:?}"
        );
    }

    let names: Vec<String> = impact
        .iter()
        .filter_map(|n| n["name"].as_str().map(|s| s.to_string()))
        .collect();
    // main is the entry — callers(main) may be empty. Check a mid-chain callee.
    let (ok5, stdout5, err5) = run_ag(
        &root,
        &["impact", "authenticate", "--sound", "--depth", "3"],
    );
    assert!(ok5, "{err5}");
    let mid: Value = serde_json::from_str(&stdout5).unwrap();
    let mid_nodes = mid["impact"].as_array().cloned().unwrap_or_default();
    assert!(
        !mid_nodes.is_empty(),
        "impact(authenticate)--sound must be non-empty; main_impact_names={names:?}"
    );
}

fn which_node() -> Option<PathBuf> {
    let out = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "where", "node"])
            .output()
            .ok()?
    } else {
        Command::new("which").arg("node").output().ok()?
    };
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(PathBuf::from(first))
    }
}

/// Guard against temp-dir collisions from coarse wall-clock resolution (macOS CI).
#[test]
fn unique_tag_is_collision_free_under_rapid_calls() {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for _ in 0..10_000 {
        let tag = unique_tag();
        assert!(seen.insert(tag), "unique_tag collided within one process");
    }
}
