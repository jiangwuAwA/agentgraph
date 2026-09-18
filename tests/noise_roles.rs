//! Noise governance (L1): callers vs implementor separation + high-freq demote.
//!
//! Design (locked):
//! - Query-time `edge_role` from `rule_id` + confidence (no DB migration).
//! - Default `callers`: plain array when zero implementors; wrapped object
//!   `{callers, implementors, implementor_count, implementors_truncated, truncated, note}`
//!   when any implementor is present.
//! - `--include-implementors` merges (old noisy shape + edge_role tags).
//! - `--implementors-only` returns implementors section only.
//! - `--exact-only` stays pure Exact calls (plain array).
//! - HIGH_FREQ_NAMES: implementors section capped at 20 + truncated flag.
//! - impact rows carry `edge_role`; store still keeps all edges.

use agentgraph::model::{
    edge_role_for, is_high_freq_name, Confidence, EdgeKind, Evidence, ImpactNode, ReferenceRecord,
    HIGH_FREQ_IMPLEMENTOR_CAP, HIGH_FREQ_NAMES,
};
use agentgraph::query::{build_callers_payload, CallersRoleMode};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-noise-{name}"));
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

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
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

// ---------------------------------------------------------------------------
// Unit: rule_id → edge_role mapping table
// ---------------------------------------------------------------------------

#[test]
fn rule_id_maps_to_edge_role() {
    // Implementor family
    assert_eq!(
        edge_role_for(Confidence::Heuristic, Some("rs.di.impl_trait")),
        agentgraph::model::EdgeRole::Implementor
    );
    assert_eq!(
        edge_role_for(Confidence::Heuristic, Some("rs.di.dyn_trait_method")),
        agentgraph::model::EdgeRole::Implementor
    );
    assert_eq!(
        edge_role_for(Confidence::Heuristic, Some("go.di.interface_impl")),
        agentgraph::model::EdgeRole::Implementor
    );
    assert_eq!(
        edge_role_for(Confidence::Heuristic, Some("go.di.interface_impl_v2")),
        agentgraph::model::EdgeRole::Implementor
    );
    assert_eq!(
        edge_role_for(Confidence::Heuristic, Some("go.di.interface_assert")),
        agentgraph::model::EdgeRole::Implementor
    );

    // Registration family
    for rid in [
        "ts.di.register",
        "ts.di.bind",
        "ts.nest.module_providers",
        "ts.nest.ctor_inject",
        "ts.framework.register",
        "py.di.entry_points",
        "go.di.route_register",
        "rs.di.inventory_submit",
        "rs.di.linkme_distributed_slice",
        "ts.event.subscribe",
    ] {
        assert_eq!(
            edge_role_for(Confidence::Heuristic, Some(rid)),
            agentgraph::model::EdgeRole::Registration,
            "rule {rid} must be registration"
        );
    }

    // Default Exact call
    assert_eq!(
        edge_role_for(Confidence::Exact, None),
        agentgraph::model::EdgeRole::Call
    );
    // DynamicCandidate → dynamic
    assert_eq!(
        edge_role_for(Confidence::DynamicCandidate, Some("ts.dynamic.computed")),
        agentgraph::model::EdgeRole::Dynamic
    );
    assert_eq!(
        edge_role_for(Confidence::DynamicCandidate, None),
        agentgraph::model::EdgeRole::Dynamic
    );
}

#[test]
fn high_freq_names_constant_contains_std_ish() {
    assert!(is_high_freq_name("fmt"));
    assert!(is_high_freq_name("Drop")); // case-insensitive
    assert!(is_high_freq_name("clone"));
    assert!(is_high_freq_name("default"));
    assert!(!is_high_freq_name("createUser"));
    assert!(HIGH_FREQ_NAMES.len() >= 10);
    assert_eq!(HIGH_FREQ_IMPLEMENTOR_CAP, 20);
}

// ---------------------------------------------------------------------------
// Unit: payload shape
// ---------------------------------------------------------------------------

#[test]
fn payload_plain_array_when_no_implementors() {
    let hits = vec![
        ref_row(
            "helper",
            "src/a.ts",
            1,
            "createUser",
            Confidence::Exact,
            None,
        ),
        ref_row(
            "helper",
            "src/b.ts",
            2,
            "registerHelper",
            Confidence::Heuristic,
            Some("ts.di.register"),
        ),
    ];
    let v = build_callers_payload("helper", hits, 50, CallersRoleMode::Separate);
    assert!(v.is_array(), "zero implementors → plain array: {v}");
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["edge_role"], "call");
    assert_eq!(arr[1]["edge_role"], "registration");
    assert!(arr[0]["at"].as_str().unwrap().contains("src/a.ts"));
}

#[test]
fn payload_wraps_when_implementors_present() {
    let hits = vec![
        ref_row(
            "area",
            "src/main.rs",
            14,
            "Circle",
            Confidence::Heuristic,
            Some("rs.di.impl_trait"),
        ),
        ref_row(
            "area",
            "src/main.rs",
            20,
            "Rect",
            Confidence::Heuristic,
            Some("rs.di.impl_trait"),
        ),
        ref_row("area", "src/main.rs", 33, "main", Confidence::Exact, None),
    ];
    let v = build_callers_payload("area", hits, 50, CallersRoleMode::Separate);
    assert!(v.is_object(), "implementors present → wrapped object: {v}");
    assert_eq!(v["callers"].as_array().unwrap().len(), 1);
    assert_eq!(v["implementors"].as_array().unwrap().len(), 2);
    assert_eq!(v["implementor_count"], 2);
    assert_eq!(v["implementors_truncated"], false);
    assert_eq!(v["callers"][0]["edge_role"], "call");
    assert_eq!(v["implementors"][0]["edge_role"], "implementor");
    assert!(v.get("note").is_some());
}

#[test]
fn payload_high_freq_caps_implementors() {
    // 25 implementor rows on a high-freq name → implementors capped at 20.
    let mut hits = Vec::new();
    for i in 0..25 {
        hits.push(ref_row(
            "fmt",
            &format!("src/t{i}.rs"),
            i + 1,
            "Impl",
            Confidence::Heuristic,
            Some("rs.di.impl_trait"),
        ));
    }
    // Exact user calls must not be dropped.
    hits.push(ref_row(
        "fmt",
        "src/user.rs",
        99,
        "render",
        Confidence::Exact,
        None,
    ));
    let v = build_callers_payload("fmt", hits, 50, CallersRoleMode::Separate);
    assert!(v.is_object());
    assert_eq!(v["implementors"].as_array().unwrap().len(), 20);
    assert_eq!(v["implementor_count"], 25);
    assert_eq!(v["implementors_truncated"], true);
    assert_eq!(v["callers"].as_array().unwrap().len(), 1);
    assert_eq!(v["callers"][0]["edge_role"], "call");
}

#[test]
fn payload_include_implementors_merges() {
    let hits = vec![
        ref_row(
            "area",
            "src/main.rs",
            14,
            "Circle",
            Confidence::Heuristic,
            Some("rs.di.impl_trait"),
        ),
        ref_row("area", "src/main.rs", 33, "main", Confidence::Exact, None),
    ];
    let v = build_callers_payload("area", hits, 50, CallersRoleMode::IncludeImplementors);
    assert!(
        v.is_array(),
        "include-implementors → merged plain array: {v}"
    );
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let roles: Vec<_> = arr
        .iter()
        .map(|r| r["edge_role"].as_str().unwrap())
        .collect();
    assert!(roles.contains(&"implementor"));
    assert!(roles.contains(&"call"));
}

#[test]
fn payload_exact_only_stays_pure_calls() {
    // Even if someone passes heuristic rows, ExactOnly mode is selected upstream
    // by the confidence filter. Here we assert Call classification + array shape
    // when only Exact rows are supplied (regression contract).
    let hits = vec![
        ref_row(
            "helper",
            "src/a.ts",
            1,
            "createUser",
            Confidence::Exact,
            None,
        ),
        ref_row("helper", "src/b.ts", 2, "login", Confidence::Exact, None),
    ];
    let v = build_callers_payload("helper", hits, 50, CallersRoleMode::Separate);
    assert!(v.is_array());
    for row in v.as_array().unwrap() {
        assert_eq!(row["edge_role"], "call");
        assert_eq!(row["confidence"], "exact");
    }
}

#[test]
fn impact_node_edge_role_field_optional() {
    let n = ImpactNode {
        name: "render".into(),
        path: "src/h.rs".into(),
        line: 11,
        kind: EdgeKind::Call,
        depth: 1,
        enclosing: Some("HtmlHandler".into()),
        resolved: None,
        confidence: Confidence::Heuristic,
        root_id: String::new(),
        edge_role: Some(agentgraph::model::EdgeRole::Implementor),
    };
    let v = serde_json::to_value(&n).unwrap();
    assert_eq!(v["edge_role"], "implementor");
}

// ---------------------------------------------------------------------------
// CLI e2e: trait impl + direct call → separated payload
// ---------------------------------------------------------------------------

fn write_rust_trait_fixture(root: &Path) {
    std::fs::write(
        root.join("src/main.rs"),
        r#"
trait Shape {
    fn area(&self) -> f64;
}

struct Circle { r: f64 }
struct Rect { w: f64, h: f64 }

impl Shape for Circle {
    fn area(&self) -> f64 { 3.0 }
}

impl Shape for Rect {
    fn area(&self) -> f64 { self.w * self.h }
}

fn paint(s: &dyn Shape) -> f64 {
    s.area()
}

fn main() {
    let c = Circle { r: 1.0 };
    println!("{}", paint(&c));
}
"#,
    )
    .unwrap();
}

#[test]
fn e2e_callers_separates_implementors_from_calls() {
    let root = temp_root("sep");
    write_rust_trait_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = run(&root, &["callers", "area"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(
        v.is_object(),
        "default callers on trait method must wrap when implementors exist: {v}"
    );
    let impls = v["implementors"].as_array().expect("implementors[]");
    assert!(!impls.is_empty(), "need implementor rows: {v}");
    for row in impls {
        assert_eq!(row["edge_role"], "implementor");
    }
    let calls = v["callers"].as_array().expect("callers[]");
    // Exact call site `s.area()` inside paint / map may appear as call.
    for row in calls {
        let role = row["edge_role"].as_str().unwrap();
        assert!(
            role == "call" || role == "registration" || role == "dynamic",
            "callers section must not mix implementor: {row}"
        );
    }
    assert!(v["implementor_count"].as_u64().unwrap() >= 2);

    // --include-implementors → merged array
    let merged = run(&root, &["callers", "area", "--include-implementors"]);
    assert!(merged.status.success(), "{}", stderr(&merged));
    let mv: serde_json::Value = serde_json::from_str(&stdout(&merged)).unwrap();
    assert!(mv.is_array(), "include-implementors merges: {mv}");
    let has_imp = mv
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["edge_role"] == "implementor");
    assert!(has_imp, "merged array keeps implementors: {mv}");

    // --implementors-only
    let only = run(&root, &["callers", "area", "--implementors-only"]);
    assert!(only.status.success(), "{}", stderr(&only));
    let ov: serde_json::Value = serde_json::from_str(&stdout(&only)).unwrap();
    let olist = ov["implementors"]
        .as_array()
        .unwrap_or_else(|| ov.as_array().expect("implementors-only shape"));
    assert!(!olist.is_empty());
    for row in olist {
        assert_eq!(row["edge_role"], "implementor");
    }

    // Regression: --exact-only stays pure Exact calls (array, no implementors flood)
    let exact = run(&root, &["callers", "area", "--exact-only"]);
    assert!(exact.status.success(), "{}", stderr(&exact));
    let ev: serde_json::Value = serde_json::from_str(&stdout(&exact)).unwrap();
    if ev.is_array() {
        for row in ev.as_array().unwrap() {
            assert_eq!(row["confidence"], "exact");
            assert_ne!(row["edge_role"], "implementor");
        }
    } else if ev.is_object() {
        // If Exact somehow has implementor rows (shouldn't), fail loudly.
        let imps = ev["implementors"].as_array().cloned().unwrap_or_default();
        assert!(
            imps.is_empty(),
            "exact-only must not return implementors: {ev}"
        );
    } else {
        panic!("unexpected exact-only shape: {ev}");
    }
}

#[test]
fn e2e_high_freq_name_demotes_implementors() {
    let root = temp_root("hf");
    // Many impls of a high-freq name (fmt-like) + one exact call.
    let mut src = String::from("trait T { fn fmt(&self) -> String; }\n");
    for i in 0..25 {
        src.push_str(&format!(
            "struct S{i};\nimpl T for S{i} {{ fn fmt(&self) -> String {{ \"x\".into() }} }}\n"
        ));
    }
    src.push_str("fn show(t: &dyn T) -> String { t.fmt() }\nfn main() { let _ = show(&S0); }\n");
    std::fs::write(root.join("src/main.rs"), src).unwrap();
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = run(&root, &["callers", "fmt", "--limit", "50"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v.is_object(), "high-freq fmt with implementors wraps: {v}");
    let impls = v["implementors"].as_array().unwrap();
    assert_eq!(
        impls.len(),
        HIGH_FREQ_IMPLEMENTOR_CAP,
        "high-freq implementors capped: {v}"
    );
    assert_eq!(v["implementors_truncated"], true);
    assert!(v["implementor_count"].as_u64().unwrap() > HIGH_FREQ_IMPLEMENTOR_CAP as u64);
    // Exact call must still be present in callers section.
    let calls = v["callers"].as_array().unwrap();
    assert!(
        !calls.is_empty(),
        "exact call on high-freq name must remain: {v}"
    );
    for row in calls {
        assert_ne!(row["edge_role"], "implementor");
    }
}

#[test]
fn e2e_callers_help_documents_role_flags() {
    let root = temp_root("help");
    let out = run(&root, &["callers", "--help"]);
    assert!(out.status.success());
    let text = stdout(&out).to_lowercase();
    assert!(
        text.contains("include-implementors"),
        "help must document --include-implementors: {text}"
    );
    assert!(
        text.contains("implementors-only"),
        "help must document --implementors-only: {text}"
    );
}

#[test]
fn e2e_impact_tags_edge_role() {
    let root = temp_root("impact-role");
    write_rust_trait_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let out = run(&root, &["impact", "area", "--depth", "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("edge_role"),
        "impact rows must tag edge_role: {text}"
    );
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let rows = v.as_array().expect("impact stays array");
    assert!(rows.iter().any(|r| r["edge_role"] == "implementor"));
}

#[test]
fn e2e_graph_html_shows_role_badge() {
    let root = temp_root("viz-role");
    write_rust_trait_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let out_path = root.join("g.html");
    let g = run(
        &root,
        &[
            "graph",
            "area",
            "--direction",
            "impact",
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    assert!(g.status.success(), "{}", stderr(&g));
    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(
        html.contains("IMP") || html.contains("data-edge-role"),
        "graph HTML must badge/color implementor vs call: {}",
        &html[html.len().saturating_sub(400)..]
    );
}

// Silence unused import warning when json is only used in some cfgs.
#[allow(dead_code)]
fn _keep_json() -> serde_json::Value {
    json!({})
}
