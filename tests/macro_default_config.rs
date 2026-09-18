//! P2-1 — repo/project-level macro default config (TDD).
//!
//! Contract:
//! - Global default remains OFF (no silent `--with-macro`)
//! - Primary file: `<root>/.agentgraph/config.toml` field `macro_default`
//! - Fallback file: `<root>/agentgraph.toml`
//! - Env `AGENTGRAPH_MACRO_DEFAULT` overrides file
//! - Explicit CLI `--include-macro` / `--no-include-macro` wins over config
//! - `if_fresh` + fresh sidecar → blast_radius `include_macro=true` +
//!   `include_macro_reason=repo_config_if_fresh`
//! - `if_fresh` + stale sidecar → refused
//! - Sound window still refuses macro (not sound-certified)
//! - `macro status` shows effective config source

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_pair(tag: &str) -> (PathBuf, PathBuf) {
    let base = common::temp_root(&format!("agentgraph-macrodef-{tag}"));
    let root = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    (root, expanded)
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .env_remove("AGENTGRAPH_MACRO_DEFAULT")
        .output()
        .expect("run agentgraph")
}

fn run_env(root: &Path, args: &[&str], env_key: &str, env_val: &str) -> Output {
    Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .env_remove("AGENTGRAPH_MACRO_DEFAULT")
        .env(env_key, env_val)
        .output()
        .expect("run agentgraph env")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn parse_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).unwrap_or_else(|e| {
        panic!(
            "invalid JSON ({e}): stdout={} stderr={}",
            stdout(out),
            stderr(out)
        )
    })
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

fn write_dirty_rust(root: &Path) {
    // S-violation via unsafe → window=default (not sound).
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub unsafe fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
}

fn write_expanded_dirty(expanded: &Path) {
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub unsafe fn process() -> i32 { helper() + 1 }
pub fn fmt() -> i32 { helper() }
"#,
    )
    .unwrap();
}

fn write_config(root: &Path, body: &str) {
    let dir = root.join(".agentgraph");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), body).unwrap();
}

fn build_index(root: &Path) {
    let idx = run(root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
}

fn build_sidecar(root: &Path, expanded: &Path) {
    build_index(root);
    let exp = expanded.to_string_lossy().into_owned();
    let side = run(root, &["index", "--force", "--macro-expanded-root", &exp]);
    assert!(side.status.success(), "sidecar index: {}", stderr(&side));
}

fn blast(root: &Path, symbol: &str, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["blast-radius", symbol];
    args.extend_from_slice(extra);
    let out = run(root, &args);
    assert!(out.status.success(), "{}", stderr(&out));
    parse_json(&out)
}

// ---------------------------------------------------------------------------
// Unit: config resolution (no CLI)
// ---------------------------------------------------------------------------

#[test]
fn unit_builtin_default_is_off() {
    let root = common::temp_root("agentgraph-macrodef-unit-off");
    let cfg = agentgraph::config::load_macro_default_config(&root);
    assert_eq!(cfg.policy, agentgraph::config::MacroDefaultPolicy::Off);
    assert_eq!(cfg.source_label(), "builtin_default");
    assert!(!cfg.requests_include());
}

#[test]
fn unit_file_if_fresh_primary_path() {
    let root = common::temp_root("agentgraph-macrodef-unit-file");
    write_config(&root, "# P2-1\nmacro_default = \"if_fresh\"\n");
    let cfg = agentgraph::config::load_macro_default_config(&root);
    assert_eq!(cfg.policy, agentgraph::config::MacroDefaultPolicy::IfFresh);
    assert!(
        cfg.source_label().contains(".agentgraph/config.toml"),
        "source={}",
        cfg.source_label()
    );
}

#[test]
fn unit_file_fallback_agentgraph_toml() {
    let root = common::temp_root("agentgraph-macrodef-unit-fallback");
    std::fs::write(root.join("agentgraph.toml"), "macro_default = \"on\"\n").unwrap();
    let cfg = agentgraph::config::load_macro_default_config(&root);
    assert_eq!(cfg.policy, agentgraph::config::MacroDefaultPolicy::On);
    assert!(cfg.source_label().contains("agentgraph.toml"));
}

#[test]
fn unit_env_overrides_file() {
    let root = common::temp_root("agentgraph-macrodef-unit-env");
    write_config(&root, "macro_default = \"if_fresh\"\n");
    // std::env is process-wide; use resolve helper + direct parse for purity.
    assert_eq!(
        agentgraph::config::parse_macro_default_policy("off"),
        Some(agentgraph::config::MacroDefaultPolicy::Off)
    );
    let file_cfg = agentgraph::config::load_macro_default_config(&root);
    assert_eq!(
        file_cfg.policy,
        agentgraph::config::MacroDefaultPolicy::IfFresh
    );
    // Simulate env overlay via resolve_macro_include_request.
    let env_cfg = agentgraph::config::MacroDefaultConfig {
        policy: agentgraph::config::MacroDefaultPolicy::Off,
        source: agentgraph::config::MacroConfigSource::Env,
    };
    let (req, reason) = agentgraph::config::resolve_macro_include_request(None, &env_cfg);
    assert!(!req);
    assert!(reason.is_none());
}

// ---------------------------------------------------------------------------
// e2e: blast_radius + repo config
// Dirty fixtures (unsafe / eval) keep window=default so auto-include is testable.
// ---------------------------------------------------------------------------

#[test]
fn e2e_default_off_no_config_no_auto_include() {
    let (root, expanded) = temp_pair("off-default");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    build_sidecar(&root, &expanded);
    // Fresh sidecar present, but no config → global default stays OFF.
    let v = blast(&root, "helper", &[]);
    assert_eq!(v["include_macro"], false, "payload={v}");
    let reason = v["include_macro_reason"].as_str().unwrap_or("");
    assert!(
        !reason.contains("repo_config"),
        "global default must not auto-include: {reason}"
    );
}

#[test]
fn e2e_env_off_overrides_file_if_fresh() {
    let (root, expanded) = temp_pair("env-off");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"if_fresh\"\n");
    build_sidecar(&root, &expanded);
    let out = run_env(
        &root,
        &["blast-radius", "helper"],
        "AGENTGRAPH_MACRO_DEFAULT",
        "off",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let v = parse_json(&out);
    assert_eq!(v["include_macro"], false, "payload={v}");
    let reason = v["include_macro_reason"].as_str().unwrap_or("");
    assert!(
        !reason.contains("repo_config_if_fresh"),
        "env off must override file if_fresh: {reason}"
    );
}

#[test]
fn e2e_file_if_fresh_fresh_sidecar_auto_includes() {
    let (root, expanded) = temp_pair("if-fresh-ok");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"if_fresh\"\n");
    build_sidecar(&root, &expanded);
    // Confirm status sees fresh sidecar + config source.
    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let sv = parse_json(&st);
    assert_eq!(sv["exists"], true);
    assert_eq!(sv["stale"], false);
    assert_eq!(sv["macro_default"], "if_fresh", "status={sv}");
    let src = sv["macro_default_source"].as_str().unwrap_or("");
    assert!(
        src.contains("config.toml") || src.contains("file:"),
        "src={src}"
    );

    let v = blast(&root, "helper", &[]);
    assert_eq!(v["window"], "default", "dirty fixture: {v}");
    assert_eq!(v["include_macro"], true, "payload={v}");
    assert_eq!(
        v["include_macro_reason"], "repo_config_if_fresh",
        "payload={v}"
    );
}

#[test]
fn e2e_file_if_fresh_stale_sidecar_refused() {
    let (root, expanded) = temp_pair("if-fresh-stale");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"if_fresh\"\n");
    build_sidecar(&root, &expanded);
    // Mutate main source after sidecar build → fingerprint stale.
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub unsafe fn process() -> i32 { helper() + 2 }
pub fn extra() -> i32 { helper() }
"#,
    )
    .unwrap();
    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let sv = parse_json(&st);
    assert_eq!(sv["stale"], true, "status={sv}");

    let v = blast(&root, "helper", &[]);
    assert_eq!(v["include_macro"], false, "payload={v}");
    let reason = v["include_macro_reason"]
        .as_str()
        .expect("refusal reason required");
    assert!(
        reason.to_lowercase().contains("stale"),
        "must refuse stale sidecar: {reason}"
    );
}

#[test]
fn e2e_cli_no_include_macro_wins_over_if_fresh() {
    let (root, expanded) = temp_pair("cli-no-include");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"if_fresh\"\n");
    build_sidecar(&root, &expanded);
    let v = blast(&root, "helper", &["--no-include-macro"]);
    assert_eq!(v["include_macro"], false, "payload={v}");
    let reason = v["include_macro_reason"].as_str().unwrap_or("");
    assert!(
        !reason.contains("repo_config_if_fresh"),
        "explicit --no-include-macro must win: {reason}"
    );
}

#[test]
fn e2e_cli_include_macro_wins_over_off_config() {
    let (root, expanded) = temp_pair("cli-include");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"off\"\n");
    build_sidecar(&root, &expanded);
    let v = blast(&root, "helper", &["--include-macro"]);
    assert_eq!(v["include_macro"], true, "payload={v}");
    // Explicit CLI success keeps reason null (not repo_config_*).
    let reason = v["include_macro_reason"].as_str().unwrap_or("");
    assert!(
        !reason.contains("repo_config"),
        "explicit CLI include must not tag repo_config reason: {reason}"
    );
}

#[test]
fn e2e_env_if_fresh_overrides_file_off() {
    let (root, expanded) = temp_pair("env-on");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"off\"\n");
    build_sidecar(&root, &expanded);
    let out = run_env(
        &root,
        &["blast-radius", "helper"],
        "AGENTGRAPH_MACRO_DEFAULT",
        "if_fresh",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let v = parse_json(&out);
    assert_eq!(v["include_macro"], true, "payload={v}");
    assert_eq!(v["include_macro_reason"], "repo_config_if_fresh");
}

#[test]
fn e2e_sound_window_still_refuses_macro() {
    // Clean TS project → subset_ok / sound window. Repo config asks for macro.
    let base = common::temp_root("agentgraph-macrodef-sound");
    let root = base.join("app");
    let expanded = base.join("app-expanded");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    write_clean_js(&root);
    // Sibling expanded tree with extra symbol (must stay outside root).
    std::fs::write(
        expanded.join("src/auth.ts"),
        r#"
export function validateEmail(email: string): boolean {
  return email.includes("@");
}
export function createUser(email: string) {
  if (!validateEmail(email)) throw new Error("bad");
  return { email };
}
export function macroOnly(email: string) {
  return createUser(email);
}
"#,
    )
    .unwrap();
    write_config(&root, "macro_default = \"if_fresh\"\n");
    build_sidecar(&root, &expanded);

    let v = blast(&root, "createUser", &[]);
    // Sound window on clean project must not auto-include macro.
    assert_eq!(v["include_macro"], false, "payload={v}");
    let reason = v["include_macro_reason"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    assert!(
        reason.contains("sound") || reason.contains("exclusive"),
        "sound window must refuse macro: reason={reason} payload={v}"
    );
}

#[test]
fn e2e_on_policy_still_refuses_missing_sidecar() {
    let (root, expanded) = temp_pair("on-missing");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"on\"\n");
    build_index(&root); // no sidecar
    let v = blast(&root, "helper", &[]);
    assert_eq!(v["include_macro"], false, "payload={v}");
    let reason = v["include_macro_reason"]
        .as_str()
        .expect("reason when on-policy refused");
    assert!(
        reason.to_lowercase().contains("missing") || reason.to_lowercase().contains("macro"),
        "reason={reason}"
    );
}

#[test]
fn e2e_macro_status_shows_config_source() {
    let (root, expanded) = temp_pair("status-cfg");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"on\"\n");
    build_sidecar(&root, &expanded);
    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let v = parse_json(&st);
    assert_eq!(v["macro_default"], "on");
    assert_eq!(v["macro_default_requests_include"], true);
    let src = v["macro_default_source"].as_str().unwrap_or("");
    assert!(src.starts_with("file:"), "src={src}");
    // Honesty: global default note present.
    let note = v["macro_default_note"].as_str().unwrap_or("");
    assert!(
        note.to_lowercase().contains("global") && note.to_lowercase().contains("off"),
        "note={note}"
    );
}

#[test]
fn e2e_if_fresh_missing_sidecar_refused_not_silent_on() {
    let (root, expanded) = temp_pair("if-fresh-missing");
    write_dirty_rust(&root);
    write_expanded_dirty(&expanded);
    write_config(&root, "macro_default = \"if_fresh\"\n");
    build_index(&root);
    let v = blast(&root, "helper", &[]);
    assert_eq!(v["include_macro"], false, "payload={v}");
    let reason = v["include_macro_reason"].as_str().unwrap_or("");
    assert!(
        reason.to_lowercase().contains("missing") || reason.to_lowercase().contains("macro"),
        "if_fresh without sidecar must refuse: {reason}"
    );
}

// ---------------------------------------------------------------------------
// e2e: dirty eval project (window=default) + if_fresh + fresh sidecar
// ---------------------------------------------------------------------------

#[test]
fn e2e_default_window_if_fresh_includes_macro_candidates() {
    let base = common::temp_root("agentgraph-macrodef-dirty");
    let root = base.join("app");
    let expanded = base.join("app-expanded");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    write_eval_js(&root);
    std::fs::write(
        expanded.join("src/evil.js"),
        r#"
function runCode(code) {
  return eval(code);
}
function helper() { return 1; }
function fmt() { return helper(); }
module.exports = { runCode, helper, fmt };
"#,
    )
    .unwrap();
    write_config(&root, "macro_default = \"if_fresh\"\n");
    build_sidecar(&root, &expanded);
    let v = blast(&root, "helper", &[]);
    assert_eq!(v["window"], "default", "payload={v}");
    assert_eq!(v["include_macro"], true, "payload={v}");
    assert_eq!(
        v["include_macro_reason"], "repo_config_if_fresh",
        "payload={v}"
    );
}
