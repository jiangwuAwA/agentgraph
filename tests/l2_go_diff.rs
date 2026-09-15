//! L2 Go differential: `go test -coverprofile` executed functions ⊆ sound graph.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agentgraph"))
}

fn fixture_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/eval-l2/s-go-auth")
}

fn copy_to_temp(tag: &str) -> PathBuf {
    let src = fixture_src();
    let dst = std::env::temp_dir().join(format!("agentgraph-l2-go-{tag}"));
    let _ = std::fs::remove_dir_all(&dst);
    std::fs::create_dir_all(&dst).unwrap();
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        let _ = std::fs::copy(e.path(), dst.join(e.file_name()));
    }
    dst
}

fn which_go() -> Option<PathBuf> {
    let out = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "where", "go"])
            .output()
            .ok()?
    } else {
        Command::new("which").arg("go").output().ok()?
    };
    if !out.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if first.is_empty() {
        None
    } else {
        Some(PathBuf::from(first))
    }
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

fn covered_func_names(go: &Path, root: &Path) -> Vec<String> {
    let profile = root.join("c.out");
    let test = Command::new(go)
        .arg("test")
        .arg("-coverprofile")
        .arg(&profile)
        .arg("-covermode=set")
        .current_dir(root)
        .output()
        .expect("go test");
    if !test.status.success() {
        panic!("go test failed: {}", String::from_utf8_lossy(&test.stderr));
    }
    let func = Command::new(go)
        .arg("tool")
        .arg("cover")
        .arg("-func")
        .arg(&profile)
        .current_dir(root)
        .output()
        .expect("go tool cover");
    let text = String::from_utf8_lossy(&func.stdout);
    let mut names = Vec::new();
    for line in text.lines() {
        // e.g. .../auth.go:7:		validateEmail		100.0%
        let line = line.trim();
        if line.is_empty() || line.starts_with("total:") {
            continue;
        }
        if let Some(rest) = line.split('\t').next_back() {
            let name = rest.split_whitespace().next().unwrap_or("");
            // name may be pkg.Func or Func
            let bare = name.rsplit('.').next().unwrap_or(name);
            if !bare.is_empty() && !bare.contains('%') {
                names.push(bare.to_string());
            }
        }
        // also parse middle column more robustly
        let cols: Vec<&str> = line
            .split('\t')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if cols.len() >= 2 {
            let bare = cols[1].rsplit('.').next().unwrap_or(cols[1]);
            if bare.chars().all(|c| c.is_alphanumeric() || c == '_') && !bare.is_empty() {
                names.push(bare.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

#[test]
fn go_cover_funcs_subset_of_sound_graph() {
    let Some(go) = which_go() else {
        eprintln!("skip: go toolchain not on PATH");
        return;
    };
    let root = copy_to_temp(&format!("{}", std::process::id()));
    // Need a module for go test
    let init = Command::new(&go)
        .args(["mod", "init", "sgo"])
        .current_dir(&root)
        .output()
        .expect("go mod init");
    if !init.status.success() {
        eprintln!(
            "go mod init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
    }

    let funcs = covered_func_names(&go, &root);
    assert!(
        funcs.iter().any(|f| f == "Authenticate"),
        "cover must see Authenticate; got {funcs:?}"
    );

    let (ok, _, err) = run_ag(&root, &["index"]);
    assert!(ok, "index: {err}");
    let (ok, stdout, err) = run_ag(&root, &["subset"]);
    assert!(ok, "subset: {err}");
    let v: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["in_subset"], true, "{stdout}");

    for f in &funcs {
        if f == "Test" || f.starts_with("Test") {
            continue;
        }
        let (ok, stdout, err) = run_ag(&root, &["callers", f, "--sound", "--limit", "50"]);
        // Entry points may have zero callers — containment is about *callers* of
        // executed callees existing in the sound window when they have call sites.
        assert!(ok, "callers --sound {f}: {err}");
        let cv: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(cv["subset_ok"], true, "{stdout}");
        let hits = cv["callers"].as_array().cloned().unwrap_or_default();
        // If the function is called from another covered function, at least one
        // sound caller must exist OR it is an entry (Authenticate from Test).
        let is_entry = f == "Authenticate";
        if !is_entry {
            assert!(
                !hits.is_empty(),
                "covered callee {f} must have sound callers or be documented entry; hits={hits:?}"
            );
        }
    }

    // Stronger: Authenticate's sound callers must include Test (or empty if
    // Test is outside package graph). impact Authenticate non-empty when
    // validateEmail is called from Authenticate — check validateEmail callers.
    let (ok, stdout, err) = run_ag(
        &root,
        &["impact", "validateEmail", "--sound", "--depth", "2"],
    );
    assert!(ok, "{err}");
    let iv: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(iv["subset_ok"], true);
    let nodes = iv["impact"].as_array().cloned().unwrap_or_default();
    assert!(
        nodes.iter().any(|n| n["enclosing"]
            .as_str()
            .map(|e| e.contains("Authenticate"))
            .unwrap_or(false)
            || n["name"].as_str() == Some("Authenticate")),
        "impact(validateEmail)--sound must include Authenticate; {nodes:?}"
    );
}
