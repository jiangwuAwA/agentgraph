//! TDD: MCP `graph` tool returns self-contained HTML + honesty payload.
//!
//! Contract:
//! - tools/list advertises `graph`
//! - graph returns `html` containing the symbol + honesty footer
//! - sound=true on a dirty/S-violated fixture is **not** labeled OK sound;
//!   recommendation present; promise_tier=disabled
//! - sound + with_macro rejected (fail-closed)
//! - payload always carries window / subset_ok / recommendation / note + counts
//! - HTML escapes untrusted names (reuse viz renderer)
//! - optional `out` is jailed under the workspace root

use agentgraph::viz::{
    build_graph_html_payload, escape_html, render_graph_html, GraphEdge, GraphFlags,
    GraphHtmlPayloadInput, GraphNode, GraphVizData,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-mcp-graph-{name}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
    dir
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

fn write_clean_js(root: &Path) {
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

/// Dirty fixture: eval() violates S (not AST-modeled sound).
fn write_eval_js(root: &Path) {
    std::fs::write(
        root.join("src/evil.js"),
        r#"
function runCode(code) {
  return eval(code);
}
function helper() { return 1; }
module.exports = { runCode, helper };
"#,
    )
    .unwrap();
}

/// Spawn MCP, send NDJSON requests, return stdout.
fn mcp_roundtrip(root: &Path, msgs: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(bin())
        .arg("--root")
        .arg(root)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp");
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(msgs.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());
    child.wait_with_output().expect("mcp out")
}

fn mcp_init_list() -> String {
    concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
    )
    .to_string()
}

/// Parse MCP stdout for a given id; return (isError, payload_text).
fn mcp_result(text: &str, id: i64) -> (bool, String) {
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == id {
                let is_err = v["result"]["isError"].as_bool().unwrap_or(false);
                let payload = v["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                return (is_err, payload);
            }
        }
    }
    panic!("no MCP result for id={id} in:\n{text}");
}

// ---------------------------------------------------------------------------
// tools/list
// ---------------------------------------------------------------------------

#[test]
fn e2e_mcp_tools_list_contains_graph() {
    let root = temp_root("tools-list");
    write_clean_js(&root);
    let _ = run(&root, &["index", "--force"]);
    let text = stdout(&mcp_roundtrip(&root, &mcp_init_list()));
    assert!(
        text.contains("\"graph\"")
            || text.contains("\"name\":\"graph\"")
            || text.contains("\"name\": \"graph\""),
        "tools/list must include graph tool: {text}"
    );
    // Schema surfaces for Agents.
    assert!(
        text.contains("with_macro"),
        "graph schema needs with_macro: {text}"
    );
    assert!(
        text.contains("include_recommendation"),
        "graph schema needs include_recommendation: {text}"
    );
    // Honesty — no oversell in tool docs.
    assert!(!text.contains("零漏报"));
    assert!(!text.to_lowercase().contains("zero-miss coverage"));
}

// ---------------------------------------------------------------------------
// MCP graph happy path
// ---------------------------------------------------------------------------

#[test]
fn e2e_mcp_graph_returns_html_with_symbol_and_honesty() {
    let root = temp_root("happy");
    write_clean_js(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let msgs = format!(
        "{}{}\n",
        mcp_init_list(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graph","arguments":{"symbol":"createUser","depth":3,"direction":"impact"}}}"#
    );
    let out = mcp_roundtrip(&root, &msgs);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    let (is_err, payload) = mcp_result(&text, 3);
    assert!(!is_err, "graph must succeed on clean fixture: {payload}");
    let v: serde_json::Value = serde_json::from_str(&payload).expect("graph payload json");
    assert_eq!(v["tool"], "graph");
    assert_eq!(v["symbol"], "createUser");
    let html = v["html"].as_str().expect("html string required");
    assert!(html.contains("createUser"), "html must contain symbol");
    // Honesty footer / line always present.
    assert!(
        html.contains("not a complete runtime graph")
            || html.contains("非完整运行时图")
            || html.contains("仅展示已索引边"),
        "html must carry honesty footer"
    );
    // Machine fields for Agents.
    assert!(v["html_bytes"].as_u64().unwrap_or(0) as usize == html.len());
    assert!(v["sha256"].as_str().unwrap_or("").len() >= 32);
    assert!(v["node_count"].as_u64().is_some());
    assert!(v["edge_count"].as_u64().is_some());
    assert!(v["recommendation"].as_str().unwrap_or("").len() > 8);
    assert!(v["note"]
        .as_str()
        .unwrap()
        .contains("not a complete runtime graph"));
    assert!(v["promise_tier"].as_str().is_some());
    // String-only default: no path unless `out` requested.
    assert!(v["path"].is_null(), "default must be string-only: {v}");
    // Clean TS fixture → subset_ok true (window may be default unless auto_window).
    assert_eq!(v["subset_ok"], true);
}

#[test]
fn e2e_mcp_graph_auto_window_sound_on_clean() {
    let root = temp_root("auto-window");
    write_clean_js(&root);
    let _ = run(&root, &["index", "--force"]);
    let msgs = format!(
        "{}{}\n",
        mcp_init_list(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graph","arguments":{"symbol":"createUser","auto_window":true}}}"#
    );
    let text = stdout(&mcp_roundtrip(&root, &msgs));
    let (is_err, payload) = mcp_result(&text, 3);
    assert!(!is_err, "{payload}");
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["window"], "sound", "clean auto_window → sound: {v}");
    assert_eq!(v["subset_ok"], true);
    let html = v["html"].as_str().unwrap();
    assert!(html.contains("subset_ok=true") || html.contains("sound-eligible"));
}

// ---------------------------------------------------------------------------
// sound honesty on dirty fixture
// ---------------------------------------------------------------------------

#[test]
fn e2e_mcp_graph_sound_dirty_not_labeled_ok() {
    let root = temp_root("sound-dirty");
    write_eval_js(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let msgs = format!(
        "{}{}\n",
        mcp_init_list(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graph","arguments":{"symbol":"helper","sound":true,"include_recommendation":true}}}"#
    );
    let text = stdout(&mcp_roundtrip(&root, &msgs));
    let (is_err, payload) = mcp_result(&text, 3);
    // Payload is still a successful tool result (honest HTML deliverable),
    // but must NOT claim sound OK.
    assert!(
        !is_err,
        "dirty sound still returns honest payload: {payload}"
    );
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["subset_ok"], false, "dirty fixture: {v}");
    assert_eq!(v["promise_tier"], "disabled");
    let window = v["window"].as_str().unwrap_or("");
    assert_ne!(
        window, "sound",
        "must not label window=sound when subset_ok=false: {v}"
    );
    let rec = v["recommendation"].as_str().unwrap_or("");
    assert!(
        !rec.is_empty(),
        "recommendation required on dirty sound request: {v}"
    );
    let rec_l = rec.to_lowercase();
    assert!(
        rec_l.contains("sound")
            || rec_l.contains("disabled")
            || rec_l.contains("violat")
            || rec_l.contains("default")
            || rec_l.contains("subset"),
        "recommendation must explain honesty: {rec}"
    );
    // Must not claim sound OK in payload fields.
    assert_ne!(v["promise_tier"], "ast_modeled");
    let html = v["html"].as_str().expect("html still returned");
    assert!(
        html.contains("NOT a sound graph")
            || html.contains("S VIOLATED")
            || html.contains("DISABLED")
            || html.contains("不是 sound 图")
            || html.contains("禁用"),
        "html must be honest disabled page: {}",
        &html[..html.len().min(400)]
    );
    // Never oversell.
    assert!(!html.contains("subset_ok=true · promise_tier=ast_modeled"));
}

#[test]
fn e2e_mcp_graph_auto_window_dirty_stays_default() {
    let root = temp_root("auto-dirty");
    write_eval_js(&root);
    let _ = run(&root, &["index", "--force"]);
    let msgs = format!(
        "{}{}\n",
        mcp_init_list(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graph","arguments":{"symbol":"helper","auto_window":true}}}"#
    );
    let text = stdout(&mcp_roundtrip(&root, &msgs));
    let (is_err, payload) = mcp_result(&text, 3);
    assert!(!is_err, "{payload}");
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["window"], "default", "auto_window never blind sound: {v}");
    assert_eq!(v["subset_ok"], false);
    assert!(!v["recommendation"].as_str().unwrap_or("").is_empty());
}

// ---------------------------------------------------------------------------
// mutex + jail
// ---------------------------------------------------------------------------

#[test]
fn e2e_mcp_graph_sound_with_macro_rejected() {
    let root = temp_root("mutex");
    write_clean_js(&root);
    let _ = run(&root, &["index", "--force"]);
    let msgs = format!(
        "{}{}\n",
        mcp_init_list(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graph","arguments":{"symbol":"createUser","sound":true,"with_macro":true}}}"#
    );
    let text = stdout(&mcp_roundtrip(&root, &msgs));
    let (is_err, payload) = mcp_result(&text, 3);
    assert!(is_err, "sound+with_macro must fail-closed: {payload}");
    let lower = payload.to_lowercase();
    assert!(
        lower.contains("mutually exclusive")
            || lower.contains("with_macro")
            || lower.contains("sound"),
        "mutex error message: {payload}"
    );
}

#[test]
fn e2e_mcp_graph_out_jail_rejects_escape() {
    let root = temp_root("jail");
    write_clean_js(&root);
    let _ = run(&root, &["index", "--force"]);
    let msgs = format!(
        "{}{}\n",
        mcp_init_list(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graph","arguments":{"symbol":"createUser","out":"../escape-graph.html"}}}"#
    );
    let text = stdout(&mcp_roundtrip(&root, &msgs));
    let (is_err, payload) = mcp_result(&text, 3);
    assert!(is_err, "out jail must reject ../escape: {payload}");
    assert!(
        payload.to_lowercase().contains("jail")
            || payload.to_lowercase().contains("outside")
            || payload.to_lowercase().contains("root"),
        "jail error: {payload}"
    );
}

#[test]
fn e2e_mcp_graph_out_writes_under_root() {
    let root = temp_root("out-ok");
    write_clean_js(&root);
    let _ = run(&root, &["index", "--force"]);
    let msgs = format!(
        "{}{}\n",
        mcp_init_list(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graph","arguments":{"symbol":"createUser","out":".agentgraph/graph.html"}}}"#
    );
    let text = stdout(&mcp_roundtrip(&root, &msgs));
    let (is_err, payload) = mcp_result(&text, 3);
    assert!(!is_err, "{payload}");
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let path = v["path"].as_str().expect("path when out provided");
    assert!(
        path.contains("graph.html"),
        "path must point at written file: {path}"
    );
    assert!(Path::new(path).is_file(), "file must exist: {path}");
    let written = std::fs::read_to_string(path).unwrap();
    assert!(written.contains("createUser"));
}

// ---------------------------------------------------------------------------
// Unit: payload builder + HTML escape (reuse viz)
// ---------------------------------------------------------------------------

fn sample_data() -> GraphVizData {
    GraphVizData {
        query: "createUser".into(),
        direction: agentgraph::viz::GraphDirection::Impact,
        depth: 3,
        flags: GraphFlags {
            direction: agentgraph::viz::GraphDirection::Impact,
            ..GraphFlags::default()
        },
        nodes: vec![
            GraphNode {
                id: "q".into(),
                name: "createUser".into(),
                depth: 0,
                confidence: "exact",
                path: Some("src/auth.ts".into()),
                line: Some(5),
                origin: None,
                is_query: true,
                role: "call",
                root_id: String::new(),
            },
            GraphNode {
                id: "n1".into(),
                name: "loginHandler".into(),
                depth: 1,
                confidence: "exact",
                path: Some("src/api.ts".into()),
                line: Some(3),
                origin: None,
                is_query: false,
                role: "call",
                root_id: String::new(),
            },
        ],
        edges: vec![GraphEdge {
            from: "q".into(),
            to: "n1".into(),
            confidence: "exact",
            kind: "call",
            role: "call",
        }],
        truncated: false,
        max_nodes: 300,
        subset_ok: Some(true),
        promise_tier: Some("ast_modeled".into()),
        empty_note: None,
        root_filter: None,
    }
}

#[test]
fn unit_graph_html_payload_has_honesty_fields() {
    let data = sample_data();
    let html = render_graph_html(&data);
    let payload = build_graph_html_payload(GraphHtmlPayloadInput {
        symbol: "createUser",
        data: &data,
        html: &html,
        window: "sound",
        promise_tier: "ast_modeled",
        subset_ok: Some(true),
        recommendation: Some("subset_ok: sound window over S-qualified edges"),
        path: None,
        note: agentgraph::viz::GRAPH_HTML_NOTE,
        include_macro: false,
        include_macro_reason: None,
    });
    assert_eq!(payload["tool"], "graph");
    assert_eq!(payload["symbol"], "createUser");
    assert_eq!(payload["window"], "sound");
    assert_eq!(payload["subset_ok"], true);
    assert_eq!(payload["promise_tier"], "ast_modeled");
    assert_eq!(payload["node_count"], 2);
    assert_eq!(payload["edge_count"], 1);
    assert!(payload["html"].as_str().unwrap().contains("createUser"));
    assert_eq!(payload["html_bytes"].as_u64().unwrap() as usize, html.len());
    assert!(payload["sha256"].as_str().unwrap().len() >= 32);
    assert!(payload["path"].is_null());
    assert_eq!(payload["note"], agentgraph::viz::GRAPH_HTML_NOTE);
    assert!(payload["recommendation"]
        .as_str()
        .unwrap()
        .contains("subset_ok"));
}

#[test]
fn unit_graph_html_escapes_untrusted_symbol() {
    let evil = "evil<script>alert(1)</script>";
    let mut data = sample_data();
    data.query = evil.into();
    data.nodes[0].name = evil.into();
    data.nodes[1].name = "\"onmouseover=\"alert(1)".into();
    let html = render_graph_html(&data);
    assert!(
        !html.contains("<script>alert(1)</script>"),
        "raw script tag must be escaped"
    );
    assert!(html.contains(&escape_html(evil)));
    assert!(html.contains("&lt;script&gt;"));
    let payload = build_graph_html_payload(GraphHtmlPayloadInput {
        symbol: evil,
        data: &data,
        html: &html,
        window: "default",
        promise_tier: "disabled",
        subset_ok: Some(false),
        recommendation: Some("sound disabled"),
        path: None,
        note: agentgraph::viz::GRAPH_HTML_NOTE,
        include_macro: false,
        include_macro_reason: None,
    });
    // JSON field itself holds the raw symbol (JSON-safe); HTML is escaped.
    assert_eq!(payload["symbol"], evil);
    assert!(payload["html"].as_str().unwrap().contains("&lt;script&gt;"));
}

#[test]
fn unit_include_recommendation_omitted_when_false() {
    let data = sample_data();
    let html = render_graph_html(&data);
    let payload = build_graph_html_payload(GraphHtmlPayloadInput {
        symbol: "createUser",
        data: &data,
        html: &html,
        window: "default",
        promise_tier: "ast_modeled",
        subset_ok: Some(true),
        recommendation: None,
        path: None,
        note: agentgraph::viz::GRAPH_HTML_NOTE,
        include_macro: false,
        include_macro_reason: None,
    });
    assert!(payload["recommendation"].is_null());
}
