//! P0-5b live host-session Agent A/B evals — coverage, replay, honesty gates.
//!
//! Honesty (docs/eval-agent-baseline.md P0-5b):
//! - Trajectories are **live host-session LLM** agent runs
//!   (`model_note=mimo-desktop-host-session`) — **not** a public benchmark
//!   model id, **not** a standardized lab harness.
//! - Public fixtures only; no private corpus paths.
//! - Offline replay scores committed trajectories without network.
//! - Live Arm B must not call agentgraph recipes.
//! - Live Arm A must invoke agentgraph recipes.

use std::collections::{BTreeMap, HashSet};
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
    panic!("no python interpreter for agent_ab_live harness: {last_err}");
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
    // Absolute Windows user profiles / drive roots in public trajectories
    if s.contains("C:\\Users\\") || s.contains("D:\\Users\\") {
        return true;
    }
    false
}

fn agentgraph_recipe_tool(tool: &str) -> bool {
    let t = tool.to_ascii_lowercase().replace('_', "-");
    matches!(
        t.as_str(),
        "blast-radius"
            | "who-calls"
            | "subset"
            | "agentgraph.blast-radius"
            | "agentgraph.who-calls"
            | "agentgraph.subset"
            | "agentgraph.index"
            | "agentgraph.find"
            | "agentgraph.related"
    ) || t.starts_with("agentgraph.")
}

#[test]
fn live_protocol_doc_and_readme_exist() {
    let doc = repo_root().join("docs").join("eval-agent-baseline.md");
    assert!(doc.is_file(), "missing docs/eval-agent-baseline.md");
    let text = std::fs::read_to_string(&doc).expect("read protocol doc");
    assert!(
        text.contains("P0-5b"),
        "protocol must document P0-5b live section"
    );
    assert!(
        text.to_lowercase().contains("live"),
        "protocol must label live vs scripted"
    );
    assert!(
        text.contains("mimo-desktop-host-session"),
        "protocol must record honest model_note"
    );
    assert!(
        text.contains("contamination") || text.to_lowercase().contains("contaminat"),
        "protocol must disclose arm/session contamination limits"
    );
    assert!(
        text.contains("No oversell")
            || text.contains("no oversell")
            || text.contains("no oversell"),
        "protocol must state no-oversell for live results"
    );

    let readme = repo_root()
        .join("evals")
        .join("agent-ab-live")
        .join("README.md");
    assert!(readme.is_file(), "missing evals/agent-ab-live/README.md");

    let script = repo_root().join("scripts").join("eval_agent_ab_live.py");
    assert!(script.is_file(), "missing scripts/eval_agent_ab_live.py");
}

#[test]
fn live_trajectories_cover_ge6_tasks_ab_and_honesty() {
    let traj_root = repo_root().join("evals").join("agent-ab-live");
    assert!(
        traj_root.is_dir(),
        "missing evals/agent-ab-live/ replay fixtures"
    );

    let mut task_dirs: Vec<PathBuf> = std::fs::read_dir(&traj_root)
        .expect("read evals/agent-ab-live")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    task_dirs.sort();
    assert!(
        task_dirs.len() >= 6,
        "P0-5b needs >=6 public tasks with live trajectories, found {}",
        task_dirs.len()
    );

    let mut tasks_with_ab = 0usize;
    let mut private_hits: Vec<String> = vec![];

    for tdir in &task_dirs {
        let tid = tdir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut counts: BTreeMap<char, usize> = BTreeMap::new();
        let mut seen: HashSet<String> = HashSet::new();

        let mut files: Vec<PathBuf> = std::fs::read_dir(tdir)
            .expect("read live task traj dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        files.sort();
        assert!(
            !files.is_empty(),
            "{tid}: no live trajectory JSON under {}",
            tdir.display()
        );

        for f in &files {
            let payload = read_json(f);
            assert_eq!(
                payload.get("schema").and_then(|v| v.as_str()),
                Some("agentgraph.eval_agent_ab.trajectory.v1"),
                "{}: bad schema",
                f.display()
            );
            let kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            assert_eq!(
                kind,
                "live_llm_agent",
                "{}: kind must be live_llm_agent",
                f.display()
            );
            let honesty = payload.get("honesty").cloned().unwrap_or_default();
            assert_eq!(
                honesty.get("live_llm_agent").and_then(|v| v.as_bool()),
                Some(true),
                "{}: honesty.live_llm_agent must be true",
                f.display()
            );
            assert_eq!(
                honesty.get("private_corpus").and_then(|v| v.as_bool()),
                Some(false),
                "{}: must not claim private corpus",
                f.display()
            );
            assert_eq!(
                honesty
                    .get("not_public_benchmark_model")
                    .and_then(|v| v.as_bool()),
                Some(true),
                "{}: must flag non-benchmark host session model",
                f.display()
            );

            let model_note = payload
                .get("model_note")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            assert!(
                model_note.contains("mimo-desktop-host-session"),
                "{}: model_note must be honest host-session id, got {model_note}",
                f.display()
            );

            let policy = payload
                .get("policy")
                .or_else(|| payload.get("arm"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_uppercase();
            let pol_ch = policy.chars().next().unwrap_or('?');
            assert!(
                pol_ch == 'A' || pol_ch == 'B',
                "{}: live policy/arm must be A or B, got {policy}",
                f.display()
            );
            *counts.entry(pol_ch).or_insert(0) += 1;

            // Live B must not use agentgraph tools
            if pol_ch == 'B' {
                let allowed = payload
                    .get("tools_allowed")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                for a in &allowed {
                    let s = a.as_str().unwrap_or("");
                    assert!(
                        !s.to_ascii_lowercase().contains("agentgraph"),
                        "{}: live B tools_allowed must not include agentgraph: {s}",
                        f.display()
                    );
                }
                for c in payload
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
                {
                    let tool = c.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                    assert!(
                        !agentgraph_recipe_tool(tool),
                        "{}: live B must not call agentgraph recipes (saw {tool})",
                        f.display()
                    );
                }
            }

            // Live A must include agentgraph recipe evidence
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
                    tools
                        .iter()
                        .any(|t| t.contains("blast-radius") || t.contains("blast_radius")),
                    "{}: live A must invoke blast-radius",
                    f.display()
                );
                assert!(
                    tools.iter().any(|t| t.contains("who-calls")
                        || t.contains("who_calls")
                        || t.contains("index")),
                    "{}: live A must invoke who-calls or index",
                    f.display()
                );
            }

            let run_id = payload
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            assert!(!run_id.is_empty(), "{}: missing run_id", f.display());
            seen.insert(format!("{policy}:{run_id}"));

            // private path scan (file_set + tool args)
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
            for c in payload
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
            {
                for a in c
                    .get("args")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
                {
                    if let Some(s) = a.as_str() {
                        if looks_private(s) {
                            private_hits.push(format!("{} args:{s}", f.display()));
                        }
                    }
                }
            }

            // score block present (stamped after file_set commitment)
            let score = payload.get("score").expect("score block");
            assert!(
                score.get("expected_file_recall").is_some(),
                "{}: score.expected_file_recall required",
                f.display()
            );
            assert!(
                score.get("extra_noise_count").is_some(),
                "{}: score.extra_noise_count required",
                f.display()
            );
            assert_eq!(
                score.get("stamped").and_then(|v| v.as_bool()),
                Some(true),
                "{}: live score must be stamped offline after commitment",
                f.display()
            );
        }

        assert!(
            private_hits.is_empty(),
            "{tid}: private paths leaked into live trajectories: {private_hits:?}"
        );

        let a = counts.get(&'A').copied().unwrap_or(0);
        let b = counts.get(&'B').copied().unwrap_or(0);
        if a >= 1 && b >= 1 {
            tasks_with_ab += 1;
        }
        assert!(
            a >= 1 && b >= 1,
            "{tid}: need >=1 live A and >=1 live B, got A={a} B={b} (runs: {seen:?})"
        );
    }

    assert!(
        tasks_with_ab >= 6,
        "need >=6 tasks with live A and live B trajectories, got {tasks_with_ab}"
    );
}

#[test]
fn offline_replay_scores_live_trajectories() {
    let script = repo_root().join("scripts").join("eval_agent_ab.py");
    let traj_dir = repo_root().join("evals").join("agent-ab-live");
    assert!(script.is_file());
    assert!(traj_dir.is_dir());

    let out = repo_root()
        .join("target")
        .join("agent_ab_live_replay_gate.json");
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
        .expect("spawn eval_agent_ab score replay on live trajs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "offline live replay must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(out.is_file(), "replay must write {}", out.display());
    let payload = read_json(&out);
    assert_eq!(
        payload.get("schema").and_then(|v| v.as_str()),
        Some("agentgraph.eval_agent_ab.replay.v1")
    );
    assert_eq!(
        payload.get("offline").and_then(|v| v.as_bool()),
        Some(true),
        "replay must be offline"
    );
    let results = payload
        .get("results")
        .and_then(|v| v.as_array())
        .expect("results[]");
    assert!(
        results.len() >= 12,
        "live replay results expected >=12, got {}",
        results.len()
    );
    for r in results {
        assert_eq!(
            r.get("recorded_matches_replay").and_then(|v| v.as_bool()),
            Some(true),
            "live trajectory replay mismatch: {r}"
        );
        assert_eq!(
            r.get("live_llm_agent").and_then(|v| v.as_bool()),
            Some(true),
            "live replay rows must flag live_llm_agent=true: {r}"
        );
    }
}

#[test]
fn live_specific_scorer_exits_zero() {
    let script = repo_root().join("scripts").join("eval_agent_ab_live.py");
    assert!(script.is_file(), "missing scripts/eval_agent_ab_live.py");
    let traj_dir = repo_root().join("evals").join("agent-ab-live");
    let out = repo_root()
        .join("target")
        .join("agent_ab_live_score_gate.json");
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
        .expect("spawn eval_agent_ab_live score");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "live scorer must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let payload = read_json(&out);
    let summary = payload
        .get("summary_by_arm")
        .unwrap_or_else(|| panic!("summary_by_arm required"));
    for arm in ["A", "B"] {
        let s = summary
            .get(arm)
            .unwrap_or_else(|| panic!("summary_by_arm.{arm}"));
        assert!(
            s.get("run_count").and_then(|v| v.as_u64()).unwrap_or(0) >= 6,
            "{arm}: live run_count should cover >=6 tasks"
        );
        assert!(
            s.get("mean_expected_file_recall").is_some(),
            "{arm}: mean recall required"
        );
        assert!(
            s.get("mean_extra_noise_files").is_some(),
            "{arm}: mean noise required"
        );
    }
}
