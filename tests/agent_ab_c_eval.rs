//! P0-5c multi-runner hard-task A/B evals — coverage, metrics keys, honesty.
//!
//! Honesty (docs/eval-agent-baseline.md P0-5c):
//! - Hard fixtures under `fixtures/eval-agent-tasks-hard/` (public only).
//! - Trajectories record ≥2 runner kinds in one traj dir (`evals/agent-ab-c/`).
//! - Extended metrics keys present; `approx_tokens` null unless reported.
//! - No private corpus paths; `saw_labels_before_commit=false`.
//! - Host-session runner is **not** a multi-model lab (`independent_session` disclosed).
//! - Offline replay scores committed trajectories without network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn python_command() -> Command {
    let mut last_err = String::from("python not found");
    for cand in ["python", "python3", "py"] {
        let mut probe = Command::new(cand);
        probe
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match probe.status() {
            Ok(st) if st.success() => {
                let mut run = Command::new(cand);
                run.env("PYTHONIOENCODING", "utf-8");
                run.env("PYTHONUTF8", "1");
                return run;
            }
            Ok(st) => last_err = format!("{cand} exited {st}"),
            Err(e) => last_err = format!("{cand}: {e}"),
        }
    }
    panic!("no python interpreter for agent_ab_c harness: {last_err}");
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("cannot read {}: {e}", path.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("invalid JSON at {}: {e}", path.display());
    })
}

fn looks_private(s: &str) -> bool {
    if s.contains("stock-trading") || s.contains("private-stock") {
        return true;
    }
    if s.contains("C:\\Users\\") || s.contains("D:\\Users\\") {
        return true;
    }
    false
}

fn agentgraph_recipe_tool(tool: &str) -> bool {
    let t = tool.to_ascii_lowercase().replace('_', "-");
    t.starts_with("agentgraph.")
        || matches!(
            t.as_str(),
            "blast-radius" | "who-calls" | "subset" | "index" | "find" | "related" | "impact"
        )
}

#[test]
fn p0_5c_protocol_doc_and_readmes_exist() {
    let doc = repo_root().join("docs").join("eval-agent-baseline.md");
    assert!(doc.is_file(), "missing docs/eval-agent-baseline.md");
    let text = std::fs::read_to_string(&doc).expect("read protocol doc");
    assert!(
        text.contains("P0-5c"),
        "protocol must document P0-5c section"
    );
    assert!(
        text.contains("host_session_llm") && text.contains("scripted_external_runner"),
        "protocol must document ≥2 runner kinds"
    );
    assert!(
        text.contains("saw_labels_before_commit"),
        "protocol must record saw_labels_before_commit"
    );
    assert!(
        text.contains("No oversell") || text.to_lowercase().contains("no oversell"),
        "protocol must state no-oversell for P0-5c"
    );
    assert!(
        text.contains("chose_correct_workspace_root"),
        "protocol must document workspace-root metric"
    );

    let c_readme = repo_root()
        .join("evals")
        .join("agent-ab-c")
        .join("README.md");
    assert!(c_readme.is_file(), "missing evals/agent-ab-c/README.md");
    let live_readme = repo_root()
        .join("evals")
        .join("agent-ab-live")
        .join("README.md");
    assert!(
        live_readme.is_file(),
        "missing evals/agent-ab-live/README.md"
    );
    let live_text = std::fs::read_to_string(&live_readme).expect("read live readme");
    assert!(
        live_text.contains("P0-5c") || live_text.contains("agent-ab-c"),
        "live README must link P0-5c / agent-ab-c honesty"
    );

    let script = repo_root().join("scripts").join("eval_agent_ab_c.py");
    assert!(script.is_file(), "missing scripts/eval_agent_ab_c.py");
}

#[test]
fn p0_5c_hard_fixtures_exist_with_task_json() {
    let hard = repo_root().join("fixtures").join("eval-agent-tasks-hard");
    assert!(hard.is_dir(), "missing fixtures/eval-agent-tasks-hard/");
    let mut tasks: Vec<PathBuf> = std::fs::read_dir(&hard)
        .expect("read hard fixtures")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && (p.join("task.json")).is_file())
        .collect();
    tasks.sort();
    assert!(
        tasks.len() >= 4,
        "P0-5c needs >=4 hard tasks with task.json, found {}",
        tasks.len()
    );

    let mut saw_multi_root = false;
    let mut saw_sound_scoped = false;
    let mut saw_dense_noise = false;
    let mut saw_cross_crate = false;

    for tdir in &tasks {
        let tid = tdir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let meta = read_json(&tdir.join("task.json"));
        let expected = meta.get("expected").cloned().unwrap_or_default();
        let files = expected
            .get("files_that_matter")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let noise = expected
            .get("noise_files")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !files.is_empty(),
            "{tid}: expected.files_that_matter required"
        );
        assert!(!noise.is_empty(), "{tid}: expected.noise_files required");
        for p in files.iter().chain(noise.iter()) {
            let s = p.as_str().unwrap_or("");
            assert!(!looks_private(s), "{tid}: private path in task.json: {s}");
            assert!(
                tdir.join(s).is_file(),
                "{tid}: expected/noise file missing on disk: {s}"
            );
        }
        let issue = meta.get("issue").and_then(|v| v.as_str()).unwrap_or("");
        assert!(!issue.is_empty(), "{tid}: issue text required");
        let symbol = meta.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
        assert!(!symbol.is_empty(), "{tid}: symbol required");

        let ws = meta.get("workspace").cloned().unwrap_or_default();
        let multi = expected
            .get("multi_root")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || ws
                .get("roots")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
        if multi {
            saw_multi_root = true;
            let roots = expected
                .get("correct_workspace_roots")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            assert!(
                !roots.is_empty(),
                "{tid}: multi-root hard task must list correct_workspace_roots"
            );
        }
        if tid.contains("sound")
            || expected.get("window").and_then(|v| v.as_str()) == Some("default")
        {
            saw_sound_scoped = true;
        }
        if tid.contains("noise") || noise.len() >= 3 {
            saw_dense_noise = true;
        }
        if tid.contains("cross-crate") || multi {
            saw_cross_crate = true;
        }
        if expected
            .get("scoped_guidance")
            .map(|v| v.is_object())
            .unwrap_or(false)
        {
            let sg = expected.get("scoped_guidance").cloned().unwrap_or_default();
            if sg
                .get("expect_window_not_sound")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                saw_sound_scoped = true;
            }
        }
    }

    assert!(saw_multi_root, "need ≥1 multi-root hard task");
    assert!(
        saw_sound_scoped,
        "need ≥1 sound-disabled + clean sibling scoped case"
    );
    assert!(saw_dense_noise, "need ≥1 real-noise dense case");
    assert!(
        saw_cross_crate,
        "need ≥1 cross-crate / multi-root blast case"
    );
}

#[test]
fn p0_5c_trajectories_metrics_runners_honesty() {
    let traj_root = repo_root().join("evals").join("agent-ab-c");
    assert!(traj_root.is_dir(), "missing evals/agent-ab-c/");

    let rand_path = traj_root.join("task_randomization.json");
    assert!(
        rand_path.is_file(),
        "task-level randomization order must be recorded"
    );
    let rand = read_json(&rand_path);
    assert!(
        rand.get("seeds").and_then(|v| v.as_array()).is_some(),
        "randomization log needs seeds[]"
    );

    let mut runner_kinds: BTreeSet<String> = BTreeSet::new();
    let mut tasks_with_runners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut private_hits: Vec<String> = vec![];
    let mut traj_count = 0usize;

    let mut task_dirs: Vec<PathBuf> = std::fs::read_dir(&traj_root)
        .expect("read agent-ab-c")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    task_dirs.sort();
    assert!(
        !task_dirs.is_empty(),
        "need hard-task trajectory directories under evals/agent-ab-c/"
    );

    for tdir in &task_dirs {
        let tid = tdir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut arm_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(tdir)
            .expect("read task traj dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "{tid}: no trajectories");

        for f in &files {
            traj_count += 1;
            let payload = read_json(f);
            assert_eq!(
                payload.get("schema").and_then(|v| v.as_str()),
                Some("agentgraph.eval_agent_ab.trajectory.v1"),
                "{}: bad schema",
                f.display()
            );
            let runner_id = payload
                .get("runner_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            assert!(!runner_id.is_empty(), "{}: runner_id required", f.display());
            runner_kinds.insert(runner_id.clone());
            tasks_with_runners
                .entry(tid.clone())
                .or_default()
                .insert(runner_id.clone());

            let policy = payload
                .get("policy")
                .or_else(|| payload.get("arm"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_uppercase();
            let pol_ch = policy.chars().next().unwrap_or('?');
            assert!(
                pol_ch == 'A' || pol_ch == 'B',
                "{}: arm must be A or B",
                f.display()
            );
            *arm_counts
                .entry(format!("{runner_id}:{pol_ch}"))
                .or_insert(0) += 1;

            // Extended metrics keys
            assert!(
                payload.get("mcp_or_cli_calls").is_some(),
                "{}: mcp_or_cli_calls required",
                f.display()
            );
            let mcp = payload.get("mcp_or_cli_calls").cloned().unwrap_or_default();
            assert!(
                mcp.get("count").is_some(),
                "{}: mcp_or_cli_calls.count required",
                f.display()
            );
            assert!(
                mcp.get("recipe_tools").and_then(|v| v.as_array()).is_some(),
                "{}: mcp_or_cli_calls.recipe_tools required",
                f.display()
            );
            assert!(
                mcp.get("grep_count").is_some(),
                "{}: mcp_or_cli_calls.grep_count required",
                f.display()
            );
            // chose_correct_workspace_root present (bool or null)
            assert!(
                payload.get("chose_correct_workspace_root").is_some(),
                "{}: chose_correct_workspace_root key required (bool|null)",
                f.display()
            );
            assert!(
                payload
                    .get("file_budget")
                    .and_then(|v| v.as_u64())
                    .is_some(),
                "{}: file_budget required",
                f.display()
            );
            // read_budget may be null; key must exist
            assert!(
                payload.get("read_budget").is_some(),
                "{}: read_budget key required",
                f.display()
            );
            // approx_tokens never invented: null or number
            let approx = payload.get("approx_tokens");
            assert!(
                approx.is_some(),
                "{}: approx_tokens key required",
                f.display()
            );
            if let Some(a) = approx {
                assert!(
                    a.is_null() || a.is_number(),
                    "{}: approx_tokens must be null or number",
                    f.display()
                );
            }
            assert_eq!(
                payload
                    .get("saw_labels_before_commit")
                    .and_then(|v| v.as_bool()),
                Some(false),
                "{}: saw_labels_before_commit must be false",
                f.display()
            );
            let model_note = payload
                .get("model_note")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            assert!(
                !model_note.is_empty(),
                "{}: model_note required",
                f.display()
            );

            let honesty = payload.get("honesty").cloned().unwrap_or_default();
            assert_eq!(
                honesty.get("private_corpus").and_then(|v| v.as_bool()),
                Some(false),
                "{}: private_corpus must be false",
                f.display()
            );
            assert_eq!(
                honesty
                    .get("saw_labels_before_commit")
                    .and_then(|v| v.as_bool()),
                Some(false),
                "{}: honesty.saw_labels_before_commit false",
                f.display()
            );
            assert_eq!(
                honesty
                    .get("no_oversell_live_a_beats_b")
                    .and_then(|v| v.as_bool()),
                Some(true),
                "{}: must flag no oversell",
                f.display()
            );

            // Runner-kind honesty
            if runner_id == "host_session_llm" {
                assert!(
                    model_note.contains("mimo-desktop-host-session"),
                    "{}: host_session model_note",
                    f.display()
                );
                assert_eq!(
                    payload.get("independent_session").and_then(|v| v.as_bool()),
                    Some(false),
                    "{}: host_session independent_session must be false (disclosed)",
                    f.display()
                );
            } else if runner_id == "scripted_external_runner" {
                assert_eq!(
                    payload.get("independent_session").and_then(|v| v.as_bool()),
                    Some(true),
                    "{}: scripted decision path independent_session true",
                    f.display()
                );
            }

            // Arm isolation
            if pol_ch == 'B' {
                for c in payload
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
                {
                    let tool = c.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                    assert!(
                        !agentgraph_recipe_tool(tool),
                        "{}: arm B must not call agentgraph (saw {tool})",
                        f.display()
                    );
                }
                let count = mcp.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                assert_eq!(
                    count,
                    0,
                    "{}: arm B mcp_or_cli_calls.count must be 0",
                    f.display()
                );
            }
            if pol_ch == 'A' {
                let tools: Vec<String> = payload
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|c| {
                        c.get("tool")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect();
                assert!(
                    tools.iter().any(|t| agentgraph_recipe_tool(t)),
                    "{}: arm A must invoke agentgraph recipes",
                    f.display()
                );
                let count = mcp.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                assert!(
                    count >= 1,
                    "{}: arm A mcp_or_cli_calls.count >= 1",
                    f.display()
                );
            }

            // score stamped
            let score = payload.get("score").expect("score block");
            assert_eq!(
                score.get("stamped").and_then(|v| v.as_bool()),
                Some(true),
                "{}: score must be stamped offline",
                f.display()
            );
            assert!(score.get("expected_file_recall").is_some());
            assert!(score.get("extra_noise_count").is_some());

            // private path scan
            for p in payload
                .get("file_set")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
            {
                if let Some(s) = p.as_str() {
                    if looks_private(s) {
                        private_hits.push(format!("{} file_set:{s}", f.display()));
                    }
                }
            }
        }

        // Each hard task dir should have both arms for at least one runner
        let has_a = arm_counts.keys().any(|k| k.ends_with(":A"));
        let has_b = arm_counts.keys().any(|k| k.ends_with(":B"));
        assert!(
            has_a && has_b,
            "{tid}: need both arms A and B, got {arm_counts:?}"
        );
    }

    assert!(
        private_hits.is_empty(),
        "private paths leaked into P0-5c trajectories: {private_hits:?}"
    );
    assert!(
        runner_kinds.len() >= 2,
        "need ≥2 runner kinds in evals/agent-ab-c, got {runner_kinds:?}"
    );
    assert!(
        runner_kinds.contains("host_session_llm") && runner_kinds.contains("scripted_external_runner"),
        "runner kinds must include host_session_llm + scripted_external_runner, got {runner_kinds:?}"
    );
    assert!(
        traj_count >= 16,
        "expected substantial hard-task trajectories, got {traj_count}"
    );
    // ≥2 runner kinds present together on at least one task
    let together = tasks_with_runners.values().filter(|s| s.len() >= 2).count();
    assert!(
        together >= 1,
        "need ≥2 runner kinds in one traj dir for at least one task"
    );
}

#[test]
fn p0_5c_offline_replay_includes_extended_metrics() {
    let script = repo_root().join("scripts").join("eval_agent_ab_c.py");
    let traj_dir = repo_root().join("evals").join("agent-ab-c");
    assert!(script.is_file());
    assert!(traj_dir.is_dir());

    let out = repo_root()
        .join("target")
        .join("agent_ab_c_replay_gate.json");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let output = python_command()
        .arg(&script)
        .arg("score")
        .arg("--traj-dir")
        .arg(&traj_dir)
        .arg("--out")
        .arg(&out)
        .current_dir(repo_root())
        .output()
        .expect("spawn eval_agent_ab_c score");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "P0-5c offline score must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let payload = read_json(&out);
    assert_eq!(
        payload.get("schema").and_then(|v| v.as_str()),
        Some("agentgraph.eval_agent_ab.c.replay.v1")
    );
    assert_eq!(payload.get("offline").and_then(|v| v.as_bool()), Some(true));
    let kinds = payload
        .get("runner_kinds_present")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let kind_set: BTreeSet<String> = kinds
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        kind_set.len() >= 2,
        "replay must report ≥2 runner kinds, got {kind_set:?}"
    );
    let summary = payload
        .get("summary_by_runner_arm")
        .and_then(|v| v.as_object())
        .cloned()
        .expect("summary_by_runner_arm");
    assert!(
        summary.len() >= 4,
        "summary should cover runner×arm cells, got {}",
        summary.len()
    );
    for (key, cell) in &summary {
        assert!(
            cell.get("mean_expected_file_recall").is_some(),
            "{key}: recall required"
        );
        assert!(
            cell.get("mean_extra_noise_files").is_some(),
            "{key}: noise required"
        );
        assert!(
            cell.get("mean_mcp_or_cli_calls").is_some(),
            "{key}: mean_mcp_or_cli_calls required"
        );
        assert!(
            cell.get("mean_file_budget").is_some(),
            "{key}: mean_file_budget required"
        );
    }
    let results = payload
        .get("results")
        .and_then(|v| v.as_array())
        .expect("results[]");
    assert!(results.len() >= 16);
    for r in results {
        assert_eq!(
            r.get("recorded_matches_replay").and_then(|v| v.as_bool()),
            Some(true),
            "replay mismatch: {r}"
        );
        assert!(
            r.get("mcp_or_cli_calls").is_some(),
            "replay rows must echo mcp_or_cli_calls: {r}"
        );
        assert!(
            r.get("chose_correct_workspace_root").is_some(),
            "replay rows must echo chose_correct_workspace_root: {r}"
        );
        assert_eq!(
            r.get("saw_labels_before_commit").and_then(|v| v.as_bool()),
            Some(false),
            "replay rows must keep saw_labels_before_commit=false: {r}"
        );
    }
}
