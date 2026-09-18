//! Track M4-W: multi-root workspace indexing (single SQLite store + root_id).
//!
//! TDD contract — see docs/workspace.md.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-ws-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_root_a(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
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

fn write_root_b(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    // Same relative path + same symbol names as root A (collision case).
    std::fs::write(
        root.join("src/auth.ts"),
        r#"
export function validateEmail(email: string): boolean {
  return email.endsWith(".com");
}
export function createUser(email: string) {
  validateEmail(email);
  return { email, source: "b" };
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/unique_b.ts"),
        r#"
export function onlyInB(): number {
  return 42;
}
"#,
    )
    .unwrap();
}

fn write_root_c_py(root: &Path) {
    std::fs::create_dir_all(root.join("pkg")).unwrap();
    std::fs::write(
        root.join("pkg/service.py"),
        r#"
def validate_email(email: str) -> bool:
    return "@" in email

def create_user(email: str):
    if not validate_email(email):
        raise ValueError("bad")
    return {"email": email}
"#,
    )
    .unwrap();
}

fn run_raw(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn run_in(root: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(bin());
    cmd.arg("--root").arg(root).args(args);
    cmd.stdin(Stdio::null()).output().expect("run agentgraph")
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
            "invalid JSON: {e}\nstdout={}\nstderr={}",
            stdout(out),
            stderr(out)
        )
    })
}

fn write_manifest(path: &Path, roots: &[(&str, &Path)]) {
    let entries: Vec<serde_json::Value> = roots
        .iter()
        .map(|(id, p)| {
            serde_json::json!({
                "id": id,
                "path": p.to_string_lossy(),
            })
        })
        .collect();
    let doc = serde_json::json!({ "roots": entries });
    std::fs::write(path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
}

/// 1 + 2: two roots → workspace index → per-root find + union collision rows.
#[test]
fn workspace_two_roots_find_filter_and_union() {
    let base = temp_dir("two-roots");
    let root_a = base.join("api");
    let root_b = base.join("web");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    write_root_a(&root_a);
    write_root_b(&root_b);

    let db = base.join("ws.db");
    let idx = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(idx.status.success(), "index stderr={}", stderr(&idx));
    let stats = parse_json(&idx);
    assert!(stats["files"].as_u64().unwrap() >= 4, "stats={stats}");
    assert!(
        stats["roots"].as_array().map(|a| a.len()).unwrap_or(0) >= 2,
        "expected per-root breakdown: {stats}"
    );

    // Per-root filter: onlyInB exists only in web.
    let find_b = run_raw(&[
        "find",
        "onlyInB",
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(find_b.status.success(), "{}", stderr(&find_b));
    let hits_b = parse_json(&find_b);
    let arr_b = hits_b.as_array().expect("find returns array");
    assert!(!arr_b.is_empty());
    assert!(arr_b.iter().all(|h| h["root_id"] == "web"), "{hits_b}");
    assert!(
        arr_b.iter().any(|h| h["path"] == "src/unique_b.ts"),
        "{hits_b}"
    );

    // Per-root filter: onlyInB must NOT appear when scoped to api.
    let find_a = run_raw(&[
        "find",
        "onlyInB",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(
        !find_a.status.success(),
        "onlyInB must not resolve under api root"
    );

    // Symbol only in A (api has loginHandler).
    let find_login = run_raw(&[
        "find",
        "loginHandler",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(find_login.status.success(), "{}", stderr(&find_login));
    let login_hits = parse_json(&find_login);
    assert!(login_hits
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["name"] == "loginHandler" && h["root_id"] == "api"));

    // Union: same symbol name in both roots → two rows with distinct root_id/path pairing.
    let union = run_raw(&[
        "find",
        "createUser",
        "--workspace-db",
        db.to_str().unwrap(),
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-root",
        root_b.to_str().unwrap(),
    ]);
    // With both roots selected, union of selected roots.
    assert!(union.status.success(), "{}", stderr(&union));
    let union_hits = parse_json(&union);
    let arr = union_hits.as_array().unwrap();
    let root_ids: Vec<&str> = arr
        .iter()
        .filter(|h| h["name"] == "createUser")
        .filter_map(|h| h["root_id"].as_str())
        .collect();
    assert!(
        root_ids.contains(&"api") && root_ids.contains(&"web"),
        "expected both roots in union: {union_hits}"
    );
    // Same relative path may appear under both roots — distinguished by root_id.
    let api_row = arr
        .iter()
        .find(|h| h["name"] == "createUser" && h["root_id"] == "api")
        .unwrap();
    let web_row = arr
        .iter()
        .find(|h| h["name"] == "createUser" && h["root_id"] == "web")
        .unwrap();
    assert_eq!(api_row["path"], "src/auth.ts");
    assert_eq!(web_row["path"], "src/auth.ts");
    assert_ne!(api_row["root_id"], web_row["root_id"]);

    // Unfiltered union across entire workspace DB (no --workspace-root): both tagged.
    let all = run_raw(&["find", "createUser", "--workspace-db", db.to_str().unwrap()]);
    assert!(all.status.success(), "{}", stderr(&all));
    let all_hits = parse_json(&all);
    let all_roots: Vec<&str> = all_hits
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["name"] == "createUser")
        .filter_map(|h| h["root_id"].as_str())
        .collect();
    assert!(
        all_roots.contains(&"api") && all_roots.contains(&"web"),
        "union without filter must tag root_id: {all_hits}"
    );

    // workspace status
    let status = run_raw(&[
        "workspace",
        "status",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let st = parse_json(&status);
    let roots = st["roots"].as_array().expect("status.roots");
    assert!(roots.len() >= 2, "{st}");
    assert!(roots.iter().any(|r| r["id"] == "api"));
    assert!(roots.iter().any(|r| r["id"] == "web"));
}

/// Manifest form + mixed languages.
#[test]
fn workspace_manifest_mixed_langs() {
    let base = temp_dir("manifest");
    let root_a = base.join("api");
    let root_py = base.join("svc");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_py).unwrap();
    write_root_a(&root_a);
    write_root_c_py(&root_py);
    let manifest = base.join("workspace.json");
    write_manifest(&manifest, &[("api", &root_a), ("svc", &root_py)]);

    let idx = run_raw(&[
        "index",
        "--workspace",
        manifest.to_str().unwrap(),
        "--force",
    ]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    // Default DB = manifest sibling `.agentgraph/index.db`
    let db = base.join(".agentgraph").join("index.db");
    assert!(db.exists(), "expected workspace db at {}", db.display());

    let find_py = run_raw(&[
        "find",
        "create_user",
        "--workspace",
        manifest.to_str().unwrap(),
        "--workspace-root",
        root_py.to_str().unwrap(),
    ]);
    assert!(find_py.status.success(), "{}", stderr(&find_py));
    let hits = parse_json(&find_py);
    assert!(hits
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["name"] == "create_user" && h["root_id"] == "svc"));
}

/// 3: single-root CLI unchanged — rows need not expose root_id.
#[test]
fn single_root_cli_unchanged() {
    let root = temp_dir("single-root");
    write_root_a(&root);
    let idx = run_in(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let stats = parse_json(&idx);
    assert!(stats["symbols"].as_u64().unwrap() >= 3);
    // Single-root payload does not require workspace fields.
    assert!(stats.get("roots").is_none() || stats["roots"].is_null());

    let find = run_in(&root, &["find", "createUser"]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits = parse_json(&find);
    let arr = hits.as_array().unwrap();
    assert!(arr.iter().any(|h| h["name"] == "createUser"));
    // root_id absent or empty for classic single-root store.
    for h in arr {
        let rid = h.get("root_id").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            rid.is_empty(),
            "single-root rows must not invent root_id: {h}"
        );
    }

    let callers = run_in(&root, &["callers", "validateEmail"]);
    assert!(callers.status.success());
    let refs = parse_json(&callers);
    for r in refs.as_array().unwrap() {
        let rid = r.get("root_id").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            rid.is_empty(),
            "single-root callers must stay untagged: {r}"
        );
    }
}

/// 4: duplicate workspace root path → fail-loud.
#[test]
fn workspace_duplicate_root_path_fails() {
    let base = temp_dir("dup-root");
    let root_a = base.join("api");
    std::fs::create_dir_all(&root_a).unwrap();
    write_root_a(&root_a);
    let db = base.join("ws.db");
    let out = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(!out.status.success(), "duplicate root must fail");
    let err = stderr(&out).to_lowercase();
    assert!(
        err.contains("duplicate") || err.contains("same path"),
        "stderr={err}"
    );
}

/// 5: single-root then workspace on the same DB — no crash; migration defaults root_id.
#[test]
fn workspace_migration_from_single_root_db() {
    let base = temp_dir("migrate");
    let root_a = base.join("api");
    let root_b = base.join("web");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    write_root_a(&root_a);
    write_root_b(&root_b);

    // Classic single-root index into root_a's default store.
    let single = run_in(&root_a, &["index", "--force"]);
    assert!(single.status.success(), "{}", stderr(&single));
    let db = root_a.join(".agentgraph").join("index.db");
    assert!(db.exists());

    // Workspace index reuses that DB (first root = api).
    let idx = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(
        idx.status.success(),
        "migration index stderr={}",
        stderr(&idx)
    );

    // Queries still work; workspace rows carry root_id.
    let find = run_raw(&[
        "find",
        "onlyInB",
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits = parse_json(&find);
    assert!(hits
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["root_id"] == "web"));

    // Union still works after migration.
    let union = run_raw(&["find", "createUser", "--workspace-db", db.to_str().unwrap()]);
    assert!(union.status.success(), "{}", stderr(&union));
}

/// 6: subset / sound per-root filter + workspace status per-root counts.
#[test]
fn workspace_subset_sound_per_root() {
    let base = temp_dir("subset");
    let root_a = base.join("clean");
    let root_b = base.join("dirty");
    std::fs::create_dir_all(root_a.join("src")).unwrap();
    std::fs::create_dir_all(root_b.join("src")).unwrap();
    std::fs::write(
        root_a.join("src/ok.ts"),
        "export function ok(x: number): number { return x + 1; }\n",
    )
    .unwrap();
    std::fs::write(
        root_b.join("src/bad.ts"),
        r#"
export function bad(code: string): unknown {
  return eval(code);
}
"#,
    )
    .unwrap();

    let db = base.join("ws.db");
    let idx = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    // Per-root subset: clean root has no violations.
    let subset_clean = run_raw(&[
        "subset",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(subset_clean.status.success(), "{}", stderr(&subset_clean));
    let sc = parse_json(&subset_clean);
    assert_eq!(sc["in_subset"], true, "{sc}");
    assert_eq!(sc["violation_count"], 0, "{sc}");

    // Dirty root fails S.
    let subset_dirty = run_raw(&[
        "subset",
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(!subset_dirty.status.success(), "dirty root must fail S");
    let sd = parse_json(&subset_dirty);
    assert_eq!(sd["in_subset"], false, "{sd}");
    assert!(sd["violation_count"].as_u64().unwrap() >= 1, "{sd}");

    // Sound scoped to clean root: subset_ok true.
    let sound_clean = run_raw(&[
        "callers",
        "ok",
        "--sound",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    // No callers of ok — may fail "no symbol" or return empty sound payload.
    if sound_clean.status.success() {
        let payload = parse_json(&sound_clean);
        if payload.get("mode").and_then(|v| v.as_str()) == Some("sound") {
            assert_eq!(payload["subset_ok"], true, "{payload}");
        }
    }

    // Sound union (no root filter) must be weakest-root: subset_ok false.
    let sound_all = run_raw(&["subset", "--workspace-db", db.to_str().unwrap()]);
    assert!(
        !sound_all.status.success(),
        "union subset must fail when any root violates"
    );
    let sa = parse_json(&sound_all);
    assert_eq!(sa["in_subset"], false, "{sa}");

    // Per-root counts in workspace status / stats.
    let status = run_raw(&[
        "workspace",
        "status",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let st = parse_json(&status);
    let clean = st["roots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "clean")
        .expect("clean root in status");
    let dirty = st["roots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "dirty")
        .expect("dirty root in status");
    assert!(clean["files"].as_u64().unwrap() >= 1);
    assert!(dirty["subset_violations"].as_u64().unwrap() >= 1);
}

/// Manifest simple array form + basename ids.
#[test]
fn workspace_manifest_simple_array() {
    let base = temp_dir("manifest-array");
    let root_a = base.join("api");
    let root_b = base.join("web");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    write_root_a(&root_a);
    write_root_b(&root_b);
    let manifest = base.join("ws.json");
    let doc = serde_json::json!([root_a.to_string_lossy(), root_b.to_string_lossy(),]);
    std::fs::write(&manifest, doc.to_string()).unwrap();

    let idx = run_raw(&[
        "index",
        "--workspace",
        manifest.to_str().unwrap(),
        "--force",
    ]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    // Basename ids: api / web
    let find = run_raw(&[
        "find",
        "onlyInB",
        "--workspace",
        manifest.to_str().unwrap(),
        "--workspace-root",
        root_b.to_str().unwrap(),
    ]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits = parse_json(&find);
    assert!(hits
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["root_id"] == "web"));
}

/// Nested roots: allow + warn (not hard-reject); duplicate still rejected.
#[test]
fn workspace_nested_roots_warn_not_reject() {
    let base = temp_dir("nested");
    let parent = base.join("mono");
    let child = parent.join("packages/api");
    std::fs::create_dir_all(&child).unwrap();
    write_root_a(&parent);
    write_root_b(&child);
    let db = base.join("ws.db");
    let out = run_raw(&[
        "index",
        "--workspace-root",
        parent.to_str().unwrap(),
        "--workspace-root",
        child.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(
        out.status.success(),
        "nested roots should index with a warning, not hard-fail: {}",
        stderr(&out)
    );
    let err = stderr(&out).to_lowercase();
    assert!(
        err.contains("nest") || err.contains("overlap"),
        "expected nesting warning: {err}"
    );
}
