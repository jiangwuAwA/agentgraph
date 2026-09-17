//! TDD: language-aware sound promise tiers.
//!
//! Stop the false-green loop: a single global SOUND_PROMISE_OK must not be
//! emitted for ALL languages. JS/TS/Rust (AST-modeled) vs Python/Go (lexical v1)
//! get distinct promise strings; mixed corpora never silently claim AST green.

use agentgraph::index::subset::{
    is_ast_modeled_language, is_lexical_v1_language, select_sound_promise, sound_promise_text,
    sound_promise_tier, SoundPromiseTier, SOUND_PROMISE_DISABLED, SOUND_PROMISE_OK_AST,
    SOUND_PROMISE_OK_LEXICAL_V1, SOUND_PROMISE_OK_MIXED_LEXICAL_V1,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-l2-promise-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn run(root: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run agentgraph");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write_js_auth(root: &Path) {
    std::fs::write(
        root.join("src/auth.js"),
        r#"
export function authenticate(email, password) {
  return { email, password };
}
export function loginHandler(email, password) {
  return authenticate(email, password);
}
"#,
    )
    .unwrap();
}

fn write_py_auth(root: &Path) {
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
}

fn index_and_impact(root: &Path, name: &str) -> Value {
    let (ok, _, err) = run(root, &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(root, &["impact", name, "--sound", "--depth", "2"]);
    assert!(ok, "impact --sound failed: {err}");
    serde_json::from_str(&stdout).expect("impact sound json")
}

// --- unit: tier selection ---------------------------------------------------

#[test]
fn unit_pure_ast_languages_select_ast_tier() {
    let langs = vec!["javascript".to_string(), "typescript".to_string()];
    assert_eq!(
        sound_promise_tier(true, &langs),
        SoundPromiseTier::AstModeled
    );
    assert_eq!(
        sound_promise_text(SoundPromiseTier::AstModeled),
        SOUND_PROMISE_OK_AST
    );
}

#[test]
fn unit_pure_python_selects_lexical_v1_not_ast() {
    let langs = vec!["python".to_string()];
    let tier = sound_promise_tier(true, &langs);
    assert_eq!(tier, SoundPromiseTier::LexicalV1);
    let text = sound_promise_text(tier);
    assert_eq!(text, SOUND_PROMISE_OK_LEXICAL_V1);
    assert_ne!(
        text, SOUND_PROMISE_OK_AST,
        "py OK must not be the AST OK string"
    );
    assert!(
        text.contains("lexical") || text.contains("Lexical"),
        "lexical tier text must say lexical: {text}"
    );
}

#[test]
fn unit_mixed_js_python_selects_weakest_lexical_tier() {
    let langs = vec!["javascript".to_string(), "python".to_string()];
    let (tier, text) = select_sound_promise(true, &langs);
    assert_eq!(tier, SoundPromiseTier::MixedLexicalV1);
    assert_eq!(text, SOUND_PROMISE_OK_MIXED_LEXICAL_V1);
    assert_ne!(
        text, SOUND_PROMISE_OK_AST,
        "mixed must never claim AST-only green"
    );
    assert_ne!(
        text, SOUND_PROMISE_OK_LEXICAL_V1,
        "mixed text names both tiers"
    );
    assert!(
        text.to_ascii_lowercase().contains("mix"),
        "mixed text must mention mixed: {text}"
    );
}

#[test]
fn unit_go_selects_lexical_v1() {
    let langs = vec!["go".to_string()];
    assert_eq!(
        sound_promise_tier(true, &langs),
        SoundPromiseTier::LexicalV1
    );
}

#[test]
fn unit_violation_always_disables_regardless_of_languages() {
    for langs in [
        vec!["javascript".to_string()],
        vec!["python".to_string()],
        vec!["rust".to_string(), "go".to_string()],
    ] {
        let (tier, text) = select_sound_promise(false, &langs);
        assert_eq!(tier, SoundPromiseTier::Disabled);
        assert_eq!(text, SOUND_PROMISE_DISABLED);
    }
}

#[test]
fn unit_language_classifiers() {
    assert!(is_ast_modeled_language("javascript"));
    assert!(is_ast_modeled_language("typescript"));
    assert!(is_ast_modeled_language("tsx"));
    assert!(is_ast_modeled_language("jsx"));
    assert!(is_ast_modeled_language("rust"));
    assert!(!is_ast_modeled_language("python"));
    assert!(!is_ast_modeled_language("go"));

    assert!(is_lexical_v1_language("python"));
    assert!(is_lexical_v1_language("go"));
    assert!(!is_lexical_v1_language("javascript"));
    assert!(!is_lexical_v1_language("rust"));
}

// --- CLI: pure JS tree → AST OK ---------------------------------------------

#[test]
fn cli_impact_sound_pure_js_tree_emits_ast_ok() {
    let root = temp_root("pure-js");
    write_js_auth(&root);
    let v = index_and_impact(&root, "authenticate");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "{v}");
    assert_eq!(v["promise_tier"], "ast_modeled", "{v}");
    let promise = v["promise"].as_str().unwrap_or("");
    assert_eq!(
        promise, SOUND_PROMISE_OK_AST,
        "pure JS must emit AST OK, not lexical: {v}"
    );
    assert_ne!(promise, SOUND_PROMISE_OK_LEXICAL_V1);
    assert!(
        !promise.contains("lexical v1"),
        "AST promise must not claim lexical v1: {promise}"
    );
}

// --- CLI: pure Python tree → LEXICAL_V1, not AST ----------------------------

#[test]
fn cli_impact_sound_pure_python_tree_emits_lexical_v1() {
    let root = temp_root("pure-py");
    write_py_auth(&root);
    let v = index_and_impact(&root, "authenticate");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "clean py must be in S: {v}");
    assert_eq!(v["promise_tier"], "lexical_v1", "{v}");
    let promise = v["promise"].as_str().unwrap_or("");
    assert_eq!(
        promise, SOUND_PROMISE_OK_LEXICAL_V1,
        "pure Python must emit lexical v1 OK: {v}"
    );
    assert_ne!(
        promise, SOUND_PROMISE_OK_AST,
        "false-green: py must never get the AST OK string"
    );
}

// --- CLI: mixed JS+Python → weakest tier, never silent AST ------------------

#[test]
fn cli_impact_sound_mixed_js_python_never_ast_only_green() {
    let root = temp_root("mixed-js-py");
    write_js_auth(&root);
    write_py_auth(&root);
    let v = index_and_impact(&root, "authenticate");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "{v}");
    assert_eq!(v["promise_tier"], "mixed_lexical_v1", "{v}");
    let promise = v["promise"].as_str().unwrap_or("");
    assert_eq!(promise, SOUND_PROMISE_OK_MIXED_LEXICAL_V1, "{v}");
    assert_ne!(
        promise, SOUND_PROMISE_OK_AST,
        "mixed corpus must not silently claim AST-only green"
    );
}

// --- CLI: S violation → DISABLED --------------------------------------------

#[test]
fn cli_impact_sound_violation_emits_disabled() {
    let root = temp_root("viol");
    std::fs::write(
        root.join("src/evil.js"),
        r#"
export function dangerous(code) {
  return eval(code);
}
"#,
    )
    .unwrap();
    let v = index_and_impact(&root, "dangerous");
    assert_eq!(v["subset_ok"], false, "{v}");
    assert_eq!(v["promise_tier"], "disabled", "{v}");
    assert_eq!(v["promise"].as_str().unwrap_or(""), SOUND_PROMISE_DISABLED);
}

// --- CLI: callers --sound uses the same tier --------------------------------

#[test]
fn cli_callers_sound_pure_python_emits_lexical_v1() {
    let root = temp_root("callers-py");
    write_py_auth(&root);
    let (ok, _, err) = run(&root, &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["callers", "authenticate", "--sound"]);
    assert!(ok, "callers --sound failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("callers sound json");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "{v}");
    assert_eq!(v["promise_tier"], "lexical_v1", "{v}");
    assert_eq!(
        v["promise"].as_str().unwrap_or(""),
        SOUND_PROMISE_OK_LEXICAL_V1,
        "{v}"
    );
}

// --- CLI: subset command reports tier ---------------------------------------

#[test]
fn cli_subset_reports_promise_tier() {
    let root = temp_root("subset-py");
    write_py_auth(&root);
    let (ok, _, err) = run(&root, &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["subset"]);
    assert!(ok, "subset failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("subset json");
    assert_eq!(v["in_subset"], true, "{stdout}");
    assert_eq!(v["promise_tier"], "lexical_v1", "{stdout}");
    assert_eq!(
        v["promise"].as_str().unwrap_or(""),
        SOUND_PROMISE_OK_LEXICAL_V1,
        "{stdout}"
    );
}

// --- MCP identity: shared re-export path ------------------------------------

#[test]
fn mcp_server_reexports_shared_promise_constants() {
    // Identity — CLI and MCP must use the same const values (no drift).
    assert_eq!(
        agentgraph::mcp::server::SOUND_PROMISE_OK_AST,
        SOUND_PROMISE_OK_AST
    );
    assert_eq!(
        agentgraph::mcp::server::SOUND_PROMISE_OK_LEXICAL_V1,
        SOUND_PROMISE_OK_LEXICAL_V1
    );
    assert_eq!(
        agentgraph::mcp::server::SOUND_PROMISE_DISABLED,
        SOUND_PROMISE_DISABLED
    );
}
