//! P0-1 public Agent code-change task evals — fixture schema + harness gate.
//!
//! - Public fixtures under `fixtures/eval-agent-tasks/` must exist (≥8) with
//!   parseable `task.json` (issue, symbol, expected structure facts).
//! - Harness `scripts/eval_agent_tasks.py` must exit 0 when python + binary
//!   are available (this test always builds/uses `CARGO_BIN_EXE_agentgraph`).
//!
//! Non-claims (docs/eval-agent-tasks.md): public synthetic fixtures only;
//! name-grep baseline is not an LLM baseline; scores are structure facts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures_dir() -> PathBuf {
    repo_root().join("fixtures").join("eval-agent-tasks")
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
    panic!("no python interpreter for agent_task_eval harness: {last_err}");
}

fn load_task_json(dir: &Path) -> serde_json::Value {
    let p = dir.join("task.json");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!("cannot read {}: {e}", p.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("invalid task.json at {}: {e}", p.display());
    })
}

#[test]
fn public_agent_task_fixtures_exist_and_parse() {
    let dir = fixtures_dir();
    assert!(
        dir.is_dir(),
        "missing fixtures/eval-agent-tasks (P0-1 public tasks)"
    );
    let mut tasks: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read fixtures dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("task.json").is_file())
        .collect();
    tasks.sort();
    assert!(
        tasks.len() >= 8,
        "P0-1 requires >=8 public tasks, found {}",
        tasks.len()
    );

    for tdir in &tasks {
        let meta = load_task_json(tdir);
        let id = meta
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        assert!(!id.is_empty(), "{}: missing id", tdir.display());
        let symbol = meta.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
        assert!(!symbol.is_empty(), "{id}: missing symbol");
        let issue = meta.get("issue").and_then(|v| v.as_str()).unwrap_or("");
        assert!(issue.len() > 20, "{id}: issue text too short");
        let expected = meta.get("expected").expect("expected object");
        let files = expected
            .get("files_that_matter")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("{id}: expected.files_that_matter"));
        assert!(!files.is_empty(), "{id}: files_that_matter empty");
        // recommended tools/commands present for agent path
        assert!(
            meta.get("recommended_tools")
                .and_then(|v| v.as_array())
                .is_some(),
            "{id}: recommended_tools required"
        );
        assert!(
            meta.get("recommended_commands")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            "{id}: recommended_commands required"
        );
        // mini-repo sources exist next to task.json
        let has_src = tdir
            .read_dir()
            .map(|rd| {
                rd.filter_map(|e| e.ok()).any(|e| {
                    let p = e.path();
                    p.is_file() && p.file_name().map(|n| n != "task.json").unwrap_or(false)
                        || p.is_dir()
                })
            })
            .unwrap_or(false);
        assert!(has_src, "{id}: mini-repo sources missing");
    }
}

#[test]
fn harness_eval_agent_tasks_exits_zero() {
    let script = repo_root().join("scripts").join("eval_agent_tasks.py");
    assert!(
        script.is_file(),
        "missing harness script {}",
        script.display()
    );
    let out_json = repo_root().join("target").join("agent_task_eval.json");
    if let Some(parent) = out_json.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let output = python_command()
        .arg(&script)
        .arg("--repo")
        .arg(repo_root())
        .arg("--bin")
        .arg(bin())
        .arg("--out")
        .arg(&out_json)
        .current_dir(repo_root())
        .output()
        .expect("spawn eval_agent_tasks harness");

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
    let text = std::fs::read_to_string(&out_json).expect("read eval json");
    let payload: serde_json::Value = serde_json::from_str(&text).expect("eval json parse");
    assert_eq!(
        payload.get("schema").and_then(|v| v.as_str()),
        Some("agentgraph.eval_agent_tasks.v1")
    );
    let tasks = payload
        .get("tasks")
        .and_then(|v| v.as_array())
        .expect("tasks[]");
    assert!(tasks.len() >= 8, "JSON tasks >= 8, got {}", tasks.len());
    for t in tasks {
        let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let pass = t.get("pass").and_then(|v| v.as_bool()).unwrap_or(false);
        assert!(pass, "task {id} must pass (honesty/structure gates)");
        let ag = t.get("agentgraph").expect("agentgraph block");
        assert!(
            ag.get("recommendation_present").and_then(|v| v.as_bool()) == Some(true),
            "{id}: recommendation must be present"
        );
        let note_ok = ag
            .get("note_honest")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(note_ok, "{id}: note_honest flag missing");
        // Baseline is name-grep — never invent LLM numbers in the payload.
        let gg = t
            .get("name_grep_baseline")
            .expect("name_grep_baseline block");
        assert!(
            gg.get("method")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("name-grep"),
            "{id}: baseline method must stay name-grep"
        );
    }
    // Unsafe / sound-disabled task must not claim sound window.
    let unsafe_task = tasks
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some("rust-unsafe-scoped"))
        .expect("rust-unsafe-scoped task present");
    let window = unsafe_task
        .get("agentgraph")
        .and_then(|a| a.get("window"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_ne!(
        window, "sound",
        "dirty unsafe workspace must not claim window=sound"
    );
}
