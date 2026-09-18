//! R32 adversarial probes — MCP graph out jail (Windows reparse) + recipe honesty.
//!
//! Locked contracts:
//! - `resolve_out_under_root` must refuse when the **leaf** is a file symlink
//!   pointing outside the workspace root (write would follow the reparse).
//! - `resolve_out_under_root` must refuse **before mkdir** when a path component
//!   is a directory junction that canonicalizes outside the root.
//! - `include_recommendation=false` must not embed recommendation text in HTML.
//! - blast_radius auto-window on a dirty union never claims `window=sound`.
//! - who_calls always `window=default` (never invents a sound claim).

use agentgraph::query::recipes::{
    build_blast_radius_payload, build_who_calls_payload, decide_blast_window,
    BlastRadiusPayloadInput,
};
use agentgraph::viz::graph_tool::resolve_out_under_root;
use agentgraph::viz::{
    build_graph_html_payload, render_graph_html, GraphEdge, GraphFlags, GraphHtmlPayloadInput,
    GraphNode, GraphVizData,
};
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

fn temp_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-r32-{name}"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn mklink(target: &Path, link: &Path) -> bool {
    let out = Command::new("cmd")
        .args([
            "/C",
            "mklink",
            &link.to_string_lossy(),
            &target.to_string_lossy(),
        ])
        .output()
        .expect("mklink");
    out.status.success()
}

fn mklink_junction(target: &Path, link: &Path) -> bool {
    let out = Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            &link.to_string_lossy(),
            &target.to_string_lossy(),
        ])
        .output()
        .expect("mklink /J");
    out.status.success()
}

fn sample_data() -> GraphVizData {
    GraphVizData {
        query: "helper".into(),
        direction: agentgraph::viz::GraphDirection::Impact,
        depth: 2,
        flags: GraphFlags {
            direction: agentgraph::viz::GraphDirection::Impact,
            ..GraphFlags::default()
        },
        nodes: vec![GraphNode {
            id: "q".into(),
            name: "helper".into(),
            depth: 0,
            confidence: "exact",
            path: Some("src/a.ts".into()),
            line: Some(1),
            origin: None,
            is_query: true,
            role: "call",
            root_id: String::new(),
        }],
        edges: vec![GraphEdge {
            from: "q".into(),
            to: "q".into(),
            confidence: "exact",
            kind: "call",
            role: "call",
        }],
        truncated: false,
        max_nodes: 300,
        subset_ok: Some(false),
        promise_tier: Some("disabled".into()),
        empty_note: None,
        root_filter: None,
    }
}

/// Leaf file symlink → outside: resolve must refuse (write follows reparse).
#[test]
fn out_jail_rejects_leaf_file_symlink_escape() {
    let root = temp_root("leaf-sym-root");
    let outside = temp_root("leaf-sym-outside");
    let victim = outside.join("victim.html");
    std::fs::write(&victim, "ORIGINAL").unwrap();
    let link = root.join("escape.html");
    if !mklink(&victim, &link) {
        eprintln!("skip: cannot create file symlink on this host");
        return;
    }
    let res = resolve_out_under_root(&root, "escape.html");
    assert!(
        res.is_err(),
        "leaf symlink escape must be refused, got {:?}",
        res
    );
    let msg = format!("{:#}", res.unwrap_err()).to_lowercase();
    assert!(
        msg.contains("jail") || msg.contains("outside") || msg.contains("root"),
        "jail error message: {msg}"
    );
    // Victim must remain untouched if anyone attempted a write.
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "ORIGINAL");
}

/// Directory junction + missing child: refuse **before** create_dir_all.
#[test]
fn out_jail_rejects_junction_mkdir_escape() {
    let root = temp_root("junc-root");
    let outside = temp_root("junc-outside");
    let link = root.join("link");
    if !mklink_junction(&outside, &link) {
        eprintln!("skip: cannot create directory junction on this host");
        return;
    }
    let res = resolve_out_under_root(&root, "link/newdir/evil.html");
    assert!(
        res.is_err(),
        "junction mkdir escape must be refused, got {:?}",
        res
    );
    assert!(
        !outside.join("newdir").exists(),
        "must not create directories outside the jail via junction"
    );
}

/// Absolute path lexically under root whose leaf is a symlink outside.
#[test]
fn out_jail_rejects_absolute_leaf_symlink() {
    let root = temp_root("abs-sym-root");
    let outside = temp_root("abs-sym-outside");
    let victim = outside.join("abs-victim.html");
    std::fs::write(&victim, "ABS-ORIGINAL").unwrap();
    let link = root.join("abs-escape.html");
    if !mklink(&victim, &link) {
        eprintln!("skip: cannot create file symlink on this host");
        return;
    }
    let abs = link.to_string_lossy().to_string();
    let res = resolve_out_under_root(&root, &abs);
    assert!(
        res.is_err(),
        "absolute leaf symlink escape must be refused, got {:?}",
        res
    );
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "ABS-ORIGINAL");
}

/// include_recommendation=false → HTML must not embed the recommendation sentence.
#[test]
fn include_recommendation_false_does_not_leak_into_html() {
    let data = sample_data();
    let html = render_graph_html(&data);
    let rec = "SECRET-REC-MUST-NOT-APPEAR-IN-HTML";
    let payload = build_graph_html_payload(GraphHtmlPayloadInput {
        symbol: "helper",
        data: &data,
        html: &html,
        window: "default",
        promise_tier: "disabled",
        subset_ok: Some(false),
        recommendation: None,
        path: None,
        note: agentgraph::viz::GRAPH_HTML_NOTE,
    });
    assert!(payload["recommendation"].is_null());
    let page = payload["html"].as_str().unwrap();
    assert!(!page.contains(rec));
    // Honesty banners may still exist; they are not the recommendation field.
    assert!(
        page.contains("not a complete runtime graph")
            || page.contains("非完整运行时图")
            || page.contains("S VIOLATED")
            || page.contains("subset_ok")
    );
}

/// Dirty union + blast_radius auto-window must never claim sound.
#[test]
fn blast_radius_dirty_union_never_claims_sound() {
    let d = decide_blast_window(false, None);
    assert_eq!(d.window, "default");
    assert!(!d.use_sound);
    let v = build_blast_radius_payload(BlastRadiusPayloadInput {
        symbol: "x".into(),
        depth: 2,
        limit: 10,
        nodes: vec![],
        window: d,
        promise_tier: "disabled".into(),
        languages: vec!["python".into()],
        include_macro: false,
        include_macro_reason: None,
        stale: None,
    });
    assert_eq!(v["window"], "default");
    assert_eq!(v["subset_ok"], false);
    assert_ne!(v["window"], "sound");
}

/// who_calls never invents window=sound.
#[test]
fn who_calls_window_is_always_default() {
    let v = build_who_calls_payload("helper", false, 10, &[], true, "ast_modeled");
    assert_eq!(v["window"], "default");
    assert!(v["callers"].as_array().is_some());
    assert!(v["implementors"].as_array().is_some());
}
