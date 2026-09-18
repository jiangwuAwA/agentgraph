//! P0-5d isolated lab harness — brief isolation, offline stamp/score, honesty, lab_ready gate.
//!
//! Honesty (docs/eval-agent-baseline.md § P0-5d):
//! - Brief packs are issue-only: no `expected` / `noise_files` strings.
//! - Harness never injects goldens; stamp scores offline from fixture task.json.
//! - Trajectories carry honesty fields (`saw_labels_before_commit=false`, no oversell).
//! - `lab_ready=true` is forbidden when the live matrix is incomplete (S1 default).
//! - `score` refuses forged fields (bad stamp, forged independence, forged lab_ready).
//! - Offline stamp/score only — no live agents, no network.

use std::collections::BTreeSet;
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
    panic!("no python interpreter for agent_ab_d harness: {last_err}");
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("cannot read {}: {e}", path.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("invalid JSON at {}: {e}", path.display());
    })
}

fn write_json(path: &Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir for json");
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(value).expect("serialize") + "\n",
    )
    .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
}

fn harness_script() -> PathBuf {
    repo_root().join("scripts").join("eval_agent_ab_d.py")
}

fn run_harness(args: &[&str], cwd: &Path) -> (bool, String, String) {
    let output = python_command()
        .arg(harness_script())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn eval_agent_ab_d");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.success(), stdout, stderr)
}

const BAN_SUBSTRINGS: &[&str] = &[
    "expected",
    "noise_files",
    "files_that_matter",
    "forbidden_files",
];

fn brief_files_contain_ban(root: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    let briefs = root.join("_briefs");
    if !briefs.is_dir() {
        return hits;
    }
    for entry in walk_files(&briefs) {
        let ext = entry.extension().map(|s| s.to_string_lossy().to_string());
        if !matches!(ext.as_deref(), Some("md") | Some("json") | Some("txt")) {
            continue;
        }
        // Runner outputs after a live/scripted run may mention schema keys in
        // meta/file_set *schemas*; ban applies to brief *instruction* packs.
        let name = entry
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if name != "ISSUE.md" && name != "RUNNER_CONTRACT.md" {
            continue;
        }
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        let lower = text.to_lowercase();
        for ban in BAN_SUBSTRINGS {
            if lower.contains(ban) {
                hits.push(format!("{}: contains {ban}", entry.display()));
            }
        }
    }
    hits
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn ensure_prepare_artifacts() {
    let selection = repo_root()
        .join("evals")
        .join("agent-ab-d")
        .join("task_selection.json");
    if selection.is_file() {
        return;
    }
    let (ok, stdout, stderr) = run_harness(
        &[
            "prepare",
            "--traj-dir",
            "evals/agent-ab-d",
            "--seeds",
            "0,1,2,3,4",
        ],
        &repo_root(),
    );
    assert!(
        ok,
        "prepare must succeed when selection missing\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn p0_5d_protocol_doc_and_harness_exist() {
    let doc = repo_root().join("docs").join("eval-agent-baseline.md");
    assert!(doc.is_file(), "missing docs/eval-agent-baseline.md");
    let text = std::fs::read_to_string(&doc).expect("read protocol doc");
    assert!(text.contains("P0-5d"), "protocol must document P0-5d");
    assert!(text.contains("lab_ready"), "protocol must define lab_ready");
    assert!(
        text.contains("independent_session"),
        "protocol must document independent_session"
    );
    assert!(
        text.contains("N≥5") || text.contains("N>=5") || text.contains("min_seeds_per_cell"),
        "protocol must document N≥5 lab target"
    );
    assert!(
        text.to_lowercase().contains("no oversell") || text.contains("No oversell"),
        "protocol must keep no-oversell language"
    );
    assert!(
        text.contains("Contamination ban list") || text.contains("污染"),
        "protocol must list contamination bans"
    );
    assert!(
        text.contains("easy≥4") || text.contains("easy"),
        "protocol must document easy/hard task selection"
    );

    let script = harness_script();
    assert!(script.is_file(), "missing scripts/eval_agent_ab_d.py");
    let script_text = std::fs::read_to_string(&script).expect("read harness");
    for needle in [
        "prepare",
        "run-runner",
        "stamp",
        "score",
        "lab-ready",
        "lab_ready",
        "never injects",
    ] {
        assert!(
            script_text.contains(needle),
            "harness must mention {needle}"
        );
    }

    let readme = repo_root()
        .join("evals")
        .join("agent-ab-d")
        .join("README.md");
    assert!(readme.is_file(), "missing evals/agent-ab-d/README.md");
    let readme_text = std::fs::read_to_string(&readme).expect("read d readme");
    assert!(
        readme_text.contains("lab_ready"),
        "P0-5d README must state lab_ready honesty"
    );
    assert!(
        readme_text.contains("false") || readme_text.contains("FALSE"),
        "P0-5d README must not oversell lab_ready=true"
    );
}

#[test]
fn p0_5d_task_selection_easy_and_hard_plus_randomization() {
    ensure_prepare_artifacts();
    let droot = repo_root().join("evals").join("agent-ab-d");
    let selection = read_json(&droot.join("task_selection.json"));
    assert_eq!(
        selection.get("schema").and_then(|v| v.as_str()),
        Some("agentgraph.eval_agent_ab_d.selection.v1")
    );
    let easy: Vec<String> = selection
        .get("easy_tasks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    let hard: Vec<String> = selection
        .get("hard_tasks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    assert!(easy.len() >= 4, "P0-5d needs easy≥4 tasks, got {:?}", easy);
    assert!(hard.len() >= 4, "P0-5d needs hard≥4 tasks, got {:?}", hard);
    // Documented selection anchors from the task card.
    for t in [
        "ts-nest-user-repo",
        "rust-trait-handler",
        "py-plugin-registry",
        "go-store-api",
    ] {
        assert!(
            easy.iter().any(|x| x == t),
            "easy set should include {t}, got {easy:?}"
        );
    }
    for t in [
        "rust-cross-crate-blast",
        "rust-real-noise-dense",
        "rust-sound-scoped-clean",
        "ts-multi-root-client",
    ] {
        assert!(
            hard.iter().any(|x| x == t),
            "hard set should include {t}, got {hard:?}"
        );
    }
    assert!(
        selection
            .get("rationale")
            .map(|v| v.is_object())
            .unwrap_or(false),
        "selection must document rationale"
    );
    assert!(
        selection
            .get("lab_ready_definition")
            .map(|v| v.is_object())
            .unwrap_or(false),
        "selection must document lab_ready criteria"
    );
    let ban = selection
        .get("contamination_ban_list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        ban.len() >= 4,
        "contamination ban list must be recorded, got {ban:?}"
    );

    let rand = read_json(&droot.join("task_randomization.json"));
    assert_eq!(
        rand.get("schema").and_then(|v| v.as_str()),
        Some("agentgraph.eval_agent_ab_d.randomization.v1")
    );
    let seeds = rand
        .get("seeds")
        .and_then(|v| v.as_array())
        .cloned()
        .expect("randomization seeds[]");
    assert!(!seeds.is_empty(), "randomization log must not be empty");
    for row in &seeds {
        let order = row
            .get("task_order")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            order.len() >= 8,
            "each seed must randomize the full task set, got {}",
            order.len()
        );
        let arm_order = row
            .get("arm_order")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            arm_order.len() == 2,
            "arm_order must cover A and B, got {arm_order:?}"
        );
    }
}

#[test]
fn p0_5d_briefs_are_issue_only_no_label_strings() {
    ensure_prepare_artifacts();
    let droot = repo_root().join("evals").join("agent-ab-d");
    let briefs = droot.join("_briefs");
    assert!(briefs.is_dir(), "missing evals/agent-ab-d/_briefs/");
    let issues: Vec<PathBuf> = walk_files(&briefs)
        .into_iter()
        .filter(|p| p.file_name().map(|s| s == "ISSUE.md").unwrap_or(false))
        .collect();
    assert!(
        issues.len() >= 16,
        "prepare must materialize brief packs, found {} ISSUE.md",
        issues.len()
    );

    let hits = brief_files_contain_ban(&droot);
    assert!(
        hits.is_empty(),
        "brief packs must not contain structure-fact label strings: {hits:?}"
    );

    // Isolated workdir: if present, must not ship task.json (decision-path isolation).
    let mut checked_workdirs = 0usize;
    for issue in &issues {
        let workdir = issue.parent().map(|p| p.join("workdir"));
        if let Some(wd) = workdir {
            if wd.is_dir() {
                checked_workdirs += 1;
                assert!(
                    !wd.join("task.json").is_file(),
                    "workdir must not contain task.json: {}",
                    wd.display()
                );
            }
        }
    }
    // workdirs are optional (gitignored); when present they must be clean.
    if checked_workdirs == 0 {
        eprintln!("note: no workdirs materialized under briefs (ok for harness-only tree)");
    }
}

#[test]
fn p0_5d_offline_stamp_score_and_lab_ready_false() {
    ensure_prepare_artifacts();
    let tmp = repo_root().join("target").join("agent_ab_d_eval_offline");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).expect("clean tmp traj dir");
    }
    std::fs::create_dir_all(&tmp).expect("mkdir tmp traj dir");

    // Prepare a minimal isolated matrix (2 seeds × scripted runner only).
    let tmp_s = tmp.to_string_lossy().to_string();
    let (ok, stdout, stderr) = run_harness(
        &[
            "prepare",
            "--traj-dir",
            &tmp_s,
            "--seeds",
            "0,1",
            "--runner",
            "scripted_isolated_runner",
            "--skip-workdirs",
        ],
        &repo_root(),
    );
    assert!(
        ok,
        "prepare tmp matrix must succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let hits = brief_files_contain_ban(&tmp);
    assert!(hits.is_empty(), "tmp briefs banned strings: {hits:?}");

    // Offline scripted fill + contract validate (not live LLM).
    let (ok, stdout, stderr) = run_harness(
        &[
            "run-runner",
            "--traj-dir",
            &tmp_s,
            "--runner",
            "scripted_isolated_runner",
            "--fill-scripted",
        ],
        &repo_root(),
    );
    assert!(
        ok,
        "run-runner fill-scripted must succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // Stamp offline from fixture task.json.
    let (ok, stdout, stderr) = run_harness(
        &[
            "stamp",
            "--traj-dir",
            &tmp_s,
            "--runner",
            "scripted_isolated_runner",
        ],
        &repo_root(),
    );
    assert!(
        ok,
        "stamp must succeed offline\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let stamped: Vec<PathBuf> = walk_files(&tmp)
        .into_iter()
        .filter(|p| {
            p.file_name()
                .map(|s| s.to_string_lossy().starts_with("run-"))
                .unwrap_or(false)
                && p.extension().map(|s| s == "json").unwrap_or(false)
        })
        .filter(|p| !p.to_string_lossy().contains("_briefs"))
        .collect();
    assert!(
        !stamped.is_empty(),
        "stamp must write trajectory JSON under task dirs"
    );

    for path in &stamped {
        let payload = read_json(path);
        assert_eq!(
            payload.get("schema").and_then(|v| v.as_str()),
            Some("agentgraph.eval_agent_ab.trajectory.v1"),
            "{}: schema",
            path.display()
        );
        assert_eq!(
            payload.get("schema_alias").and_then(|v| v.as_str()),
            Some("agentgraph.eval_agent_ab.d.v1"),
            "{}: schema_alias",
            path.display()
        );
        let score = payload.get("score").expect("score block");
        assert_eq!(
            score.get("stamped").and_then(|v| v.as_bool()),
            Some(true),
            "{}: score must be stamped",
            path.display()
        );
        assert!(
            score.get("stamp_offline").and_then(|v| v.as_bool()) == Some(true)
                || score
                    .get("stamp_source")
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains("task.json"))
                    .unwrap_or(false),
            "{}: stamp must record offline fixture source",
            path.display()
        );
        assert_eq!(
            payload
                .get("saw_labels_before_commit")
                .and_then(|v| v.as_bool()),
            Some(false),
            "{}: saw_labels_before_commit",
            path.display()
        );
        let honesty = payload.get("honesty").expect("honesty");
        assert_eq!(
            honesty.get("private_corpus").and_then(|v| v.as_bool()),
            Some(false),
            "{}: private_corpus",
            path.display()
        );
        assert_eq!(
            honesty
                .get("no_oversell_live_a_beats_b")
                .and_then(|v| v.as_bool()),
            Some(true),
            "{}: no_oversell",
            path.display()
        );
        assert_eq!(
            honesty.get("lab_ready_claim").and_then(|v| v.as_bool()),
            Some(false),
            "{}: individual traj must not claim lab_ready",
            path.display()
        );
        assert_eq!(
            honesty
                .get("saw_labels_before_commit")
                .and_then(|v| v.as_bool()),
            Some(false),
            "{}: honesty.saw_labels_before_commit",
            path.display()
        );
        // Metrics keys aligned P0-5c
        assert!(
            payload.get("mcp_or_cli_calls").is_some(),
            "{}: mcp_or_cli_calls",
            path.display()
        );
        assert!(
            payload.get("chose_correct_workspace_root").is_some(),
            "{}: chose_correct_workspace_root key",
            path.display()
        );
        assert!(
            payload
                .get("file_budget")
                .and_then(|v| v.as_u64())
                .is_some(),
            "{}: file_budget",
            path.display()
        );
        assert!(
            payload.get("read_budget").is_some(),
            "{}: read_budget key",
            path.display()
        );
        let approx = payload.get("approx_tokens");
        assert!(approx.is_some(), "{}: approx_tokens key", path.display());
        if let Some(a) = approx {
            assert!(
                a.is_null() || a.is_number(),
                "{}: approx_tokens null-or-number",
                path.display()
            );
        }
        let kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            kind,
            "live_llm_agent",
            "{}: scripted fill must not be labeled live",
            path.display()
        );
        assert_eq!(
            payload.get("independent_session").and_then(|v| v.as_bool()),
            Some(true),
            "{}: scripted decision path independent_session",
            path.display()
        );
        assert!(
            payload
                .get("model_note")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty() && !s.contains("mimo-desktop-host-session"))
                .unwrap_or(false),
            "{}: scripted model_note must be non-author",
            path.display()
        );
    }

    // Offline score (scripted-only matrix is partial — allow, but lab_ready must be false).
    let out = tmp.join("replay.json");
    let out_s = out.to_string_lossy().to_string();
    let (ok, stdout, stderr) = run_harness(
        &[
            "score",
            "--traj-dir",
            &tmp_s,
            "--out",
            &out_s,
            "--allow-empty",
            "--allow-partial",
        ],
        &repo_root(),
    );
    assert!(
        ok,
        "score on stamped scripted cells must exit 0 when forgeries absent\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let replay = read_json(&out);
    assert_eq!(
        replay.get("schema").and_then(|v| v.as_str()),
        Some("agentgraph.eval_agent_ab_d.replay.v1")
    );
    assert_eq!(replay.get("offline").and_then(|v| v.as_bool()), Some(true));
    let lab = replay
        .get("lab_ready_eval")
        .cloned()
        .expect("lab_ready_eval");
    assert_eq!(
        lab.get("lab_ready").and_then(|v| v.as_bool()),
        Some(false),
        "incomplete/scripted matrix must not be lab_ready: {lab}"
    );
    let gaps = lab
        .get("gaps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !gaps.is_empty(),
        "lab_ready=false requires a non-empty gap list"
    );

    // lab-ready command agrees.
    let lab_out = tmp.join("lab_ready.json");
    let lab_out_s = lab_out.to_string_lossy().to_string();
    let (ok, stdout, stderr) = run_harness(
        &["lab-ready", "--traj-dir", &tmp_s, "--out", &lab_out_s],
        &repo_root(),
    );
    assert!(
        ok,
        "lab-ready must exit 0 while printing false\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("LAB_READY=false") || stdout.contains("\"lab_ready\": false"),
        "lab-ready stdout must report false\n{stdout}"
    );
    let lab2 = read_json(&lab_out);
    assert_eq!(
        lab2.get("lab_ready").and_then(|v| v.as_bool()),
        Some(false),
        "lab-ready artifact must be false on harness-only matrix"
    );
}

#[test]
fn p0_5d_score_refuses_forged_fields() {
    ensure_prepare_artifacts();
    let tmp = repo_root().join("target").join("agent_ab_d_eval_forged");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).expect("clean forged tmp");
    }
    std::fs::create_dir_all(&tmp).expect("mkdir forged tmp");

    // Minimal valid stamped-looking trajectory, then forge honesty / scores.
    let task_id = "ts-nest-user-repo";
    let forged_dir = tmp.join(task_id);
    std::fs::create_dir_all(&forged_dir).expect("mkdir task dir");
    let forged_path = forged_dir.join("run-host_session_llm-a-0.json");

    let mut payload = serde_json::json!({
        "schema": "agentgraph.eval_agent_ab.trajectory.v1",
        "schema_alias": "agentgraph.eval_agent_ab.d.v1",
        "protocol": "p0-5d-isolated-lab",
        "policy": "A",
        "arm": "A",
        "runner_id": "host_session_llm",
        "task_id": task_id,
        "seed": 0,
        "run_id": "host_session_llm-a-0",
        "kind": "live_llm_agent",
        "model_note": "mimo-desktop-host-session",
        "independent_session": true,
        "saw_labels_before_commit": false,
        "mcp_or_cli_calls": {"count": 2, "recipe_tools": ["blast-radius"], "grep_count": 0},
        "chose_correct_workspace_root": null,
        "file_budget": 1,
        "read_budget": null,
        "approx_tokens": "one-milllion",  // forged non-numeric
        "file_set": ["src/user.repository.ts"],
        "tool_calls": [],
        "task": {
            "id": task_id,
            "expected_files": ["src/user.repository.ts", "src/FORGED.ts"],
            "noise_files": [],
            "forbidden_files": []
        },
        "score": {
            "stamped": true,
            "expected_file_recall": 1.0,
            "extra_noise_count": 0,
            "forbidden_hit_count": 0,
            "file_set_size": 1
        },
        "honesty": {
            "live_llm_agent": true,
            "private_corpus": false,
            "saw_labels_before_commit": false,
            "no_oversell_live_a_beats_b": true,
            "lab_ready_claim": true,
            "lab_eligible_runner": true
        }
    });
    // Keep an honest key set too.
    payload["honesty"]["isolated_session"] = serde_json::json!(true);
    write_json(&forged_path, &payload);

    // Also write a second forged cell that claims N completeness without data.
    let forged2 = tmp
        .join("rust-trait-handler")
        .join("run-external_live_runner_1-b-4.json");
    write_json(
        &forged2,
        &serde_json::json!({
            "schema": "agentgraph.eval_agent_ab.trajectory.v1",
            "policy": "B",
            "arm": "B",
            "runner_id": "external_live_runner_1",
            "task_id": "rust-trait-handler",
            "seed": 4,
            "kind": "live_llm_agent",
            "model_note": "public-lab-model",
            "independent_session": true,
            "saw_labels_before_commit": false,
            "mcp_or_cli_calls": {"count": 0, "recipe_tools": [], "grep_count": 2},
            "chose_correct_workspace_root": null,
            "file_budget": 1,
            "read_budget": null,
            "approx_tokens": null,
            "file_set": ["src/handlers.rs"],
            "tool_calls": [{"tool": "grep", "args": ["render"], "ok": true, "summary": {}, "note": ""}],
            "task": {
                "id": "rust-trait-handler",
                "expected_files": ["src/handlers.rs"],
                "noise_files": ["src/metrics.rs"],
                "forbidden_files": ["src/metrics.rs"]
            },
            "score": {
                "stamped": true,
                "expected_file_recall": 0.0,  // forged vs fixture
                "extra_noise_count": 99,
                "forbidden_hit_count": 0,
                "file_set_size": 1
            },
            "honesty": {
                "live_llm_agent": true,
                "private_corpus": false,
                "saw_labels_before_commit": false,
                "no_oversell_live_a_beats_b": true,
                "lab_ready_claim": false
            }
        }),
    );

    let out = tmp.join("replay_forged.json");
    let tmp_s = tmp.to_string_lossy().to_string();
    let out_s = out.to_string_lossy().to_string();
    let (ok, stdout, stderr) = run_harness(
        &["score", "--traj-dir", &tmp_s, "--out", &out_s],
        &repo_root(),
    );
    assert!(
        !ok,
        "score must refuse forged fields (non-zero exit)\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("forg")
            || stdout.to_lowercase().contains("forg")
            || stderr.contains("gate"),
        "score must report forgery/gate failure\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    if out.is_file() {
        let replay = read_json(&out);
        let lab = replay.get("lab_ready_eval").cloned().unwrap_or_default();
        assert_eq!(
            lab.get("lab_ready").and_then(|v| v.as_bool()),
            Some(false),
            "forged trees must never report lab_ready=true"
        );
        let viols = replay
            .get("forgery_violations")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !viols.is_empty(),
            "forgery_violations must be non-empty on forged trajectories"
        );
        let blob = serde_json::to_string(&replay).unwrap_or_default();
        assert!(
            blob.contains("forged")
                || blob.contains("lab_ready")
                || blob.contains("approx_tokens")
                || blob.contains("independent_session"),
            "forgery report must name the violation class"
        );
    }

    // lab-ready on forged tree stays false.
    let lab_out = tmp.join("lab_ready_forged.json");
    let lab_out_s = lab_out.to_string_lossy().to_string();
    let (ok, _stdout, _) = run_harness(
        &["lab-ready", "--traj-dir", &tmp_s, "--out", &lab_out_s],
        &repo_root(),
    );
    assert!(ok, "lab-ready still exits 0 while reporting false");
    let lab = read_json(&lab_out);
    assert_eq!(
        lab.get("lab_ready").and_then(|v| v.as_bool()),
        Some(false),
        "lab-ready must be false on forged/incomplete matrix"
    );
}

#[test]
fn p0_5d_committed_lab_ready_gate_is_consistent() {
    ensure_prepare_artifacts();
    let droot = repo_root().join("evals").join("agent-ab-d");
    let out = repo_root()
        .join("target")
        .join("agent_ab_d_lab_ready_gate.json");
    let droot_s = droot.to_string_lossy().to_string();
    let out_s = out.to_string_lossy().to_string();
    let (ok, stdout, stderr) = run_harness(
        &["lab-ready", "--traj-dir", &droot_s, "--out", &out_s],
        &repo_root(),
    );
    assert!(
        ok,
        "lab-ready on committed tree must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let lab = read_json(&out);
    let ready = lab
        .get("lab_ready")
        .and_then(|v| v.as_bool())
        .expect("lab_ready bool");
    let eligible = lab
        .get("lab_eligible_live_runners")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let incomplete = lab
        .get("incomplete_cell_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::MAX);
    if ready {
        // Complete isolated live matrix (P0-5d acceptance) — still not ecosystem sound.
        assert!(
            incomplete == 0,
            "lab_ready=true requires complete matrix: {lab}"
        );
        assert!(
            eligible.len() >= 2,
            "lab_ready=true requires ≥2 lab-eligible live runners: {lab:?}"
        );
        assert!(
            !eligible.iter().any(|v| {
                v.as_str()
                    .map(|s| s.contains("host_session") || s.contains("author"))
                    .unwrap_or(true)
            }),
            "author/host-session runners are not lab-eligible: {eligible:?}"
        );
    } else {
        // Incomplete / harness-only tree must list gaps.
        let gaps = lab
            .get("gaps")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            incomplete > 0 || !gaps.is_empty() || eligible.is_empty(),
            "lab_ready=false needs an explicit gap: {lab}"
        );
    }

    // README must not oversell ecosystem sound regardless of lab_ready.
    let readme = std::fs::read_to_string(droot.join("README.md")).expect("read README");
    let low = readme.to_lowercase();
    assert!(
        !(low.contains("ecosystem sound")
            && !readme.contains("生态 sound")
            && !low.contains("not ecosystem")),
        "evals/agent-ab-d/README must keep non-sound honesty"
    );
}

#[test]
fn p0_5d_readme_states_lab_status_without_sound_oversell() {
    let readme = std::fs::read_to_string(repo_root().join("README.md")).expect("read README");
    let baseline = std::fs::read_to_string(repo_root().join("docs").join("eval-agent-baseline.md"))
        .expect("read baseline doc");
    assert!(
        readme.contains("P0-5d"),
        "README should mention P0-5d isolated lab status"
    );
    // Either harness-only (false) or complete matrix (true) — never silent.
    assert!(
        readme.contains("lab_ready="),
        "README must state lab_ready= explicitly for P0-5d"
    );
    assert!(
        readme.contains("不是") || readme.contains("not ") || readme.contains("non-goal"),
        "README must keep non-sound honesty near P0-5d"
    );
    assert!(
        baseline.contains("lab_ready"),
        "eval-agent-baseline.md must document lab_ready"
    );
}

#[test]
fn p0_5d_brief_path_contract_layout() {
    ensure_prepare_artifacts();
    let briefs = repo_root().join("evals").join("agent-ab-d").join("_briefs");
    assert!(briefs.is_dir());
    // _briefs/<runner>/<arm>/<seed>/<task>/ISSUE.md
    let mut runners: BTreeSet<String> = BTreeSet::new();
    let mut arms: BTreeSet<String> = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(&briefs) {
        for e in rd.filter_map(|e| e.ok()) {
            if e.path().is_dir() {
                runners.insert(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    assert!(
        !runners.is_empty(),
        "prepare must create runner slot directories under _briefs/"
    );
    assert!(
        runners.contains("scripted_isolated_runner"),
        "scripted isolated runner slot expected for offline protocol parity, got {runners:?}"
    );
    // At least one ISSUE.md under a known runner/arm/seed/task path.
    let issues: Vec<PathBuf> = walk_files(&briefs)
        .into_iter()
        .filter(|p| p.file_name().map(|s| s == "ISSUE.md").unwrap_or(false))
        .collect();
    let mut saw_deep = false;
    for issue in &issues {
        // .../_briefs/<runner>/<arm>/<seed>/<task>/ISSUE.md => depth >= 5 components after _briefs
        let rel = issue.strip_prefix(&briefs).unwrap_or(issue);
        let parts: Vec<_> = rel.components().collect();
        if parts.len() >= 5 {
            saw_deep = true;
            if let Some(arm) = parts.get(1) {
                let a = arm.as_os_str().to_string_lossy().to_string();
                arms.insert(a);
            }
        }
    }
    assert!(saw_deep, "ISSUE.md must sit at runner/arm/seed/task depth");
    assert!(
        arms.contains("A") && arms.contains("B"),
        "briefs must cover both arms A and B, got {arms:?}"
    );
}
