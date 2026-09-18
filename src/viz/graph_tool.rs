//! MCP `graph` HTML tool — Agent-facing visualization without shelling to CLI.
//!
//! Shared with CLI semantics (`agentgraph graph`): builds the same
//! `GraphVizData` neighborhood, renders via `render_graph_html`, and wraps
//! honesty fields (`window`, `subset_ok`, `promise_tier`, `recommendation`,
//! `note`) so Agents never treat the page as a complete runtime graph.
//!
//! **Non-claims:** HTML shows indexed L0/L1 (optional dynamic / macro sidecar)
//! candidates — not a complete runtime graph, not ecosystem sound.
//! `sound=true` is only labeled OK when `subset_ok` on the selected root.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

use crate::index::macro_map::{map_expanded_path, CrateLayout};
use crate::index::store::Store;
use crate::index::subset::select_sound_promise;
use crate::index::Indexer;
use crate::model::{ConfidenceFilter, ImpactNode, ReferenceRecord};
use crate::query::parse_query_flags;
use crate::query::recipes::{decide_blast_window, BlastWindowDecision};
use crate::viz::{
    add_macro_caller_rows, add_macro_impact_rows, build_callers_graph, build_impact_graph,
    merge_graphs, render_graph_html, GraphDirection, GraphFlags, GraphVizData, MAX_GRAPH_NODES,
};

/// Shared honesty note on every MCP graph payload.
pub const GRAPH_HTML_NOTE: &str =
    "not a complete runtime graph (indexed L0/L1 candidates; HTML is a visualization, not a runtime proof)";

/// Args for MCP `graph` (CLI parity + Agent extras).
#[derive(Debug, Clone)]
pub struct GraphHtmlArgs {
    pub symbol: String,
    pub depth: usize,
    pub direction: GraphDirection,
    pub sound: bool,
    pub with_macro: bool,
    pub exact_only: bool,
    pub include_dynamic: bool,
    pub include_recommendation: bool,
    pub auto_window: bool,
    /// Optional file write (string-only when `None`). Jailed under server root.
    pub out: Option<String>,
    pub root_id: Option<String>,
}

impl Default for GraphHtmlArgs {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            depth: 3,
            direction: GraphDirection::Impact,
            sound: false,
            with_macro: false,
            exact_only: false,
            include_dynamic: false,
            include_recommendation: true,
            auto_window: false,
            out: None,
            root_id: None,
        }
    }
}

/// SHA-256 hex digest of HTML bytes (Agents can pin/compare pages).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Inputs for `build_graph_html_payload` (keeps clippy arity green).
pub struct GraphHtmlPayloadInput<'a> {
    pub symbol: &'a str,
    pub data: &'a GraphVizData,
    pub html: &'a str,
    pub window: &'a str,
    pub promise_tier: &'a str,
    pub subset_ok: Option<bool>,
    pub recommendation: Option<&'a str>,
    pub path: Option<&'a str>,
    pub note: &'a str,
}

/// Build the locked MCP `graph` JSON payload from rendered HTML + honesty meta.
///
/// Pure — unit-testable without a store.
pub fn build_graph_html_payload(input: GraphHtmlPayloadInput<'_>) -> Value {
    let GraphHtmlPayloadInput {
        symbol,
        data,
        html,
        window,
        promise_tier,
        subset_ok,
        recommendation,
        path,
        note,
    } = input;
    let html_bytes = html.len();
    let sha256 = sha256_hex(html.as_bytes());
    let rec = match recommendation {
        Some(r) => Value::String(r.to_string()),
        None => Value::Null,
    };
    json!({
        "tool": "graph",
        "symbol": symbol,
        "direction": data.direction.as_str(),
        "depth": data.depth,
        "window": window,
        "subset_ok": subset_ok,
        "promise_tier": promise_tier,
        "recommendation": rec,
        "note": note,
        "html": html,
        "html_bytes": html_bytes,
        "sha256": sha256,
        "path": match path {
            Some(p) => Value::String(p.to_string()),
            None => Value::Null,
        },
        "node_count": data.nodes.len(),
        "edge_count": data.edges.len(),
        "truncated": data.truncated || data.nodes.len() > data.max_nodes,
        "max_nodes": data.max_nodes,
        "sound": data.flags.sound,
        "with_macro": data.flags.with_macro,
        "exact_only": data.flags.exact_only,
        "include_dynamic": data.flags.include_dynamic,
        "root_filter": data.root_filter.clone().map(Value::String).unwrap_or(Value::Null),
        "empty_note": data.empty_note.clone().map(Value::String).unwrap_or(Value::Null),
    })
}

fn outside_jail_msg(candidate: &Path, root: &Path) -> String {
    format!(
        "out '{}' is outside workspace root jail '{}'; set AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1 to override",
        candidate.display(),
        root.display()
    )
}

fn path_under_root(canon: &Path, root_canon: &Path) -> bool {
    canon == root_canon || canon.starts_with(root_canon)
}

/// Resolve optional `out` under the MCP workspace root jail.
///
/// Rejects `..` escapes and absolute paths outside the root (unless
/// `AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1`).
///
/// Windows reparse points (junction / file symlink):
/// - Nearest **existing** ancestor is canonicalized **before** any `mkdir`
///   (junction → refuse; do not create directories outside the jail).
/// - If the resolved leaf already exists, its canonical path must also stay
///   under the root (file symlink → refuse; write would follow the reparse).
pub fn resolve_out_under_root(root: &Path, out: &str) -> Result<PathBuf> {
    let allow_any = std::env::var("AGENTGRAPH_MCP_ALLOW_ANY_ROOT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if out.trim().is_empty() {
        bail!("out path is empty");
    }
    let raw = PathBuf::from(out);
    let candidate = if raw.is_absolute() {
        raw.clone()
    } else {
        root.join(&raw)
    };
    if allow_any {
        return Ok(candidate);
    }
    // Fail-closed on lexical `..` before any mkdir (create would escape).
    if candidate.components().any(|c| c == Component::ParentDir) {
        bail!(outside_jail_msg(&candidate, root));
    }
    let root_canon = crate::index::parser::normalize_root(&root.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "server root '{}' cannot be canonicalized ({e})",
            root.display()
        )
    })?);

    // Absolute path outside root: refuse before mkdir.
    if raw.is_absolute() {
        let cand_n = crate::index::parser::normalize_root(&candidate);
        if !path_under_root(&cand_n, &root_canon) {
            let parent_ok = candidate
                .parent()
                .map(|p| {
                    p.exists()
                        && path_under_root(
                            &crate::index::parser::normalize_root(
                                &p.canonicalize().unwrap_or_else(|_| p.to_path_buf()),
                            ),
                            &root_canon,
                        )
                })
                .unwrap_or(false);
            if !parent_ok {
                bail!(outside_jail_msg(&candidate, &root_canon));
            }
        }
    }

    // Fail-closed **before** mkdir: nearest existing ancestor must jail-resolve
    // under the workspace root. A directory junction pointing outside is
    // refused here so create_dir_all cannot materialize dirs outside the jail.
    {
        let mut probe = candidate.as_path();
        loop {
            if probe.as_os_str().is_empty() {
                break;
            }
            if probe.exists() {
                let a_canon = crate::index::parser::normalize_root(&probe.canonicalize()?);
                if !path_under_root(&a_canon, &root_canon) {
                    bail!(outside_jail_msg(&candidate, &root_canon));
                }
                break;
            }
            match probe.parent() {
                Some(p) if !p.as_os_str().is_empty() && p != probe => probe = p,
                _ => break,
            }
        }
    }

    if let Some(parent) = candidate.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
        if parent.exists() {
            let p_canon = crate::index::parser::normalize_root(&parent.canonicalize()?);
            if !path_under_root(&p_canon, &root_canon) {
                bail!(outside_jail_msg(&candidate, &root_canon));
            }
            let name = candidate
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("out path missing file name"))?;
            let resolved = p_canon.join(name);
            // Leaf reparse point (file symlink): write follows the link, so the
            // canonical leaf must also stay under the root.
            if resolved.exists() {
                let f_canon = crate::index::parser::normalize_root(&resolved.canonicalize()?);
                if !path_under_root(&f_canon, &root_canon) {
                    bail!(outside_jail_msg(&resolved, &root_canon));
                }
            }
            return Ok(resolved);
        }
    }
    if candidate.exists() {
        let f_canon = crate::index::parser::normalize_root(&candidate.canonicalize()?);
        if !path_under_root(&f_canon, &root_canon) {
            bail!(outside_jail_msg(&candidate, &root_canon));
        }
    }
    Ok(candidate)
}

fn query_limit() -> usize {
    MAX_GRAPH_NODES.saturating_add(50).max(100)
}

fn map_side_path(
    path: &str,
    expanded_root: &Path,
    indexer_root: &Path,
    layout: &CrateLayout,
) -> String {
    map_expanded_path(path, expanded_root, indexer_root, layout).unwrap_or_else(|| path.to_string())
}

/// Union optional macro sidecar rows into `data` (candidates only; not sound).
fn union_macro_graph(
    data: &mut GraphVizData,
    indexer: &Indexer,
    name: &str,
    depth: usize,
    direction: GraphDirection,
    filter: ConfidenceFilter,
    qlim: usize,
) {
    let Ok(Some(side)) = indexer.open_macro_store() else {
        return;
    };
    let Ok((layout, _map, _present)) = indexer.macro_crate_layout() else {
        return;
    };
    let Ok(status) = indexer.macro_status() else {
        return;
    };
    let expanded_root = status
        .expanded_root
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| indexer.root.clone());
    match direction {
        GraphDirection::Callers => {
            if let Ok(side_hits) = side.callers_filtered(name, qlim, filter) {
                let mapped: Vec<ReferenceRecord> = side_hits
                    .into_iter()
                    .map(|mut r| {
                        r.path = map_side_path(&r.path, &expanded_root, &indexer.root, &layout);
                        r
                    })
                    .collect();
                add_macro_caller_rows(data, name, &mapped);
            }
        }
        GraphDirection::Both => {
            if let Ok(si) = side.impact_filtered(name, depth, qlim, filter) {
                let map_imp: Vec<ImpactNode> = si
                    .into_iter()
                    .map(|mut n| {
                        n.path = map_side_path(&n.path, &expanded_root, &indexer.root, &layout);
                        n
                    })
                    .collect();
                add_macro_impact_rows(data, name, &map_imp);
            }
            if let Ok(sc) = side.callers_filtered(name, qlim, filter) {
                let mapped: Vec<ReferenceRecord> = sc
                    .into_iter()
                    .map(|mut r| {
                        r.path = map_side_path(&r.path, &expanded_root, &indexer.root, &layout);
                        r
                    })
                    .collect();
                add_macro_caller_rows(data, name, &mapped);
            }
        }
        GraphDirection::Impact => {
            if let Ok(side_hits) = side.impact_filtered(name, depth, qlim, filter) {
                let map_imp: Vec<ImpactNode> = side_hits
                    .into_iter()
                    .map(|mut n| {
                        n.path = map_side_path(&n.path, &expanded_root, &indexer.root, &layout);
                        n
                    })
                    .collect();
                add_macro_impact_rows(data, name, &map_imp);
            }
        }
    }
}

/// Decide confidence window + honesty labels for MCP graph.
///
/// - `auto_window`: blast_radius logic — sound only when subset_ok; else default
/// - `sound=true` (no auto_window): sound walk; window=`disabled` when !subset_ok
/// - else: default Exact+Heuristic (`window=default`)
fn decide_graph_window(
    sound: bool,
    auto_window: bool,
    subset_ok: bool,
    root_label: Option<&str>,
) -> BlastWindowDecision {
    if auto_window {
        decide_blast_window(subset_ok, root_label)
    } else if sound {
        if subset_ok {
            decide_blast_window(true, root_label)
        } else {
            let root_part = match root_label {
                Some(r) if !r.is_empty() => format!(" in root '{r}'"),
                _ => String::new(),
            };
            BlastWindowDecision {
                window: "disabled",
                subset_ok: false,
                use_sound: true,
                recommendation: format!(
                    "sound requested but subset_ok=false{root_part}; HTML is a disabled honesty page — do NOT treat it as a sound graph (promise_tier=disabled)"
                ),
            }
        }
    } else {
        let root_part = match root_label {
            Some(r) if !r.is_empty() => format!(" in root '{r}'"),
            _ => String::new(),
        };
        let recommendation = if subset_ok {
            "default window (Exact+Heuristic impact/callers); subset_ok=true but this call did not request sound — pass sound=true or auto_window=true for S-qualified walk".to_string()
        } else {
            format!(
                "default window (Exact+Heuristic); sound disabled because unsafe/S-violated{root_part} — not a complete runtime graph"
            )
        };
        BlastWindowDecision {
            window: "default",
            subset_ok,
            use_sound: false,
            recommendation,
        }
    }
}

/// Payload window label — never claims `sound` when `subset_ok=false`.
fn window_payload_label(
    decision: &BlastWindowDecision,
    sound_requested: bool,
    auto_window: bool,
    subset_ok: bool,
) -> &'static str {
    if auto_window {
        if decision.use_sound && subset_ok {
            "sound"
        } else {
            "default"
        }
    } else if sound_requested {
        if subset_ok {
            "sound"
        } else {
            "disabled"
        }
    } else {
        "default"
    }
}

/// Execute MCP `graph` against an open store + indexer. Returns JSON payload.
pub fn run_graph_html(
    store: &Store,
    indexer: &Indexer,
    root: &Path,
    args: &GraphHtmlArgs,
) -> Result<Value> {
    store.ensure_indexed()?;
    let rf = args.root_id.as_deref();
    let name = args.symbol.as_str();
    let depth = args.depth.max(1);
    let direction = args.direction;
    let qlim = query_limit();

    if args.sound && args.with_macro {
        bail!(
            "sound is mutually exclusive with with_macro \
             (macro sidecar is not sound-certified)"
        );
    }
    if args.sound && (args.exact_only || args.include_dynamic) {
        bail!(
            "sound is mutually exclusive with exact_only / include_dynamic \
             (sound walk uses its own eligibility filter)"
        );
    }
    if args.with_macro {
        let roots = store.workspace_roots_meta().unwrap_or_default();
        let multi = roots.iter().filter(|r| !r.id.is_empty()).count() > 1;
        if multi && rf.is_none() {
            bail!(
                "with_macro + workspace multi-root requires a single root_id filter \
                 (macro sidecar is per-root)"
            );
        }
    }

    let violations = store.subset_violations_in(rf)?;
    let subset_ok = violations.is_empty();
    let languages = store.stats(&root.to_string_lossy())?.languages;
    let (promise_tier_obj, _promise) = select_sound_promise(subset_ok, &languages);
    let promise_tier = promise_tier_obj.as_str().to_string();

    let decision = decide_graph_window(args.sound, args.auto_window, subset_ok, rf);

    // Macro never under a sound walk.
    let with_macro = args.with_macro && !decision.use_sound;

    let filter = parse_query_flags(args.exact_only, args.include_dynamic, false);
    // Page flags: sound=true when a sound walk was used (including disabled UX).
    let page_flags = GraphFlags {
        exact_only: args.exact_only,
        include_dynamic: args.include_dynamic,
        with_macro,
        sound: decision.use_sound,
        direction,
    };

    let mut data: GraphVizData = if decision.use_sound {
        match direction {
            GraphDirection::Callers => {
                let (hits, _v) = store.callers_sound_in(name, qlim, rf)?;
                build_callers_graph(name, &hits, page_flags.clone())
            }
            GraphDirection::Both => {
                let (ih, _iv) = store.impact_sound_in(name, depth, qlim, rf)?;
                let (ch, _cv) = store.callers_sound_in(name, qlim, rf)?;
                let a = build_impact_graph(name, &ih, page_flags.clone(), depth);
                let b = build_callers_graph(name, &ch, page_flags.clone());
                let mut d = merge_graphs(a, b);
                d.depth = depth;
                d
            }
            GraphDirection::Impact => {
                let (hits, _v) = store.impact_sound_in(name, depth, qlim, rf)?;
                build_impact_graph(name, &hits, page_flags.clone(), depth)
            }
        }
    } else {
        match direction {
            GraphDirection::Callers => {
                let hits = store.callers_filtered_in(name, qlim, filter, rf)?;
                build_callers_graph(name, &hits, page_flags.clone())
            }
            GraphDirection::Both => {
                let impact_hits = store.impact_filtered_in(name, depth, qlim, filter, rf)?;
                let callers_hits = store.callers_filtered_in(name, qlim, filter, rf)?;
                let a = build_impact_graph(name, &impact_hits, page_flags.clone(), depth);
                let b = build_callers_graph(name, &callers_hits, page_flags.clone());
                let mut d = merge_graphs(a, b);
                d.depth = depth;
                d
            }
            GraphDirection::Impact => {
                let hits = store.impact_filtered_in(name, depth, qlim, filter, rf)?;
                build_impact_graph(name, &hits, page_flags.clone(), depth)
            }
        }
    };

    data.flags = page_flags.clone();
    data.root_filter = rf.map(|s| s.to_string());
    data.subset_ok = Some(subset_ok);
    data.promise_tier = Some(if decision.use_sound && !subset_ok {
        "disabled".to_string()
    } else {
        promise_tier.clone()
    });

    if decision.use_sound && !subset_ok {
        data.flags.sound = true;
        data.subset_ok = Some(false);
        data.promise_tier = Some("disabled".into());
        data.empty_note = Some(
            "S violated — sound walk disabled; this page is NOT a sound graph. \
             Best-effort sound-eligible candidates only (promise_tier=disabled)."
                .to_string(),
        );
    } else if !decision.use_sound {
        data.flags.sound = false;
    }

    if with_macro {
        union_macro_graph(&mut data, indexer, name, depth, direction, filter, qlim);
    }

    let html = render_graph_html(&data);

    let window_label = window_payload_label(&decision, args.sound, args.auto_window, subset_ok);
    let promise_for_payload = if decision.use_sound && !subset_ok {
        "disabled"
    } else {
        promise_tier.as_str()
    };

    let recommendation = if args.include_recommendation {
        Some(decision.recommendation.clone())
    } else {
        None
    };

    let path = match &args.out {
        Some(o) if !o.trim().is_empty() => {
            let p = resolve_out_under_root(root, o)?;
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&p, &html)?;
            Some(p.to_string_lossy().replace('\\', "/"))
        }
        _ => None,
    };

    Ok(build_graph_html_payload(GraphHtmlPayloadInput {
        symbol: name,
        data: &data,
        html: &html,
        window: window_label,
        promise_tier: promise_for_payload,
        subset_ok: Some(subset_ok),
        recommendation: recommendation.as_deref(),
        path: path.as_deref(),
        note: GRAPH_HTML_NOTE,
    }))
}
