//! R31 adversarial probes — noise governance role fetch + workspace sound gates.
//!
//! Locked contracts (docs/noise-governance.md):
//! - Exact user calls in `callers[]` are never dropped under default Separate mode
//!   (even when implementor flood exists and `--limit` is small).
//! - `--include-implementors` merge must still surface Exact call rows when they
//!   exist in the store (limit interaction must not starve them behind path-ordered
//!   implementor edges).
//! - `rs.di.impl_trait` remains sound-eligible; `rs.di.dyn_trait_method` stays Unsound.
//! - Workspace union `--sound` uses weakest-root S gate (subset_ok false when any root dirty).

use agentgraph::index::subset::is_sound_eligible;
use agentgraph::model::{Confidence, EdgeKind, Evidence, ReferenceRecord, HIGH_FREQ_NAMES};
use agentgraph::query::{build_callers_payload, CallersRoleMode};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-r31-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
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

fn run_raw(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph raw")
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
        .unwrap_or_else(|e| panic!("json parse ({e}): stdout={text} stderr={}", stderr(out)))
}

fn ref_row(
    name: &str,
    path: &str,
    line: usize,
    enclosing: &str,
    confidence: Confidence,
    rule_id: Option<&str>,
) -> ReferenceRecord {
    ReferenceRecord {
        name: name.into(),
        kind: EdgeKind::Call,
        path: path.into(),
        line,
        enclosing: Some(enclosing.into()),
        module: None,
        resolved: None,
        qualifier: None,
        confidence,
        evidence: rule_id.map(|r| Evidence {
            rule_id: r.into(),
            snippet: String::new(),
        }),
        root_id: String::new(),
    }
}

fn write_impl_flood_fixture(root: &Path, n_impls: usize) {
    let mut src = String::from("trait T { fn fmt(&self) -> String; }\n");
    for i in 0..n_impls {
        // Paths sort before the Exact call site (src/z_user.rs).
        src.push_str(&format!(
            "struct S{i};\nimpl T for S{i} {{ fn fmt(&self) -> String {{ \"x\".into() }} }}\n"
        ));
    }
    src.push_str("fn show(t: &dyn T) -> String { t.fmt() }\nfn main() { let _ = show(&S0); }\n");
    // Exact call lives in a late-sorting file via path order in refs:
    // store ORDER BY root_id, path, line — put the call in src/z_user.rs.
    std::fs::create_dir_all(root.join("src")).unwrap();
    // Split: impls in a.rs, exact call in z_user.rs
    let mut impls = String::from("pub trait T { fn fmt(&self) -> String; }\n");
    for i in 0..n_impls {
        impls.push_str(&format!(
            "pub struct S{i};\nimpl T for S{i} {{ fn fmt(&self) -> String {{ \"x\".into() }} }}\n"
        ));
    }
    std::fs::write(root.join("src/a_impls.rs"), impls).unwrap();
    std::fs::write(
        root.join("src/z_user.rs"),
        "use crate::a_impls::{T, S0};\npub fn show(t: &dyn T) -> String { t.fmt() }\npub fn main() { let _ = show(&S0); }\n",
    )
    .unwrap();
    let _ = src;
}

// ── Unit: Separate-mode payload keeps Exact when implementors come first ──

#[test]
fn separate_payload_keeps_exact_when_implementors_precede() {
    let mut hits = Vec::new();
    for i in 0..40 {
        hits.push(ref_row(
            "fmt",
            &format!("src/a_impl{i}.rs"),
            i + 1,
            "Impl",
            Confidence::Heuristic,
            Some("rs.di.impl_trait"),
        ));
    }
    // Exact call sorts last by path.
    hits.push(ref_row(
        "fmt",
        "src/z_user.rs",
        99,
        "show",
        Confidence::Exact,
        None,
    ));
    // Simulate store fetch that only returned the first N path-ordered rows
    // (role_fetch_cap interaction with low --limit).
    let limited: Vec<_> = hits.iter().take(8).cloned().collect();
    // After SQL window: only implementors — that is the starvation bug shape.
    // The *contract* is that the product path must not produce this window when
    // Exact edges exist. Unit-level: build_callers_payload on a *complete* hit
    // list with small limit must still keep Exact in callers[].
    let v = build_callers_payload("fmt", hits.clone(), 5, CallersRoleMode::Separate);
    assert!(v.is_object(), "fmt + implementors wraps: {v}");
    let calls = v["callers"].as_array().expect("callers[]");
    assert!(
        calls.iter().any(|r| r["edge_role"] == "call"),
        "Separate + limit=5 must keep Exact call in callers[]: {v}"
    );
    // Product path: callers_for_roles / CLI must deliver the Exact row into hits
    // even when implementors flood earlier paths — covered by e2e below.
    let _ = limited;
}

// ── Unit: IncludeImplementors merge must not drop Exact behind impl flood ──

#[test]
fn include_implementors_merge_keeps_exact_call_row() {
    let mut hits = Vec::new();
    for i in 0..30 {
        hits.push(ref_row(
            "fmt",
            &format!("src/a_impl{i}.rs"),
            i + 1,
            "Impl",
            Confidence::Heuristic,
            Some("rs.di.impl_trait"),
        ));
    }
    hits.push(ref_row(
        "fmt",
        "src/z_user.rs",
        99,
        "show",
        Confidence::Exact,
        None,
    ));
    let v = build_callers_payload("fmt", hits, 5, CallersRoleMode::IncludeImplementors);
    assert!(v.is_array(), "include-implementors → array: {v}");
    let arr = v.as_array().unwrap();
    assert!(
        arr.iter().any(|r| r["edge_role"] == "call"),
        "include-implementors + limit=5 must still surface the Exact call: {v}"
    );
}

// ── e2e: low --limit + implementor flood must not starve callers[] ────────

#[test]
fn e2e_low_limit_impl_flood_does_not_starve_exact_callers() {
    assert!(is_high_freq_name_for_test("fmt"));
    let root = temp_root("starve");
    write_impl_flood_fixture(&root, 220);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    // Default Separate mode, tiny limit — Exact call in z_user.rs must appear.
    let out = run(&root, &["callers", "fmt", "--limit", "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v = parse_json(&out);
    let calls = v["callers"]
        .as_array()
        .unwrap_or_else(|| panic!("expected wrapped object with callers[]: {v}"));
    assert!(
        calls.iter().any(|r| {
            r["edge_role"] == "call"
                && r["path"]
                    .as_str()
                    .map(|p| p.contains("z_user"))
                    .unwrap_or(false)
        }),
        "low --limit + 220 implementors must still return Exact call from z_user: {v}"
    );

    // --include-implementors must also surface the Exact call under small limit.
    let merged = run(
        &root,
        &["callers", "fmt", "--include-implementors", "--limit", "5"],
    );
    assert!(merged.status.success(), "{}", stderr(&merged));
    let mv = parse_json(&merged);
    let marr = mv.as_array().expect("include-implementors array");
    assert!(
        marr.iter().any(|r| r["edge_role"] == "call"),
        "include-implementors + --limit 5 must surface Exact call: {mv}"
    );
}

fn is_high_freq_name_for_test(n: &str) -> bool {
    HIGH_FREQ_NAMES.iter().any(|h| h.eq_ignore_ascii_case(n))
}

// ── Sound eligibility: impl_trait allowlisted, dyn_trait still Unsound ────

#[test]
fn sound_eligibility_impl_vs_dyn_trait() {
    assert!(
        is_sound_eligible(Confidence::Heuristic, Some("rs.di.impl_trait")),
        "rs.di.impl_trait is allowlisted for sound walk"
    );
    assert!(
        !is_sound_eligible(Confidence::Heuristic, Some("rs.di.dyn_trait_method")),
        "rs.di.dyn_trait_method must stay Unsound (open dyn dispatch)"
    );
}

// ── Workspace: union impact/callers --sound must not claim OK on dirty union ──

#[test]
fn workspace_union_sound_not_ok_when_any_root_dirty() {
    let base = temp_root("ws-sound");
    let root_a = base.join("clean");
    let root_b = base.join("dirty");
    std::fs::create_dir_all(root_a.join("src")).unwrap();
    std::fs::create_dir_all(root_b.join("src")).unwrap();
    std::fs::write(
        root_a.join("src/ok.ts"),
        "export function probeOk(x: number): number { return x + 1; }\nexport function callerOfOk(): number { return probeOk(1); }\n",
    )
    .unwrap();
    std::fs::write(
        root_b.join("src/bad.ts"),
        "export function bad(code: string): unknown { return eval(code); }\n",
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

    // Union impact --sound (no --workspace-root): weakest-root → subset_ok false.
    let sound = run_raw(&[
        "impact",
        "probeOk",
        "--sound",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    // Command may succeed with disabled sound payload, or exit non-zero.
    let payload = parse_json(&sound);
    if payload.get("mode").and_then(|v| v.as_str()) == Some("sound") {
        assert_eq!(
            payload["subset_ok"], false,
            "union sound must not claim OK on dirty union: {payload}"
        );
    } else {
        // Fail-loud is acceptable if it refuses sound on dirty union.
        assert!(
            !sound.status.success() || payload["subset_ok"] == false,
            "union sound must refuse or disable on dirty union: {payload} {}",
            stderr(&sound)
        );
    }

    // Clean-root scoped sound may claim OK.
    let scoped = run_raw(&[
        "impact",
        "probeOk",
        "--sound",
        "--workspace-root",
        root_a.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    if scoped.status.success() {
        let sp = parse_json(&scoped);
        if sp.get("mode").and_then(|v| v.as_str()) == Some("sound") {
            assert_eq!(sp["subset_ok"], true, "clean root sound should be OK: {sp}");
        }
    }
}

// ── MCP/CLI payload shape symmetry is builder-level (same fn) — smoke only ──
// Shape lock is already in tests/noise_roles.rs; this guards the role-merge
// Exact-presence contract shared by CLI and MCP (both call build_callers_payload).
