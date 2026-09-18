//! TDD: language-aware sound promise tiers.
//!
//! Stop the false-green loop: a single global SOUND_PROMISE_OK must not be
//! emitted for ALL languages. All shipped languages (js/ts/tsx/jsx, python,
//! go, rust) are AST-modeled. LexicalV1 remains in the enum for API stability
//! but no currently shipped language selects it.

use agentgraph::index::subset::{
    is_ast_modeled_language, is_lexical_v1_language, select_sound_promise, sound_promise_text,
    sound_promise_tier, SoundPromiseTier, SOUND_PROMISE_DISABLED, SOUND_PROMISE_OK_AST,
    SOUND_PROMISE_OK_LEXICAL_V1, SOUND_PROMISE_OK_MIXED_LEXICAL_V1,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-l2-promise-{name}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
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

fn write_rust_auth(root: &Path) {
    std::fs::write(
        root.join("src/auth.rs"),
        r#"
pub fn authenticate(email: &str, password: &str) -> bool {
    !email.is_empty() && !password.is_empty()
}

pub fn login_handler(email: &str, password: &str) -> bool {
    authenticate(email, password)
}
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
fn unit_pure_python_selects_ast_tier() {
    // Python is now AST-modeled (tree-sitter scanner), not lexical v1.
    let langs = vec!["python".to_string()];
    let tier = sound_promise_tier(true, &langs);
    assert_eq!(tier, SoundPromiseTier::AstModeled);
    let text = sound_promise_text(tier);
    assert_eq!(text, SOUND_PROMISE_OK_AST);
    assert!(
        !text.contains("lexical v1"),
        "py OK must not claim lexical v1: {text}"
    );
}

#[test]
fn unit_pure_go_selects_ast_tier() {
    let langs = vec!["go".to_string()];
    assert_eq!(
        sound_promise_tier(true, &langs),
        SoundPromiseTier::AstModeled
    );
}

#[test]
fn unit_pure_rust_selects_ast_tier() {
    // Rust scan_rust is now a tree-sitter AST scanner.
    let langs = vec!["rust".to_string()];
    let tier = sound_promise_tier(true, &langs);
    assert_eq!(tier, SoundPromiseTier::AstModeled);
    let text = sound_promise_text(tier);
    assert_eq!(text, SOUND_PROMISE_OK_AST);
    assert!(
        !text.contains("lexical v1"),
        "rust OK must not claim lexical v1: {text}"
    );
}

#[test]
fn unit_lexical_v1_is_reserved_for_future_scanners() {
    // No shipped language is lexical-v1; LexicalV1 arms remain for API stability.
    for lang in [
        "javascript",
        "typescript",
        "tsx",
        "jsx",
        "python",
        "go",
        "rust",
    ] {
        assert!(
            !is_lexical_v1_language(lang),
            "{lang} must not be lexical_v1"
        );
    }
    // The enum arm still exists and maps to a distinct promise string.
    assert_eq!(
        sound_promise_text(SoundPromiseTier::LexicalV1),
        SOUND_PROMISE_OK_LEXICAL_V1
    );
    assert_ne!(SOUND_PROMISE_OK_LEXICAL_V1, SOUND_PROMISE_OK_AST);
}

#[test]
fn unit_mixed_js_python_selects_ast_tier() {
    // Both are AST-modeled — no downgrade.
    let langs = vec!["javascript".to_string(), "python".to_string()];
    let (tier, text) = select_sound_promise(true, &langs);
    assert_eq!(tier, SoundPromiseTier::AstModeled);
    assert_eq!(text, SOUND_PROMISE_OK_AST);
}

#[test]
fn unit_mixed_js_rust_selects_ast_tier() {
    // Both are AST-modeled after the Rust scanner upgrade — no downgrade.
    let langs = vec!["javascript".to_string(), "rust".to_string()];
    let (tier, text) = select_sound_promise(true, &langs);
    assert_eq!(tier, SoundPromiseTier::AstModeled);
    assert_eq!(text, SOUND_PROMISE_OK_AST);
    assert_ne!(
        text, SOUND_PROMISE_OK_MIXED_LEXICAL_V1,
        "js+rust is pure AST — must not emit mixed lexical"
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
    assert!(is_ast_modeled_language("python"));
    assert!(is_ast_modeled_language("go"));
    assert!(is_ast_modeled_language("rust"));

    assert!(!is_lexical_v1_language("rust"));
    assert!(!is_lexical_v1_language("javascript"));
    assert!(!is_lexical_v1_language("python"));
    assert!(!is_lexical_v1_language("go"));
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

// --- CLI: pure Python tree → AST OK (upgraded from lexical v1) --------------

#[test]
fn cli_impact_sound_pure_python_tree_emits_ast_ok() {
    let root = temp_root("pure-py");
    write_py_auth(&root);
    let v = index_and_impact(&root, "authenticate");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "clean py must be in S: {v}");
    assert_eq!(v["promise_tier"], "ast_modeled", "{v}");
    let promise = v["promise"].as_str().unwrap_or("");
    assert_eq!(
        promise, SOUND_PROMISE_OK_AST,
        "pure Python must emit AST OK: {v}"
    );
    assert_ne!(promise, SOUND_PROMISE_OK_LEXICAL_V1);
}

// --- CLI: pure Rust tree → AST OK (scan_rust is AST-modeled) ----------------

#[test]
fn cli_impact_sound_pure_rust_tree_emits_ast_ok() {
    let root = temp_root("pure-rs");
    write_rust_auth(&root);
    let v = index_and_impact(&root, "authenticate");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "clean rust must be in S: {v}");
    assert_eq!(v["promise_tier"], "ast_modeled", "{v}");
    let promise = v["promise"].as_str().unwrap_or("");
    assert_eq!(
        promise, SOUND_PROMISE_OK_AST,
        "pure Rust must emit AST OK: {v}"
    );
    assert_ne!(promise, SOUND_PROMISE_OK_LEXICAL_V1);
    assert!(
        !promise.contains("lexical v1"),
        "AST promise must not claim lexical v1: {promise}"
    );
}

// --- CLI: mixed JS+Python → both AST, no downgrade --------------------------

#[test]
fn cli_impact_sound_mixed_js_python_stays_ast() {
    let root = temp_root("mixed-js-py");
    write_js_auth(&root);
    write_py_auth(&root);
    let v = index_and_impact(&root, "authenticate");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "{v}");
    assert_eq!(v["promise_tier"], "ast_modeled", "{v}");
    let promise = v["promise"].as_str().unwrap_or("");
    assert_eq!(promise, SOUND_PROMISE_OK_AST, "{v}");
}

// --- CLI: mixed JS+Rust → both AST, no lexical downgrade --------------------

#[test]
fn cli_impact_sound_mixed_js_rust_stays_ast() {
    let root = temp_root("mixed-js-rs");
    write_js_auth(&root);
    write_rust_auth(&root);
    let v = index_and_impact(&root, "authenticate");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "{v}");
    assert_eq!(v["promise_tier"], "ast_modeled", "{v}");
    let promise = v["promise"].as_str().unwrap_or("");
    assert_eq!(promise, SOUND_PROMISE_OK_AST, "{v}");
    assert_ne!(promise, SOUND_PROMISE_OK_MIXED_LEXICAL_V1, "{v}");
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
fn cli_callers_sound_pure_python_emits_ast_ok() {
    let root = temp_root("callers-py");
    write_py_auth(&root);
    let (ok, _, err) = run(root.as_path(), &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(root.as_path(), &["callers", "authenticate", "--sound"]);
    assert!(ok, "callers --sound failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("callers sound json");
    assert_eq!(v["mode"], "sound");
    assert_eq!(v["subset_ok"], true, "{v}");
    assert_eq!(v["promise_tier"], "ast_modeled", "{v}");
    assert_eq!(
        v["promise"].as_str().unwrap_or(""),
        SOUND_PROMISE_OK_AST,
        "{v}"
    );
}

// --- CLI: subset command reports tier ---------------------------------------

#[test]
fn cli_subset_reports_promise_tier() {
    let root = temp_root("subset-py");
    write_py_auth(&root);
    let (ok, _, err) = run(root.as_path(), &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(root.as_path(), &["subset"]);
    assert!(ok, "subset failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("subset json");
    assert_eq!(v["in_subset"], true, "{stdout}");
    assert_eq!(v["promise_tier"], "ast_modeled", "{stdout}");
    assert_eq!(
        v["promise"].as_str().unwrap_or(""),
        SOUND_PROMISE_OK_AST,
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
