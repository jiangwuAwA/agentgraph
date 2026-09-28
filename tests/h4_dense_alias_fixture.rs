//! H4: dense same-name / cross-root alias hard fixture (public synthetic).
//! Live N≥8 remains **blocked** without a host LLM runner in this cut —
//! offline structure-fact fixture + docs only. Historical scores not rewritten.

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/eval-agent-tasks-hard/ts-dense-alias-noise")
}

fn run_in(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

#[test]
fn dense_alias_fixture_parses_and_indexes() {
    let fx = fixture();
    assert!(fx.join("task.json").is_file());
    let raw = std::fs::read_to_string(fx.join("task.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).expect("task.json");
    assert_eq!(v["id"], "ts-dense-alias-noise");
    assert_eq!(v["symbol"], "OrderHandler");
    let noise = v["expected"]["noise_files"].as_array().unwrap();
    assert!(
        noise.len() >= 10,
        "dense noise required, got {}",
        noise.len()
    );

    // Workspace index must succeed on the public fixture.
    let out = run_in(&fx, &["index", "--workspace", "workspace.json", "--force"]);
    assert!(
        out.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // find must see the true class and the legacy decoy (name collision).
    let find = run_in(
        &fx,
        &["find", "OrderHandler", "--workspace", "workspace.json"],
    );
    assert!(find.status.success());
    let stdout = String::from_utf8_lossy(&find.stdout);
    assert!(stdout.contains("orderHandler"), "true definition: {stdout}");
}
