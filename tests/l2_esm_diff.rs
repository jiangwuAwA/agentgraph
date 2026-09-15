//! TDD: multi-file ESM S_js corpus — sound graph must contain the export call chain.
//! M4: each test copies the fixture to a unique temp dir (no shared `.agentgraph` race).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agentgraph"))
}

fn fixture_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/eval-l2/s-js-esm")
}

fn unique_tag() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{}-{n}", std::process::id())
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        // Never copy a pre-existing index DB into the isolated fixture.
        if name == ".agentgraph" {
            continue;
        }
        let t = dst.join(&name);
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            let _ = std::fs::copy(e.path(), &t);
        }
    }
}

fn isolated_fixture() -> PathBuf {
    let src = fixture_src();
    let dst = std::env::temp_dir().join(format!("agentgraph-l2-esm-{}", unique_tag()));
    let _ = std::fs::remove_dir_all(&dst);
    copy_dir(&src, &dst);
    dst
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
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Golden export-level runtime chain for this fixture (from source):
/// main → loginHandler → authenticate → {validateEmail, hashPassword} → normalize
fn golden_chain() -> Vec<(String, String)> {
    vec![
        ("main".into(), "loginHandler".into()),
        ("loginHandler".into(), "authenticate".into()),
        ("authenticate".into(), "validateEmail".into()),
        ("authenticate".into(), "hashPassword".into()),
        ("validateEmail".into(), "normalize".into()),
    ]
}

fn enclosing_of(e: &Value) -> Option<&str> {
    e.get("enclosing").and_then(|n| n.as_str())
}

#[test]
fn multi_file_esm_in_subset_and_sound_contains_chain() {
    let root = isolated_fixture();
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");

    let (ok, stdout, err) = run(&root, &["subset"]);
    assert!(ok, "subset failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("subset json");
    assert_eq!(v["in_subset"], true, "s-js-esm must be in S: {stdout}");

    for (from, to) in golden_chain() {
        let (ok, stdout, err) = run(&root, &["callers", &to, "--sound", "--limit", "100"]);
        assert!(ok, "callers --sound {to}: {err}");
        let v: Value = serde_json::from_str(&stdout).expect("callers json");
        assert_eq!(
            v["subset_ok"], true,
            "esm corpus must stay subset_ok for {to}: {stdout}"
        );
        let hits = v["callers"].as_array().cloned().unwrap_or_default();
        let found = hits.iter().any(|h| {
            enclosing_of(h)
                .map(|e| e == from || e.ends_with(&from))
                .unwrap_or(false)
        });
        assert!(
            found,
            "sound callers({to}) must include enclosing={from}; hits={hits:?}"
        );
    }

    // impact --sound from authenticate must reach validateEmail's caller sites
    // and expand through enclosing (loginHandler / authenticate).
    let (ok, stdout, err) = run(
        &root,
        &[
            "impact",
            "normalize",
            "--sound",
            "--depth",
            "4",
            "--limit",
            "200",
        ],
    );
    assert!(ok, "impact --sound: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("impact json");
    assert_eq!(v["subset_ok"], true, "{stdout}");
    let nodes = v["impact"].as_array().cloned().unwrap_or_default();
    let names: Vec<String> = nodes
        .iter()
        .filter_map(|n| {
            n.get("name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    assert!(
        names
            .iter()
            .any(|n| n == "normalize" || n == "validateEmail"),
        "impact(normalize)--sound should surface the chain; names={names:?}"
    );
}

#[test]
fn multi_file_esm_imports_resolve_across_modules() {
    let root = isolated_fixture();
    let (ok, _, err) = run(&root, &["index"]);
    assert!(ok, "index failed: {err}");
    let (ok, stdout, err) = run(&root, &["importers", "src/util.js"]);
    assert!(ok, "importers failed: {err}");
    let v: Value = serde_json::from_str(&stdout).expect("importers json");
    let arr = v.as_array().cloned().unwrap_or_default();
    assert!(
        arr.iter().any(|r| {
            r.get("path")
                .and_then(|p| p.as_str())
                .map(|p| p.ends_with("auth.js"))
                .unwrap_or(false)
        }),
        "auth.js must import util.js; got {stdout}"
    );
}
