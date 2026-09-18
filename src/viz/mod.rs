//! Self-contained HTML graph visualization for `agentgraph graph`.
//!
//! Pure functions: build view models from query rows, then `render_graph_html`
//! → one offline HTML file (inline CSS/JS/SVG, no CDN).
//!
//! Honesty: rendered edges are **indexed** L0/L1 (and optional L1 dynamic /
//! macro sidecar) candidates — not a complete runtime graph.

use crate::model::{Confidence, ImpactNode, ReferenceRecord};
use serde_json::json;

/// Hard cap so a large neighborhood cannot hang the browser.
pub const MAX_GRAPH_NODES: usize = 300;

/// Distinct fill colors by confidence (asserted in tests + legend).
pub const COLOR_EXACT: &str = "#1f9d55";
pub const COLOR_HEURISTIC: &str = "#d97706";
pub const COLOR_DYNAMIC: &str = "#a855f7";
pub const COLOR_MACRO_STROKE: &str = "#0ea5e9";

/// Primary neighborhood direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphDirection {
    /// Outgoing blast radius (who depends on the symbol) — default.
    #[default]
    Impact,
    /// Direct callers / reference sites.
    Callers,
    /// Union of impact + callers.
    Both,
}

impl GraphDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            GraphDirection::Impact => "impact",
            GraphDirection::Callers => "callers",
            GraphDirection::Both => "both",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "impact" => Some(GraphDirection::Impact),
            "callers" => Some(GraphDirection::Callers),
            "both" => Some(GraphDirection::Both),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GraphFlags {
    pub exact_only: bool,
    pub include_dynamic: bool,
    pub with_macro: bool,
    pub sound: bool,
    pub direction: GraphDirection,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub depth: usize,
    /// `exact` | `heuristic` | `dynamic_candidate`
    pub confidence: &'static str,
    pub path: Option<String>,
    pub line: Option<usize>,
    /// `Some("macro_expanded")` for sidecar-origin rows.
    pub origin: Option<&'static str>,
    pub is_query: bool,
}

impl GraphNode {
    pub fn location(&self) -> Option<String> {
        match (&self.path, self.line) {
            (Some(p), Some(l)) => Some(format!("{p}:{l}")),
            (Some(p), None) => Some(p.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub confidence: &'static str,
    pub kind: &'static str,
}

#[derive(Debug, Clone)]
pub struct GraphVizData {
    pub query: String,
    pub direction: GraphDirection,
    pub depth: usize,
    pub flags: GraphFlags,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
    pub max_nodes: usize,
    pub subset_ok: Option<bool>,
    pub promise_tier: Option<String>,
    pub empty_note: Option<String>,
}

impl GraphVizData {
    /// Empty neighborhood page (unknown symbol / no indexed edges).
    pub fn empty(query: &str, flags: GraphFlags, depth: usize) -> Self {
        Self {
            query: query.to_string(),
            direction: flags.direction,
            depth,
            flags,
            nodes: Vec::new(),
            edges: Vec::new(),
            truncated: false,
            max_nodes: MAX_GRAPH_NODES,
            subset_ok: None,
            promise_tier: None,
            empty_note: Some(
                "无已索引关系（空图） · Empty neighborhood — no indexed L0/L1 edges for this symbol."
                    .to_string(),
            ),
        }
    }
}

/// Escape text for HTML text nodes / attributes (XSS: names and paths are untrusted).
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// JSON string safe to embed inside `<script type="application/json">`.
fn json_embed(value: &serde_json::Value) -> String {
    let s = serde_json::to_string(value).unwrap_or_else(|_| "null".into());
    s.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

fn conf_str(c: Confidence) -> &'static str {
    c.as_str()
}

fn conf_rank(c: &str) -> u8 {
    match c {
        "exact" => 0,
        "heuristic" => 1,
        _ => 2,
    }
}

/// Prefer the more certain confidence when a node is introduced multiple times.
fn tighten_confidence(a: &'static str, b: &'static str) -> &'static str {
    if conf_rank(a) <= conf_rank(b) {
        a
    } else {
        b
    }
}

fn node_id_for_symbol(name: &str) -> String {
    format!("s:{name}")
}

fn node_id_for_site(path: &str, line: usize) -> String {
    format!("cs:{path}:{line}")
}

fn push_node(nodes: &mut Vec<GraphNode>, node: GraphNode) {
    if nodes.len() >= MAX_GRAPH_NODES && !nodes.iter().any(|n| n.id == node.id) {
        return;
    }
    if let Some(existing) = nodes.iter_mut().find(|n| n.id == node.id) {
        existing.confidence = tighten_confidence(existing.confidence, node.confidence);
        if existing.origin.is_none() {
            existing.origin = node.origin;
        }
        if existing.path.is_none() {
            existing.path = node.path;
            existing.line = node.line;
        }
        return;
    }
    nodes.push(node);
}

fn push_edge(edges: &mut Vec<GraphEdge>, edge: GraphEdge) {
    if edges.iter().any(|e| e.from == edge.from && e.to == edge.to) {
        return;
    }
    edges.push(edge);
}

fn ensure_query_node(nodes: &mut Vec<GraphNode>, query: &str) {
    let id = "q".to_string();
    if nodes.iter().any(|n| n.id == id) {
        return;
    }
    nodes.insert(
        0,
        GraphNode {
            id,
            name: query.to_string(),
            depth: 0,
            confidence: "exact",
            path: None,
            line: None,
            origin: None,
            is_query: true,
        },
    );
}

fn finalize(
    query: &str,
    mut nodes: Vec<GraphNode>,
    mut edges: Vec<GraphEdge>,
    flags: GraphFlags,
    depth: usize,
) -> GraphVizData {
    ensure_query_node(&mut nodes, query);
    if nodes.len() > MAX_GRAPH_NODES {
        nodes.truncate(MAX_GRAPH_NODES);
    }
    // Drop edges that reference dropped nodes.
    let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    edges.retain(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()));
    let truncated = nodes.len() >= MAX_GRAPH_NODES;
    let empty_note = if nodes.len() <= 1 && edges.is_empty() {
        Some(
            "无已索引关系（空图） · Empty neighborhood — no indexed L0/L1 edges for this symbol."
                .to_string(),
        )
    } else {
        None
    };
    let direction = flags.direction;
    GraphVizData {
        query: query.to_string(),
        direction,
        depth,
        flags,
        nodes,
        edges,
        truncated,
        max_nodes: MAX_GRAPH_NODES,
        subset_ok: None,
        promise_tier: None,
        empty_note,
    }
}

/// Build impact-style BFS graph: center = query, edges point toward dependents
/// (blast radius). `ImpactNode` rows: depth1 `name=query`, `enclosing=dependent`.
pub fn build_impact_graph(
    query: &str,
    impact: &[ImpactNode],
    flags: GraphFlags,
    depth: usize,
) -> GraphVizData {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    for row in impact {
        if row.depth == 0 {
            continue;
        }
        let conf = conf_str(row.confidence);
        let from_id = if row.depth <= 1 {
            "q".to_string()
        } else {
            node_id_for_symbol(&row.name)
        };
        // Source symbol node (for depth>1 linkage) — not always drawn as dependent.
        if row.depth > 1 {
            push_node(
                &mut nodes,
                GraphNode {
                    id: node_id_for_symbol(&row.name),
                    name: row.name.clone(),
                    depth: row.depth.saturating_sub(1),
                    confidence: conf,
                    path: None,
                    line: None,
                    origin: None,
                    is_query: false,
                },
            );
        }

        let (to_id, to_name, is_site) = match &row.enclosing {
            Some(enc) if !enc.is_empty() => (node_id_for_symbol(enc), enc.clone(), false),
            _ => (
                node_id_for_site(&row.path, row.line),
                format!("{}:{}", row.path, row.line),
                true,
            ),
        };
        let dep_depth = row.depth;
        push_node(
            &mut nodes,
            GraphNode {
                id: to_id.clone(),
                name: to_name,
                depth: dep_depth,
                confidence: conf,
                path: Some(row.path.clone()),
                line: Some(row.line),
                origin: None,
                is_query: false,
            },
        );
        let _ = is_site;
        push_edge(
            &mut edges,
            GraphEdge {
                from: from_id,
                to: to_id,
                confidence: conf,
                kind: row.kind.as_str(),
            },
        );
    }

    finalize(query, nodes, edges, flags, depth)
}

/// Merge macro-origin impact rows into an existing impact graph (badge nodes).
pub fn add_macro_impact_rows(data: &mut GraphVizData, query: &str, impact: &[ImpactNode]) {
    let mut nodes = std::mem::take(&mut data.nodes);
    let mut edges = std::mem::take(&mut data.edges);
    for row in impact {
        if row.depth == 0 {
            continue;
        }
        let conf = conf_str(row.confidence);
        let from_id = if row.depth <= 1 {
            "q".to_string()
        } else {
            node_id_for_symbol(&row.name)
        };
        if row.depth > 1 {
            push_node(
                &mut nodes,
                GraphNode {
                    id: node_id_for_symbol(&row.name),
                    name: row.name.clone(),
                    depth: row.depth.saturating_sub(1),
                    confidence: conf,
                    path: None,
                    line: None,
                    origin: Some("macro_expanded"),
                    is_query: false,
                },
            );
        }
        let (to_id, to_name) = match &row.enclosing {
            Some(enc) if !enc.is_empty() => (node_id_for_symbol(enc), enc.clone()),
            _ => (
                node_id_for_site(&row.path, row.line),
                format!("{}:{}", row.path, row.line),
            ),
        };
        push_node(
            &mut nodes,
            GraphNode {
                id: to_id.clone(),
                name: to_name,
                depth: row.depth,
                confidence: conf,
                path: Some(row.path.clone()),
                line: Some(row.line),
                origin: Some("macro_expanded"),
                is_query: false,
            },
        );
        push_edge(
            &mut edges,
            GraphEdge {
                from: from_id,
                to: to_id,
                confidence: conf,
                kind: row.kind.as_str(),
            },
        );
    }
    ensure_query_node(&mut nodes, query);
    data.nodes = nodes;
    data.edges = edges;
    if data.nodes.len() > MAX_GRAPH_NODES {
        data.nodes.truncate(MAX_GRAPH_NODES);
        data.truncated = true;
    }
    let ids: std::collections::HashSet<&str> = data.nodes.iter().map(|n| n.id.as_str()).collect();
    data.edges
        .retain(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()));
}

/// Build callers neighborhood: center = query, edges from caller → query.
pub fn build_callers_graph(
    query: &str,
    callers: &[ReferenceRecord],
    flags: GraphFlags,
) -> GraphVizData {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    for row in callers {
        let conf = conf_str(row.confidence);
        let (from_id, from_name) = match &row.enclosing {
            Some(enc) if !enc.is_empty() && enc != query => (node_id_for_symbol(enc), enc.clone()),
            _ => (
                node_id_for_site(&row.path, row.line),
                format!("{}:{}", row.path, row.line),
            ),
        };
        push_node(
            &mut nodes,
            GraphNode {
                id: from_id.clone(),
                name: from_name,
                depth: 1,
                confidence: conf,
                path: Some(row.path.clone()),
                line: Some(row.line),
                origin: None,
                is_query: false,
            },
        );
        push_edge(
            &mut edges,
            GraphEdge {
                from: from_id,
                to: "q".to_string(),
                confidence: conf,
                kind: row.kind.as_str(),
            },
        );
    }

    finalize(query, nodes, edges, flags, 1)
}

/// Merge macro-origin caller rows (badge + edge into center).
pub fn add_macro_caller_rows(data: &mut GraphVizData, query: &str, callers: &[ReferenceRecord]) {
    let mut nodes = std::mem::take(&mut data.nodes);
    let mut edges = std::mem::take(&mut data.edges);
    for row in callers {
        let conf = conf_str(row.confidence);
        let (from_id, from_name) = match &row.enclosing {
            Some(enc) if !enc.is_empty() && enc != query => (node_id_for_symbol(enc), enc.clone()),
            _ => (
                node_id_for_site(&row.path, row.line),
                format!("{}:{}", row.path, row.line),
            ),
        };
        push_node(
            &mut nodes,
            GraphNode {
                id: from_id.clone(),
                name: from_name,
                depth: 1,
                confidence: conf,
                path: Some(row.path.clone()),
                line: Some(row.line),
                origin: Some("macro_expanded"),
                is_query: false,
            },
        );
        push_edge(
            &mut edges,
            GraphEdge {
                from: from_id,
                to: "q".to_string(),
                confidence: conf,
                kind: row.kind.as_str(),
            },
        );
    }
    ensure_query_node(&mut nodes, query);
    data.nodes = nodes;
    data.edges = edges;
    if data.nodes.len() > MAX_GRAPH_NODES {
        data.nodes.truncate(MAX_GRAPH_NODES);
        data.truncated = true;
    }
    let ids: std::collections::HashSet<&str> = data.nodes.iter().map(|n| n.id.as_str()).collect();
    data.edges
        .retain(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()));
}

/// Union two graphs (e.g. impact + callers). Query/flags come from `primary`.
pub fn merge_graphs(primary: GraphVizData, secondary: GraphVizData) -> GraphVizData {
    let mut out = primary;
    out.direction = GraphDirection::Both;
    out.flags.direction = GraphDirection::Both;
    for n in secondary.nodes {
        push_node(&mut out.nodes, n);
    }
    for e in secondary.edges {
        push_edge(&mut out.edges, e);
    }
    ensure_query_node(&mut out.nodes, &out.query);
    if out.nodes.len() > MAX_GRAPH_NODES {
        out.nodes.truncate(MAX_GRAPH_NODES);
        out.truncated = true;
    }
    let ids: std::collections::HashSet<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
    out.edges
        .retain(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()));
    if out.nodes.len() <= 1 && out.edges.is_empty() {
        out.empty_note = Some(
            "无已索引关系（空图） · Empty neighborhood — no indexed L0/L1 edges for this symbol."
                .to_string(),
        );
    }
    out
}

fn flags_summary(f: &GraphFlags) -> String {
    let mut parts = vec![format!("direction={}", f.direction.as_str())];
    if f.exact_only {
        parts.push("exact_only".into());
    }
    if f.include_dynamic {
        parts.push("include_dynamic".into());
    }
    if f.with_macro {
        parts.push("with_macro".into());
    }
    if f.sound {
        parts.push("sound".into());
    }
    parts.join(" ")
}

fn honesty_line(data: &GraphVizData) -> String {
    // Track M4: sound pages must never oversell; violated S is not a sound graph.
    if data.flags.sound {
        return match data.subset_ok {
            Some(true) => {
                let mut s = String::from(
                    "S-qualified sound-eligible edges only (modeled L2) — not a complete runtime graph · \
                     仅 S 合格 sound 边（已建模 L2），非完整运行时图 · subset_ok=true",
                );
                if let Some(tier) = &data.promise_tier {
                    s.push_str(&format!(" · promise_tier={tier}"));
                }
                s.push_str(
                    " · engineering S gate, not ecosystem sound · 工程 S 门，非生态 sound",
                );
                s
            }
            Some(false) => String::from(
                "S VIOLATED — this page is NOT a sound graph · S 违例 — 本页不是 sound 图 · \
                 sound walk disabled · promise_tier=disabled · not a complete runtime graph · 非完整运行时图",
            ),
            None => String::from(
                "sound flag set but subset status unknown — not a complete runtime graph · \
                 已请求 sound 但 S 状态未知，非完整运行时图",
            ),
        };
    }
    let mut s = String::from(
        "L0/L1 candidates, not a complete runtime graph · L0/L1 候选边，非完整运行时图",
    );
    if let Some(ok) = data.subset_ok {
        s.push_str(&format!(" · subset_ok={ok}"));
    }
    if let Some(tier) = &data.promise_tier {
        s.push_str(&format!(" · promise_tier={tier}"));
    }
    s
}

fn truncate_label(name: &str) -> String {
    const MAX: usize = 22;
    let mut chars = name.chars();
    let head: String = chars.by_ref().take(MAX).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// Layout: query at center; other nodes on rings by BFS depth.
fn layout_positions(nodes: &[GraphNode]) -> std::collections::HashMap<String, (f64, f64)> {
    let width = 960.0;
    let height = 720.0;
    let cx = width / 2.0;
    let cy = height / 2.0;
    let mut by_depth: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        by_depth.entry(n.depth).or_default().push(i);
    }
    let mut max_d = 1usize;
    for d in by_depth.keys() {
        if *d > max_d {
            max_d = *d;
        }
    }
    let mut pos = std::collections::HashMap::new();
    let mut depths: Vec<usize> = by_depth.keys().copied().collect();
    depths.sort_unstable();
    for d in depths {
        let idxs = by_depth.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
        if idxs.is_empty() {
            continue;
        }
        if d == 0 {
            for &i in idxs {
                pos.insert(nodes[i].id.clone(), (cx, cy));
            }
            continue;
        }
        let radius = 90.0 + (d as f64) * 110.0;
        let n = idxs.len() as f64;
        for (k, &i) in idxs.iter().enumerate() {
            let angle =
                -std::f64::consts::FRAC_PI_2 + (k as f64) * (2.0 * std::f64::consts::PI / n);
            let x = cx + radius * angle.cos();
            let y = cy + radius * angle.sin() * 0.85;
            pos.insert(nodes[i].id.clone(), (x, y));
        }
    }
    // Any missing nodes (shouldn't happen) get a grid fallback.
    let mut fallback = 0usize;
    for n in nodes {
        if !pos.contains_key(&n.id) {
            let x = 40.0 + (fallback % 8) as f64 * 100.0;
            let y = 40.0 + (fallback / 8) as f64 * 60.0;
            pos.insert(n.id.clone(), (x, y));
            fallback += 1;
        }
    }
    pos
}

fn conf_color(c: &str) -> &'static str {
    match c {
        "exact" => COLOR_EXACT,
        "heuristic" => COLOR_HEURISTIC,
        _ => COLOR_DYNAMIC,
    }
}

fn node_json(n: &GraphNode) -> serde_json::Value {
    json!({
        "id": n.id,
        "name": n.name,
        "depth": n.depth,
        "confidence": n.confidence,
        "path": n.path,
        "line": n.line,
        "origin": n.origin,
        "is_query": n.is_query,
        "at": n.location(),
    })
}

fn edge_json(e: &GraphEdge) -> serde_json::Value {
    json!({
        "from": e.from,
        "to": e.to,
        "confidence": e.confidence,
        "kind": e.kind,
    })
}

/// Pure renderer: GraphVizData → one self-contained HTML document.
pub fn render_graph_html(data: &GraphVizData) -> String {
    // Cap again at render time (defense in depth).
    let truncated_flag = data.truncated || data.nodes.len() > data.max_nodes;
    let cap = data.max_nodes.max(1);
    let nodes: Vec<&GraphNode> = data.nodes.iter().take(cap).collect();
    let node_ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let edges: Vec<&GraphEdge> = data
        .edges
        .iter()
        .filter(|e| node_ids.contains(e.from.as_str()) && node_ids.contains(e.to.as_str()))
        .collect();

    let owned: Vec<GraphNode> = nodes.iter().map(|n| (*n).clone()).collect();
    let pos = layout_positions(&owned);

    let mut svg_edges = String::new();
    for e in &edges {
        let (x1, y1) = pos.get(e.from.as_str()).copied().unwrap_or((0.0, 0.0));
        let (x2, y2) = pos.get(e.to.as_str()).copied().unwrap_or((0.0, 0.0));
        let color = conf_color(e.confidence);
        let kind = escape_html(e.kind);
        let conf = escape_html(e.confidence);
        svg_edges.push_str(&format!(
            r##"<line class="edge conf-{conf}" data-from="{from}" data-to="{to}" x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{color}" stroke-width="2" marker-end="url(#arrow)" opacity="0.85"><title>{kind} · {conf}</title></line>"##,
            from = escape_html(&e.from),
            to = escape_html(&e.to),
        ));
    }

    let mut svg_nodes = String::new();
    for n in &nodes {
        let (x, y) = pos.get(n.id.as_str()).copied().unwrap_or((40.0, 40.0));
        let fill = conf_color(n.confidence);
        let r = if n.is_query { 28.0 } else { 18.0 };
        let label = escape_html(&truncate_label(&n.name));
        let full_name = escape_html(&n.name);
        let conf = escape_html(n.confidence);
        let depth = n.depth;
        let loc = escape_html(&n.location().unwrap_or_else(|| "—".into()));
        let macro_cls = if n.origin == Some("macro_expanded") {
            " node-macro"
        } else {
            ""
        };
        let macro_badge = if n.origin == Some("macro_expanded") {
            format!(
                r#"<text x="{bx:.1}" y="{by:.1}" text-anchor="middle" class="macro-badge">MACRO</text>"#,
                bx = x,
                by = y - r - 8.0
            )
        } else {
            String::new()
        };
        let query_cls = if n.is_query { " node-query" } else { "" };
        svg_nodes.push_str(&format!(
            r##"<g class="node{query_cls}{macro_cls} conf-{conf}" data-id="{id}" data-name="{full_name}" data-depth="{depth}" data-confidence="{conf}" data-location="{loc}" data-origin="{origin}" transform="translate({x:.1},{y:.1})">
  <circle r="{r}" fill="{fill}" stroke="#1e293b" stroke-width="2"/>
  <text y="4" text-anchor="middle" class="node-label">{label}</text>
  <text y="{ty:.1}" text-anchor="middle" class="node-meta">d{depth} · {conf}</text>
  <title>{full_name} · d{depth} · {conf} · {loc}</title>
</g>{badge}"##,
            id = escape_html(&n.id),
            origin = escape_html(n.origin.unwrap_or("")),
            ty = r + 14.0,
            badge = macro_badge,
        ));
    }

    let legend = format!(
        r#"<div class="legend">
  <span class="lg"><i style="background:{COLOR_EXACT}"></i> Exact / 精确 (L0)</span>
  <span class="lg"><i style="background:{COLOR_HEURISTIC}"></i> Heuristic / 启发式 (L1)</span>
  <span class="lg"><i style="background:{COLOR_DYNAMIC}"></i> DynamicCandidate / 动态候选 (L1)</span>
  <span class="lg"><i class="macro-swatch"></i> macro_expanded / 宏展开 sidecar</span>
</div>"#
    );

    let honesty = honesty_line(data);
    let flags = flags_summary(&data.flags);
    let query_esc = escape_html(&data.query);
    let empty_block = if data.nodes.len() <= 1 && edges.is_empty() {
        if data.flags.sound && data.subset_ok == Some(false) {
            format!(
                r#"<div class="empty-state" id="empty-state">S 违例，sound 图已禁用 · <strong>S violated — sound graph disabled</strong> for <code>{q}</code>. 即使存在 sound-eligible 候选边，本页也不得当作 sound 图。请先修复 subset 违例（<code>agentgraph subset</code>）。</div>"#,
                q = query_esc
            )
        } else if data.flags.sound {
            format!(
                r#"<div class="empty-state" id="empty-state">无 sound-eligible 已索引关系（空图） · <strong>Empty sound neighborhood</strong> — no sound-eligible edges for <code>{q}</code>. 请先运行 <code>agentgraph index</code>，或检查符号名。图仍已生成：这只是诚实的空态，不是完整图。</div>"#,
                q = query_esc
            )
        } else {
            format!(
                r#"<div class="empty-state" id="empty-state">无已索引关系（空图） · <strong>Empty neighborhood</strong> — no indexed L0/L1 edges for <code>{q}</code>. 请先运行 <code>agentgraph index</code>，或检查符号名。图仍已生成：这只是诚实的空态，不是完整图。</div>"#,
                q = query_esc
            )
        }
    } else {
        String::new()
    };
    let trunc_block = if truncated_flag {
        format!(
            r#"<div class="trunc-notice">图已截断 · Graph truncated at {cap} nodes（浏览器保护）. 更多边未显示，非不存在。</div>"#
        )
    } else {
        String::new()
    };

    let sound_block = match (data.flags.sound, data.subset_ok, &data.promise_tier) {
        (true, Some(false), Some(tier)) => format!(
            r#"<div class="sound-line sound-disabled">⚠ S violated — sound graph DISABLED · S 违例 — sound 图已禁用 · subset_ok=false · promise_tier={tier} · 本页不得当作 sound 图使用 / do not treat this page as a sound graph</div>"#
        ),
        (true, Some(true), Some(tier)) => format!(
            r#"<div class="sound-line sound-ok">subset_ok=true · promise_tier={tier} · sound-eligible modeled edges only (engineering S gate, not ecosystem sound)</div>"#
        ),
        (true, _, tier) => {
            let t = tier.clone().unwrap_or_else(|| "unknown".into());
            format!(
                r#"<div class="sound-line">sound requested · promise_tier={t} · subset status unknown</div>"#
            )
        }
        (_, Some(ok), Some(tier)) => format!(
            r#"<div class="sound-line">subset_ok={ok} · promise_tier={tier} · sound flags 仅在 S 子集内有意义</div>"#
        ),
        _ => String::new(),
    };

    let payload = json!({
        "query": data.query,
        "direction": data.direction.as_str(),
        "depth": data.depth,
        "flags": {
            "exact_only": data.flags.exact_only,
            "include_dynamic": data.flags.include_dynamic,
            "with_macro": data.flags.with_macro,
            "sound": data.flags.sound,
            "direction": data.flags.direction.as_str(),
        },
        "honesty": honesty,
        "truncated": truncated_flag,
        "max_nodes": cap,
        "subset_ok": data.subset_ok,
        "promise_tier": data.promise_tier,
        "empty_note": data.empty_note,
        "nodes": nodes.iter().map(|n| node_json(n)).collect::<Vec<_>>(),
        "edges": edges.iter().map(|e| edge_json(e)).collect::<Vec<_>>(),
        "colors": {
            "exact": COLOR_EXACT,
            "heuristic": COLOR_HEURISTIC,
            "dynamic_candidate": COLOR_DYNAMIC,
        },
    });
    let payload_s = json_embed(&payload);

    format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>agentgraph · {query_esc} · 代码关系图</title>
<style>
  :root {{
    --bg: #f8fafc; --card: #ffffff; --ink: #0f172a; --muted: #64748b;
    --exact: {COLOR_EXACT}; --heuristic: {COLOR_HEURISTIC}; --dynamic: {COLOR_DYNAMIC};
    --macro: {COLOR_MACRO_STROKE}; --line: #e2e8f0;
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0; font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
    background: var(--bg); color: var(--ink); line-height: 1.45;
  }}
  header {{
    padding: 16px 20px 8px; border-bottom: 1px solid var(--line); background: var(--card);
  }}
  h1 {{ margin: 0 0 6px; font-size: 1.25rem; font-weight: 650; }}
  h1 .sub {{ font-weight: 500; color: var(--muted); font-size: 0.95rem; }}
  .honesty {{
    margin: 6px 0; padding: 8px 10px; border-radius: 8px;
    background: #fff7ed; border: 1px solid #fdba74; color: #9a3412;
    font-size: 0.9rem;
  }}
  .meta {{ color: var(--muted); font-size: 0.85rem; }}
  .meta code {{ background: #f1f5f9; padding: 1px 6px; border-radius: 4px; }}
  .legend {{ display: flex; flex-wrap: wrap; gap: 12px; margin: 10px 0 4px; font-size: 0.85rem; }}
  .lg i {{
    display: inline-block; width: 12px; height: 12px; border-radius: 50%;
    margin-right: 6px; vertical-align: -1px; background: var(--exact);
  }}
  .lg:nth-child(2) i {{ background: var(--heuristic); }}
  .lg:nth-child(3) i {{ background: var(--dynamic); }}
  .lg .macro-swatch, .lg:nth-child(4) i {{
    background: transparent; border: 2px solid var(--macro); width: 12px; height: 12px;
  }}
  .layout {{ display: grid; grid-template-columns: 1fr 280px; gap: 12px; padding: 12px 16px 24px; }}
  @media (max-width: 800px) {{ .layout {{ grid-template-columns: 1fr; }} }}
  .canvas-card, .panel {{
    background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 8px;
  }}
  svg {{ width: 100%; height: auto; display: block; min-height: 420px; }}
  .node {{ cursor: pointer; }}
  .node-label {{ font-size: 11px; fill: #0f172a; pointer-events: none; font-weight: 600; }}
  .node-meta {{ font-size: 9px; fill: var(--muted); pointer-events: none; }}
  .macro-badge {{ font-size: 9px; fill: var(--macro); font-weight: 700; }}
  .node.is-selected circle {{ stroke: #0f172a; stroke-width: 3; }}
  .node.is-neighbor circle {{ stroke: var(--macro); stroke-width: 3; }}
  .edge.is-dim {{ opacity: 0.15; }}
  .edge.is-hl {{ opacity: 1; stroke-width: 3; }}
  .panel h2 {{ margin: 4px 8px 8px; font-size: 1rem; }}
  .panel .hint {{ margin: 0 8px 10px; color: var(--muted); font-size: 0.85rem; }}
  #info {{ margin: 0 8px 8px; font-size: 0.9rem; }}
  #info dt {{ color: var(--muted); font-size: 0.75rem; margin-top: 8px; }}
  #info dd {{ margin: 2px 0 0; word-break: break-all; }}
  .empty-state {{
    margin: 8px; padding: 12px; border-radius: 8px; background: #f1f5f9;
    border: 1px dashed #94a3b8; font-size: 0.92rem;
  }}
  .trunc-notice, .sound-line {{
    margin: 8px 20px; padding: 8px 10px; border-radius: 8px;
    background: #eff6ff; border: 1px solid #93c5fd; font-size: 0.85rem;
  }}
  .trunc-notice {{ background: #fef3c7; border-color: #fcd34d; color: #92400e; }}
  .sound-disabled {{
    background: #fef2f2; border-color: #fca5a5; color: #991b1b; font-weight: 600;
  }}
  .sound-ok {{
    background: #ecfdf5; border-color: #6ee7b7; color: #065f46;
  }}
  footer {{ padding: 8px 20px 20px; color: var(--muted); font-size: 0.8rem; }}
</style>
</head>
<body>
<header>
  <h1>代码关系图 <span class="sub">/ Code graph</span> — <code>{query_esc}</code></h1>
  <div class="honesty" id="honesty">{honesty}</div>
  <div class="meta">查询 / Query: <code>{query_esc}</code> · depth={depth} · flags: <code>{flags_esc}</code></div>
  {sound_block}
  {legend}
</header>
{trunc_block}
{empty_block}
<div class="layout">
  <div class="canvas-card">
    <svg id="graph" viewBox="0 0 960 720" role="img" aria-label="code graph">
      <defs>
        <marker id="arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M 0 0 L 10 5 L 0 10 z" fill="#64748b"/>
        </marker>
      </defs>
      <g id="edges">{svg_edges}</g>
      <g id="nodes">{svg_nodes}</g>
    </svg>
  </div>
  <aside class="panel">
    <h2>节点详情 / Node info</h2>
    <p class="hint">点击节点查看邻居与路径 · Click a node to highlight neighbors</p>
    <dl id="info">
      <dt>状态 / Status</dt>
      <dd id="info-status">未选择 · none</dd>
    </dl>
  </aside>
</div>
<footer>
  agentgraph graph · self-contained HTML · offline · 仅展示已索引边（L0/L1 候选），不证明完整性。
</footer>
<script type="application/json" id="graph-data">{payload_s}</script>
<script>
(function () {{
  var raw = document.getElementById("graph-data").textContent;
  var data = {{ nodes: [], edges: [] }};
  try {{ data = JSON.parse(raw); }} catch (e) {{ /* keep empty */ }}
  var nodesEl = document.getElementById("nodes");
  var edgesEl = document.getElementById("edges");
  var info = document.getElementById("info");
  var statusEl = document.getElementById("info-status");
  var nodeById = {{}};
  (data.nodes || []).forEach(function (n) {{ nodeById[n.id] = n; }});
  var neighbors = {{}};
  (data.edges || []).forEach(function (e) {{
    if (!neighbors[e.from]) neighbors[e.from] = {{ ids: [], edges: [] }};
    if (!neighbors[e.to]) neighbors[e.to] = {{ ids: [], edges: [] }};
    neighbors[e.from].ids.push(e.to);
    neighbors[e.from].edges.push(e);
    neighbors[e.to].ids.push(e.from);
    neighbors[e.to].edges.push(e);
  }});
  function setText(id, text) {{
    var dt = document.createElement("dt");
    dt.textContent = id;
    var dd = document.createElement("dd");
    dd.textContent = text == null || text === "" ? "—" : String(text);
    info.appendChild(dt);
    info.appendChild(dd);
  }}
  function clearInfo() {{
    while (info.firstChild) info.removeChild(info.firstChild);
  }}
  function selectNode(id) {{
    var n = nodeById[id];
    var gNodes = nodesEl.querySelectorAll("g.node");
    var gEdges = edgesEl.querySelectorAll("line.edge");
    gNodes.forEach(function (g) {{
      g.classList.remove("is-selected", "is-neighbor");
    }});
    gEdges.forEach(function (ln) {{
      ln.classList.remove("is-dim", "is-hl");
    }});
    if (!n) {{
      clearInfo();
      setText("状态 / Status", "未选择 · none");
      return;
    }}
    var neigh = (neighbors[id] && neighbors[id].ids) || [];
    var neighSet = {{}};
    neigh.forEach(function (x) {{ neighSet[x] = true; }});
    gNodes.forEach(function (g) {{
      var gid = g.getAttribute("data-id");
      if (gid === id) g.classList.add("is-selected");
      else if (neighSet[gid]) g.classList.add("is-neighbor");
    }});
    gEdges.forEach(function (ln) {{
      var f = ln.getAttribute("data-from");
      var t = ln.getAttribute("data-to");
      if (f === id || t === id) ln.classList.add("is-hl");
      else ln.classList.add("is-dim");
    }});
    clearInfo();
    setText("名称 / Name", n.name);
    setText("深度 / Depth", n.depth);
    setText("置信度 / Confidence", n.confidence);
    setText("路径 / Path:line", n.at || n.location || ((n.path || "—") + (n.line != null ? ":" + n.line : "")));
    setText("来源 / Origin", n.origin || "source");
    setText("边类型 / Kind", (data.edges || []).filter(function (e) {{
      return e.from === id || e.to === id;
    }}).map(function (e) {{ return e.kind + "→" + e.confidence; }}).join(", ") || "—");
    setText("邻居 / Neighbors", neigh.length ? neigh.map(function (x) {{
      return (nodeById[x] && nodeById[x].name) || x;
    }}).join(", ") : "—");
  }}
  if (nodesEl) {{
    nodesEl.addEventListener("click", function (ev) {{
      var g = ev.target.closest ? ev.target.closest("g.node") : null;
      if (!g) return;
      selectNode(g.getAttribute("data-id"));
    }});
  }}
  if ((data.nodes || []).length <= 1 && (data.edges || []).length === 0) {{
    clearInfo();
    setText("状态 / Status", "空图 · empty neighborhood");
  }} else {{
    clearInfo();
    setText("状态 / Status", "点击图中节点 · click a node");
  }}
}})();
</script>
</body>
</html>
"##,
        flags_esc = escape_html(&flags),
        depth = data.depth,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_basic() {
        assert_eq!(escape_html("a<b&c"), "a&lt;b&amp;c");
    }

    #[test]
    fn direction_parse() {
        assert_eq!(
            GraphDirection::parse("impact"),
            Some(GraphDirection::Impact)
        );
        assert_eq!(GraphDirection::parse("nope"), None);
    }
}
