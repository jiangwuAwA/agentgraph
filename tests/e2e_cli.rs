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
    // L0.3 evidence: callers rows include at=path:line
    assert!(
        refs.as_array()
            .unwrap()
            .iter()
            .any(|r| r["at"].as_str().map(|s| s.contains(':')).unwrap_or(false)),
        "expected at=path:line evidence: {}",
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

    // R20 residual: MCP tools/list must not drift from CLI flags.
    // enrich default limit is 50 (same as `agentgraph enrich --limit`).
    assert!(
        text.contains("\"default\": 50") || text.contains("\"default\":50"),
        "enrich default must be 50 to match CLI; tools/list={text}"
    );
    // callers/impact expose the L2 sound + L1 recall flags present on the CLI.
    assert!(
        text.contains("\"sound\""),
        "callers/impact schema must expose sound; tools/list={text}"
    );
    assert!(
        text.contains("\"recall\""),
        "callers/impact schema must expose recall; tools/list={text}"
    );
    // R26 residual: P2 macro sidecar surface must stay advertised (docs/README).
    assert!(
        text.contains("\"with_macro\"") || text.contains("\"with_macro\":"),
        "callers/impact schema must expose with_macro; tools/list={text}"
    );
    assert!(
        text.contains("macro_status"),
        "tools/list must include macro_status; tools/list={text}"
    );
    // M1: new macro surface advertised.
    assert!(
        text.contains("macro_rebuild"),
        "tools/list must include macro_rebuild; tools/list={text}"
    );
    assert!(
        text.contains("no_macro_dedup"),
        "callers/impact schema must expose no_macro_dedup; tools/list={text}"
    );
}

/// Track M1 CLI flag matrix: --no-macro-dedup exists; --sound && --with-macro
/// still mutually exclusive; macro rebuild help exists; exact-only+with-macro OK.
#[test]
fn e2e_cli_macro_m1_flag_matrix() {
    // Help surfaces.
    let c = Command::new(bin())
        .args(["callers", "--help"])
        .stdin(Stdio::null())
        .output()
        .expect("callers help");
    let ctext = stdout(&c);
    assert!(
        ctext.contains("--no-macro-dedup") || ctext.contains("no-macro-dedup"),
        "callers help must document --no-macro-dedup: {ctext}"
    );
    assert!(
        ctext.contains("with-macro") || ctext.contains("with_macro"),
        "callers help must document --with-macro"
    );
    assert!(
        ctext.to_lowercase().contains("per store") || ctext.contains("2N") || ctext.contains("~2N"),
        "callers help must document per-store limit: {ctext}"
    );

    let mh = Command::new(bin())
        .args(["macro", "--help"])
        .stdin(Stdio::null())
        .output()
        .expect("macro help");
    let mtext = stdout(&mh);
    assert!(mtext.contains("status"), "macro help: {mtext}");
    assert!(
        mtext.contains("rebuild"),
        "macro help must include rebuild: {mtext}"
    );

    // --sound && --with-macro still fail closed.
    let root = temp_root("m1-flags");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let combo = run(&root, &["callers", "createUser", "--sound", "--with-macro"]);
    assert!(
        !combo.status.success(),
        "sound+with_macro must stay mutually exclusive"
    );
    let err = stderr(&combo).to_lowercase();
    assert!(
        err.contains("mutually") || err.contains("with-macro") || err.contains("with_macro"),
        "err={err}"
    );

    // --exact-only --with-macro is accepted (sidecar ignored, not an error).
    let exact_macro = run(
        &root,
        &["callers", "createUser", "--with-macro", "--exact-only"],
    );
    assert!(
        exact_macro.status.success(),
        "exact-only + with-macro must succeed: {}",
        stderr(&exact_macro)
    );

    // --no-macro-dedup is accepted without a sidecar.
    let nodedup = run(
        &root,
        &["callers", "createUser", "--with-macro", "--no-macro-dedup"],
    );
    assert!(
        nodedup.status.success(),
        "no-macro-dedup without sidecar: {}",
        stderr(&nodedup)
    );

    // Default without sidecar stays a plain JSON array (M1 regression).
    let plain = run(&root, &["callers", "createUser"]);
    assert!(plain.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&plain)).unwrap();
    assert!(v.is_array(), "default callers JSON must stay an array: {v}");
}

/// MCP query tools on an empty index must return isError=true with 'index' in the message.
#[test]
fn e2e_mcp_empty_index_returns_error() {
    use std::io::Write;
    let root = temp_root("mcp-empty");
    // No write_fixture — root has no indexed files.

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
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"callers","arguments":{"name":"nobody"}}}"#,
            "\n",
        );
        stdin.write_all(msgs.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());

    let out = child.wait_with_output().expect("mcp output");
    let text = stdout(&out);
    // Parse the tools/call response (id=2) and assert isError=true with 'index' in message.
    let mut found_error = false;
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 2 {
                assert_eq!(
                    v["result"]["isError"], true,
                    "callers on empty index must return isError=true, got: {line}"
                );
                let msg = v["result"]["content"][0]["text"].as_str().unwrap_or("");
                assert!(
                    msg.to_lowercase().contains("index"),
                    "error message must mention 'index', got: {msg}"
                );
                found_error = true;
            }
        }
    }
    assert!(found_error, "expected id=2 response in MCP output:\n{text}");
}

/// C1: MCP root jail must reject `..` escapes and must not create `.agentgraph` outside.
#[test]
fn e2e_mcp_root_jail_rejects_dotdot_escape() {
    use std::io::Write;
    let base = temp_root("mcp-jail");
    write_fixture(&base);

    let parent = base.parent().unwrap().to_path_buf();
    let outside = parent.join("agentgraph-e2e-mcp-jail-outside");
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(outside.join("src")).unwrap();
    std::fs::write(
        outside.join("src/x.ts"),
        "export function x() { return 1; }\n",
    )
    .unwrap();

    let escape = base.join("..").join("agentgraph-e2e-mcp-jail-outside");
    let escape_json = serde_json::to_string(&escape.to_string_lossy()).unwrap();

    let mut child = Command::new(bin())
        .arg("--root")
        .arg(&base)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp");

    {
        let stdin = child.stdin.as_mut().unwrap();
        let msgs = format!(
            concat!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2024-11-05","capabilities":{{}},"clientInfo":{{"name":"e2e","version":"0"}}}}}}"#,
                "\n",
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"index","arguments":{{"root":{escape_json},"force":true}}}}}}"#,
                "\n",
            ),
            escape_json = escape_json
        );
        stdin.write_all(msgs.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());

    let out = child.wait_with_output().expect("mcp output");
    let text = stdout(&out);
    let mut found_reject = false;
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 2 {
                let is_tool_err = v["result"]["isError"] == true;
                let is_rpc_err = v.get("error").is_some();
                assert!(
                    is_tool_err || is_rpc_err,
                    "jail escape must be rejected (error or isError), got: {line}"
                );
                let msg = format!(
                    "{}{}",
                    v["error"]["message"].as_str().unwrap_or(""),
                    v["result"]["content"][0]["text"].as_str().unwrap_or("")
                );
                assert!(
                    msg.to_lowercase().contains("outside") || msg.to_lowercase().contains("root"),
                    "rejection message must mention outside/root: {msg}"
                );
                found_reject = true;
            }
        }
    }
    assert!(found_reject, "expected id=2 rejection:\n{text}");
    assert!(
        !outside.join(".agentgraph").exists(),
        "must not create .agentgraph outside the jail"
    );
    let _ = std::fs::remove_dir_all(&outside);
}

/// M6: MCP callers tool accepts sound=true and returns CLI-shaped promise JSON.
#[test]
fn e2e_mcp_callers_sound_flag() {
    use std::io::Write;
    let root = temp_root("mcp-sound");
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
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"force":true}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"callers","arguments":{"name":"validateEmail","sound":true}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"callers","arguments":{"name":"validateEmail","sound":true,"exact_only":true}}}"#,
            "\n",
        );
        stdin.write_all(msgs.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());

    let out = child.wait_with_output().expect("mcp output");
    let text = stdout(&out);
    let mut saw_sound = false;
    let mut saw_mutex = false;
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 3 {
                assert_eq!(
                    v["result"]["isError"], false,
                    "callers sound must succeed: {line}"
                );
                let payload: serde_json::Value =
                    serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap())
                        .expect("callers sound payload json");
                assert_eq!(payload["mode"], "sound");
                assert_eq!(payload["subset_ok"], true);
                let promise = payload["promise"].as_str().unwrap_or("");
                // Pure TS fixture → AST-modeled tier (not lexical v1).
                assert_eq!(
                    promise,
                    agentgraph::mcp::server::SOUND_PROMISE_OK_AST,
                    "MCP callers promise must equal shared SOUND_PROMISE_OK_AST for pure TS corpus: {payload}"
                );
                assert_eq!(
                    payload["promise_tier"], "ast_modeled",
                    "MCP must report language-aware promise_tier: {payload}"
                );
                assert!(payload["callers"].is_array());
                saw_sound = true;
            }
            if v["id"] == 4 {
                assert_eq!(
                    v["result"]["isError"], true,
                    "sound+exact_only must be rejected: {line}"
                );
                saw_mutex = true;
            }
        }
    }
    assert!(saw_sound, "expected id=3 sound result:\n{text}");
    assert!(saw_mutex, "expected id=4 mutex error:\n{text}");
}

/// M2: `subset` JSON fields stay stable (CLI contract for Agents / docs).
#[test]
fn e2e_subset_json_fields_stable() {
    let root = temp_root("subset-fields");
    write_fixture(&root);
    // Clean TS + Python corpus → ast_modeled.
    std::fs::write(
        root.join("src/auth.py"),
        r#"
def authenticate(email, password):
    return {"email": email, "password": password}

def login_handler(email, password):
    return authenticate(email, password)
"#,
    )
    .unwrap();

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index stderr={}", stderr(&idx));

    let sub = run(&root, &["subset"]);
    assert!(sub.status.success(), "subset stderr={}", stderr(&sub));
    let v: serde_json::Value = serde_json::from_str(&stdout(&sub)).expect("subset json");
    for key in [
        "in_subset",
        "violation_count",
        "violations",
        "promise_tier",
        "promise",
        "promise_languages",
        "note",
    ] {
        assert!(
            v.get(key).is_some(),
            "subset JSON missing `{key}`: {}",
            stdout(&sub)
        );
    }
    assert_eq!(v["in_subset"], true, "{}", stdout(&sub));
    assert_eq!(v["promise_tier"], "ast_modeled", "{}", stdout(&sub));
    assert!(v["violations"].is_array());
    assert!(v["promise_languages"].is_array());
    let langs: Vec<&str> = v["promise_languages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x.as_str())
        .collect();
    assert!(langs.contains(&"typescript"), "langs={langs:?}");
    assert!(langs.contains(&"python"), "langs={langs:?}");
    assert!(!langs.contains(&"lexical_v1"));

    // Impact --sound payload uses the same promise fields.
    let imp = run(&root, &["impact", "validateEmail", "--sound"]);
    assert!(imp.status.success(), "{}", stderr(&imp));
    let iv: serde_json::Value = serde_json::from_str(&stdout(&imp)).expect("impact json");
    assert_eq!(iv["mode"], "sound");
    assert_eq!(iv["subset_ok"], true);
    assert_eq!(iv["promise_tier"], "ast_modeled");
    assert!(iv.get("promise_languages").is_some());
    assert!(iv.get("promise").is_some());
}

/// M2 over-flag: clean Nest-like TS with type-only Function stays in S end-to-end.
#[test]
fn e2e_type_only_function_fixture_stays_in_s() {
    let root = temp_root("type-only-fn");
    std::fs::write(
        root.join("src/app.ts"),
        r#"
type Ctor = typeof Function;
export interface ProviderLike {
  provide: string;
  useClass: Function;
}
export class AppService {
  getHello(): string {
    return "Hello World!";
  }
}
export function make(h: Function): string {
  return new AppService().getHello();
}
"#,
    )
    .unwrap();
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let sub = run(&root, &["subset"]);
    assert!(
        sub.status.success(),
        "type-only Function fixture must stay in S: {}",
        stderr(&sub)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&sub)).unwrap();
    assert_eq!(v["in_subset"], true, "{}", stdout(&sub));
    assert_eq!(v["promise_tier"], "ast_modeled");
    assert_eq!(v["violation_count"], 0);
}

/// Same-line multi-ref fixture: scip lint must exit 0 (distinct ranges for duplicate refs).
#[test]
fn e2e_scip_lint_same_line_multi_ref() {
    let scip = which_scip();
    let Some(scip) = scip else {
        eprintln!("skip: scip CLI not on PATH");
        return;
    };
    let root = temp_root("sameline");
    std::fs::write(
        root.join("src/mod.js"),
        "function a(){}; export function b(){ return a(1)+a(2); }\n",
    )
    .unwrap();
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
        "scip lint failed on same-line fixture: {}",
        String::from_utf8_lossy(&lint.stderr)
    );
}

/// Protobuf binary export must start with a valid protobuf field tag (not JSON `{`).
#[test]
fn e2e_export_scip_protobuf_format() {
    let root = temp_root("protofmt");
    write_fixture(&root);
    assert!(run(&root, &["index", "--force"]).status.success());
    let out_path = root.join("index.scip");
    assert!(run(
        &root,
        &["export", "scip", "--out", out_path.to_str().unwrap()]
    )
    .status
    .success());

    let bytes = std::fs::read(&out_path).unwrap();
    assert!(!bytes.is_empty());
    // Protobuf wire format: first byte is a field tag. For scip.Index, field 1
    // (metadata) is tag 0x0a (field 1, wire type 2 = length-delimited).
    assert_ne!(bytes[0], b'{', "scip binary must not be JSON");
    // Parse with the official scip crate to verify it's valid protobuf.
    use protobuf::Message;
    let index =
        scip::types::Index::parse_from_bytes(&bytes).expect("must parse as scip.Index protobuf");
    assert!(index.metadata.is_some(), "parsed Index must have metadata");
    assert!(!index.documents.is_empty());
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
