//! L2 Python differential: runtime call edges ⊆ sound walk on S_py fixture.
//!
//! Spec: `docs/product-boundary-migration.md` § Track M2 (`tests/l2_py_diff.rs`).
//! Tracer: `scripts/py_trace.py` (pure Python `sys.setprofile`).
//! Honest skip when no Python interpreter is on PATH.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agentgraph"))
}

fn fixture_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/eval-l2/s-py-auth")
}

fn tracer_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/py_trace.py")
}

fn unique_tag() -> String {
    common::unique_tag()
}

fn copy_dir(src: &Path, dst: &Path) {
    common::copy_dir(src, dst)
}

fn copy_to_temp(tag: &str) -> PathBuf {
    common::copy_fixture_to_temp(&fixture_src(), &format!("agentgraph-l2-py-{tag}"))
}

fn which_python() -> Option<String> {
    for cand in ["python", "python3", "py"] {
        let out = Command::new(cand).arg("--version").output().ok()?;
        if out.status.success() {
            return Some(cand.to_string());
        }
    }
    None
}

fn run_ag(root: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("agentgraph");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn sound_name_set(root: &Path, entry: &str) -> (Value, Vec<String>) {
    let (ok, stdout, err) = run_ag(
        root,
        &["impact", entry, "--sound", "--depth", "5", "--limit", "200"],
    );
    assert!(ok, "impact --sound failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("impact sound json");
    let mut names = Vec::new();
    if let Some(arr) = v["impact"].as_array() {
        for n in arr {
            if let Some(name) = n["name"].as_str() {
                names.push(name.to_string());
            }
            if let Some(enc) = n["enclosing"].as_str() {
                names.push(enc.to_string());
            }
        }
    }
    if let Some(arr) = v["callers"].as_array() {
        for n in arr {
            if let Some(name) = n["name"].as_str() {
                names.push(name.to_string());
            }
            if let Some(enc) = n["enclosing"].as_str() {
                names.push(enc.to_string());
            }
        }
    }
    // Also collect any nodes under nested keys the CLI may use.
    collect_names(&v, &mut names);
    names.sort();
    names.dedup();
    (v, names)
}

fn collect_names(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            if let Some(name) = map.get("name").and_then(|x| x.as_str()) {
                out.push(name.to_string());
            }
            if let Some(enc) = map.get("enclosing").and_then(|x| x.as_str()) {
                out.push(enc.to_string());
            }
            for (_, child) in map {
                collect_names(child, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                collect_names(child, out);
            }
        }
        _ => {}
    }
}

#[test]
fn py_clean_fixture_stays_in_s() {
    let root = copy_to_temp(&unique_tag());
    let (ok, _, err) = run_ag(&root, &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run_ag(&root, &["subset"]);
    assert!(ok, "subset failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("subset json");
    assert_eq!(
        v["in_subset"], true,
        "s-py-auth must stay in S: {stdout} / {err}"
    );
    assert_eq!(v["promise_tier"], "ast_modeled", "{stdout}");
}

#[test]
fn py_eval_fixture_leaves_s_disabled_promise() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/eval-l2/s-py-evil");
    let dst = common::temp_root(&format!("agentgraph-l2-py-evil-{}", unique_tag()));
    copy_dir(&src, &dst);
    let (ok, _, err) = run_ag(&dst, &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, _) = run_ag(&dst, &["subset"]);
    assert!(!ok, "subset must exit non-zero on eval corpus");
    let v: Value = serde_json::from_str(&stdout).expect("subset json");
    assert_eq!(v["in_subset"], false, "{stdout}");
    assert_eq!(v["promise_tier"], "disabled", "{stdout}");
}

#[test]
fn py_runtime_edges_subset_of_sound_impact() {
    let Some(python) = which_python() else {
        eprintln!("skip: python not on PATH (l2_py_diff differential)");
        return;
    };
    let tracer = tracer_script();
    assert!(tracer.is_file(), "missing tracer {}", tracer.display());

    let root = copy_to_temp(&unique_tag());
    let (ok, _, err) = run_ag(&root, &["index", "--force"]);
    assert!(ok, "index failed: {err}");

    let mod_path = root.join("src/auth.py");
    let out = Command::new(&python)
        .arg(&tracer)
        .arg(&mod_path)
        .arg("main")
        .output()
        .expect("run py tracer");
    assert!(
        out.status.success(),
        "tracer failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let trace: Value = serde_json::from_slice(&out.stdout).expect("trace json");
    let edges = trace["edges"].as_array().expect("edges array");
    assert!(
        !edges.is_empty(),
        "tracer must observe runtime edges: {trace}"
    );

    // Expect the documented chain at least: main → login_handler → authenticate
    let has_main_login = edges
        .iter()
        .any(|e| e["from"].as_str() == Some("main") && e["to"].as_str() == Some("login_handler"));
    assert!(
        has_main_login,
        "expected main->login_handler runtime edge: {trace}"
    );

    let (payload, names) = sound_name_set(&root, "main");
    assert_eq!(
        payload["subset_ok"], true,
        "S_py fixture must be in S: {payload}"
    );
    assert_eq!(payload["promise_tier"], "ast_modeled", "{payload}");

    // Impact/callers --sound of callees must contain each runtime `from` caller
    // (same containment class as the Node/Go differentials).
    for e in edges {
        let from = e["from"].as_str().unwrap_or("");
        let to = e["to"].as_str().unwrap_or("");
        if from.is_empty() || to.is_empty() {
            continue;
        }
        let (ok, stdout, err) = run_ag(
            &root,
            &["impact", to, "--sound", "--depth", "3", "--limit", "100"],
        );
        assert!(ok, "impact {to} --sound failed: {err}");
        let iv: Value = serde_json::from_str(&stdout).expect("impact json");
        let mut impact_names = Vec::new();
        collect_names(&iv, &mut impact_names);
        impact_names.sort();
        impact_names.dedup();

        let found = impact_names.iter().any(|n| n == from)
            || iv["impact"]
                .as_array()
                .map(|arr| {
                    arr.iter().any(|n| {
                        n["name"].as_str() == Some(from) || n["enclosing"].as_str() == Some(from)
                    })
                })
                .unwrap_or(false);

        // Also accept callers --sound of `to` containing `from`.
        let found_callers = if found {
            true
        } else {
            let (ok, stdout, err) = run_ag(&root, &["callers", to, "--sound", "--limit", "100"]);
            if !ok {
                eprintln!("callers {to} --sound failed: {err}");
                false
            } else {
                let cv: Value = serde_json::from_str(&stdout).expect("callers json");
                let mut cnames = Vec::new();
                collect_names(&cv, &mut cnames);
                cnames.iter().any(|n| n == from)
                    || cv["callers"]
                        .as_array()
                        .map(|arr| {
                            arr.iter().any(|n| {
                                n["name"].as_str() == Some(from)
                                    || n["enclosing"].as_str() == Some(from)
                            })
                        })
                        .unwrap_or(false)
            }
        };

        assert!(
            found_callers,
            "runtime edge {from}->{to} must be in --sound impact/callers; impact names={impact_names:?}; payload keys ok"
        );
        let _ = &names;
    }
}

#[test]
fn py_subset_json_fields_stable() {
    let root = copy_to_temp(&unique_tag());
    let (ok, _, err) = run_ag(&root, &["index", "--force"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run_ag(&root, &["subset"]);
    assert!(ok, "subset failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("subset json");
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
            "subset JSON missing `{key}`: {stdout}"
        );
    }
    assert!(v["violations"].is_array(), "{stdout}");
    assert!(v["promise_languages"].is_array(), "{stdout}");
    let langs = v["promise_languages"].as_array().unwrap();
    assert!(
        langs.iter().any(|l| l.as_str() == Some("python")),
        "promise_languages must include python: {stdout}"
    );
}
