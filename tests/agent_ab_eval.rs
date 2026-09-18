//! P0-5 scripted tool-policy A/B/C evals — protocol, trajectories, harness, replay.
//!
//! Honesty (docs/eval-agent-baseline.md):
//! - Policies are **scripted deterministic tool-policy agents**, not live LLM agents.
//! - Public fixtures only (`fixtures/eval-agent-tasks/`); no private corpus.
//! - Offline replay scores recorded trajectories without network / binary.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
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
    panic!("no python interpreter for agent_ab_eval harness: {last_err}");
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
    // Committed trajectories must stay fixture-relative / public.
    if s.contains("stock-trading") || s.contains("private-stock") {
        return true;
    }
    // Absolute Windows user profiles / drive roots in tool args
    if s.contains("C:\\Users\\") || s.contains("D:\\Users\\") {
        return true;
    }
    false
}

#[test]
fn protocol_doc_and_harness_exist() {
    let doc = repo_root().join("docs").join("eval-agent-baseline.md");
    assert!(
        doc.is_file(),
        "missing docs/eval-agent-baseline.md (P0-5 protocol)"
    );
    let text = std::fs::read_to_string(&doc).expect("read protocol doc");
    assert!(
        text.contains("scripted") && text.to_lowercase().contains("not live"),
        "protocol must label scripted tool-policy agents vs live LLM"
    );
    assert!(
        text.contains("A") && text.contains("B") && text.contains("C"),
        "protocol must define A/B/C policies"
    );
    assert!(
        text.contains("Non-goals") || text.contains("non-goal") || text.contains("Non-claim"),
        "protocol must list non-goals / non-claims"
    );
    assert!(
        text.contains("replay") || text.contains("Replay"),
        "protocol must document offline replay"
    );

    let script = repo_root().join("scripts").join("eval_agent_ab.py");
    assert!(script.is_file(), "missing scripts/eval_agent_ab.py");
}

#[test]
fn committed_trajectories_cover_ge6_tasks_ge3_runs_ab() {
    let traj_root = repo_root().join("evals").join("agent-ab");
    assert!(
        traj_root.is_dir(),
        "missing evals/agent-ab/ replay fixtures"
    );

    let mut task_dirs: Vec<PathBuf> = std::fs::read_dir(&traj_root)
        .expect("read evals/agent-ab")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    task_dirs.sort();
    assert!(
        task_dirs.len() >= 6,
        "P0-5 needs >=6 public tasks with trajectories, found {}",
        task_dirs.len()
    );

    let mut tasks_with_ab = 0usize;
    for tdir in &task_dirs {
        let tid = tdir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut counts: BTreeMap<char, usize> = BTreeMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut private_hits: Vec<String> = vec![];

        let mut files: Vec<PathBuf> = std::fs::read_dir(tdir)
            .expect("read task traj dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        files.sort();
        assert!(
            !files.is_empty(),
            "{tid}: no trajectory JSON under {}",
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
            let policy = payload
                .get("policy")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_uppercase();
            let pol_ch = policy.chars().next().unwrap_or('?');
            *counts.entry(pol_ch).or_insert(0) += 1;

            let honesty = payload.get("honesty").cloned().unwrap_or_default();
            assert_eq!(
                honesty.get("live_llm_agent").and_then(|v| v.as_bool()),
                Some(false),
                "{}: must not claim live LLM agent",
                f.display()
            );
            assert_eq!(
                honesty.get("private_corpus").and_then(|v| v.as_bool()),
                Some(false),
                "{}: must not claim private corpus",
                f.display()
            );
            let kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            assert!(
                kind.starts_with("scripted") || kind == "name_grep_control",
                "{}: kind must be scripted policy / name-grep, got {kind}",
                f.display()
            );

            // C must remain name-grep control
            if pol_ch == 'C' {
                let method = payload
                    .pointer("/extras/method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                assert!(
                    method.contains("name-grep"),
                    "{}: C method must stay name-grep",
                    f.display()
                );
            }

            // B must not use agentgraph tools
            if pol_ch == 'B' {
                for c in payload
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
                {
                    let tool = c.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                    assert!(
                        !matches!(
                            tool,
                            "blast-radius" | "who-calls" | "subset" | "blast_radius" | "who_calls"
                        ),
                        "{}: B policy must not call agentgraph recipes (saw {tool})",
                        f.display()
                    );
                }
            }

            // A policy tool calls should include recipe tools when successful
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
                    tools.iter().any(|t| t == "blast-radius"),
                    "{}: A must invoke blast-radius",
                    f.display()
                );
                assert!(
                    tools.iter().any(|t| t == "who-calls" || t == "index"),
                    "{}: A must invoke who-calls or index",
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

            // private path scan (file set + tool args)
            for p in payload
                .get("file_set")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
            {
                if let Some(s) = p.as_str() {
                    if looks_private(s) {
                        private_hits.push(format!("file_set:{s}"));
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
                            private_hits.push(format!("args:{s}"));
                        }
                    }
                }
            }

            // score block present
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
        }

        assert!(
            private_hits.is_empty(),
            "{tid}: private paths leaked into trajectories: {private_hits:?}"
        );

        let a = counts.get(&'A').copied().unwrap_or(0);
        let b = counts.get(&'B').copied().unwrap_or(0);
        let _c = counts.get(&'C').copied().unwrap_or(0);
        if a >= 3 && b >= 3 {
            tasks_with_ab += 1;
        } else {
            eprintln!(
                "note: {tid} has A={a} B={b} (need >=3 each for full gate; tasks_with_ab counts complete ones)"
            );
        }
        assert!(
            a >= 3 && b >= 3,
            "{tid}: need >=3 A and >=3 B runs, got A={a} B={b} (runs: {seen:?})"
        );
    }

    assert!(
        tasks_with_ab >= 6,
        "need >=6 tasks with >=3 A and >=3 B runs, got {tasks_with_ab}"
    );
}

#[test]
fn offline_replay_scores_committed_trajectories() {
    let script = repo_root().join("scripts").join("eval_agent_ab.py");
    let traj_dir = repo_root().join("evals").join("agent-ab");
    assert!(script.is_file());
    assert!(traj_dir.is_dir());

    let out = repo_root().join("target").join("agent_ab_replay.json");
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
        .expect("spawn eval_agent_ab score replay");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "offline replay must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
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
        results.len() >= 18,
        "replay results expected >=18, got {}",
        results.len()
    );
    for r in results {
        assert_eq!(
            r.get("recorded_matches_replay").and_then(|v| v.as_bool()),
            Some(true),
            "trajectory replay mismatch: {r}"
        );
        assert_eq!(
            r.get("live_llm_agent").and_then(|v| v.as_bool()),
            Some(false)
        );
    }
}

#[test]
fn harness_eval_agent_ab_exits_zero() {
    let script = repo_root().join("scripts").join("eval_agent_ab.py");
    assert!(script.is_file(), "missing harness script");
    let out_json = repo_root().join("target").join("agent_ab_eval.json");
    if let Some(parent) = out_json.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let output = python_command()
        .arg(&script)
        .arg("run")
        .arg("--repo")
        .arg(repo_root())
        .arg("--bin")
        .arg(bin())
        .arg("--out")
        .arg(&out_json)
        .arg("--traj-dir")
        .arg(repo_root().join("evals").join("agent-ab"))
        .arg("--runs")
        .arg("3")
        .current_dir(repo_root())
        .output()
        .expect("spawn eval_agent_ab harness");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "harness must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        out_json.is_file(),
        "harness must write {}",
        out_json.display()
    );

    let payload = read_json(&out_json);
    assert_eq!(
        payload.get("schema").and_then(|v| v.as_str()),
        Some("agentgraph.eval_agent_ab.v1")
    );
    assert_eq!(
        payload
            .pointer("/honesty/live_llm_agent")
            .and_then(|v| v.as_bool()),
        Some(false),
        "result payload must not claim live LLM agents"
    );
    let summary = payload.get("summary").expect("summary");
    for pol in ["A", "B", "C"] {
        let s = summary
            .pointer(&format!("/summary_by_policy/{pol}"))
            .unwrap_or_else(|| panic!("summary_by_policy.{pol}"));
        assert!(
            s.get("mean_expected_file_recall").is_some(),
            "{pol}: mean recall required"
        );
        assert!(
            s.get("mean_extra_noise_files").is_some(),
            "{pol}: mean noise required"
        );
        assert!(
            s.get("run_count").and_then(|v| v.as_u64()).unwrap_or(0) >= 6,
            "{pol}: run_count should cover >=6 tasks × 3 seeds"
        );
    }
    // Product structure-fact expectation on these fixtures: A noise stays 0.
    let a_noise = summary
        .pointer("/summary_by_policy/A/mean_extra_noise_files")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    assert!(
        a_noise <= 0.001,
        "A mean extra-noise should be ~0 on public fixtures, got {a_noise}"
    );
    let a_rec = summary
        .pointer("/summary_by_policy/A/mean_expected_file_recall")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    assert!(
        a_rec >= 0.5,
        "A mean expected-file recall gate, got {a_rec}"
    );

    // Trajectories still present after harness rewrite
    let traj_root = repo_root().join("evals").join("agent-ab");
    let n_tasks = std::fs::read_dir(&traj_root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .count()
        })
        .unwrap_or(0);
    assert!(
        n_tasks >= 6,
        "traj dirs >=6 after harness run, got {n_tasks}"
    );
}
