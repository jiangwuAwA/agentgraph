//! R27 adversarial: sidecar residual surfaces + inventory alias honesty + CLI/MCP schema.
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
    let base = std::env::temp_dir().join(format!("ag-r27-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn run_in(cwd: &Path, root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn run(root: &Path, args: &[&str]) -> Output {
    run_in(root, root, args)
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

/// Relative `--macro-expanded-root` must be resolved against `--root`, not cwd.
/// A cwd-relative path that happens to exist elsewhere must not be silently
/// dual-indexed into this project's sidecar (wrong-tree / nesting bypass).
#[test]
fn relative_expanded_root_resolves_against_project_root() {
    let base = temp_root("rel-root");
    let root = base.join("app");
    let other = base.join("other");
    // Unrelated tree that would be picked up if the flag is cwd-relative.
    let decoy = other.join("expand-shadow");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(decoy.join("src")).unwrap();
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); }\n",
    )
    .unwrap();
    std::fs::write(
        decoy.join("src/core.rs"),
        "pub fn helper() {}\npub fn only_in_decoy() { helper(); }\n",
    )
    .unwrap();

    // Run from `other` with a relative expanded path. Project-relative meaning
    // of `expand-shadow` is `<root>/expand-shadow` (nested → reject), NOT
    // `<cwd>/expand-shadow` (unrelated sibling that would be accepted).
    let idx = run_in(
        &other,
        &root,
        &["index", "--force", "--macro-expanded-root", "expand-shadow"],
    );
    assert!(
        !idx.status.success(),
        "cwd-relative decoy expanded root must not be accepted; stdout={} stderr={}",
        stdout(&idx),
        stderr(&idx)
    );
    let err = stderr(&idx).to_lowercase();
    assert!(
        err.contains("expanded")
            || err.contains("macro")
            || err.contains("under")
            || err.contains("root")
            || err.contains("sibling")
            || err.contains("nested"),
        "rejection must explain path/nesting: {err}"
    );

    // Nested path under --root (explicit) still rejected even when relative
    // resolution is against --root.
    std::fs::create_dir_all(root.join("expand-shadow/src")).unwrap();
    std::fs::write(
        root.join("expand-shadow/src/core.rs"),
        "pub fn helper() {}\npub fn fmt() { unsafe { helper() } }\n",
    )
    .unwrap();
    let idx2 = run_in(
        &other,
        &root,
        &["index", "--force", "--macro-expanded-root", "expand-shadow"],
    );
    assert!(
        !idx2.status.success(),
        "project-relative nested expanded root must be rejected; stderr={}",
        stderr(&idx2)
    );
    let sidecar = root.join(".agentgraph").join("index.macro.db");
    assert!(
        !sidecar.exists(),
        "rejected relative expanded root must not create sidecar"
    );

    // Absolute sibling still works (cwd irrelevant).
    let sibling = base.join("app-expanded");
    std::fs::create_dir_all(sibling.join("src")).unwrap();
    std::fs::write(
        sibling.join("src/core.rs"),
        "pub fn helper() {}\npub fn clone() { helper(); }\n",
    )
    .unwrap();
    let idx3 = run_in(
        &other,
        &root,
        &[
            "index",
            "--force",
            "--macro-expanded-root",
            &sibling.to_string_lossy(),
        ],
    );
    assert!(
        idx3.status.success(),
        "absolute sibling expanded root must work from any cwd: {}",
        stderr(&idx3)
    );
    let payload = parse_json(&idx3);
    assert_eq!(
        payload["macro_sidecar"]["origin"], "macro_expanded",
        "{payload}"
    );

    // Project-relative sibling form `../app-expanded` resolved against --root.
    // Use OS-native separator so Linux/macOS see a parent component, not `..\`.
    let rel_sibling = Path::new("..").join("app-expanded");
    let rel_sibling_s = rel_sibling.to_string_lossy().into_owned();
    let idx4 = run_in(
        &other,
        &root,
        &["index", "--force", "--macro-expanded-root", &rel_sibling_s],
    );
    assert!(
        idx4.status.success(),
        "project-relative sibling {rel_sibling_s} must work: {}",
        stderr(&idx4)
    );
}

/// After a successful sibling sidecar build, recreating the recorded
/// `expanded_root` path as a junction/symlink that now resolves **under**
/// `--root` must surface `expanded_root_nested` on `macro status` (not only
/// `expanded_root_missing`). Nesting re-validation belongs on status, not only
/// at index time.
#[test]
fn macro_status_flags_expanded_root_now_nested() {
    let base = temp_root("nested-status");
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
    let exp = expanded.to_string_lossy().into_owned();
    let built = run(&root, &["index", "--force", "--macro-expanded-root", &exp]);
    assert!(built.status.success(), "{}", stderr(&built));

    // Move content under --root, then recreate the recorded path as a
    // junction/symlink so `expanded_root` still "exists" but canonicalizes
    // under main root.
    let moved = root.join("expand-shadow");
    std::fs::rename(&expanded, &moved).expect("move expanded under root");
    let link_ok = if cfg!(windows) {
        Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                &expanded.to_string_lossy(),
                &moved.to_string_lossy(),
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&moved, &expanded).is_ok()
        }
        #[cfg(not(unix))]
        {
            let _ = (&moved, &expanded);
            false
        }
    };
    if !link_ok {
        eprintln!("skip nested-junction assertion (link create failed)");
        // Deleted-path honesty still applies when junction cannot be created.
        let st = parse_json(&run(&root, &["macro", "status"]));
        assert_eq!(st["expanded_root_missing"], true, "{st}");
        return;
    }

    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let status = parse_json(&st);
    assert_eq!(status["exists"], true, "{status}");
    assert_eq!(
        status["expanded_root_missing"], false,
        "junction target path still exists: {status}"
    );
    assert_eq!(
        status["expanded_root_nested"], true,
        "macro status must re-validate nesting after the expanded tree resolves under --root: {status}"
    );

    // Rebuild with a path that canonicalizes under --root must still hard-reject.
    let nested_str = moved.to_string_lossy().into_owned();
    let idx = run(
        &root,
        &["index", "--force", "--macro-expanded-root", &nested_str],
    );
    assert!(
        !idx.status.success(),
        "nested expanded root after move must reject: {}",
        stderr(&idx)
    );
    // Also reject the junction path itself (canonicalize → under root).
    let junction_str = expanded.to_string_lossy().into_owned();
    let idx2 = run(
        &root,
        &["index", "--force", "--macro-expanded-root", &junction_str],
    );
    assert!(
        !idx2.status.success(),
        "junction expanded root must reject after canonicalize: {}",
        stderr(&idx2)
    );
}

/// Sidecar built from a tree with S violations (unsafe / eval-shaped) must not
/// make `macro status` look "sound-ready". Report sidecar subset violation
/// count without claiming main `subset_ok`.
#[test]
fn macro_status_reports_sidecar_subset_violations() {
    let base = temp_root("side-viol");
    let root = base.join("app");
    let expanded = base.join("app-expanded");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("js")).unwrap();
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); }\n",
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/core.rs"),
        "pub fn helper() {}\npub fn fmt() { unsafe { helper() } }\n",
    )
    .unwrap();
    // Expanded tree also contains Python/JS unsafe — sidecar-only S noise.
    std::fs::write(
        expanded.join("js/eval.js"),
        "export function go(x) { return eval(x); }\n",
    )
    .unwrap();
    std::fs::write(
        expanded.join("src/dyn.py"),
        "def go(x):\n    return eval(x)\n",
    )
    .unwrap();

    assert!(run(&root, &["index", "--force"]).status.success());
    let exp = expanded.to_string_lossy().into_owned();
    let built = run(&root, &["index", "--force", "--macro-expanded-root", &exp]);
    assert!(built.status.success(), "{}", stderr(&built));

    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let status = parse_json(&st);
    assert_eq!(status["exists"], true, "{status}");
    assert_eq!(status["origin"], "macro_expanded", "{status}");
    let viol = status["subset_violation_count"]
        .as_u64()
        .unwrap_or_else(|| {
            panic!("macro status must expose sidecar subset_violation_count: {status}")
        });
    assert!(
        viol >= 1,
        "sidecar with unsafe/eval must report subset violations: {status}"
    );

    // Main subset must remain independent — clean source stays in_subset.
    let subset = run(&root, &["subset"]);
    assert!(subset.status.success(), "{}", stderr(&subset));
    let sj = parse_json(&subset);
    assert_eq!(
        sj["in_subset"], true,
        "sidecar S violations must not flip main subset: {sj}"
    );
}

/// `index --macro-expanded-root` without `--force` when main is already a noop
/// must still validate nesting and still write/stamp the sidecar.
#[test]
fn index_without_force_still_validates_and_writes_sidecar() {
    let base = temp_root("noop-force");
    let root = base.join("app");
    let expanded = base.join("app-expanded");
    let nested = root.join("expand-shadow");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::create_dir_all(nested.join("src")).unwrap();
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
    std::fs::write(
        nested.join("src/core.rs"),
        "pub fn helper() {}\npub fn fmt() { unsafe { helper() } }\n",
    )
    .unwrap();

    // Main already indexed.
    assert!(run(&root, &["index", "--force"]).status.success());

    // Nested expanded root without --force: still rejected, still no sidecar.
    let nested_str = nested.to_string_lossy().into_owned();
    let bad = run(&root, &["index", "--macro-expanded-root", &nested_str]);
    assert!(
        !bad.status.success(),
        "nested expanded root must reject even without --force: {}",
        stderr(&bad)
    );
    let sidecar = root.join(".agentgraph").join("index.macro.db");
    assert!(!sidecar.exists(), "rejected path must not write sidecar");

    // Valid sibling without --force (main noop): still validate + write sidecar.
    let exp = expanded.to_string_lossy().into_owned();
    let ok = run(&root, &["index", "--macro-expanded-root", &exp]);
    assert!(
        ok.status.success(),
        "sibling sidecar build without --force must work when main is noop: {}",
        stderr(&ok)
    );
    let payload = parse_json(&ok);
    assert!(
        payload["macro_sidecar"].is_object(),
        "payload must include macro_sidecar: {payload}"
    );
    assert_eq!(payload["macro_sidecar"]["origin"], "macro_expanded");
    assert!(sidecar.exists(), "sidecar file must be written");

    let st = parse_json(&run(&root, &["macro", "status"]));
    assert_eq!(st["exists"], true, "{st}");
    assert_eq!(st["origin"], "macro_expanded", "{st}");
    assert_eq!(st["expanded_root_nested"], false, "{st}");
}

/// Impact `--with-macro` for a sidecar-only **callee** must return only
/// `origin=macro_expanded` rows (independent BFS per store; no main-enclosing
/// walk). Impact walks *callers* of the seed.
#[test]
fn impact_with_macro_sidecar_only_symbol_independent_bfs() {
    let base = temp_root("impact-side");
    let root = base.join("app");
    let expanded = base.join("app-expanded");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::write(
        root.join("src/core.rs"),
        "pub fn helper() {}\npub fn process() { helper(); }\n",
    )
    .unwrap();
    // Expanded-only reverse-call chain: outer → mid → only_in_side
    // impact(only_in_side) = callers { mid, outer, … } in sidecar only.
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() {}
pub fn process() { helper(); }
pub fn only_in_side() {}
pub fn mid() { only_in_side(); }
pub fn outer() { mid(); }
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

    for depth in ["0", "1", "2"] {
        let out = run(
            &root,
            &["impact", "only_in_side", "--with-macro", "--depth", depth],
        );
        assert!(
            out.status.success(),
            "depth={depth} impact --with-macro: {}",
            stderr(&out)
        );
        let hits = parse_json(&out);
        let arr: Vec<serde_json::Value> = if let Some(a) = hits.as_array() {
            a.clone()
        } else if let Some(a) = hits.get("impact").and_then(|x| x.as_array()) {
            a.clone()
        } else {
            panic!("array or wrapped impact expected: {hits}");
        };
        for row in &arr {
            assert_eq!(
                row["origin"], "macro_expanded",
                "sidecar-only seed must not mix untagged main rows at depth={depth}: {row}"
            );
        }
        if depth == "0" {
            // depth 0 → no expansion (consistent main+sidecar contract).
            assert!(arr.is_empty(), "depth 0 must be empty: {arr:?}");
        }
        if depth == "1" {
            let enc: Vec<&str> = arr.iter().filter_map(|r| r["enclosing"].as_str()).collect();
            assert!(
                enc.contains(&"mid"),
                "depth 1 sidecar BFS must include enclosing caller mid: names={:?} enc={enc:?}",
                arr.iter()
                    .filter_map(|r| r["name"].as_str())
                    .collect::<Vec<_>>()
            );
        }
        if depth == "2" {
            let enc: Vec<&str> = arr.iter().filter_map(|r| r["enclosing"].as_str()).collect();
            assert!(
                enc.contains(&"mid"),
                "depth 2 sidecar BFS must include enclosing mid: {enc:?}"
            );
            assert!(
                enc.contains(&"outer"),
                "depth 2 sidecar BFS must include enclosing outer: {enc:?}"
            );
        }
    }

    // Default callers for sidecar-only symbol: empty (main), with_macro tagged.
    let plain = run(&root, &["callers", "only_in_side"]);
    assert!(plain.status.success(), "{}", stderr(&plain));
    let ph = parse_json(&plain);
    assert!(
        ph.as_array().map(|a| a.is_empty()).unwrap_or(false),
        "default callers must not include sidecar-only symbol: {ph}"
    );
    let with = run(&root, &["callers", "only_in_side", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    let wh = parse_json(&with);
    let wh_rows: Vec<serde_json::Value> = if let Some(a) = wh.as_array() {
        a.clone()
    } else if let Some(a) = wh.get("callers").and_then(|x| x.as_array()) {
        a.clone()
    } else {
        vec![]
    };
    let enc: Vec<String> = wh_rows
        .iter()
        .filter_map(|r| r["enclosing"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        enc.iter().any(|e| e == "mid"),
        "with_macro callers of sidecar-only callee must include mid: {enc:?}"
    );
    for row in &wh_rows {
        if row["enclosing"] == "mid" || row["enclosing"] == "outer" {
            assert_eq!(row["origin"], "macro_expanded", "{row}");
        }
    }
}

/// Deleting `index.macro.db` then querying `--with-macro` must not resurrect
/// the sidecar file (docs: absent → empty union, file not created).
#[test]
fn delete_sidecar_query_does_not_resurrect_file() {
    let base = temp_root("del-side");
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
    let exp = expanded.to_string_lossy().into_owned();
    assert!(
        run(&root, &["index", "--force", "--macro-expanded-root", &exp])
            .status
            .success()
    );
    let sidecar = root.join(".agentgraph").join("index.macro.db");
    assert!(sidecar.exists());

    std::fs::remove_file(&sidecar).expect("delete sidecar");

    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    assert!(
        !sidecar.exists(),
        "with_macro after delete must not recreate index.macro.db"
    );
    let hits = parse_json(&with);
    let enc: Vec<String> = hits
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r["enclosing"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !enc.iter().any(|e| e == "clone"),
        "deleted sidecar must not union clone: {enc:?}"
    );

    let st = parse_json(&run(&root, &["macro", "status"]));
    assert_eq!(st["exists"], false, "{st}");
    assert!(!sidecar.exists(), "macro status must not create sidecar");
}

/// MCP `callers` payload must stay schema-stable: `at` present on main rows
/// whether or not `with_macro` is set (CLI always emits `at`; with_macro path
/// already does). Consumers must not see field shape flip on the flag.
#[test]
fn mcp_callers_at_field_stable_with_and_without_macro() {
    use std::io::Write;
    let base = temp_root("mcp-at");
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
    let exp = expanded.to_string_lossy().into_owned();
    assert!(
        run(&root, &["index", "--force", "--macro-expanded-root", &exp])
            .status
            .success()
    );

    let script = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"callers","arguments":{"name":"helper"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"callers","arguments":{"name":"helper","with_macro":true}}}"#,
        "\n",
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
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(script.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("mcp out");
    let text = stdout(&out);
    assert!(out.status.success(), "stderr={}", stderr(&out));

    // Parse each tools/call result payload from the JSON-RPC stream.
    let mut default_payload: Option<serde_json::Value> = None;
    let mut macro_payload: Option<serde_json::Value> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let id = v["id"].as_i64().unwrap_or(0);
        let content = &v["result"]["content"][0]["text"];
        let Some(s) = content.as_str() else { continue };
        let parsed: serde_json::Value = serde_json::from_str(s).unwrap_or(serde_json::Value::Null);
        if id == 2 {
            default_payload = Some(parsed.clone());
        }
        if id == 3 {
            macro_payload = Some(parsed);
        }
    }
    let default_payload =
        default_payload.unwrap_or_else(|| panic!("no default callers payload in {text}"));
    let macro_payload =
        macro_payload.unwrap_or_else(|| panic!("no with_macro callers payload in {text}"));

    let darr = default_payload
        .as_array()
        .unwrap_or_else(|| panic!("default callers must be array: {default_payload}"));
    assert!(!darr.is_empty(), "expected main callers: {default_payload}");
    for row in darr {
        assert!(
            row["at"].is_string(),
            "MCP callers default rows must include CLI-stable `at`: {row}"
        );
        assert!(
            row.get("origin").is_none(),
            "main rows must not carry origin: {row}"
        );
    }

    let marr: Vec<serde_json::Value> = if let Some(a) = macro_payload.as_array() {
        a.clone()
    } else if let Some(a) = macro_payload.get("callers").and_then(|x| x.as_array()) {
        a.clone()
    } else {
        panic!("with_macro callers must be array or wrapped object: {macro_payload}");
    };
    let mut saw_side = false;
    for row in &marr {
        assert!(
            row["at"].is_string(),
            "MCP callers with_macro rows must include `at`: {row}"
        );
        if row["origin"] == "macro_expanded" {
            saw_side = true;
        }
    }
    assert!(
        saw_side,
        "with_macro must include tagged sidecar rows: {macro_payload}"
    );
}

/// Inventory `use inventory::submit; submit!(...)` must mint the same
/// sound-allowlisted `rs.di.inventory_submit` edges as the qualified form.
/// Stock corpus uses qualified form; alias is still a sound-allowlist hole
/// for other production Rust DI crates.
#[test]
fn inventory_use_alias_submit_mints_sound_eligible_edges() {
    let src = r#"
use inventory::submit;

struct StrategyRegistration {
    factory: fn(),
}
struct Plugin;
impl Plugin {
    fn new() -> Self { Self }
}

submit! {
    StrategyRegistration {
        factory: || Plugin::new(),
    }
}
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "StrategyRegistration"),
        "aliased submit! must mint registration type: {hits:?}"
    );
    assert!(
        hits.iter().any(|n| n == "Plugin"),
        "aliased submit! must mint factory type: {hits:?}"
    );
}

/// Rename form: `use inventory::submit as inv_submit; inv_submit!(...)`.
#[test]
fn inventory_use_rename_alias_mints_edges() {
    let src = r#"
use inventory::submit as inv_submit;

struct Reg;
struct T;
impl T { fn new() -> Self { Self } }

inv_submit! {
    Reg { factory: || T::new() }
}
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "rename alias inv_submit! must mint Reg: {hits:?}"
    );
    // Rename: bare `submit` is not in scope — a bare submit! elsewhere without
    // the alias name must not fire just because the use exists.
    let bare = r#"
use inventory::submit as inv_submit;
struct Reg;
submit! { Reg { } }
"#;
    let bare_out = extract(bare);
    let bare_hits = rule_hits(&bare_out, "rs.di.inventory_submit");
    assert!(
        bare_hits.is_empty(),
        "bare submit! after rename-only import must not mint: {bare_hits:?}"
    );
}

/// Brace-import form: `use inventory::{submit}; submit!(...)`.
#[test]
fn inventory_use_brace_import_mints_edges() {
    let src = r#"
use inventory::{submit};

struct Reg;
submit! { Reg { } }
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "brace import submit! must mint Reg: {hits:?}"
    );
}

/// Crate-relative re-export path: `crate::inventory::submit!`.
#[test]
fn inventory_crate_relative_path_mints_edges() {
    let src = r#"
struct Reg;
crate::inventory::submit! { Reg { } }
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg"),
        "crate::inventory::submit! must mint Reg: {hits:?}"
    );
}

/// `foo::inventory::submit!` (unrelated crate with a module named inventory)
/// must not mint sound-eligible inventory edges. Exact last-two-segment match
/// is not enough when the prefix is an unrelated crate path.
#[test]
fn inventory_foreign_crate_inventory_module_rejected() {
    let src = r#"
struct Foo;
evil::inventory::submit! { Foo { } }
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "unrelated `*::inventory::submit` must not mint sound-eligible edges: {hits:?}"
    );
}

/// cfg(test)-only inventory submit is still an over-approx (allowed for sound)
/// but must not claim Exact confidence.
#[test]
fn inventory_cfg_test_submit_stays_heuristic() {
    let src = r#"
struct Reg;
struct T;
impl T { fn new() -> Self { Self } }

#[cfg(test)]
mod tests {
    use super::*;
    inventory::submit! {
        Reg { factory: || T::new() }
    }
}
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.iter().any(|n| n == "Reg" || n == "T"),
        "cfg(test) inventory still extractable (over-approx): {hits:?}"
    );
    for r in &out.references {
        if r.evidence
            .as_ref()
            .map(|e| e.rule_id == "rs.di.inventory_submit")
            .unwrap_or(false)
        {
            assert_eq!(
                r.confidence,
                agentgraph::model::Confidence::Heuristic,
                "inventory edges must stay Heuristic"
            );
        }
    }
}

/// Bare `submit!` with no `use inventory::submit` must not mint sound-eligible
/// edges (any crate can define a `submit!` macro).
#[test]
fn inventory_bare_submit_without_import_rejected() {
    let src = r#"
struct Reg;
submit! { Reg { } }
"#;
    let out = extract(src);
    let hits = rule_hits(&out, "rs.di.inventory_submit");
    assert!(
        hits.is_empty(),
        "bare submit! without inventory import must not mint: {hits:?}"
    );
}

/// Stock corpus smoke (optional): `macro status` on the operator stock root
/// when sidecar was never built → exists:false, no file created.
#[test]
fn stock_corpus_macro_status_absent_when_no_sidecar() {
    let stock = Path::new(r"D:\projects\eval-corpus\stock-trading-app");
    if !stock.exists() {
        eprintln!("skip: stock corpus not present");
        return;
    }
    let sidecar = stock.join(".agentgraph").join("index.macro.db");
    if sidecar.exists() {
        eprintln!("skip: stock sidecar already built");
        return;
    }
    let st = run(stock, &["macro", "status"]);
    // Command may still succeed even if main index.db is missing — status
    // must not require ensure_indexed and must not create the sidecar.
    assert!(st.status.success(), "{}", stderr(&st));
    let status = parse_json(&st);
    assert_eq!(status["exists"], false, "{status}");
    assert!(
        !sidecar.exists(),
        "macro status on stock root must not create sidecar"
    );
}
