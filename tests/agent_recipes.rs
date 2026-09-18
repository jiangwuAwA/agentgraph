//! Priority-3 high-level agent recipes: MCP/CLI `blast_radius` + `who_calls`.
//!
//! TDD contract:
//! - blast_radius auto-window: sound when subset_ok else default (never blind --recall)
//! - P0-4: default/disabled window recommendation names scoped sound candidates +
//!   example legal commands; all-dirty stays honest (no sound command / no blind --recall)
//! - include_macro only when sidecar exists && !stale && !nested; else refuse with reason
//! - who_calls noisy=false separates implementors; noisy=true merges (old noisy shape)
//! - response always carries recommendation + promise_tier + note (honesty)
//! - stable payload keys: window, promise_tier, subset_ok, recommendation, note,
//!   sound_candidates (勿改名)
//! - MCP tools/list advertises blast_radius / who_calls
//! - docs_claims stays green for recipe flags/commands

use agentgraph::index::subset::{SoundAggregation, SoundCandidate, SoundScopeKind};
use agentgraph::model::{Confidence, EdgeKind, Evidence, MacroSidecarStatus, ReferenceRecord};
use agentgraph::query::recipes::{
    build_blast_radius_payload, build_scoped_sound_guidance, build_who_calls_payload,
    decide_blast_window, decide_include_macro, BlastRadiusPayloadInput, BlastWindowDecision,
    ScopedSoundGuidance, RECIPE_NOTE,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-recipes-{name}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
    dir
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

fn write_clean_js(root: &Path) {
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

fn write_eval_js(root: &Path) {
    std::fs::write(
        root.join("src/evil.js"),
        r#"
function runCode(code) {
  return eval(code);
}
function helper() { return 1; }
module.exports = { runCode, helper };
"#,
    )
    .unwrap();
}

fn write_rust_trait_flood(root: &Path) {
    let mut src = String::from("trait T { fn fmt(&self) -> String; }\n");
    for i in 0..25 {
        src.push_str(&format!(
            "struct S{i};\nimpl T for S{i} {{ fn fmt(&self) -> String {{ \"x\".into() }} }}\n"
        ));
    }
    src.push_str("fn show(t: &dyn T) -> String { t.fmt() }\nfn main() { let _ = show(&S0); }\n");
    std::fs::write(root.join("src/main.rs"), src).unwrap();
}

fn write_rust_trait_calls(root: &Path) {
    std::fs::write(
        root.join("src/main.rs"),
        r#"
trait Shape {
    fn area(&self) -> f64;
}
struct Circle { r: f64 }
struct Rect { w: f64, h: f64 }
impl Shape for Circle { fn area(&self) -> f64 { 3.0 } }
impl Shape for Rect { fn area(&self) -> f64 { self.w * self.h } }
fn paint(s: &dyn Shape) -> f64 { s.area() }
fn main() { let c = Circle { r: 1.0 }; let _ = paint(&c); }
"#,
    )
    .unwrap();
}

fn ref_row(rule: Option<&str>, path: &str, line: usize, conf: Confidence) -> ReferenceRecord {
    ReferenceRecord {
        name: "area".into(),
        kind: EdgeKind::Call,
        path: path.into(),
        line,
        enclosing: Some("Impl".into()),
        module: None,
        resolved: None,
        qualifier: None,
        confidence: conf,
        evidence: rule.map(|r| Evidence {
            rule_id: r.into(),
            snippet: String::new(),
        }),
        root_id: String::new(),
    }
}

// ---------------------------------------------------------------------------
// Unit: auto-window + macro gate
// ---------------------------------------------------------------------------

#[test]
fn decide_window_sound_when_subset_ok() {
    let d = decide_blast_window(true, None);
    assert_eq!(d.window, "sound");
    assert!(d.use_sound);
    assert!(d.subset_ok);
    assert!(!d.recommendation.is_empty());
}

#[test]
fn decide_window_default_when_not_subset_ok() {
    let d = decide_blast_window(false, Some("rootA"));
    assert_eq!(d.window, "default");
    assert!(!d.use_sound);
    let rec = d.recommendation.to_lowercase();
    assert!(
        rec.contains("disabled") || rec.contains("sound"),
        "recommendation must explain sound disabled: {}",
        d.recommendation
    );
    assert!(d.recommendation.contains("rootA") || rec.contains("roota"));
}

#[test]
fn include_macro_refused_when_missing_sidecar() {
    let st = MacroSidecarStatus::default(); // exists=false
    let (ok, reason) = decide_include_macro(true, Some(&st), false);
    assert!(!ok);
    let reason = reason.expect("must refuse with reason");
    assert!(
        reason.to_lowercase().contains("missing") || reason.contains("macro"),
        "reason={reason}"
    );
}

#[test]
fn include_macro_refused_when_stale_or_nested() {
    let st = MacroSidecarStatus {
        exists: true,
        stale: true,
        ..MacroSidecarStatus::default()
    };
    let (ok, reason) = decide_include_macro(true, Some(&st), false);
    assert!(!ok);
    assert!(reason.unwrap().to_lowercase().contains("stale"));

    let nested = MacroSidecarStatus {
        exists: true,
        expanded_root_nested: true,
        ..MacroSidecarStatus::default()
    };
    let (ok2, reason2) = decide_include_macro(true, Some(&nested), false);
    assert!(!ok2);
    assert!(reason2.unwrap().to_lowercase().contains("nest"));
}

#[test]
fn include_macro_refused_under_sound_window() {
    let st = MacroSidecarStatus {
        exists: true,
        ..MacroSidecarStatus::default()
    };
    let (ok, reason) = decide_include_macro(true, Some(&st), true);
    assert!(!ok);
    let r = reason.unwrap().to_lowercase();
    assert!(r.contains("sound") || r.contains("exclusive"), "{r}");
}

#[test]
fn include_macro_allowed_when_fresh_non_nested() {
    let st = MacroSidecarStatus {
        exists: true,
        stale: false,
        expanded_root_nested: false,
        expanded_root: Some("expanded".into()),
        ..MacroSidecarStatus::default()
    };
    let (ok, reason) = decide_include_macro(true, Some(&st), false);
    assert!(ok, "reason={reason:?}");
    assert!(reason.is_none());
}

// ---------------------------------------------------------------------------
// Unit: payload builders
// ---------------------------------------------------------------------------

#[test]
fn blast_payload_always_has_honesty_fields() {
    let d = decide_blast_window(false, None);
    let v = build_blast_radius_payload(BlastRadiusPayloadInput {
        symbol: "createUser".into(),
        depth: 3,
        limit: 100,
        nodes: vec![serde_json::json!({"name":"x","path":"a.ts","line":1,"edge_role":"call"})],
        window: d,
        promise_tier: "disabled".into(),
        languages: vec!["javascript".into()],
        include_macro: false,
        include_macro_reason: Some("include_macro refused: macro sidecar missing".into()),
        stale: Some(false),
        scoped_sound: ScopedSoundGuidance::default(),
    });
    assert_eq!(v["tool"], "blast_radius");
    assert_eq!(v["window"], "default");
    assert_eq!(v["subset_ok"], false);
    assert_eq!(v["promise_tier"], "disabled");
    assert!(v["nodes"].as_array().unwrap().len() == 1);
    assert_eq!(v["nodes"][0]["edge_role"], "call");
    assert!(v["include_macro"] == false);
    assert!(v["include_macro_reason"]
        .as_str()
        .unwrap()
        .contains("missing"));
    assert_eq!(v["stale"], false);
    assert!(v["recommendation"].as_str().unwrap().len() > 8);
    assert_eq!(v["note"], RECIPE_NOTE);
    assert!(v["note"]
        .as_str()
        .unwrap()
        .contains("not a complete runtime graph"));
    // Stable keys always present (勿改名) — even without scoped guidance.
    for key in [
        "window",
        "promise_tier",
        "subset_ok",
        "recommendation",
        "note",
        "sound_candidates",
    ] {
        assert!(v.get(key).is_some(), "stable key missing: {key} in {v}");
    }
    assert!(v["sound_candidates"].as_array().is_some());
}

#[test]
fn who_calls_payload_separates_and_flags_high_freq() {
    let hits = vec![
        ref_row(
            Some("rs.di.impl_trait"),
            "src/a.rs",
            1,
            Confidence::Heuristic,
        ),
        ref_row(None, "src/b.rs", 2, Confidence::Exact),
    ];
    let v = build_who_calls_payload("fmt", false, 50, &hits, true, "ast_modeled");
    assert_eq!(v["tool"], "who_calls");
    assert_eq!(v["noisy"], false);
    assert_eq!(v["high_freq_name"], true);
    assert_eq!(v["promise_tier"], "ast_modeled");
    assert_eq!(v["window"], "default");
    let callers = v["callers"].as_array().unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0]["edge_role"], "call");
    let imps = v["implementors"].as_array().unwrap();
    assert_eq!(imps.len(), 1);
    assert_eq!(imps[0]["edge_role"], "implementor");
    assert_eq!(v["implementor_count"], 1);
    assert!(v["recommendation"]
        .as_str()
        .unwrap()
        .contains("high_freq_name=true"));
    assert_eq!(v["note"], RECIPE_NOTE);
}

#[test]
fn who_calls_payload_noisy_merges() {
    let hits = vec![
        ref_row(
            Some("rs.di.impl_trait"),
            "src/a.rs",
            1,
            Confidence::Heuristic,
        ),
        ref_row(None, "src/b.rs", 2, Confidence::Exact),
    ];
    let v = build_who_calls_payload("area", true, 50, &hits, true, "ast_modeled");
    assert_eq!(v["noisy"], true);
    assert_eq!(v["high_freq_name"], false);
    let callers = v["callers"].as_array().unwrap();
    assert_eq!(callers.len(), 2);
    let roles: Vec<_> = callers
        .iter()
        .map(|r| r["edge_role"].as_str().unwrap())
        .collect();
    assert!(roles.contains(&"implementor"));
    assert!(roles.contains(&"call"));
    assert!(v["recommendation"].as_str().unwrap().contains("noisy=true"));
}

// ---------------------------------------------------------------------------
// P0-4: scoped sound guidance when window is default/disabled
// ---------------------------------------------------------------------------

fn candidate(root_or_path: &str, scope: SoundScopeKind, eligible: bool) -> SoundCandidate {
    SoundCandidate {
        key: root_or_path.into(),
        kind: scope,
        promise_tier: if eligible {
            "ast_modeled".into()
        } else {
            "disabled".into()
        },
        reason: if eligible {
            "0 S violations".into()
        } else {
            "S violations".into()
        },
        sound_eligible: eligible,
    }
}

fn aggregation(scope: SoundScopeKind, candidates: Vec<SoundCandidate>) -> SoundAggregation {
    SoundAggregation {
        scope,
        buckets: vec![],
        sound_candidates: candidates,
        recommendation: String::new(),
    }
}

#[test]
fn unit_scoped_guidance_names_eligible_root_and_example_command() {
    let agg = aggregation(
        SoundScopeKind::Root,
        vec![
            candidate("api", SoundScopeKind::Root, true),
            candidate("nn-ranker", SoundScopeKind::Root, false),
        ],
    );
    let g = build_scoped_sound_guidance("createUser", &agg, false, false);
    assert!(g.has_eligible);
    assert_eq!(g.sound_candidates.len(), 2);
    assert_eq!(
        g.sound_candidates[0]
            .get("root_id")
            .and_then(|v| v.as_str()),
        Some("api"),
        "eligible first: {:?}",
        g.sound_candidates
    );
    let cmd = g.example_command.as_deref().unwrap_or_default();
    assert!(
        cmd.contains("impact createUser --sound --workspace-root api"),
        "example command: {cmd}"
    );
    assert!(g.guidance.contains("api"), "{}", g.guidance);
    assert!(g.guidance.contains("nn-ranker"), "{}", g.guidance);
    // Union dirty must never be framed as sound.
    assert!(
        !g.guidance.contains("window=sound"),
        "guidance must not claim union sound: {}",
        g.guidance
    );
}

#[test]
fn unit_scoped_guidance_all_dirty_honest_no_sound_command() {
    let agg = aggregation(
        SoundScopeKind::Root,
        vec![
            candidate("dirty-a", SoundScopeKind::Root, false),
            candidate("dirty-b", SoundScopeKind::Root, false),
        ],
    );
    let g = build_scoped_sound_guidance("helper", &agg, false, false);
    assert!(!g.has_eligible);
    assert!(g.example_command.is_none());
    assert!(
        g.guidance.contains("no sound-eligible") || g.guidance.contains("every scope"),
        "honest all-dirty: {}",
        g.guidance
    );
    assert!(
        !g.guidance.contains("--sound --workspace-root"),
        "all-dirty must not emit scoped sound command: {}",
        g.guidance
    );
    assert!(
        g.guidance.to_lowercase().contains("default")
            && (g.guidance.contains("implementor") || g.guidance.contains("implementors")),
        "suggest default + review implementors: {}",
        g.guidance
    );
    assert!(
        !g.guidance.contains("blind --recall")
            || g.guidance.contains("do NOT")
            || g.guidance.contains("never"),
        "must never suggest blind --recall as default: {}",
        g.guidance
    );
}

#[test]
fn unit_scoped_guidance_top_dir_path_hint() {
    let agg = aggregation(
        SoundScopeKind::TopDir,
        vec![
            candidate("repository", SoundScopeKind::TopDir, true),
            candidate("nn-ranker", SoundScopeKind::TopDir, false),
        ],
    );
    let g = build_scoped_sound_guidance("insert", &agg, false, false);
    assert!(g.has_eligible);
    assert!(
        g.guidance.contains("by_top_dir") || g.guidance.contains("repository"),
        "top_dir path hint: {}",
        g.guidance
    );
    assert!(g.guidance.contains("repository"), "{}", g.guidance);
}

#[test]
fn unit_scoped_guidance_mentions_stale_flags_when_true() {
    let agg = aggregation(
        SoundScopeKind::Root,
        vec![candidate("api", SoundScopeKind::Root, true)],
    );
    let g = build_scoped_sound_guidance("x", &agg, true, true);
    assert!(g.baseline_stale);
    assert!(g.sidecar_stale);
    assert!(g.guidance.contains("baseline_stale"), "{}", g.guidance);
    assert!(g.guidance.contains("sidecar_stale"), "{}", g.guidance);
}

#[test]
fn unit_blast_payload_with_guidance_keeps_stable_keys() {
    let d = decide_blast_window(false, Some("ws"));
    let agg = aggregation(
        SoundScopeKind::Root,
        vec![candidate("api", SoundScopeKind::Root, true)],
    );
    let g = build_scoped_sound_guidance("createUser", &agg, true, false);
    let rec = format!("{}; {}", d.recommendation, g.guidance);
    let window = BlastWindowDecision {
        recommendation: rec,
        ..d
    };
    let v = build_blast_radius_payload(BlastRadiusPayloadInput {
        symbol: "createUser".into(),
        depth: 3,
        limit: 50,
        nodes: vec![],
        window,
        promise_tier: "disabled".into(),
        languages: vec!["typescript".into()],
        include_macro: false,
        include_macro_reason: None,
        stale: None,
        scoped_sound: g,
    });
    assert_eq!(v["window"], "default");
    assert_ne!(v["window"], "sound");
    for key in [
        "window",
        "promise_tier",
        "subset_ok",
        "recommendation",
        "note",
        "sound_candidates",
    ] {
        assert!(v.get(key).is_some(), "stable key missing: {key}");
    }
    let cands = v["sound_candidates"]
        .as_array()
        .expect("sound_candidates[]");
    assert_eq!(cands[0]["root_id"], "api");
    assert_eq!(cands[0]["sound_eligible"], true);
    assert!(v["example_command"]
        .as_str()
        .unwrap_or("")
        .contains("--sound --workspace-root api"));
    assert_eq!(v["baseline_stale"], true);
    assert!(v["recommendation"]
        .as_str()
        .unwrap()
        .contains("--sound --workspace-root api"));
}

// ---------------------------------------------------------------------------
// CLI e2e
// ---------------------------------------------------------------------------

#[test]
fn e2e_cli_help_lists_recipe_commands() {
    let h = run(Path::new("."), &["--help"]);
    // --help on empty root still prints clap help
    let text = stdout(&h) + &stderr(&h);
    assert!(
        text.contains("blast-radius") || text.contains("BlastRadius"),
        "{text}"
    );
    assert!(
        text.contains("who-calls") || text.contains("WhoCalls"),
        "{text}"
    );

    let br = Command::new(bin())
        .args(["blast-radius", "--help"])
        .output()
        .expect("blast-radius help");
    let bt = stdout(&br) + &stderr(&br);
    assert!(
        bt.contains("include-macro") || bt.contains("include_macro"),
        "{bt}"
    );
    assert!(bt.contains("--depth"), "{bt}");

    let wc = Command::new(bin())
        .args(["who-calls", "--help"])
        .output()
        .expect("who-calls help");
    let wt = stdout(&wc) + &stderr(&wc);
    assert!(wt.contains("--noisy") || wt.contains("noisy"), "{wt}");
}

#[test]
fn e2e_blast_radius_clean_js_sound_window() {
    let root = temp_root("blast-clean");
    write_clean_js(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = run(&root, &["blast-radius", "createUser"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["tool"], "blast_radius");
    assert_eq!(v["symbol"], "createUser");
    // Clean AST-modeled TS corpus → auto sound window.
    assert_eq!(v["window"], "sound", "payload={v}");
    assert_eq!(v["subset_ok"], true);
    assert_eq!(v["promise_tier"], "ast_modeled");
    assert!(v["nodes"].is_array());
    assert!(v["recommendation"].as_str().unwrap().len() > 8);
    // Clean window: short recommendation (already sound) — no scoped command spam.
    let rec = v["recommendation"].as_str().unwrap();
    assert!(
        rec.contains("subset_ok") || rec.contains("sound window"),
        "clean rec: {rec}"
    );
    assert!(
        !rec.contains("--sound --workspace-root"),
        "clean window rec must stay short: {rec}"
    );
    // Stable keys present even on sound window.
    assert!(v.get("sound_candidates").is_some(), "payload={v}");
    assert_eq!(v["include_macro"], false);
    assert!(v["note"]
        .as_str()
        .unwrap()
        .contains("not a complete runtime graph"));
}

fn write_clean_root_fixture(root: &Path) {
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
}

fn write_dirty_root_fixture(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/bad.ts"),
        r#"
export function evil(c: string) {
  return eval(c);
}
export function helper() { return 1; }
"#,
    )
    .unwrap();
}

fn index_workspace(roots: &[&Path], db: &Path) {
    let mut args: Vec<String> = vec!["index".into()];
    for p in roots {
        args.push("--workspace-root".into());
        args.push(p.to_string_lossy().into_owned());
    }
    args.push("--workspace-db".into());
    args.push(db.to_string_lossy().into_owned());
    args.push("--force".into());
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = Command::new(bin())
        .args(&refs)
        .stdin(Stdio::null())
        .output()
        .expect("workspace index");
    assert!(out.status.success(), "workspace index: {}", stderr(&out));
}

#[test]
fn e2e_blast_radius_dirty_multiroot_recommends_scoped_sound() {
    let base = temp_root("blast-multi");
    let api = base.join("api");
    let nn = base.join("nn-ranker");
    write_clean_root_fixture(&api);
    write_dirty_root_fixture(&nn);
    let db = base.join("ws.db");
    index_workspace(&[&api, &nn], &db);

    // Union blast_radius (no --workspace-root): dirty sibling → default window.
    let out = Command::new(bin())
        .args([
            "blast-radius",
            "createUser",
            "--workspace-db",
            db.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .output()
        .expect("blast multi");
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["window"], "default", "dirty union must not be sound: {v}");
    assert_eq!(v["subset_ok"], false);
    assert_ne!(v["window"], "sound");
    assert_ne!(v["window"], "recall");

    let rec = v["recommendation"].as_str().unwrap().to_string();
    assert!(
        rec.contains("api"),
        "recommendation must name clean root id: {rec}"
    );
    assert!(
        rec.contains("--sound --workspace-root") || rec.contains("--workspace-root"),
        "recommendation must include example legal command: {rec}"
    );
    assert!(
        rec.contains("impact") && rec.contains("createUser"),
        "example command should reference the query symbol: {rec}"
    );

    let cands = v["sound_candidates"]
        .as_array()
        .unwrap_or_else(|| panic!("sound_candidates required on default window: {v}"));
    assert!(!cands.is_empty(), "{v}");
    let api_c = cands
        .iter()
        .find(|c| c.get("root_id").and_then(|r| r.as_str()) == Some("api"))
        .expect("api candidate");
    assert_eq!(api_c["sound_eligible"], true, "{api_c}");
    // Eligible first.
    assert_eq!(
        cands[0].get("root_id").and_then(|r| r.as_str()),
        Some("api"),
        "eligible first: {cands:?}"
    );

    // Stable honesty keys still present.
    for key in [
        "window",
        "promise_tier",
        "subset_ok",
        "recommendation",
        "note",
    ] {
        assert!(v.get(key).is_some(), "missing {key}: {v}");
    }

    // Scoped to clean root → still allowed to select sound (not union).
    let scoped = Command::new(bin())
        .args([
            "blast-radius",
            "createUser",
            "--workspace-db",
            db.to_str().unwrap(),
            "--workspace-root",
            api.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .output()
        .expect("blast scoped");
    assert!(scoped.status.success(), "{}", stderr(&scoped));
    let sv: serde_json::Value = serde_json::from_str(&stdout(&scoped)).unwrap();
    assert_eq!(
        sv["window"], "sound",
        "clean root filter may select sound: {sv}"
    );
}

#[test]
fn e2e_blast_radius_all_dirty_honest_no_sound_command() {
    let base = temp_root("blast-all-dirty");
    let a = base.join("dirty-a");
    let b = base.join("dirty-b");
    write_dirty_root_fixture(&a);
    write_dirty_root_fixture(&b);
    let db = base.join("ws.db");
    index_workspace(&[&a, &b], &db);

    let out = Command::new(bin())
        .args([
            "blast-radius",
            "helper",
            "--workspace-db",
            db.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .output()
        .expect("blast all-dirty");
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["window"], "default");
    assert_eq!(v["subset_ok"], false);
    assert_ne!(v["window"], "sound");
    assert_ne!(v["window"], "recall");

    let rec = v["recommendation"].as_str().unwrap().to_lowercase();
    assert!(
        rec.contains("no sound-eligible")
            || rec.contains("every scope")
            || rec.contains("s violations"),
        "all-dirty recommendation must be honest: {}",
        v["recommendation"]
    );
    assert!(
        !rec.contains("--sound --workspace-root"),
        "all-dirty must not suggest scoped sound command: {}",
        v["recommendation"]
    );
    assert!(
        rec.contains("default") && rec.contains("implementor"),
        "suggest default blast_radius + review implementors: {}",
        v["recommendation"]
    );
    // Never present blind --recall as the default next step.
    if rec.contains("--recall") {
        assert!(
            rec.contains("do not")
                || rec.contains("never")
                || rec.contains("don't")
                || rec.contains("do NOT"),
            "if --recall is mentioned it must be prohibited: {}",
            v["recommendation"]
        );
    }

    let cands = v["sound_candidates"].as_array().expect("candidates");
    assert!(!cands.is_empty());
    assert!(
        cands.iter().all(|c| c["sound_eligible"] == false),
        "all candidates ineligible: {cands:?}"
    );
}

#[test]
fn e2e_blast_radius_single_root_dirty_by_top_dir_hint() {
    let base = temp_root("blast-topdir");
    std::fs::create_dir_all(base.join("repository/src")).unwrap();
    std::fs::create_dir_all(base.join("nn-ranker/src")).unwrap();
    std::fs::write(
        base.join("repository/src/auth.ts"),
        r#"
export function createUser(email: string) {
  return { email };
}
"#,
    )
    .unwrap();
    std::fs::write(
        base.join("nn-ranker/src/bad.ts"),
        "export function evil(c: string) { return eval(c); }\nexport function helper() { return 1; }\n",
    )
    .unwrap();
    let idx = run(&base, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = run(&base, &["blast-radius", "createUser"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["window"], "default", "single-root store is dirty: {v}");
    let rec = v["recommendation"].as_str().unwrap();
    assert!(
        rec.contains("by_top_dir") || rec.contains("repository"),
        "single-root path hint: {rec}"
    );
    assert!(rec.contains("repository"), "clean top dir named: {rec}");
    // sound_candidates present (stable key) with eligible first.
    let cands = v["sound_candidates"].as_array().expect("sound_candidates");
    assert!(
        cands.iter().any(
            |c| c.get("path").and_then(|p| p.as_str()) == Some("repository")
                && c["sound_eligible"] == true
        ),
        "{cands:?}"
    );
}

#[test]
fn e2e_blast_radius_eval_js_default_window() {
    let root = temp_root("blast-eval");
    write_eval_js(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = run(&root, &["blast-radius", "helper"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["window"], "default", "payload={v}");
    assert_eq!(v["subset_ok"], false);
    assert_eq!(v["promise_tier"], "disabled");
    let rec = v["recommendation"].as_str().unwrap().to_lowercase();
    assert!(
        rec.contains("disabled") || rec.contains("violat") || rec.contains("default"),
        "recommendation must explain disabled sound window: {}",
        v["recommendation"]
    );
    // Must NOT be blind --recall window.
    assert_ne!(v["window"], "recall");
    // P0-4: still carries stable keys + honest scoped guidance when nothing is clean.
    assert!(v.get("sound_candidates").is_some(), "payload={v}");
}

#[test]
fn e2e_blast_radius_missing_sidecar_include_macro_refused() {
    let root = temp_root("blast-macro");
    write_clean_js(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = run(&root, &["blast-radius", "createUser", "--include-macro"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["include_macro"], false);
    let reason = v["include_macro_reason"]
        .as_str()
        .expect("include_macro_reason required when refused");
    assert!(
        reason.to_lowercase().contains("missing")
            || reason.to_lowercase().contains("macro")
            || reason.to_lowercase().contains("sound"),
        "reason={reason}"
    );
}

#[test]
fn e2e_who_calls_noisy_separation() {
    let root = temp_root("who-sep");
    write_rust_trait_calls(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    // noisy=false (default): implementors separated, not flooded into callers
    let quiet = run(&root, &["who-calls", "area"]);
    assert!(quiet.status.success(), "{}", stderr(&quiet));
    let qv: serde_json::Value = serde_json::from_str(&stdout(&quiet)).unwrap();
    assert_eq!(qv["tool"], "who_calls");
    assert_eq!(qv["noisy"], false);
    assert!(qv["high_freq_name"].as_bool().is_some());
    assert!(qv["promise_tier"].as_str().is_some());
    assert!(qv["recommendation"].as_str().unwrap().len() > 8);
    let qcallers = qv["callers"].as_array().expect("callers[]");
    for row in qcallers {
        let role = row["edge_role"].as_str().unwrap();
        assert_ne!(
            role, "implementor",
            "noisy=false must hide implementors in callers: {row}"
        );
    }
    let qimps = qv["implementors"].as_array().expect("implementors[]");
    assert!(
        !qimps.is_empty(),
        "implementors should be present separately: {qv}"
    );
    assert!(qv["implementor_count"].as_u64().unwrap() >= 2);

    // noisy=true: implementors merged into callers
    let noisy = run(&root, &["who-calls", "area", "--noisy"]);
    assert!(noisy.status.success(), "{}", stderr(&noisy));
    let nv: serde_json::Value = serde_json::from_str(&stdout(&noisy)).unwrap();
    assert_eq!(nv["noisy"], true);
    let ncallers = nv["callers"].as_array().expect("callers[]");
    assert!(
        ncallers.iter().any(|r| r["edge_role"] == "implementor"),
        "noisy=true must merge implementors: {nv}"
    );
}

#[test]
fn e2e_who_calls_high_freq_name_flag() {
    let root = temp_root("who-hf");
    write_rust_trait_flood(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = run(&root, &["who-calls", "fmt", "--limit", "50"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["high_freq_name"], true);
    assert_eq!(v["tool"], "who_calls");
    let empty: Vec<serde_json::Value> = Vec::new();
    let imps = v["implementors"].as_array().unwrap_or(&empty);
    let count = v["implementor_count"].as_u64().unwrap_or(0);
    // High-freq demote: implementors capped at 20 when count > 20
    if count > 20 {
        assert_eq!(v["implementors_truncated"], true);
        assert!(imps.len() <= 20);
    }
    // callers must not be flooded with implementors under default recipe
    let callers = v["callers"].as_array().unwrap();
    for row in callers {
        assert_ne!(row["edge_role"], "implementor");
    }
}

// ---------------------------------------------------------------------------
// MCP e2e
// ---------------------------------------------------------------------------

fn mcp_tools_list(root: &Path) -> String {
    let mut child = Command::new(bin())
        .arg("--root")
        .arg(root)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp");
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin
            .write_all(
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
"#,
            )
            .unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("mcp out");
    stdout(&out)
}

#[test]
fn e2e_mcp_tools_list_has_blast_radius_and_who_calls() {
    let root = temp_root("mcp-list");
    write_clean_js(&root);
    let _ = run(&root, &["index", "--force"]);
    let text = mcp_tools_list(&root);
    assert!(
        text.contains("blast_radius"),
        "tools/list must include blast_radius: {text}"
    );
    assert!(
        text.contains("who_calls"),
        "tools/list must include who_calls: {text}"
    );
    // Honest descriptions — no oversell slogans in tool docs.
    assert!(!text.contains("零漏报"));
    assert!(!text.to_lowercase().contains("zero-miss coverage"));
}

#[test]
fn e2e_mcp_blast_radius_and_who_calls_tools() {
    use std::io::Write;
    let root = temp_root("mcp-call");
    write_clean_js(&root);
    let _ = run(&root, &["index", "--force"]);

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
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"force":true}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"blast_radius","arguments":{"symbol":"createUser","depth":3}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"who_calls","arguments":{"symbol":"createUser","noisy":false}}}"#,
            "\n",
        );
        stdin.write_all(msgs.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("mcp out");
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("blast_radius") || text.contains("\"window\""),
        "{text}"
    );
    assert!(
        text.contains("who_calls") || text.contains("high_freq_name"),
        "{text}"
    );
    assert!(
        text.contains("promise_tier") || text.contains("sound"),
        "{text}"
    );
}

// ---------------------------------------------------------------------------
// Docs claims: recipe flags/commands must exist in clap
// ---------------------------------------------------------------------------

#[test]
fn docs_agent_recipes_flags_exist_in_clap() {
    let doc = "docs/agent-recipes.md";
    assert!(
        Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join(doc)
            .is_file(),
        "docs/agent-recipes.md must exist"
    );
    let mut cmd = Command::new({
        let mut last = "python".to_string();
        for cand in ["python", "python3", "py"] {
            let mut probe = Command::new(cand);
            probe
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if probe.status().map(|s| s.success()).unwrap_or(false) {
                last = cand.to_string();
                break;
            }
        }
        last
    });
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let script = PathBuf::from(&manifest)
        .join("scripts")
        .join("check_docs_claims.py");
    let out = cmd
        .arg(&script)
        .arg("--doc")
        .arg("docs/agent-recipes.md")
        .arg("--cli")
        .arg("src/cli.rs")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .current_dir(&manifest)
        .output()
        .expect("run docs claims on agent-recipes");
    let text = stdout(&out) + &stderr(&out);
    assert_eq!(
        out.status.code().unwrap_or(-1),
        0,
        "agent-recipes docs must pass claims checker:\n{text}"
    );
}

#[test]
fn unit_decision_struct_fields() {
    // Decision is Copy/Clone and stable for CLI/MCP shared path.
    let a: BlastWindowDecision = decide_blast_window(true, None);
    let b = a.clone();
    assert_eq!(a, b);
}
