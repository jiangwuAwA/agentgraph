//! TDD: HTML code-graph visualization (`agentgraph graph` + `render_graph_html`).
//!
//! Contract:
//! - Self-contained single-file HTML (inline CSS/JS/SVG, no CDN)
//! - Honesty header + confidence legend
//! - HTML-escape untrusted names/paths
//! - Empty neighborhood still writes a page (exit 0 + note)
//! - Distinct colors for Exact vs Heuristic

use agentgraph::model::{Confidence, EdgeKind, ImpactNode, ReferenceRecord};
use agentgraph::viz::{
    build_impact_graph, escape_html, render_graph_html, GraphEdge, GraphFlags, GraphNode,
    GraphVizData, COLOR_DYNAMIC, COLOR_EXACT, COLOR_HEURISTIC, MAX_GRAPH_NODES,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-graph-html-{name}"));
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

fn sample_flags() -> GraphFlags {
    GraphFlags {
        exact_only: false,
        include_dynamic: false,
        with_macro: false,
        sound: false,
        direction: agentgraph::viz::GraphDirection::Impact,
    }
}

fn sample_data() -> GraphVizData {
    let impact = vec![
        ImpactNode {
            name: "helper".into(),
            path: "src/auth.ts".into(),
            line: 4,
            kind: EdgeKind::Call,
            depth: 1,
            enclosing: Some("createUser".into()),
            resolved: Some("helper".into()),
            confidence: Confidence::Exact,
        },
        ImpactNode {
            name: "createUser".into(),
            path: "src/api.ts".into(),
            line: 4,
            kind: EdgeKind::Call,
            depth: 2,
            enclosing: Some("loginHandler".into()),
            resolved: Some("createUser".into()),
            confidence: Confidence::Heuristic,
        },
    ];
    build_impact_graph("helper", &impact, sample_flags(), 2)
}

#[test]
fn render_contains_symbol_legend_and_honesty() {
    let html = render_graph_html(&sample_data());
    assert!(html.contains("helper"), "must show query symbol name");
    assert!(html.contains("Exact"), "legend must mention Exact");
    assert!(html.contains("Heuristic"), "legend must mention Heuristic");
    assert!(html.contains("DynamicCandidate") || html.contains("dynamic_candidate"));
    // Honesty: not a complete runtime graph claim
    assert!(
        html.contains("not a complete runtime graph"),
        "honesty line required; html snippet: {}",
        &html[..html.len().min(400)]
    );
    assert!(html.contains("L0/L1"));
    // Self-contained: no CDN
    assert!(!html.contains("cdn."), "must not load CDN assets");
    assert!(!html.contains("unpkg.com"));
    assert!(!html.contains("jsdelivr"));
    assert!(!html.contains("d3js.org"));
    // Bilingual short UI
    assert!(html.contains("代码关系图") || html.contains("代码图"));
}

#[test]
fn html_escapes_script_in_symbol_and_path() {
    let evil = "<script>alert(1)</script>";
    let evil_path = "src/<script>x</script>.ts";
    let data = GraphVizData {
        query: evil.to_string(),
        direction: agentgraph::viz::GraphDirection::Impact,
        depth: 1,
        flags: sample_flags(),
        nodes: vec![GraphNode {
            id: "n1".into(),
            name: evil.to_string(),
            depth: 1,
            confidence: "exact",
            path: Some(evil_path.to_string()),
            line: Some(1),
            origin: None,
            is_query: false,
        }],
        edges: vec![GraphEdge {
            from: "q".into(),
            to: "n1".into(),
            confidence: "exact",
            kind: "call",
        }],
        truncated: false,
        max_nodes: MAX_GRAPH_NODES,
        subset_ok: None,
        promise_tier: None,
        empty_note: None,
    };
    let html = render_graph_html(&data);
    // Raw executable payloads must not appear as HTML tags in the body.
    assert!(
        !html.contains("<script>alert(1)"),
        "raw script tag from symbol name must be escaped"
    );
    assert!(
        !html.contains("<script>alert(2)") && !html.contains("<script>x</script>"),
        "raw script tag from path must be escaped"
    );
    assert!(html.contains("&lt;script&gt;"), "escaped form expected");
    // Pure escape helper
    assert_eq!(escape_html("<a b=\"c\">"), "&lt;a b=&quot;c&quot;&gt;");
}

#[test]
fn confidence_colors_present_for_exact_and_heuristic() {
    let html = render_graph_html(&sample_data());
    assert!(html.contains(COLOR_EXACT), "exact color missing");
    assert!(html.contains(COLOR_HEURISTIC), "heuristic color missing");
    assert!(html.contains("conf-exact"));
    assert!(html.contains("conf-heuristic"));
    // Dynamic color token is always defined for the legend, even if unused in graph.
    assert!(html.contains(COLOR_DYNAMIC));
}

#[test]
fn impact_graph_nodes_edges_and_macro_badge() {
    let data = sample_data();
    assert!(
        data.nodes.len() >= 3,
        "query + 2 dependents, got {}",
        data.nodes.len()
    );
    assert!(!data.edges.is_empty());
    // Center is query
    assert!(data.nodes.iter().any(|n| n.is_query && n.name == "helper"));
    // JSON blob for interactive JS
    let html = render_graph_html(&data);
    assert!(html.contains("\"nodes\""), "JSON nodes blob required");
    assert!(html.contains("\"edges\""), "JSON edges blob required");
    assert!(html.contains("createUser"));
    assert!(html.contains("loginHandler"));

    // Macro origin badge
    let mut with_macro = sample_data();
    if let Some(n) = with_macro
        .nodes
        .iter_mut()
        .find(|n| n.name == "loginHandler")
    {
        n.origin = Some("macro_expanded");
    }
    let html_m = render_graph_html(&with_macro);
    assert!(html_m.contains("macro_expanded") || html_m.contains("MACRO"));
}

#[test]
fn empty_graph_still_renders_page() {
    let data = GraphVizData::empty("nobody_here", sample_flags(), 2);
    let html = render_graph_html(&data);
    assert!(html.contains("nobody_here"));
    assert!(html.contains("not a complete runtime graph"));
    // Empty-state callout
    assert!(
        html.contains("empty") || html.contains("Empty") || html.contains("无"),
        "empty-state text required"
    );
    assert!(html.contains("\"nodes\""));
}

#[test]
fn node_cap_truncates_with_notice() {
    let mut nodes = vec![GraphNode {
        id: "q".into(),
        name: "hub".into(),
        depth: 0,
        confidence: "exact",
        path: None,
        line: None,
        origin: None,
        is_query: true,
    }];
    for i in 0..(MAX_GRAPH_NODES + 40) {
        nodes.push(GraphNode {
            id: format!("n{i}"),
            name: format!("fn_{i}"),
            depth: 1,
            confidence: "exact",
            path: None,
            line: None,
            origin: None,
            is_query: false,
        });
    }
    let data = GraphVizData {
        query: "hub".into(),
        direction: agentgraph::viz::GraphDirection::Impact,
        depth: 1,
        flags: sample_flags(),
        nodes,
        edges: vec![],
        truncated: true,
        max_nodes: MAX_GRAPH_NODES,
        subset_ok: None,
        promise_tier: None,
        empty_note: None,
    };
    let html = render_graph_html(&data);
    assert!(html.contains("truncated") || html.contains("截断"));
    assert!(html.contains("300") || html.contains(&MAX_GRAPH_NODES.to_string()));
}

// ---------- CLI e2e ----------

fn write_fixture(root: &Path) {
    // Exact call: createUser → helper
    // Heuristic DI: bootstrap registers UserService (ts.di.*)
    std::fs::write(
        root.join("src/auth.ts"),
        r#"
export function helper(x: number): number {
  return x + 1;
}
export function createUser(email: string) {
  helper(1);
  return { email };
}
export class UserService {
  load() { return helper(2); }
}
export function bootstrap(c: any) {
  c.register(UserService);
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

#[test]
fn cli_e2e_graph_helper_writes_html() {
    let root = temp_root("cli-ok");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index: {}", stderr(&idx));

    let out_path = root.join("tmp-graph.html");
    let out_path_s = out_path.to_string_lossy().to_string();
    let g = run(
        &root,
        &[
            "graph",
            "helper",
            "--depth",
            "2",
            "--out",
            out_path_s.as_str(),
        ],
    );
    assert!(
        g.status.success(),
        "graph: stdout={} stderr={}",
        stdout(&g),
        stderr(&g)
    );
    assert!(
        out_path.exists(),
        "HTML file must exist at {}",
        out_path.display()
    );
    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(html.contains("helper"), "page must mention query symbol");
    assert!(html.contains("\"nodes\""), "JSON nodes blob");
    assert!(html.contains("\"edges\""), "JSON edges blob");
    assert!(html.contains("not a complete runtime graph"));
    // At least SVG structure present
    assert!(html.contains("<svg") || html.contains("svg"), "svg present");
    // Default impact neighborhood should include createUser
    assert!(
        html.contains("createUser"),
        "impact child expected: {}",
        &html[..html.len().min(800)]
    );
}

#[test]
fn cli_e2e_graph_confidence_colors_in_fixture() {
    let root = temp_root("cli-conf");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index: {}", stderr(&idx));

    // UserService has heuristic DI edge from bootstrap + exact call from load→helper
    let out_path = root.join("conf.html");
    let out_path_s = out_path.to_string_lossy().to_string();
    let g = run(
        &root,
        &[
            "graph",
            "UserService",
            "--depth",
            "2",
            "--out",
            out_path_s.as_str(),
        ],
    );
    assert!(g.status.success(), "{}", stderr(&g));
    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(html.contains(COLOR_HEURISTIC) || html.contains("conf-heuristic"));
    assert!(html.contains("UserService"));
    // legend always ships all three tokens
    assert!(html.contains(COLOR_EXACT));
    assert!(html.contains(COLOR_DYNAMIC));
}

#[test]
fn cli_e2e_graph_unknown_symbol_writes_empty_page_exit_zero() {
    let root = temp_root("cli-empty");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index: {}", stderr(&idx));

    let out_path = root.join("empty.html");
    let out_path_s = out_path.to_string_lossy().to_string();
    let g = run(
        &root,
        &["graph", "no_such_symbol_xyz", "--out", out_path_s.as_str()],
    );
    // Chosen contract: exit 0 + empty-state HTML + stderr note (viz is not a hard query failure)
    assert!(
        g.status.success(),
        "empty graph must still exit 0; stderr={}",
        stderr(&g)
    );
    assert!(out_path.exists(), "empty-state HTML must be written");
    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(html.contains("no_such_symbol_xyz"));
    assert!(html.contains("not a complete runtime graph"));
    assert!(
        html.contains("empty") || html.contains("Empty") || html.contains("无"),
        "empty-state body"
    );
    let err = stderr(&g).to_lowercase();
    assert!(
        err.contains("empty") || err.contains("no indexed") || stdout(&g).contains("graph"),
        "expected note on empty graph; stderr={err} stdout={}",
        stdout(&g)
    );
}

#[test]
fn cli_e2e_default_out_under_agentgraph_dir() {
    let root = temp_root("cli-default-out");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index: {}", stderr(&idx));
    let g = run(&root, &["graph", "helper"]);
    assert!(g.status.success(), "{}", stderr(&g));
    let default_out = root.join(".agentgraph").join("graph.html");
    assert!(
        default_out.exists(),
        "default out should be <root>/.agentgraph/graph.html; stdout={}",
        stdout(&g)
    );
}

#[test]
fn callers_direction_depth1_neighborhood() {
    let root = temp_root("cli-callers");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success());
    let out_path = root.join("callers.html");
    let out_path_s = out_path.to_string_lossy().to_string();
    let g = run(
        &root,
        &[
            "graph",
            "helper",
            "--direction",
            "callers",
            "--out",
            out_path_s.as_str(),
        ],
    );
    assert!(g.status.success(), "{}", stderr(&g));
    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(html.contains("createUser"), "direct caller of helper");
}

#[test]
fn reference_record_callers_builder_renders() {
    let refs = vec![ReferenceRecord {
        name: "helper".into(),
        kind: EdgeKind::Call,
        path: "src/auth.ts".into(),
        line: 6,
        enclosing: Some("createUser".into()),
        module: None,
        resolved: Some("helper".into()),
        qualifier: None,
        confidence: Confidence::Exact,
        evidence: None,
    }];
    let data = agentgraph::viz::build_callers_graph("helper", &refs, sample_flags());
    let html = render_graph_html(&data);
    assert!(html.contains("createUser"));
    assert!(html.contains("helper"));
}

/// Track M1: HTML shows mapped source path + MACRO badge when sidecar rows present.
#[test]
fn cli_graph_with_macro_shows_mapped_path_and_badge() {
    let base = std::env::temp_dir().join(format!(
        "agentgraph-graph-html-macro-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    // Source layout uses workspace crate path; expanded uses crate-dir layout.
    std::fs::create_dir_all(root.join("crates/event-engine/src")).unwrap();
    std::fs::create_dir_all(expanded.join("event-engine")).unwrap();
    std::fs::write(
        root.join("crates/event-engine/src/lib.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
"#,
    )
    .unwrap();
    std::fs::write(
        expanded.join("event-engine/lib.rs"),
        r#"
pub fn helper() -> i32 { 1 }
pub fn process() -> i32 { helper() + 1 }
pub fn fmt() -> i32 { helper() }
"#,
    )
    .unwrap();

    assert!(run(&root, &["index", "--force"]).status.success());
    let exp = expanded.to_string_lossy().into_owned();
    let side = run(&root, &["index", "--force", "--macro-expanded-root", &exp]);
    assert!(side.status.success(), "sidecar: {}", stderr(&side));

    let out_path = root.join("macro.html");
    let out_s = out_path.to_string_lossy().to_string();
    let g = run(
        &root,
        &[
            "graph",
            "helper",
            "--with-macro",
            "--direction",
            "callers",
            "--out",
            out_s.as_str(),
        ],
    );
    assert!(g.status.success(), "graph: {}", stderr(&g));
    let html = std::fs::read_to_string(&out_path).expect("read macro graph html");
    assert!(
        html.contains("MACRO") || html.contains("macro_expanded"),
        "MACRO badge / origin expected in HTML"
    );
    // Mapped source path (crate-aligned) should appear, not only the expanded
    // shadow path `event-engine/lib.rs`.
    assert!(
        html.contains("crates/event-engine/src/lib.rs"),
        "mapped source path expected in HTML; snippet: {}",
        &html[..html.len().min(2000)]
    );
    // Honesty line still present.
    assert!(html.contains("not a complete runtime graph"));
}

// ---------- Track M4: graph --sound ----------

#[test]
fn graph_sound_renders_s_qualified_header_on_clean_fixture() {
    let root = temp_root("sound-ok");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index: {}", stderr(&idx));

    let out_path = root.join("sound-ok.html");
    let out_s = out_path.to_string_lossy().to_string();
    let g = run(
        &root,
        &["graph", "helper", "--sound", "--out", out_s.as_str()],
    );
    assert!(
        g.status.success(),
        "clean --sound graph must succeed: stdout={} stderr={}",
        stdout(&g),
        stderr(&g)
    );
    let html = std::fs::read_to_string(&out_path).expect("read sound html");
    assert!(
        html.contains("subset_ok=true") || html.contains("subset_ok\":true"),
        "header must show subset_ok=true: {}",
        &html[..html.len().min(2500)]
    );
    assert!(
        html.contains("ast_modeled"),
        "header must show promise_tier=ast_modeled"
    );
    // Sound page honesty: S-qualified / sound-eligible — still not a complete runtime graph.
    assert!(
        html.contains("not a complete runtime graph") || html.contains("非完整运行时图"),
        "honesty line required"
    );
    assert!(
        html.contains("sound") || html.contains("S-qualified") || html.contains("S 合格"),
        "sound mode must be labeled: {}",
        &html[..html.len().min(2500)]
    );
    // JSON payload carries sound flag + tier.
    assert!(html.contains("\"sound\""));
    assert!(html.contains("ast_modeled"));
}

#[test]
fn graph_sound_violation_still_writes_honest_disabled_page() {
    let root = temp_root("sound-bad");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/app.js"),
        r#"
export function helper(x) { return x + 1; }
export function createUser(email) {
  helper(1);
  return eval(email);
}
"#,
    )
    .unwrap();
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index: {}", stderr(&idx));

    let out_path = root.join("sound-bad.html");
    let out_s = out_path.to_string_lossy().to_string();
    let g = run(
        &root,
        &["graph", "helper", "--sound", "--out", out_s.as_str()],
    );
    // Spec §4.4: subset_ok=false → still write HTML, exit non-zero.
    assert!(
        !g.status.success(),
        "violated --sound graph must exit non-zero; stdout={} stderr={}",
        stdout(&g),
        stderr(&g)
    );
    assert!(
        out_path.exists(),
        "HTML must still be written when subset_ok=false"
    );
    let html = std::fs::read_to_string(&out_path).expect("read violated sound html");
    assert!(
        html.contains("subset_ok=false") || html.contains("subset_ok\":false"),
        "page must record subset_ok=false: {}",
        &html[..html.len().min(2500)]
    );
    assert!(
        html.contains("disabled") || html.contains("NOT a sound") || html.contains("不是 sound"),
        "page must clearly mark sound as disabled / not a sound graph"
    );
    // Must NOT label this page as a sound graph.
    assert!(
        !html.contains("S-qualified sound-eligible edges only"),
        "violated page must not use the OK sound-graph label"
    );
    assert!(
        html.contains("promise_tier=disabled") || html.contains("disabled"),
        "promise_tier disabled expected"
    );
}

#[test]
fn graph_sound_mutually_exclusive_with_with_macro() {
    let root = temp_root("sound-mutex");
    write_fixture(&root);
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success());

    let g = run(&root, &["graph", "helper", "--sound", "--with-macro"]);
    assert!(
        !g.status.success(),
        "graph --sound && --with-macro must fail closed"
    );
    let err = stderr(&g).to_lowercase();
    assert!(
        err.contains("mutually") || err.contains("with-macro") || err.contains("with_macro"),
        "mutex error required: {err}"
    );

    // Also mutually exclusive with confidence-window flags (same as callers/impact --sound).
    let g2 = run(&root, &["graph", "helper", "--sound", "--exact-only"]);
    assert!(
        !g2.status.success(),
        "graph --sound && --exact-only must fail closed"
    );
}

#[test]
fn render_sound_violation_page_marks_disabled() {
    let mut data = sample_data();
    data.flags.sound = true;
    data.subset_ok = Some(false);
    data.promise_tier = Some("disabled".into());
    let html = render_graph_html(&data);
    assert!(html.contains("subset_ok=false") || html.contains("false"));
    assert!(html.contains("disabled"));
    assert!(
        !html.contains("S-qualified sound-eligible edges only"),
        "OK sound label must not appear when subset_ok=false"
    );
}

#[test]
fn render_sound_ok_page_includes_tier_header() {
    let mut data = sample_data();
    data.flags.sound = true;
    data.subset_ok = Some(true);
    data.promise_tier = Some("ast_modeled".into());
    let html = render_graph_html(&data);
    assert!(html.contains("ast_modeled"));
    assert!(html.contains("subset_ok=true") || html.contains("true"));
    assert!(html.contains("sound") || html.contains("S-qualified"));
}
