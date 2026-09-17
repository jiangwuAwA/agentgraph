//! R28 adversarial: inventory alias grammar residuals + sidecar/watch/CLI honesty.
//!
//! Method: tests first → fail → minimal fix → full gates green.

use agentgraph::index::extract::extract_file;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("ag-r28-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
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

fn parse_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).unwrap_or_else(|e| {
        panic!(
            "invalid JSON ({e}): stdout={} stderr={}",
            stdout(out),
            stderr(out)
        )
    })
}

fn extract(src: &str) -> agentgraph::index::extract::ExtractedFile {
    let known = HashSet::new();
    extract_file(src, Language::Rust, "src/plugins.rs", &known).expect("extract")
}

fn rule_hits(out: &agentgraph::index::extract::ExtractedFile, rule: &str) -> Vec<String> {
    out.references
        .iter()
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == rule)
                .unwrap_or(false)
        })
        .map(|r| r.name.clone())
        .collect()
}

/// R27 claimed rename-only does not unlock bare `submit` (correct Rust:
/// `use inventory::submit as s` brings `s` into scope, not `submit`).
/// The rename name itself **must** mint — `s!(Foo{})` is valid Rust.
#[test]
fn inventory_rename_single_letter_alias_mints() {
    let src = r#"
use inventory::submit as s;

struct Foo;
struct T;
impl T { fn new() -> Self { Self } }

s! {
    Foo { factory: || T::new() }
}
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Foo"),
        "single-letter rename alias s! must mint Foo: {hits:?}"
    );
    assert!(
        hits.iter().any(|n| n == "T"),
        "rename alias s! must mint factory T: {hits:?}"
    );
}

/// Brace-import rename: `use inventory::{submit as s}; s!(...)`.
#[test]
fn inventory_brace_rename_alias_mints() {
    let src = r#"
use inventory::{submit as s};

struct Reg;
s! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "brace rename inventory::{{submit as s}} must mint Reg: {hits:?}"
    );
}

/// Brace-import rename must NOT unlock the original bare `submit` name.
#[test]
fn inventory_brace_rename_does_not_unlock_bare_submit() {
    let src = r#"
use inventory::{submit as s};

struct Reg;
submit! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "brace rename must not unlock bare submit!: {hits:?}"
    );
}

/// `pub use inventory::submit; submit!(...)`.
#[test]
fn inventory_pub_use_mints() {
    let src = r#"
pub use inventory::submit;

struct Reg;
submit! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "pub use inventory::submit must mint Reg: {hits:?}"
    );
}

/// Nested module import: `mod m { use inventory::submit; submit!(...) }`.
/// File-scoped alias collection is an intentional over-approx for L1.
#[test]
fn inventory_nested_mod_use_mints() {
    let src = r#"
mod plugins {
    use inventory::submit;

    struct Reg;
    submit! { Reg { } }
}
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "nested mod use inventory::submit must mint Reg: {hits:?}"
    );
}

/// Multiple `use` items in one file still collect the inventory alias.
#[test]
fn inventory_multi_use_in_one_file_mints() {
    let src = r#"
use std::fmt::Debug;
use inventory::submit as s;
use std::sync::Arc;

struct Reg;
s! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "multi-use file must still mint via alias s: {hits:?}"
    );
}

/// `cfg_attr` / `cfg` on the import must not drop collection (fail-open OK).
#[test]
fn inventory_cfg_attr_use_mints() {
    let src = r#"
#[cfg(not(test))]
#[cfg_attr(feature = "std", allow(unused_imports))]
use inventory::submit;

struct Reg;
submit! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "cfg/cfg_attr-gated use inventory::submit must mint Reg: {hits:?}"
    );
}

/// Crate-relative brace + rename: `use crate::{inventory::submit as s}`.
#[test]
fn inventory_crate_brace_rename_mints() {
    let src = r#"
use crate::{inventory::submit as s};

struct Reg;
s! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "crate-relative brace rename must mint Reg: {hits:?}"
    );
}

/// **Major residual (R28):** `use evil::{inventory::submit}` must NOT unlock
/// bare `submit!` as sound-eligible inventory. Path form `evil::inventory::submit!`
/// is already rejected (R27); brace use-list must apply the same prefix check.
#[test]
fn inventory_foreign_brace_use_list_rejected() {
    let src = r#"
use evil::{inventory::submit};

struct Foo;
submit! { Foo { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "use evil::{{inventory::submit}} must not unlock sound-eligible submit!: {hits:?}"
    );
}

/// Same hole via rename inside a foreign brace list.
#[test]
fn inventory_foreign_brace_rename_use_list_rejected() {
    let src = r#"
use evil::{inventory::submit as s};

struct Foo;
s! { Foo { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "use evil::{{inventory::submit as s}} must not mint inventory edges: {hits:?}"
    );
}

/// Local `#[macro_export] macro_rules! submit` without inventory import: fail-closed.
#[test]
fn inventory_local_macro_export_submit_rejected() {
    let src = r#"
#[macro_export]
macro_rules! submit {
    ($($t:tt)*) => {};
}

struct Reg;
submit! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "local macro_rules! submit without inventory import must not mint: {hits:?}"
    );
}

/// Wildcard import of the inventory crate still unlocks bare submit.
#[test]
fn inventory_wildcard_use_mints() {
    let src = r#"
use inventory::*;

struct Reg;
submit! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "use inventory::* must mint Reg: {hits:?}"
    );
}

/// Foreign wildcard must not unlock submit.
#[test]
fn inventory_foreign_wildcard_rejected() {
    let src = r#"
use evil::inventory::*;

struct Reg;
submit! { Reg { } }
"#;
    let hits = rule_hits(&extract(src), "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "use evil::inventory::* must not mint inventory edges: {hits:?}"
    );
}

/// Impact/callers `--with-macro` unions each store with the same `limit`, so
/// total rows can approach ~2N. CLI help must say so (agent context budgets).
#[test]
fn cli_with_macro_help_documents_union_limit() {
    let out = Command::new(bin())
        .args(["callers", "--help"])
        .stdin(Stdio::null())
        .output()
        .expect("callers --help");
    let text = stdout(&out).to_lowercase();
    assert!(
        text.contains("with-macro") || text.contains("with_macro"),
        "callers --help must document --with-macro: {text}"
    );
    assert!(
        text.contains("2n") || text.contains("~2") || text.contains("per store"),
        "callers --help must document union/~2N (per-store limit) for --with-macro: {text}"
    );

    let iout = Command::new(bin())
        .args(["impact", "--help"])
        .stdin(Stdio::null())
        .output()
        .expect("impact --help");
    let itext = stdout(&iout).to_lowercase();
    assert!(
        itext.contains("with-macro") || itext.contains("with_macro"),
        "impact --help must document --with-macro: {itext}"
    );
    assert!(
        itext.contains("2n") || itext.contains("~2") || itext.contains("per store"),
        "impact --help must document union/~2N (per-store limit) for --with-macro: {itext}"
    );
}

/// Rust-only corpus: promise tier stays `ast_modeled` (Rust is AST-modeled).
#[test]
fn rust_only_promise_tier_is_ast_modeled() {
    let base = temp_root("rust-promise");
    let root = base.join("app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); }\n",
    )
    .unwrap();
    assert!(run(&root, &["index", "--force"]).status.success());
    let sub = run(&root, &["subset"]);
    assert!(sub.status.success(), "{}", stderr(&sub));
    let v = parse_json(&sub);
    assert_eq!(v["promise_tier"], "ast_modeled", "{v}");
}

/// MCP `impact` `with_macro` sidecar rows must carry `origin` (and a location
/// `at`) — symmetry with callers tagging so agents can filter/locate uniformly.
#[test]
fn mcp_impact_with_macro_rows_carry_origin_and_at() {
    let base = temp_root("mcp-impact-origin");
    let root = base.join("app");
    let expanded = base.join("app-expanded");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); }\n",
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() {}
pub fn process() { helper(); }
pub fn only_in_side() {}
pub fn mid() { only_in_side(); }
"#,
    )
    .unwrap();
    assert!(run(&root, &["index", "--force"]).status.success());
    let exp = expanded.to_string_lossy().into_owned();
    assert!(
        run(&root, &["index", "--force", "--macro-expanded-root", &exp])
            .status
            .success()
    );

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
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("mcp stdin");
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2024-11-05","capabilities":{{}},"clientInfo":{{"name":"r28","version":"0"}}}}}}"#
        )
        .unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"impact","arguments":{{"name":"only_in_side","with_macro":true,"depth":1}}}}}}"#
        )
        .unwrap();
        stdin.flush().unwrap();
    }
    // Give the server a moment then close stdin so it can exit after replies.
    std::thread::sleep(std::time::Duration::from_millis(800));
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("mcp output");
    let text = stdout(&out);
    let mut macro_rows: Vec<serde_json::Value> = Vec::new();
    for line in text.lines() {
        if !line.trim_start().starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["id"] != 2 {
            continue;
        }
        let content = &v["result"]["content"];
        let arr = if let Some(s) = content[0]["text"].as_str() {
            serde_json::from_str::<serde_json::Value>(s).unwrap_or_default()
        } else {
            content.clone()
        };
        if let Some(a) = arr.as_array() {
            for row in a {
                if row["origin"] == "macro_expanded" {
                    macro_rows.push(row.clone());
                }
            }
        }
    }
    assert!(
        !macro_rows.is_empty(),
        "MCP impact with_macro must return origin=macro_expanded rows: {text}"
    );
    for row in &macro_rows {
        assert!(
            row["at"].as_str().map(|s| s.contains(':')).unwrap_or(false),
            "MCP impact sidecar rows must carry at=path:line for caller symmetry: {row}"
        );
        assert!(
            row["path"].as_str().is_some(),
            "impact sidecar rows keep path: {row}"
        );
    }
}

/// Concurrent watch + sidecar index on the same root must not corrupt the
/// main store (WAL + busy_timeout). Documented guarantee under test.
#[test]
fn watch_and_sidecar_index_concurrent_no_corruption() {
    let base = temp_root("watch-sidecar");
    let root = base.join("app");
    let expanded = base.join("app-expanded");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); }\n",
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        "pub fn helper() {}\npub fn clone() { helper(); }\n",
    )
    .unwrap();
    assert!(run(&root, &["index", "--force"]).status.success());

    let mut watch = Command::new(bin())
        .arg("--root")
        .arg(&root)
        .args(["watch", "--interval", "1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn watch");
    std::thread::sleep(std::time::Duration::from_millis(400));

    // Touch source + rebuild main/sidecar while watch is live.
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); helper(); }\n",
    )
    .unwrap();
    let exp = expanded.to_string_lossy().into_owned();
    let re = run(&root, &["index", "--force", "--macro-expanded-root", &exp]);
    assert!(
        re.status.success(),
        "index --macro-expanded-root concurrent with watch must succeed: {}",
        stderr(&re)
    );

    let st = run(&root, &["stats"]);
    assert!(
        st.status.success(),
        "stats after concurrent write: {}",
        stderr(&st)
    );
    let stats = parse_json(&st);
    assert!(
        stats["files"].as_u64().unwrap_or(0) >= 1,
        "main store must remain queryable: {stats}"
    );
    let cl = run(&root, &["callers", "helper"]);
    assert!(
        cl.status.success(),
        "callers after concurrent write: {}",
        stderr(&cl)
    );
    let status = run(&root, &["macro", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let mst = parse_json(&status);
    assert_eq!(
        mst["exists"], true,
        "sidecar must survive concurrent watch: {mst}"
    );

    let _ = watch.kill();
    let _ = watch.wait();
}

/// SCIP export on a fixture with inventory + nest-style heuristic edges must
/// still be lint-clean when the official CLI is present.
#[test]
fn scip_export_inventory_fixture_lints() {
    let base = temp_root("scip-inv");
    let root = base.join("app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/plugins.rs"),
        r#"
struct StrategyRegistration { factory: fn() }
struct Plugin;
impl Plugin { fn new() -> Self { Self } }

inventory::submit! {
    StrategyRegistration {
        factory: || Plugin::new(),
    }
}

use inventory::submit as s;
struct Second;
s! { Second { } }
"#,
    )
    .unwrap();
    // Nest-flavored TS alongside inventory rust (heuristic export default).
    std::fs::write(
        root.join("src/app.module.ts"),
        r#"
import { Module } from '@nestjs/common';
import { AppService } from './app.service';

@Module({ providers: [AppService], controllers: [], imports: [], exports: [AppService] })
export class AppModule {}
"#,
    )
    .unwrap();
    assert!(run(&root, &["index", "--force"]).status.success());
    let scip = base.join("index.scip");
    let exp = run(&root, &["export", "scip", "--out", &scip.to_string_lossy()]);
    assert!(exp.status.success(), "{}", stderr(&exp));
    assert!(scip.exists());

    // Official scip CLI is optional locally; when present it must exit 0.
    let lint = Command::new("scip")
        .args(["lint", &scip.to_string_lossy()])
        .stdin(Stdio::null())
        .output();
    match lint {
        Ok(out) => {
            assert!(
                out.status.success(),
                "scip lint must exit 0: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            eprintln!("skip scip lint (CLI not installed): {e}");
        }
    }
}
