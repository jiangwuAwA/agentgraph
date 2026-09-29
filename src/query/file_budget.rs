//! I2 precision file budget: rank + prune file candidates (少而准).
//!
//! Default product stance: when a query returns many files, emit a **budgeted**
//! `selected[]` and park the rest in `pruned[]` instead of flooding the caller.
//!
//! Stable payload keys (勿改名): `file_budget`, `selected`, `pruned`,
//! `pruned_count`, `selection_reason`.

use serde_json::{json, Value};

pub const DEFAULT_FILE_BUDGET: usize = 5;

/// Directory segments that are usually noise for product blast (I2/I3 demotion).
const LOW_PRIORITY_DIRS: &[&str] = &[
    "legacy",
    "admin",
    "deprecated",
    "demo",
    "demos",
    "mock",
    "mocks",
    "fake",
    "fakes",
    "seed",
    "seeds",
    "noise",
    "test",
    "tests",
    "__tests__",
    "fixture",
    "fixtures",
    "docs",
    "doc",
    "examples",
    "example",
    "benchmark",
    "benches",
    "generated",
];

/// I3: hard decoy dirs — pruned unless unique exact call site.
fn is_hard_decoy_dir(path: &str) -> bool {
    let segs = path_segments(path);
    segs[..segs.len().saturating_sub(1)].iter().any(|s| {
        let low = s.to_ascii_lowercase();
        matches!(
            low.as_str(),
            "legacy"
                | "admin"
                | "deprecated"
                | "demo"
                | "demos"
                | "mock"
                | "mocks"
                | "fake"
                | "fakes"
                | "seed"
                | "seeds"
                | "noise"
        )
    })
}

/// Eval/flag default when a tighter budget is requested (I3).
pub const EVAL_FILE_BUDGET: usize = 5;
pub const PRECISION_MAX_SELECTED: usize = 4;

/// Basename stems that often collide (common-name demotion).
const COMMON_BASENAMES: &[&str] = &[
    "util",
    "utils",
    "helper",
    "helpers",
    "common",
    "config",
    "constants",
    "types",
    "index",
    "logger",
    "log",
    "metrics",
    "metric",
    "cache",
    "queue",
    "repo",
    "repository",
    "handler",
    "handlers",
    "registry",
    "manager",
    "service",
    "services",
];

/// One file candidate for budgeted selection.
#[derive(Debug, Clone)]
pub struct FileCandidate {
    pub path: String,
    /// Higher is better. Product rules contribute; selection sorts by this.
    pub score: f64,
    pub edge_role: Option<String>,
    pub depth: usize,
}

/// Budgeted file selection result (stable keys).
#[derive(Debug, Clone, PartialEq)]
pub struct FileSelection {
    pub file_budget: usize,
    pub selected: Vec<String>,
    pub pruned: Vec<String>,
    pub pruned_count: usize,
    pub selection_reason: String,
}

impl FileSelection {
    pub fn to_json(&self) -> Value {
        json!({
            "file_budget": self.file_budget,
            "selected": self.selected,
            "pruned": self.pruned,
            "pruned_count": self.pruned_count,
            "selection_reason": self.selection_reason,
        })
    }
}

fn path_segments(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(|s| s.to_string())
        .collect()
}

fn basename(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

fn strip_ext(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

/// Path penalty: low-priority dirs and common basenames.
pub fn path_penalty(path: &str) -> f64 {
    let segs = path_segments(path);
    let mut pen = 0.0;
    for s in &segs[..segs.len().saturating_sub(1)] {
        let low = s.to_ascii_lowercase();
        if LOW_PRIORITY_DIRS.contains(&low.as_str()) {
            pen += 8.0;
        }
    }
    let base = strip_ext(&basename(path)).to_ascii_lowercase();
    if COMMON_BASENAMES.contains(&base.as_str()) {
        pen += 10.0;
    }
    pen
}
fn edge_role_score(role: Option<&str>) -> f64 {
    // I3 rank: exact call site > registration > definition > implementor.
    match role {
        Some("call") | Some("import") => 12.0,
        Some("registration") => 10.0,
        Some("define") => 8.0,
        Some("implementor") => 3.0,
        Some("dynamic") => 1.0,
        _ => 4.0,
    }
}

fn depth_score(depth: usize) -> f64 {
    match depth {
        0 => 5.0,
        1 => 3.0,
        2 => 1.5,
        _ => 0.5,
    }
}

/// Composite score for one candidate (already includes role/depth if provided).
pub fn score_candidate(c: &FileCandidate) -> f64 {
    let role = edge_role_score(c.edge_role.as_deref());
    let d = depth_score(c.depth);
    let pen = path_penalty(&c.path);
    c.score + role + d - pen
}

/// Rank + prune files to `budget`. Diversity: same directory keeps at most 2
/// representatives (others get extra demotion before sort).
pub fn select_files_budgeted(candidates: Vec<FileCandidate>, budget: usize) -> FileSelection {
    let budget = budget.max(1);
    if candidates.is_empty() {
        return FileSelection {
            file_budget: budget,
            selected: Vec::new(),
            pruned: Vec::new(),
            pruned_count: 0,
            selection_reason: "empty_candidates".to_string(),
        };
    }

    let mut best: std::collections::BTreeMap<String, (f64, bool)> =
        std::collections::BTreeMap::new();
    for c in &candidates {
        let s = score_candidate(c);
        let exact_call = matches!(
            c.edge_role.as_deref(),
            Some("call") | Some("import") | Some("define")
        ) && c.depth <= 1;
        let e = best
            .entry(c.path.replace('\\', "/"))
            .or_insert((s, exact_call));
        if s > e.0 {
            *e = (s, exact_call);
        }
    }

    let mut scored: Vec<(String, f64, bool)> =
        best.into_iter().map(|(p, (s, e))| (p, s, e)).collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut selected: Vec<String> = Vec::new();
    let mut pruned: Vec<String> = Vec::new();
    let mut dir_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut decoy_selected = 0usize;

    for (path, _s, exact_call) in &scored {
        if is_hard_decoy_dir(path) && !(*exact_call && selected.is_empty()) {
            pruned.push(path.clone());
            continue;
        }
        let segs = path_segments(path);
        let dir = if segs.len() >= 2 {
            segs[..segs.len() - 1].join("/")
        } else {
            String::new()
        };
        let n = dir_counts.get(&dir).copied().unwrap_or(0);
        if n >= 2 {
            pruned.push(path.clone());
            continue;
        }
        let pure_decoy = path_penalty(path) >= 8.0 && !*exact_call;
        if pure_decoy && decoy_selected >= 1 {
            pruned.push(path.clone());
            continue;
        }
        let base = strip_ext(&basename(path)).to_ascii_lowercase();
        let best_for_base = scored
            .iter()
            .filter(|(p, _, _)| strip_ext(&basename(p)).to_ascii_lowercase() == base)
            .map(|(p, s, _)| (p.clone(), *s))
            .max_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.0.cmp(&a.0))
            })
            .map(|(p, _)| p)
            .unwrap_or_default();
        if best_for_base != *path {
            pruned.push(path.clone());
            continue;
        }
        if selected.len() >= budget || selected.len() >= PRECISION_MAX_SELECTED {
            pruned.push(path.clone());
            continue;
        }
        selected.push(path.clone());
        *dir_counts.entry(dir).or_insert(0) += 1;
        if pure_decoy {
            decoy_selected += 1;
        }
    }

    let reason = if pruned.is_empty() {
        format!("all_{}_files_within_budget_{}", selected.len(), budget)
    } else {
        format!(
            "precision_budget_{budget}: exact_call_gt_registration_gt_definition; hard_decoy_dir_prune; dir_max2; pure_decoy_cap1; pruned_{}",
            pruned.len()
        )
    };

    FileSelection {
        file_budget: budget,
        selected,
        pruned_count: pruned.len(),
        pruned,
        selection_reason: reason,
    }
}

/// Build candidates from impact/blast node JSON rows.
pub fn candidates_from_nodes(nodes: &[Value]) -> Vec<FileCandidate> {
    let mut out = Vec::new();
    for n in nodes {
        let path = n
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or_default()
            .to_string();
        if path.is_empty() {
            continue;
        }
        let role = n
            .get("edge_role")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());
        let depth = n.get("depth").and_then(|d| d.as_u64()).unwrap_or(0) as usize;
        let conf = n.get("confidence").and_then(|c| c.as_str()).unwrap_or("");
        let bonus = if conf == "exact" { 2.0 } else { 0.0 };
        out.push(FileCandidate {
            path,
            score: bonus,
            edge_role: role,
            depth,
        });
    }
    out
}

/// Build candidates from caller/reference JSON rows.
pub fn candidates_from_rows(rows: &[Value]) -> Vec<FileCandidate> {
    let mut out = Vec::new();
    for n in rows {
        let path = n
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or_default()
            .to_string();
        if path.is_empty() {
            continue;
        }
        let role = n
            .get("edge_role")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());
        let conf = n.get("confidence").and_then(|c| c.as_str()).unwrap_or("");
        let bonus = if conf == "exact" { 2.0 } else { 0.0 };
        out.push(FileCandidate {
            path,
            score: bonus,
            edge_role: role,
            depth: 1,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(path: &str, role: &str, depth: usize) -> FileCandidate {
        FileCandidate {
            path: path.to_string(),
            score: 0.0,
            edge_role: Some(role.to_string()),
            depth,
        }
    }

    #[test]
    fn budget_prunes_overflow() {
        let mut cs = Vec::new();
        for i in 0..20 {
            cs.push(cand(&format!("src/mod{i}/file{i}.ts"), "call", 1));
        }
        let sel = select_files_budgeted(cs, 8);
        assert_eq!(sel.selected.len(), 4);
        assert_eq!(sel.pruned_count, 16);
        assert_eq!(sel.file_budget, 8);
        assert!(sel.selected.iter().all(|p| !sel.pruned.contains(p)));
    }

    #[test]
    fn i3_hard_decoy_dir_pruned() {
        let cs = vec![
            cand("src/orders/createOrder.ts", "call", 0),
            cand("src/legacy/createOrder.ts", "call", 1),
            cand("src/admin/orderAdmin.ts", "call", 1),
            cand("src/deprecated/oldOrder.ts", "call", 1),
        ];
        let sel = select_files_budgeted(cs, 8);
        assert!(sel
            .selected
            .contains(&"src/orders/createOrder.ts".to_string()));
        assert!(sel.pruned.iter().any(|p| p.contains("legacy")));
        assert!(sel.pruned.iter().any(|p| p.contains("admin")));
        assert!(sel.pruned.iter().any(|p| p.contains("deprecated")));
    }

    #[test]
    fn i3_dir_max_two_selected() {
        let mut cs = Vec::new();
        for i in 0..5 {
            cs.push(cand(&format!("src/orders/order{i}.ts"), "call", 1));
        }
        let sel = select_files_budgeted(cs, 8);
        let n = sel
            .selected
            .iter()
            .filter(|p| p.contains("src/orders/"))
            .count();
        assert!(n <= 2, "dir diversity max 2, got {n}");
    }

    #[test]
    fn legacy_and_admin_demoted() {
        let cs = vec![
            cand("src/orders/createOrder.ts", "call", 0),
            cand("src/legacy/createOrder.ts", "call", 1),
            cand("src/admin/orderAdmin.ts", "call", 1),
            cand("src/orders/orderService.ts", "call", 1),
        ];
        let sel = select_files_budgeted(cs, 2);
        assert!(sel
            .selected
            .contains(&"src/orders/createOrder.ts".to_string()));
        assert!(sel
            .selected
            .contains(&"src/orders/orderService.ts".to_string()));
        assert!(sel.pruned.iter().any(|p| p.contains("legacy")));
        assert!(sel.pruned.iter().any(|p| p.contains("admin")));
    }

    #[test]
    fn exact_call_beats_implementor() {
        let cs = vec![
            cand("src/a/impl.ts", "implementor", 1),
            cand("src/b/call.ts", "call", 1),
        ];
        let sel = select_files_budgeted(cs, 1);
        assert_eq!(sel.selected, vec!["src/b/call.ts".to_string()]);
    }

    #[test]
    fn diversity_limits_same_dir() {
        let mut cs = Vec::new();
        for i in 0..6 {
            cs.push(cand(&format!("src/orders/order{i}.ts"), "call", 1));
        }
        cs.push(cand("src/payments/chargeCard.ts", "call", 1));
        let sel = select_files_budgeted(cs, 4);
        let orders = sel
            .selected
            .iter()
            .filter(|p| p.contains("src/orders/"))
            .count();
        // diversity demotion should not fill entire budget with one dir only
        assert!(orders <= 4);
        assert!(sel.selected.iter().any(|p| p.contains("chargeCard")));
    }

    #[test]
    fn payload_json_stable_keys() {
        let cs = vec![cand("src/a.ts", "call", 0), cand("src/b.ts", "call", 1)];
        let sel = select_files_budgeted(cs, 1);
        let v = sel.to_json();
        for k in [
            "file_budget",
            "selected",
            "pruned",
            "pruned_count",
            "selection_reason",
        ] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
    }

    #[test]
    fn i4_generic_basename_demoted() {
        let cs = vec![
            cand("src/orders/createOrder.ts", "call", 0),
            cand("src/shared/config.ts", "call", 1),
            cand("src/shared/util.ts", "call", 1),
        ];
        let sel = select_files_budgeted(cs, 5);
        assert!(sel
            .selected
            .contains(&"src/orders/createOrder.ts".to_string()));
    }

    #[test]
    fn i4_legacy_admin_zero_tolerance() {
        let cs = vec![
            cand("src/orders/createOrder.ts", "call", 0),
            cand("src/legacy/createOrder.ts", "call", 1),
            cand("src/admin/createOrder.ts", "call", 1),
            cand("src/mock/createOrder.ts", "call", 1),
            cand("src/demo/createOrder.ts", "call", 1),
        ];
        let sel = select_files_budgeted(cs, 5);
        assert_eq!(sel.selected, vec!["src/orders/createOrder.ts".to_string()]);
    }

    #[test]
    fn i4_same_basename_keeps_best_only() {
        let cs = vec![
            cand("src/cache/registry.ts", "call", 0),
            cand("src/api/registry.ts", "call", 1),
            cand("src/cli/registry.ts", "call", 1),
        ];
        let sel = select_files_budgeted(cs, 5);
        let regs: Vec<_> = sel
            .selected
            .iter()
            .filter(|p| p.ends_with("registry.ts"))
            .collect();
        assert_eq!(regs.len(), 1, "same basename keep one: {sel:?}");
    }
}
