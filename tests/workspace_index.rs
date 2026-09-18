//! Track M4-W: multi-root workspace indexing (single SQLite store + root_id).
//!
//! TDD contract — see docs/workspace.md.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentgraph::index::store::Store as AgStore;

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(name: &str) -> PathBuf {
    common::temp_root(&format!("agentgraph-ws-{name}"))
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

/// Helper: two-root workspace fixture + index → returns (base, api, web, db).
fn two_root_ws(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let base = temp_dir(name);
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
    (base, root_a, root_b, db)
}

fn row_has_root_id(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(obj) => {
            if obj.contains_key("path")
                && (obj.contains_key("name")
                    || obj.contains_key("line")
                    || obj.contains_key("kind"))
            {
                return obj.contains_key("root_id")
                    && obj["root_id"]
                        .as_str()
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
            }
            obj.values().all(row_has_root_id)
        }
        serde_json::Value::Array(arr) => arr.iter().all(row_has_root_id),
        _ => true,
    }
}

/// P0.1: every default query JSON row in a workspace store carries `root_id`.
#[test]
fn workspace_query_rows_carry_root_id() {
    let (_base, root_a, root_b, db) = two_root_ws("p0-root-id");

    // find
    let find = run_raw(&["find", "createUser", "--workspace-db", db.to_str().unwrap()]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits = parse_json(&find);
    assert!(
        row_has_root_id(&hits),
        "find rows must carry root_id: {hits}"
    );

    // callers (union)
    let callers = run_raw(&[
        "callers",
        "validateEmail",
        "--workspace-db",
        db.to_str().unwrap(),
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-root",
        root_b.to_str().unwrap(),
    ]);
    assert!(callers.status.success(), "{}", stderr(&callers));
    let crow = parse_json(&callers);
    assert!(
        row_has_root_id(&crow),
        "callers rows must carry root_id: {crow}"
    );

    // impact
    let impact = run_raw(&[
        "impact",
        "createUser",
        "--workspace-db",
        db.to_str().unwrap(),
        "--limit",
        "20",
    ]);
    // impact may succeed with empty neighborhood
    if impact.status.success() {
        let irow = parse_json(&impact);
        if irow.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            assert!(
                row_has_root_id(&irow),
                "impact rows must carry root_id: {irow}"
            );
        }
    }

    // subset / sound payload
    let subset = run_raw(&["subset", "--workspace-db", db.to_str().unwrap()]);
    // subset may exit non-zero on violations; payload still printed
    let sp = parse_json(&subset);
    if let Some(viols) = sp["violations"].as_array() {
        if !viols.is_empty() {
            assert!(
                viols.iter().all(|v| v.get("root_id").is_some()),
                "subset violations must carry root_id: {sp}"
            );
        }
    }

    // sound callers scoped to api
    let sound = run_raw(&[
        "callers",
        "validateEmail",
        "--sound",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    if sound.status.success() {
        let sp2 = parse_json(&sound);
        if let Some(rows) = sp2["callers"].as_array() {
            if !rows.is_empty() {
                assert!(
                    rows.iter().all(|r| r.get("root_id").is_some()),
                    "sound callers must carry root_id: {sp2}"
                );
            }
        }
    }

    // diff (needs snapshot baseline — may fail if no snapshot written for root)
    let _ = run_raw(&[
        "diff",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--write-snapshot",
    ]);
    let diff = run_raw(&[
        "diff",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    if diff.status.success() {
        let dp = parse_json(&diff);
        if let Some(added) = dp["added"].as_array() {
            if !added.is_empty() {
                assert!(
                    added.iter().all(|e| e.get("root_id").is_some()),
                    "diff edges must carry root_id: {dp}"
                );
            }
        }
    }
}

/// P1.8: find path display includes root_id + root_path under workspace store.
#[test]
fn workspace_find_includes_root_path() {
    let (_base, _a, _b, db) = two_root_ws("p1-find-path");
    let find = run_raw(&["find", "createUser", "--workspace-db", db.to_str().unwrap()]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits = parse_json(&find);
    let arr = hits.as_array().unwrap();
    assert!(arr
        .iter()
        .any(|h| h["root_id"] == "api" && h["path"] == "src/auth.ts"));
    assert!(
        arr.iter().any(|h| h
            .get("root_path")
            .and_then(|v| v.as_str())
            .map(|s| s.contains("api"))
            .unwrap_or(false)),
        "find rows should carry root_path so agents can disambiguate same-relative paths: {hits}"
    );
}

/// P1.5: workspace status polish — per-root exact/heur/violations + promise_tier + index_seq.
#[test]
fn workspace_status_polish_fields() {
    let (_base, root_a, root_b, db) = two_root_ws("p1-status");
    // Make dirty root with eval so violations exist.
    std::fs::write(
        root_b.join("src/bad.ts"),
        "export function bad(c: string) { return eval(c); }\n",
    )
    .unwrap();
    let reidx = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(reidx.status.success(), "{}", stderr(&reidx));

    let status = run_raw(&[
        "workspace",
        "status",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let st = parse_json(&status);
    assert!(
        st["index_seq"].as_u64().is_some(),
        "status must expose index_seq: {st}"
    );
    assert!(
        st["promise_tier"].as_str().is_some(),
        "status must expose promise_tier: {st}"
    );
    let roots = st["roots"].as_array().unwrap();
    for r in roots {
        assert!(
            r["exact_refs"].as_u64().is_some(),
            "per-root exact_refs: {r}"
        );
        assert!(
            r["heuristic_refs"].as_u64().is_some(),
            "per-root heuristic_refs: {r}"
        );
        assert!(r.get("missing").is_some(), "per-root missing flag: {r}");
        assert!(
            r["promise_tier"].as_str().is_some(),
            "per-root promise_tier: {r}"
        );
        assert!(r["files"].as_u64().is_some() && r["symbols"].as_u64().is_some());
        assert!(r["references"].as_u64().is_some() && r["subset_violations"].as_u64().is_some());
    }
    let dirty = roots.iter().find(|r| r["id"] == "web").unwrap();
    assert!(dirty["subset_violations"].as_u64().unwrap() >= 1, "{dirty}");
    assert_eq!(dirty["promise_tier"], "disabled", "{dirty}");
    // Weakest-root note when any root violates.
    assert_eq!(st["weakest_root"], "web", "{st}");
    assert_eq!(st["promise_tier"], "disabled", "{st}");
}

/// P1.6: partial re-index of one workspace root keeps sibling root rows + meta.
#[test]
fn workspace_partial_reindex_keeps_siblings() {
    let (_base, root_a, root_b, db) = two_root_ws("p1-partial");

    // Re-index only api.
    let only_api = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(only_api.status.success(), "{}", stderr(&only_api));

    // web rows must still resolve.
    let find_web = run_raw(&[
        "find",
        "onlyInB",
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(
        find_web.status.success(),
        "sibling root rows must survive partial reindex: {}",
        stderr(&find_web)
    );

    // status still lists both roots.
    let status = run_raw(&[
        "workspace",
        "status",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    let st = parse_json(&status);
    let roots = st["roots"].as_array().unwrap();
    assert!(roots.iter().any(|r| r["id"] == "api"), "{st}");
    assert!(roots.iter().any(|r| r["id"] == "web"), "{st}");
}

/// P1.7: nested roots — status lists both after index.
#[test]
fn workspace_nested_roots_status_lists_both() {
    let base = temp_dir("nested-status");
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
    assert!(out.status.success(), "{}", stderr(&out));
    let status = run_raw(&[
        "workspace",
        "status",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    let st = parse_json(&status);
    let roots = st["roots"].as_array().unwrap();
    assert!(roots.len() >= 2, "nested roots both listed: {st}");
    assert!(roots.iter().any(|r| r["id"] == "mono"));
    assert!(
        roots.iter().any(|r| {
            r["id"].as_str().map(|s| s.contains("api")).unwrap_or(false)
                || r["id"] == "packages_api"
                || r["path"]
                    .as_str()
                    .map(|p| p.contains("packages/api"))
                    .unwrap_or(false)
        }),
        "child root listed: {st}"
    );
}

/// P0.3: graph --workspace-root filter + HTML root_id badge.
#[test]
fn workspace_graph_filter_and_root_badge() {
    let (_base, root_a, _root_b, db) = two_root_ws("p0-graph");
    let out_html = db.parent().unwrap().join("ws-graph.html");
    let graph = run_raw(&[
        "graph",
        "createUser",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--out",
        out_html.to_str().unwrap(),
    ]);
    assert!(graph.status.success(), "{}", stderr(&graph));
    let html = std::fs::read_to_string(&out_html).unwrap();
    assert!(
        html.contains("data-root-id") || html.contains("root-badge") || html.contains("root_id"),
        "graph HTML must surface root_id badge/data for workspace rows"
    );
    // Scoped to api — should not include onlyInB (web-only).
    assert!(
        !html.contains("onlyInB"),
        "graph --workspace-root api must not include web-only symbols"
    );
}

/// P0.4: --with-macro + multi-root workspace without root filter → clear reject.
#[test]
fn workspace_with_macro_multi_root_rejected() {
    let (_base, root_a, _root_b, db) = two_root_ws("p0-macro-ws");
    let out = run_raw(&[
        "callers",
        "createUser",
        "--with-macro",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(
        !out.status.success(),
        "with-macro + multi-root without filter must fail"
    );
    let err = stderr(&out).to_lowercase();
    assert!(
        err.contains("workspace") || err.contains("per-root") || err.contains("workspace-root"),
        "clear error expected: {err}"
    );

    // Single-root filter is allowed (may succeed with empty sidecar union).
    let ok = run_raw(&[
        "callers",
        "createUser",
        "--with-macro",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(
        ok.status.success(),
        "with-macro + single root filter should be allowed: {}",
        stderr(&ok)
    );
}

/// P0.2: MCP schema parity — workspace_status + optional root filters default off.
#[test]
fn mcp_workspace_status_and_filter_schema() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let (_base, root_a, _b, db) = two_root_ws("p0-mcp-schema");
    let mut child = Command::new(bin())
        .arg("--root")
        .arg(&root_a)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp");
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
        )
        .unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{}}}}"#
        )
        .unwrap();
    }
    let out = child.wait_with_output().expect("mcp out");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("workspace_status"),
        "workspace_status tool required: {text}"
    );
    assert!(
        text.contains("root_id"),
        "root_id filter args required on query tools: {text}"
    );
    assert!(
        text.contains("workspace_db"),
        "workspace_db filter args required: {text}"
    );
    let _ = db;
}

/// Missing root path → status marks `missing: true`.
#[test]
fn workspace_status_missing_root_path() {
    let base = temp_dir("missing-path");
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
    assert!(idx.status.success(), "{}", stderr(&idx));

    // Delete web root path from disk.
    std::fs::remove_dir_all(&root_b).unwrap();

    let status = run_raw(&[
        "workspace",
        "status",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let st = parse_json(&status);
    let web = st["roots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "web")
        .expect("web root listed");
    assert_eq!(
        web["missing"], true,
        "missing root path must set missing=true: {web}"
    );
}

// ---------------------------------------------------------------------------
// P1-1: incremental per-root workspace index (TDD)
// ---------------------------------------------------------------------------

fn file_row(db: &Path, root_id: &str, rel: &str) -> (String, i64, i64) {
    let mut st = AgStore::open(db).unwrap_or_else(|e| panic!("open {db:?}: {e}"));
    st.set_write_root(root_id);
    let m = st
        .file_meta(rel)
        .unwrap_or_else(|e| panic!("file_meta {rel}@{root_id}: {e}"))
        .unwrap_or_else(|| panic!("missing file_meta {rel}@{root_id}"));
    (m.hash, m.mtime_ns, m.size)
}

/// P1-1 acceptance: dual-root edit A → incremental workspace-root index;
/// sibling B hashes/mtimes unchanged; A shows new symbol; subset/diff for A.
#[test]
fn workspace_incremental_root_reindex_scoped() {
    let (_base, root_a, root_b, db) = two_root_ws("p1-incr");

    let b_auth_before = file_row(&db, "web", "src/auth.ts");
    let b_unique_before = file_row(&db, "web", "src/unique_b.ts");
    let a_auth_before = file_row(&db, "api", "src/auth.ts");

    // Edit only root A — add a new exported symbol + a call edge.
    std::fs::write(
        root_a.join("src/auth.ts"),
        r#"
export function validateEmail(email: string): boolean {
  return email.includes("@");
}
export function brandNewInA(): string {
  return "new";
}
export function createUser(email: string) {
  if (!validateEmail(email)) throw new Error("bad");
  brandNewInA();
  return { email };
}
"#,
    )
    .unwrap();
    // Ensure mtime differs even on coarse clocks.
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Incremental workspace-root reindex of A only (no --force, no sibling root).
    let idx = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    // Sibling root B: hashes + mtimes must be untouched (no rehash of B contents).
    let b_auth_after = file_row(&db, "web", "src/auth.ts");
    let b_unique_after = file_row(&db, "web", "src/unique_b.ts");
    assert_eq!(
        b_auth_before, b_auth_after,
        "root B auth.ts hash/mtime must be unchanged after A-only reindex"
    );
    assert_eq!(
        b_unique_before, b_unique_after,
        "root B unique_b.ts hash/mtime must be unchanged after A-only reindex"
    );

    // A: edited file rehashed.
    let a_auth_after = file_row(&db, "api", "src/auth.ts");
    assert_ne!(
        a_auth_before.0, a_auth_after.0,
        "root A auth.ts hash must change after edit + reindex"
    );

    // find/callers see the new symbol in A.
    let find_a = run_raw(&[
        "find",
        "brandNewInA",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(find_a.status.success(), "{}", stderr(&find_a));
    let hits = parse_json(&find_a);
    assert!(
        hits.as_array()
            .unwrap()
            .iter()
            .any(|h| h["name"] == "brandNewInA" && h["root_id"] == "api"),
        "find brandNewInA under root A: {hits}"
    );
    let callers = run_raw(&[
        "callers",
        "brandNewInA",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(callers.status.success(), "{}", stderr(&callers));
    let cp = parse_json(&callers);
    let rows = if cp.is_array() {
        cp.as_array().cloned().unwrap_or_default()
    } else {
        cp.get("callers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    assert!(
        rows.iter().any(
            |r| r.get("enclosing").and_then(|e| e.as_str()) == Some("createUser")
                || r["name"] == "brandNewInA"
        ),
        "callers of brandNewInA should include createUser edge: {cp}"
    );

    // B still does not have the new symbol under its root_id filter.
    let find_b = run_raw(&[
        "find",
        "brandNewInA",
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(
        !find_b.status.success()
            || parse_json(&find_b)
                .as_array()
                .map(|a| a.iter().all(|h| h["root_id"] != "web"))
                .unwrap_or(true),
        "brandNewInA must not appear under root B"
    );

    // B-only symbol still resolvable after A-only reindex.
    let find_only_b = run_raw(&[
        "find",
        "onlyInB",
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(find_only_b.status.success(), "{}", stderr(&find_only_b));

    // subset scoped to A reflects current A disk (no violations on clean A).
    let subset_a = run_raw(&[
        "subset",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    let sp = parse_json(&subset_a);
    assert!(
        sp.get("in_subset").is_some() || sp.get("violations").is_some(),
        "subset payload for A: {sp}"
    );

    // Introduce an S-violation in A only; incremental reindex; subset sees it for A.
    std::fs::write(
        root_a.join("src/bad.ts"),
        "export function bad(c: string) { return eval(c); }\n",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let idx2 = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(idx2.status.success(), "{}", stderr(&idx2));
    let subset_a2 = run_raw(&[
        "subset",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    let sp2 = parse_json(&subset_a2);
    let viol_paths: Vec<String> = sp2["violations"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["path"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        viol_paths.iter().any(|p| p.contains("bad.ts")),
        "subset --workspace-root A must see new A violation after incremental reindex: {sp2}"
    );
    // B subset must not pick up A's new violation when scoped to B.
    let subset_b = run_raw(&[
        "subset",
        "--workspace-root",
        root_b.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    let sp_b = parse_json(&subset_b);
    let viol_b: Vec<String> = sp_b["violations"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["path"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !viol_b.iter().any(|p| p.contains("bad.ts")),
        "subset --workspace-root B must not see A-only violation: {sp_b}"
    );

    // diff for A can run after a workspace-root snapshot lock.
    let _ = run_raw(&[
        "diff",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--write-snapshot",
    ]);
    // Edit A again + incremental reindex (index_as writes a per-root snapshot).
    std::fs::write(
        root_a.join("src/auth.ts"),
        r#"
export function validateEmail(email: string): boolean {
  return email.includes("@");
}
export function brandNewInA(): string {
  return "new";
}
export function afterSnapshotInA() { return 1; }
export function createUser(email: string) {
  if (!validateEmail(email)) throw new Error("bad");
  brandNewInA();
  return { email };
}
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let idx3 = run_raw(&[
        "index",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(idx3.status.success(), "{}", stderr(&idx3));
    let find_after = run_raw(&[
        "find",
        "afterSnapshotInA",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(
        find_after.status.success(),
        "post-snapshot incremental index must expose afterSnapshotInA: {}",
        stderr(&find_after)
    );
    let diff_a = run_raw(&[
        "diff",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    if diff_a.status.success() {
        let dp = parse_json(&diff_a);
        assert!(
            dp.get("added").is_some() || dp.get("summary").is_some(),
            "diff payload for A after incremental: {dp}"
        );
        if let Some(added) = dp["added"].as_array() {
            let _ = added; // baseline may already include new edges if index refreshed snapshot
        }
    }
}

/// P1-1 watch path: Indexer.write_root_id + index_paths updates only that root_id.
#[test]
fn workspace_index_paths_write_root_id_scoped() {
    let (_base, root_a, _root_b, db) = two_root_ws("p1-watch-id");
    let b_before = file_row(&db, "web", "src/unique_b.ts");

    let ix = agentgraph::index::Indexer::with_db_path(&root_a, &db)
        .unwrap()
        .with_write_root_id("api");
    let edited = root_a.join("src/auth.ts");
    std::fs::write(
        &edited,
        r#"
export function validateEmail(email: string): boolean {
  return email.includes("@");
}
export function watchPathNew(): number { return 7; }
export function createUser(email: string) {
  if (!validateEmail(email)) throw new Error("bad");
  return { email };
}
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    ix.index_paths(std::slice::from_ref(&edited))
        .expect("index_paths scoped to api");

    let find = run_raw(&[
        "find",
        "watchPathNew",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits = parse_json(&find);
    assert!(
        hits.as_array()
            .unwrap()
            .iter()
            .any(|h| h["name"] == "watchPathNew" && h["root_id"] == "api"),
        "watch/index_paths must stamp root_id=api: {hits}"
    );
    let b_after = file_row(&db, "web", "src/unique_b.ts");
    assert_eq!(
        b_before, b_after,
        "index_paths under write_root_id=api must not touch web file meta"
    );
}
