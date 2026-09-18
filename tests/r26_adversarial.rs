//! R26 adversarial: P2 macro sidecar + rs.di.inventory_submit residuals.

use agentgraph::index::extract::extract_file;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(tag: &str) -> PathBuf {
    common::temp_root(&format!("ag-r26-{tag}"))
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

/// Expanded tree under `--root` must not pollute the main source graph or flip
/// main `subset_ok`. Product default path stays source-only.
#[test]
fn expanded_root_under_main_root_rejected() {
    let root = temp_root("under-root");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let expanded = root.join("expand-shadow");
    std::fs::create_dir_all(expanded.join("src")).unwrap();

    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn fmt() -> i32 { unsafe { helper() } }
"#,
    )
    .unwrap();

    let idx = run(
        &root,
        &[
            "index",
            "--force",
            "--macro-expanded-root",
            &expanded.to_string_lossy(),
        ],
    );
    assert!(
        !idx.status.success(),
        "expanded root under main root must be rejected; stdout={} stderr={}",
        stdout(&idx),
        stderr(&idx)
    );
    let err = stderr(&idx).to_lowercase();
    assert!(
        err.contains("expanded")
            && (err.contains("under") || err.contains("inside") || err.contains("root")),
        "stderr must explain nesting rejection: {err}"
    );

    // Rejection runs BEFORE main reindex — no sidecar. Main index.db was not
    // written by the failed command.
    let sidecar = root.join(".agentgraph").join("index.macro.db");
    assert!(
        !sidecar.exists(),
        "rejected nested expanded root must not create sidecar"
    );

    // Operator follows guidance: move shadow outside --root, then reindex.
    let sibling = common::temp_root("ag-r26-outside");
    std::fs::rename(&expanded, &sibling).expect("move expanded outside root");
    let idx2 = run(&root, &["index", "--force"]);
    assert!(idx2.status.success(), "{}", stderr(&idx2));

    // Main graph must not include expanded-only symbols.
    let stats = run(&root, &["stats"]);
    assert!(stats.status.success(), "{}", stderr(&stats));
    let sj = parse_json(&stats);
    // helper + process only (fmt lives only in the rejected expanded tree).
    assert!(
        sj["symbols"].as_u64().unwrap() < 5,
        "main symbols must not include expanded tree: {sj}"
    );

    let subset = run(&root, &["subset"]);
    assert!(
        subset.status.success(),
        "subset after reject: {}",
        stderr(&subset)
    );
    let subset_j = parse_json(&subset);
    assert_eq!(
        subset_j["in_subset"], true,
        "rejected expanded unsafe must not flip main subset: {subset_j}"
    );

    let plain = run(&root, &["callers", "helper"]);
    assert!(plain.status.success(), "{}", stderr(&plain));
    let hits = parse_json(&plain);
    let enc: Vec<String> = hits
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r["enclosing"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !enc.iter().any(|e| e == "fmt"),
        "default callers must not include expanded fmt: {enc:?}"
    );
}

/// Sidecar status/queries must not create `index.macro.db` when absent
/// (already partially locked; re-verify MCP macro_status + path-with-spaces).
#[test]
fn macro_status_absent_and_spaces_path_no_create() {
    let base = temp_root("spaces root");
    let root = base.join("app dir");
    let expanded = base.join("exp tree");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::write(
        root.join("src/a.rs"),
        "pub fn helper() {}\npub fn p() { helper(); }\n",
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/a.rs"),
        "pub fn helper() {}\npub fn extra() { helper(); }\n",
    )
    .unwrap();

    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let sidecar = root.join(".agentgraph").join("index.macro.db");
    assert!(!sidecar.exists(), "status must not create sidecar");
    let status = parse_json(&st);
    assert_eq!(status["exists"], false);

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let side_idx = run(
        &root,
        &[
            "index",
            "--force",
            "--macro-expanded-root",
            &expanded.to_string_lossy(),
        ],
    );
    assert!(
        side_idx.status.success(),
        "sibling expanded with spaces must work: {}",
        stderr(&side_idx)
    );
    assert!(sidecar.exists());
    let st2 = parse_json(&run(&root, &["macro", "status"]));
    assert_eq!(st2["exists"], true, "status={st2}");
    assert!(st2["symbols"].as_u64().unwrap() >= 2, "status={st2}");
    assert_eq!(st2["origin"], "macro_expanded");
}

/// MCP tools/list must expose `with_macro` + `macro_status` (docs/README claim).
#[test]
fn mcp_tools_list_exposes_macro_surface() {
    use std::io::Write;
    let root = temp_root("mcp-macro");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), "pub fn helper() {}\n").unwrap();

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
        );
        stdin.write_all(msgs.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("mcp out");
    let text = stdout(&out);
    assert!(out.status.success(), "stderr={}", stderr(&out));
    assert!(
        text.contains("macro_status"),
        "tools/list must include macro_status: {text}"
    );
    assert!(
        text.contains("with_macro"),
        "tools/list must include with_macro: {text}"
    );
}

/// Path-qualified inventory registration + factory must mint Heuristic edges
/// targeting the **type** (Bar), not the module segment (foo).
#[test]
fn inventory_path_qualified_registration_and_factory() {
    let src = r#"
mod foo {
    pub struct Bar;
    impl Bar {
        pub fn new() -> Self { Self }
    }
    pub struct StrategyRegistration {
        factory: fn() -> Box<dyn Send>,
    }
}

inventory::submit! {
    foo::StrategyRegistration {
        factory: || Box::new(foo::Bar::new()),
    }
}
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "StrategyRegistration"),
        "path-qualified registration type must yield StrategyRegistration; got {hits:?}"
    );
    assert!(
        hits.iter().any(|n| n == "Bar"),
        "path-qualified factory `foo::Bar::new` must yield Bar (not foo); got {hits:?}"
    );
    assert!(
        !hits.iter().any(|n| n == "foo"),
        "module segment must not become a sink: {hits:?}"
    );
}

/// Parenthesized path form: `inventory::submit!(foo::Bar::new)` — sink is Bar.
#[test]
fn inventory_paren_path_form_targets_type() {
    let src = r#"
mod foo {
    pub struct Bar;
    impl Bar { pub fn new() -> Self { Self } }
}
inventory::submit!(foo::Bar::new);
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Bar"),
        "paren path form must yield Bar; got {hits:?}"
    );
    assert!(
        !hits.iter().any(|n| n == "foo"),
        "paren path form must not sink foo: {hits:?}"
    );
}

/// Non-inventory crate path must not mint sound-eligible inventory edges.
#[test]
fn inventory_false_path_rejected() {
    let src = r#"
struct Foo;
myinventory::submit! { Foo { } }
other::submit! { Foo { } }
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "non-inventory submit paths must not mint inventory edges: {hits:?}"
    );
}

/// After expanded tree is deleted, `--with-macro` must not crash; macro status
/// should surface that the recorded expanded_root is gone (honesty signal).
#[test]
fn deleted_expanded_tree_stale_sidecar_honest() {
    let base = temp_root("stale");
    let root = base.join("app");
    let expanded = base.join("expanded");
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
    let exp_str = expanded.to_string_lossy().into_owned();
    let side = run(
        &root,
        &["index", "--force", "--macro-expanded-root", &exp_str],
    );
    assert!(side.status.success(), "{}", stderr(&side));

    std::fs::remove_dir_all(&expanded).unwrap();

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(
        with.status.success(),
        "stale sidecar must not crash: {}",
        stderr(&with)
    );
    let hits = parse_json(&with);
    let rows: Vec<serde_json::Value> = if let Some(arr) = hits.as_array() {
        arr.clone()
    } else if let Some(arr) = hits.get("callers").and_then(|x| x.as_array()) {
        arr.clone()
    } else {
        vec![]
    };
    assert!(
        !rows.is_empty(),
        "stale sidecar rows may still union: {}",
        stdout(&with)
    );

    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let status = parse_json(&st);
    assert_eq!(status["exists"], true);
    // Honesty: status should warn that expanded_root no longer exists.
    assert_eq!(
        status["expanded_root_missing"], true,
        "macro status must flag deleted expanded_root: {status}"
    );
}

/// `--with-macro --exact-only` ignores the sidecar entirely (Track M1 §1.4).
#[test]
fn with_macro_exact_only_ignores_sidecar() {
    let base = temp_root("exact");
    let root = base.join("app");
    let expanded = base.join("expanded");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); }\n",
    )
    .unwrap();
    // Expanded-only exact caller.
    std::fs::write(
        expanded.join("src/core.rs"),
        "pub fn helper() {}\npub fn clone() { helper(); }\n",
    )
    .unwrap();
    assert!(run(&root, &["index", "--force"]).status.success());
    let exp_str = expanded.to_string_lossy().into_owned();
    assert!(run(
        &root,
        &["index", "--force", "--macro-expanded-root", &exp_str]
    )
    .status
    .success());

    let with = run(
        &root,
        &["callers", "helper", "--with-macro", "--exact-only"],
    );
    assert!(with.status.success(), "{}", stderr(&with));
    let hits = parse_json(&with);
    let arr = hits.as_array().cloned().unwrap_or_else(|| {
        hits.get("callers")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default()
    });
    let enc: Vec<String> = arr
        .iter()
        .filter_map(|r| r["enclosing"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        enc.iter().any(|e| e == "process"),
        "exact main hit expected: {enc:?}"
    );
    assert!(
        !enc.iter().any(|e| e == "clone"),
        "exact-only + with-macro must ignore sidecar (no clone): {enc:?} raw={}",
        stdout(&with)
    );
    for r in &arr {
        assert_ne!(
            r["origin"], "macro_expanded",
            "exact-only must not tag sidecar rows: {r}"
        );
    }
}
