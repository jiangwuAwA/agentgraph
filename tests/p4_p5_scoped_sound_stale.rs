//! TDD P4 + P5: scoped-sound aggregation + baseline/sidecar staleness flags.
//!
//! Contract (docs/sound-subset.md, docs/graph-diff.md, docs/workspace.md):
//! - P4: `subset` / `workspace status` expose per-root / per-top-dir buckets +
//!   `sound_candidates` (eligible first) + `recommendation` for scoped --sound.
//! - P5: dirty `index_paths` marks `meta.baseline_stale=true` (does **not**
//!   auto-refresh baseline); `diff` / status / stats / macro status surface
//!   `baseline_stale` + `sidecar_stale`/`sidecar_exists` without creating sidecars.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentgraph::index::diff::{baseline_stale_flag, run_diff_for_root};
use agentgraph::index::subset::{
    aggregate_scoped_sound, scoped_sound_by_root, scoped_sound_by_top_dir, top_dir_of_path,
    SoundScopeKind,
};
use agentgraph::index::Indexer;
use agentgraph::model::WorkspaceRootInfo;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-p45-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_raw(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn run_in(root: &Path, args: &[&str]) -> Output {
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
    let text = stdout(out);
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid JSON ({e}): stdout={text} stderr={}", stderr(out)))
}

fn write_clean_root(root: &Path) {
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

fn write_dirty_root(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/bad.ts"),
        r#"
export function evil(c: string) {
  return eval(c);
}
export function alsoBad(x: number) {
  return new Function("return " + x)();
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/ok.ts"),
        "export function fine(x: number): number { return x + 1; }\n",
    )
    .unwrap();
}

fn index_workspace(roots: &[(&str, &Path)], db: &Path) {
    let mut args: Vec<String> = vec!["index".into()];
    for (_, p) in roots {
        args.push("--workspace-root".into());
        args.push(p.to_string_lossy().into_owned());
    }
    args.push("--workspace-db".into());
    args.push(db.to_string_lossy().into_owned());
    args.push("--force".into());
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_raw(&refs);
    assert!(out.status.success(), "workspace index: {}", stderr(&out));
}

// ---------------------------------------------------------------------------
// P4 — unit: aggregation helpers
// ---------------------------------------------------------------------------

#[test]
fn top_dir_of_path_splits_first_segment() {
    assert_eq!(top_dir_of_path("repository/src/lib.rs"), "repository");
    assert_eq!(top_dir_of_path("auth\\src\\a.ts"), "auth");
    assert_eq!(top_dir_of_path("top.rs"), "");
}

#[test]
fn scoped_sound_by_root_clean_and_dirty_candidates() {
    let langs = vec!["typescript".to_string()];
    let roots = vec![
        WorkspaceRootInfo {
            id: "repository".into(),
            path: "/ws/repository".into(),
            files: 3,
            symbols: 4,
            references: 5,
            subset_violations: 0,
            languages: Some(langs.clone()),
            exact_refs: 4,
            heuristic_refs: 1,
            dynamic_refs: 0,
            index_seq: Some(1),
            missing: false,
            promise_tier: Some("ast_modeled".into()),
        },
        WorkspaceRootInfo {
            id: "nn-ranker".into(),
            path: "/ws/nn-ranker".into(),
            files: 2,
            symbols: 2,
            references: 2,
            subset_violations: 2,
            languages: Some(langs.clone()),
            exact_refs: 2,
            heuristic_refs: 0,
            dynamic_refs: 0,
            index_seq: Some(1),
            missing: false,
            promise_tier: Some("disabled".into()),
        },
    ];
    let violations = vec![
        agentgraph::index::subset::SubsetViolation {
            kind: "eval".into(),
            path: "src/bad.ts".into(),
            line: 2,
            snippet: "eval(c)".into(),
            root_id: "nn-ranker".into(),
        },
        agentgraph::index::subset::SubsetViolation {
            kind: "Function".into(),
            path: "src/bad.ts".into(),
            line: 5,
            snippet: "new Function".into(),
            root_id: "nn-ranker".into(),
        },
    ];
    let agg = scoped_sound_by_root(&roots, &violations, &langs);
    assert_eq!(agg.scope, SoundScopeKind::Root);
    assert_eq!(agg.buckets.len(), 2);

    let clean = agg
        .buckets
        .iter()
        .find(|b| b.key == "repository")
        .expect("repository bucket");
    assert!(clean.sound_eligible, "{clean:?}");
    assert_eq!(clean.violations, 0);
    assert_eq!(clean.promise_tier, "ast_modeled");
    assert!(clean.top_kinds.is_empty());

    let dirty = agg
        .buckets
        .iter()
        .find(|b| b.key == "nn-ranker")
        .expect("nn-ranker bucket");
    assert!(!dirty.sound_eligible, "{dirty:?}");
    assert_eq!(dirty.violations, 2);
    assert_eq!(dirty.promise_tier, "disabled");
    assert!(dirty.top_kinds.iter().any(|(k, _)| k == "eval"));
    assert!(dirty.top_kinds.iter().any(|(k, _)| k == "Function"));

    // Eligible first.
    assert!(!agg.sound_candidates.is_empty());
    assert_eq!(agg.sound_candidates[0].key, "repository");
    assert!(agg.sound_candidates[0].sound_eligible);
    let dirty_c = agg
        .sound_candidates
        .iter()
        .find(|c| c.key == "nn-ranker")
        .expect("dirty candidate");
    assert!(!dirty_c.sound_eligible);
    assert!(dirty_c.reason.contains("eval") || dirty_c.reason.contains("S violations"));

    assert!(
        agg.recommendation.contains("repository"),
        "recommendation must name eligible root: {}",
        agg.recommendation
    );
    assert!(
        agg.recommendation.contains("nn-ranker"),
        "recommendation must name avoid root: {}",
        agg.recommendation
    );
}

#[test]
fn scoped_sound_by_top_dir_groups_single_root_tree() {
    let langs = vec!["rust".to_string()];
    let top_dirs = vec![
        ("repository".to_string(), None),
        ("nn-ranker".to_string(), None),
        ("event-engine".to_string(), None),
    ];
    let violations = vec![
        agentgraph::index::subset::SubsetViolation {
            kind: "unsafe".into(),
            path: "nn-ranker/src/lib.rs".into(),
            line: 10,
            snippet: "unsafe {".into(),
            root_id: String::new(),
        },
        agentgraph::index::subset::SubsetViolation {
            kind: "transmute".into(),
            path: "nn-ranker/src/ptr.rs".into(),
            line: 3,
            snippet: "transmute".into(),
            root_id: String::new(),
        },
    ];
    let agg = scoped_sound_by_top_dir(&top_dirs, &violations, &langs);
    assert_eq!(agg.scope, SoundScopeKind::TopDir);
    assert!(agg.buckets.len() >= 3);

    let clean = agg
        .buckets
        .iter()
        .find(|b| b.key == "repository")
        .expect("repository top-dir");
    assert!(clean.sound_eligible, "{clean:?}");
    let dirty = agg
        .buckets
        .iter()
        .find(|b| b.key == "nn-ranker")
        .expect("nn-ranker top-dir");
    assert!(!dirty.sound_eligible);
    assert!(dirty.top_kinds.iter().any(|(k, _)| k == "unsafe"));

    assert!(
        agg.sound_candidates[0].sound_eligible,
        "eligible must sort first: {:?}",
        agg.sound_candidates
    );
    assert!(
        agg.sound_candidates[0].key == "repository"
            || agg.sound_candidates[0].key == "event-engine",
        "first eligible is a clean top-dir: {:?}",
        agg.sound_candidates[0]
    );
    assert!(agg.recommendation.contains("repository"));
}

#[test]
fn aggregate_scoped_sound_all_dirty_recommendation_honest() {
    let langs = vec!["python".to_string()];
    let meta = vec![(
        "crate-a".to_string(),
        None,
        Some(vec!["python".to_string()]),
    )];
    let violations = vec![agentgraph::index::subset::SubsetViolation {
        kind: "py_eval_exec".into(),
        path: "a.py".into(),
        line: 1,
        snippet: "eval(x)".into(),
        root_id: "crate-a".into(),
    }];
    let agg = aggregate_scoped_sound(
        SoundScopeKind::Root,
        &meta,
        &violations,
        |_| "crate-a".to_string(),
        &langs,
    );
    assert!(
        agg.recommendation.contains("no sound-eligible") || agg.recommendation.contains("do not"),
        "all-dirty recommendation must not invent eligible roots: {}",
        agg.recommendation
    );
}

// ---------------------------------------------------------------------------
// P4 — e2e: multi-root fixture (clean + dirty) → subset / workspace status
// ---------------------------------------------------------------------------

#[test]
fn multi_root_subset_exposes_sound_candidates_clean_first() {
    let base = temp_dir("multi-sound");
    let api = base.join("api");
    let nn = base.join("nn-ranker");
    write_clean_root(&api);
    write_dirty_root(&nn);
    let db = base.join("ws.db");
    index_workspace(&[("api", &api), ("nn-ranker", &nn)], &db);

    let sub = run_raw(&["subset", "--workspace-db", db.to_str().unwrap()]);
    // Dirty root → subset exits non-zero; payload still on stdout.
    let payload = parse_json(&sub);
    assert_eq!(
        payload["in_subset"],
        serde_json::json!(false),
        "union subset must be false when any root violates: {payload}"
    );
    assert!(
        payload["by_root"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "workspace subset must include by_root: {payload}"
    );
    let by_root = payload["by_root"].as_array().unwrap();
    let api_b = by_root
        .iter()
        .find(|r| r["root_id"] == "api" || r["key"] == "api");
    let nn_b = by_root
        .iter()
        .find(|r| r["root_id"] == "nn-ranker" || r["key"] == "nn-ranker");
    let api_b = api_b.expect("api bucket");
    let nn_b = nn_b.expect("nn-ranker bucket");
    assert_eq!(
        api_b["sound_eligible"],
        serde_json::json!(true),
        "api must be sound_eligible: {api_b}"
    );
    assert_eq!(
        api_b["promise_tier"],
        serde_json::json!("ast_modeled"),
        "{api_b}"
    );
    assert_eq!(nn_b["sound_eligible"], serde_json::json!(false), "{nn_b}");
    assert_eq!(
        nn_b["promise_tier"],
        serde_json::json!("disabled"),
        "{nn_b}"
    );

    let cands = payload["sound_candidates"]
        .as_array()
        .unwrap_or_else(|| panic!("sound_candidates required: {payload}"));
    assert!(!cands.is_empty(), "{payload}");
    assert_eq!(
        cands[0].get("root_id").and_then(|v| v.as_str()),
        Some("api"),
        "eligible first: {cands:?}"
    );
    assert_eq!(
        cands[0]["sound_eligible"],
        serde_json::json!(true),
        "{cands:?}"
    );
    let dirty_c = cands
        .iter()
        .find(|c| c.get("root_id").and_then(|v| v.as_str()) == Some("nn-ranker"))
        .expect("dirty candidate");
    assert_eq!(dirty_c["sound_eligible"], serde_json::json!(false));

    let rec = payload["recommendation"]
        .as_str()
        .unwrap_or_else(|| panic!("recommendation required: {payload}"));
    assert!(rec.contains("api"), "recommendation: {rec}");
    assert!(rec.contains("nn-ranker"), "recommendation: {rec}");

    // workspace status must carry the same machine-readable candidates + stale flags.
    let st = run_raw(&[
        "workspace",
        "status",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(st.status.success(), "{}", stderr(&st));
    let stj = parse_json(&st);
    assert!(
        stj["sound_candidates"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "workspace status sound_candidates: {stj}"
    );
    assert!(
        stj["recommendation"]
            .as_str()
            .map(|s| s.contains("api"))
            .unwrap_or(false),
        "workspace status recommendation: {stj}"
    );
    assert!(
        stj.get("baseline_stale").is_some(),
        "workspace status must expose baseline_stale: {stj}"
    );
    assert!(
        stj.get("sidecar_exists").is_some() && stj.get("sidecar_stale").is_some(),
        "workspace status must expose sidecar flags without building sidecar: {stj}"
    );
    assert_eq!(
        stj["sidecar_exists"],
        serde_json::json!(false),
        "no sidecar was built: {stj}"
    );
}

#[test]
fn single_root_subset_includes_by_top_dir() {
    let root = temp_dir("single-topdir");
    std::fs::create_dir_all(root.join("repository/src")).unwrap();
    std::fs::create_dir_all(root.join("nn-ranker/src")).unwrap();
    std::fs::write(
        root.join("repository/src/lib.rs"),
        "pub fn ok(x: i32) -> i32 { x + 1 }\npub fn call_ok() -> i32 { ok(1) }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("nn-ranker/src/lib.rs"),
        "pub fn bad() { let _ = unsafe { 0 }; }\npub fn call_bad() { bad(); }\n",
    )
    .unwrap();

    let idx = run_in(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let sub = run_in(&root, &["subset"]);
    let payload = parse_json(&sub);
    let top = payload["by_top_dir"]
        .as_array()
        .unwrap_or_else(|| panic!("single-root subset must include by_top_dir: {payload}"));
    assert!(
        top.iter().any(|b| {
            (b["key"] == "repository" || b["path"] == "repository")
                && b["sound_eligible"] == serde_json::json!(true)
        }),
        "clean top-dir eligible: {top:?}"
    );
    assert!(
        top.iter().any(|b| {
            (b["key"] == "nn-ranker" || b["path"] == "nn-ranker")
                && b["sound_eligible"] == serde_json::json!(false)
        }),
        "dirty top-dir ineligible: {top:?}"
    );
    let cands = payload["sound_candidates"].as_array().expect("candidates");
    assert!(
        cands.iter().any(|c| {
            c.get("path").and_then(|v| v.as_str()) == Some("repository")
                && c["sound_eligible"] == serde_json::json!(true)
        }),
        "{cands:?}"
    );
}

// ---------------------------------------------------------------------------
// P5 — baseline_stale after watch / index_paths
// ---------------------------------------------------------------------------

#[test]
fn index_paths_marks_baseline_stale_and_diff_reports_it() {
    let root = temp_dir("baseline-stale");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/a.ts"),
        "export function helper(x: number): number { return x + 1; }\nexport function main() { return helper(1); }\n",
    )
    .unwrap();

    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).expect("full index");
    {
        let store = indexer.open_store().unwrap();
        assert!(
            !baseline_stale_flag(&store),
            "full index must clear baseline_stale"
        );
    }

    // Dirty edit + index_paths (watch path): do **not** auto-refresh baseline.
    let edited = root.join("src/a.ts");
    std::fs::write(
        &edited,
        "export function helper(x: number): number { return x + 1; }\nexport function extra() { return helper(2); }\nexport function main() { return helper(1); }\n",
    )
    .unwrap();
    indexer
        .index_paths(std::slice::from_ref(&edited))
        .expect("index_paths dirty");

    {
        let store = indexer.open_store().unwrap();
        assert!(
            baseline_stale_flag(&store),
            "dirty index_paths must set meta.baseline_stale=true"
        );
        // Baseline file must still be the pre-watch snapshot (not auto-refreshed).
        let d = run_diff_for_root(&indexer.root, &store, false, None, None, "").expect("diff");
        assert!(
            d.baseline_stale,
            "diff payload must carry baseline_stale=true: {d:?}"
        );
    }

    // CLI diff must include the flag + stderr one-liner.
    let d = run_in(&root, &["diff"]);
    assert!(d.status.success(), "{}", stderr(&d));
    let payload = parse_json(&d);
    assert_eq!(
        payload["baseline_stale"],
        serde_json::json!(true),
        "CLI diff baseline_stale: {payload}"
    );
    let err = stderr(&d);
    assert!(
        err.contains("baseline_stale") || err.to_ascii_lowercase().contains("stale"),
        "diff stderr must warn when baseline_stale: {err}"
    );

    // Full index refreshes baseline → flag clears.
    let idx = run_in(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let d2 = run_in(&root, &["diff"]);
    assert!(d2.status.success(), "{}", stderr(&d2));
    let p2 = parse_json(&d2);
    assert_eq!(
        p2["baseline_stale"],
        serde_json::json!(false),
        "full index must clear baseline_stale on diff: {p2}"
    );
}

#[test]
fn stats_and_macro_status_expose_sidecar_flags_without_building_sidecar() {
    let root = temp_dir("sidecar-flags");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/a.ts"),
        "export function helper(): number { return 1; }\nexport function main() { return helper(); }\n",
    )
    .unwrap();
    let idx = run_in(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let sidecar = root.join(".agentgraph").join("index.macro.db");
    assert!(!sidecar.exists(), "fixture must not pre-build sidecar");

    // stats: cheap flags present; sidecar_exists=false; never creates file.
    let st = run_in(&root, &["stats"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let stj = parse_json(&st);
    assert!(
        stj.get("baseline_stale").is_some(),
        "stats must expose baseline_stale: {stj}"
    );
    assert!(
        stj.get("sidecar_exists").is_some() && stj.get("sidecar_stale").is_some(),
        "stats must expose sidecar flags: {stj}"
    );
    assert_eq!(stj["sidecar_exists"], serde_json::json!(false), "{stj}");
    assert!(
        !sidecar.exists(),
        "stats must never create the macro sidecar file"
    );

    // macro status: absent sidecar reported honestly, flags present, no create.
    let ms = run_in(&root, &["macro", "status"]);
    assert!(ms.status.success(), "{}", stderr(&ms));
    let msj = parse_json(&ms);
    assert_eq!(msj["exists"], serde_json::json!(false), "{msj}");
    assert_eq!(msj["sidecar_exists"], serde_json::json!(false), "{msj}");
    assert_eq!(msj["sidecar_stale"], serde_json::json!(false), "{msj}");
    assert!(
        msj.get("baseline_stale").is_some(),
        "macro status must expose baseline_stale: {msj}"
    );
    assert!(
        !sidecar.exists(),
        "macro status must not create the sidecar file"
    );

    // Dirty index_paths → baseline_stale visible on stats too.
    let f = root.join("src/a.ts");
    std::fs::write(
        &f,
        "export function helper(): number { return 2; }\nexport function main() { return helper(); }\n",
    )
    .unwrap();
    let indexer = Indexer::new(&root).unwrap();
    indexer
        .index_paths(std::slice::from_ref(&f))
        .expect("index_paths");
    let st2 = run_in(&root, &["stats"]);
    let stj2 = parse_json(&st2);
    assert_eq!(
        stj2["baseline_stale"],
        serde_json::json!(true),
        "stats after dirty index_paths: {stj2}"
    );
}

/// Unit: aggregation JSON payload helpers stay stable for Agents/other tools.
#[test]
fn sound_aggregation_payload_shape_is_stable() {
    use agentgraph::index::subset::scoped_sound_by_top_dir;
    let agg = scoped_sound_by_top_dir(&[("auth".to_string(), None)], &[], &["go".to_string()]);
    let v = agg.to_payload_json();
    assert!(v["sound_candidates"].is_array(), "{v}");
    assert!(v["recommendation"].as_str().is_some(), "{v}");
    let buckets = v["by_top_dir"].as_array().expect("by_top_dir in payload");
    assert_eq!(buckets[0]["path"], "auth");
    assert_eq!(buckets[0]["sound_eligible"], true);
}
