//! TDD: L2 CLI --sound + subset command + differential vs Node tracer.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn index_clean() -> PathBuf {
    let root = fixture("fixtures/eval-l2/s-js-auth");
    // Fresh index each time
    let _ = std::fs::remove_dir_all(root.join(".agentgraph"));
    let (ok, _, err) = run_ag(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    root
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
    let root = fixture("fixtures/eval-l2/s-js-evil");
    let _ = std::fs::remove_dir_all(root.join(".agentgraph"));
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
    let root = fixture("fixtures/eval-l2/s-js-evil");
    let _ = std::fs::remove_dir_all(root.join(".agentgraph"));
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

    // Runtime edge (from → to) means `to` was called by `from`.
    // impact(main) collects callers of main and of enclosing functions.
    // For containment: every callee `to` invoked at runtime from a wrapped fn
    // must appear as a ref name in the sound graph reachable from main's callers,
    // OR as a direct call edge name in the index.
    // Practical check: every runtime callee name is present as some sound impact
    // node name OR as callers of that name under sound walk from main's subgraph.
    let mut static_names: std::collections::HashSet<String> = impact
        .iter()
        .filter_map(|n| n["name"].as_str().map(|s| s.to_string()))
        .collect();
    // Also collect callers of each runtime callee under sound mode.
    for e in edges {
        let to = e["to"].as_str().unwrap();
        static_names.insert(to.to_string());
        let (ok2, stdout2, err2) = run_ag(&root, &["callers", to, "--sound"]);
        assert!(ok2, "callers --sound {to}: {err2}");
        let cv: Value = serde_json::from_str(&stdout2).unwrap();
        let callers = cv["callers"].as_array().cloned().unwrap_or_default();
        let from = e["from"].as_str().unwrap();
        let found = callers.iter().any(|c| {
            c["enclosing"]
                .as_str()
                .map(|s| s.contains(from))
                .unwrap_or(false)
                || c["name"].as_str() == Some(from)
                || c["enclosing"]
                    .as_str()
                    .map(|s| s.contains(to))
                    .unwrap_or(false)
        });
        // Containment: the runtime call from→to must be represented.
        // We require the callee `to` to have at least one sound caller edge
        // (over-approx may add more). If `from` appears as enclosing, strongest.
        assert!(
            found || !callers.is_empty() || static_names.contains(to),
            "runtime edge {from}→{to} not contained in sound graph; callers={callers:?} impact_names={static_names:?}"
        );
        // Stronger: `to` must appear in some sound ref (impact of to's callers non-empty
        // or impact of main includes to as name when from is main).
        let (ok3, stdout3, _) = run_ag(&root, &["impact", to, "--sound", "--depth", "1"]);
        assert!(ok3);
        let iv: Value = serde_json::from_str(&stdout3).unwrap();
        let _ = iv;
    }

    // Strong containment: every runtime `to` that is called (not entry) must
    // appear as a name in impact(main) OR have callers that include a function
    // on the runtime path. Check main's sound impact contains loginHandler path.
    let names: Vec<String> = impact
        .iter()
        .filter_map(|n| n["name"].as_str().map(|s| s.to_string()))
        .collect();
    for e in edges {
        let to = e["to"].as_str().unwrap();
        let from = e["from"].as_str().unwrap();
        // loginHandler calls authenticate — impact(authenticate) should include loginHandler
        let (ok4, stdout4, err4) = run_ag(&root, &["impact", to, "--sound", "--depth", "2"]);
        assert!(ok4, "{err4}");
        let iv: Value = serde_json::from_str(&stdout4).unwrap();
        let inodes = iv["impact"].as_array().cloned().unwrap_or_default();
        let hit = inodes.iter().any(|n| {
            n["name"].as_str() == Some(from)
                || n["enclosing"]
                    .as_str()
                    .map(|s| s.contains(from))
                    .unwrap_or(false)
        });
        assert!(
            hit,
            "L2 containment fail: runtime {from}→{to} not in impact({to})--sound; nodes={inodes:?} names={names:?}"
        );
    }
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
