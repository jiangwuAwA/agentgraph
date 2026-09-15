//! CLI end-to-end: exercise the real `agentgraph` binary against a temp fixture.
//! TDD: these tests define the E2E contract; implement/fix until green.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-e2e-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn write_fixture(root: &Path) {
    std::fs::write(
        root.join("src/auth.ts"),
        r#"
export function validateEmail(email: string): boolean {
  return email.includes("@");
}
export function createUser(email: string) {
  if (!validateEmail(email)) throw new Error("bad");
  return { email };
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/api.ts"),
        r#"
import { createUser } from "./auth";
export function loginHandler(email: string) {
  return createUser(email);
}
"#,
    )
    .unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn e2e_index_find_callers_impact_importers() {
    let root = temp_root("query");
    write_fixture(&root);

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index stderr={}", stderr(&idx));
    let stats: serde_json::Value = serde_json::from_str(&stdout(&idx)).unwrap();
    assert!(stats["symbols"].as_u64().unwrap() >= 3);
    assert!(stats["files"].as_u64().unwrap() >= 2);

    let find = run(&root, &["find", "createUser"]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits: serde_json::Value = serde_json::from_str(&stdout(&find)).unwrap();
    assert!(hits
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["name"] == "createUser"));

    let callers = run(&root, &["callers", "validateEmail"]);
    assert!(callers.status.success(), "{}", stderr(&callers));
    let refs: serde_json::Value = serde_json::from_str(&stdout(&callers)).unwrap();
    assert!(
        refs.as_array()
            .unwrap()
            .iter()
            .any(|r| r["enclosing"] == "createUser"),
        "expected createUser as enclosing: {}",
        stdout(&callers)
    );

    let impact = run(&root, &["impact", "validateEmail", "--depth", "3"]);
    assert!(impact.status.success(), "{}", stderr(&impact));
    let imp: serde_json::Value = serde_json::from_str(&stdout(&impact)).unwrap();
    assert!(!imp.as_array().unwrap().is_empty());

    let importers = run(&root, &["importers", "src/auth.ts"]);
    assert!(importers.status.success(), "{}", stderr(&importers));
    let imps: serde_json::Value = serde_json::from_str(&stdout(&importers)).unwrap();
    assert!(
        imps.as_array()
            .unwrap()
            .iter()
            .any(|i| i["path"] == "src/api.ts" && i["name"] == "createUser"),
        "importers={}",
        stdout(&importers)
    );
}

#[test]
fn e2e_empty_index_fails_loudly() {
    let root = temp_root("empty");
    let out = run(&root, &["callers", "nobody"]);
    assert!(!out.status.success(), "empty index must fail");
    let err = stderr(&out).to_lowercase();
    assert!(
        err.contains("index") || err.contains("empty"),
        "stderr={err}"
    );
}

#[test]
fn e2e_incremental_skips_unchanged() {
    let root = temp_root("inc");
    write_fixture(&root);
    let a = run(&root, &["index", "--force"]);
    assert!(a.status.success());
    let b = run(&root, &["index"]);
    assert!(b.status.success(), "{}", stderr(&b));
    assert!(
        stderr(&b).contains("skipped"),
        "expected skip message: {}",
        stderr(&b)
    );
}

#[test]
fn e2e_export_scip_binary_and_json() {
    let root = temp_root("export");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success());

    let bin_out = root.join("index.scip");
    let r = run(
        &root,
        &["export", "scip", "--out", bin_out.to_str().unwrap()],
    );
    assert!(r.status.success(), "{}", stderr(&r));
    assert!(bin_out.exists());
    let bytes = std::fs::read(&bin_out).unwrap();
    assert!(!bytes.is_empty());
    // protobuf binary is not JSON
    assert_ne!(bytes[0], b'{');

    let json_out = root.join("index.scip.json");
    let r = run(
        &root,
        &["export", "scip-json", "--out", json_out.to_str().unwrap()],
    );
    assert!(r.status.success(), "{}", stderr(&r));
    let text = std::fs::read_to_string(&json_out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(v["documents"].as_array().map(|d| !d.is_empty()).unwrap());
}

#[test]
fn e2e_mcp_initialize_and_tools_call() {
    use std::io::Write;
    let root = temp_root("mcp");
    write_fixture(&root);

    let mut child = Command::new(bin())
        .arg("--root")
        .arg(&root)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp");

    {
        let stdin = child.stdin.as_mut().unwrap();
        let msgs = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0"}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"index","arguments":{"force":true}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"createUser"}}}"#,
            "\n",
        );
        stdin.write_all(msgs.as_bytes()).unwrap();
        stdin.flush().unwrap();
        // drop stdin to signal EOF
    }
    // Close stdin by taking it
    drop(child.stdin.take());

    let out = child.wait_with_output().expect("mcp output");
    assert!(out.status.success(), "mcp stderr={}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("\"id\":1") || text.contains("\"id\": 1"));
    assert!(text.contains("agentgraph"));
    assert!(text.contains("find_symbol") || text.contains("createUser"));
}

/// If official `scip` CLI is on PATH, require lint exit 0 on our binary export.
#[test]
fn e2e_scip_cli_lint_when_available() {
    let scip = which_scip();
    let Some(scip) = scip else {
        eprintln!("skip: scip CLI not on PATH");
        return;
    };
    let root = temp_root("scipcli");
    write_fixture(&root);
    assert!(run(&root, &["index", "--force"]).status.success());
    let out_path = root.join("index.scip");
    assert!(run(
        &root,
        &["export", "scip", "--out", out_path.to_str().unwrap()]
    )
    .status
    .success());

    let lint = Command::new(&scip)
        .arg("lint")
        .arg(&out_path)
        .stdin(Stdio::null())
        .output()
        .expect("run scip lint");
    assert!(
        lint.status.success(),
        "scip lint failed: {}",
        String::from_utf8_lossy(&lint.stderr)
    );
}

fn which_scip() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["scip.exe", "scip"] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
