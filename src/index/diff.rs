//! Track M4: indexed-edge snapshot diff.
//!
//! **Honesty:** this module compares **indexed edge sets** (refs rows:
//! name + path + line + confidence + enclosing). It is **not** a runtime
//! call-graph diff and does not claim semantic equivalence between commits.
//!
//! Snapshot model (dual sidecar, no large refs-schema migration):
//! - On successful full `index`, current refs are written to
//!   `<root>/.agentgraph/refs.snapshot.json`.
//! - If a previous snapshot existed it is copied to
//!   `<root>/.agentgraph/refs.snapshot.prev.json` (one generation).
//! - `meta.index_seq` is incremented in the SQLite store.
//! - `index_paths` / watch **do not** refresh the baseline (so incremental
//!   dirty-file edges remain visible in the next `diff`).
//!
//! Baseline selection for `agentgraph diff`:
//! 1. `--snapshot PATH` → that file.
//! 2. Else if live refs **match** `refs.snapshot.json` **and** a previous
//!    snapshot exists → compare against `refs.snapshot.prev.json`
//!    (changes since the *previous* full index).
//! 3. Else → compare against `refs.snapshot.json` (changes since the last
//!    full-index baseline, including watch / `index_paths` drift).
//!
//! No baseline file → **fail-loud** with a hint to run `agentgraph index`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::store::Store;
use crate::model::{Confidence, ConfidenceFilter, ReferenceRecord};

/// Sidecar snapshot written at full-index time.
pub const REFS_SNAPSHOT_NAME: &str = "refs.snapshot.json";
/// Previous-generation snapshot (dual meta).
pub const REFS_SNAPSHOT_PREV_NAME: &str = "refs.snapshot.prev.json";
/// Honesty note embedded in every diff payload.
pub const DIFF_HONESTY: &str =
    "indexed edges only; not a runtime call-graph diff (name+path+line+confidence+enclosing)";

/// One indexed reference edge (snapshot row).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SnapshotEdge {
    pub name: String,
    pub path: String,
    pub line: usize,
    /// `exact` | `heuristic` | `dynamic_candidate`
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosing: Option<String>,
    /// Workspace multi-root id. Empty for classic single-root rows.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root_id: String,
}

/// On-disk refs snapshot (sidecar JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefsSnapshot {
    /// Monotonic full-index counter stored in SQLite `meta.index_seq`.
    #[serde(default)]
    pub index_seq: u64,
    /// RFC3339-ish timestamp when the snapshot was written (best-effort).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_at: Option<String>,
    #[serde(default)]
    pub edges: Vec<SnapshotEdge>,
    /// Workspace root_id this snapshot covers (empty = classic single-root).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root_id: String,
}

/// Diff summary counters (full counts, not limited row counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiffSummary {
    pub added: usize,
    pub removed: usize,
}

/// Structured `agentgraph diff` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDiff {
    pub added: Vec<SnapshotEdge>,
    pub removed: Vec<SnapshotEdge>,
    pub summary: DiffSummary,
    pub exact_only: bool,
    /// Always present — honesty string (indexed edges only).
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_index_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_index_seq: Option<u64>,
    /// Which baseline file/strategy was used (`prev`, `snapshot`, `explicit`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_source: Option<String>,
    /// Workspace root filter applied to this diff (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    /// True when a dirty reindex (`watch` / `index_paths`) ran after the last
    /// full-index baseline write. Diff baseline was **not** auto-refreshed.
    #[serde(default)]
    pub baseline_stale: bool,
}

/// Meta key for the baseline-stale honesty flag (P5).
pub const BASELINE_STALE_META_KEY: &str = "baseline_stale";

/// Read `meta.baseline_stale` (false when unset / after full index).
///
/// Stable helper for CLI / MCP / Agents — never writes meta.
pub fn baseline_stale_flag(store: &Store) -> bool {
    store
        .get_meta(BASELINE_STALE_META_KEY)
        .ok()
        .flatten()
        .as_deref()
        == Some("true")
}

/// Set `meta.baseline_stale`. Full index / `--write-snapshot` clear it;
/// dirty `index_paths` / watch scoped reindex set it. Does **not** rewrite
/// the snapshot baseline itself.
pub fn set_baseline_stale(store: &Store, stale: bool) -> Result<()> {
    store.set_meta(
        BASELINE_STALE_META_KEY,
        if stale { "true" } else { "false" },
    )
}

/// Stable set-key for one edge: name + path + line + confidence + enclosing.
pub fn edge_key(e: &SnapshotEdge) -> String {
    format!(
        "{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}",
        e.name,
        e.path,
        e.line,
        e.confidence,
        e.enclosing.clone().unwrap_or_default()
    )
}

fn filter_exact(edges: &[SnapshotEdge]) -> Vec<SnapshotEdge> {
    edges
        .iter()
        .filter(|e| e.confidence == Confidence::Exact.as_str())
        .cloned()
        .collect()
}

fn apply_limit(mut rows: Vec<SnapshotEdge>, limit: Option<usize>) -> Vec<SnapshotEdge> {
    if let Some(n) = limit {
        rows.truncate(n);
    }
    rows
}

/// Set-difference over snapshot edges (added = in current only, removed = in baseline only).
///
/// `summary` always reflects **full** set sizes; `added`/`removed` vectors are
/// optionally truncated by `limit`.
pub fn diff_edges(
    baseline: &[SnapshotEdge],
    current: &[SnapshotEdge],
    exact_only: bool,
    limit: Option<usize>,
) -> EdgeDiff {
    let base = if exact_only {
        filter_exact(baseline)
    } else {
        baseline.to_vec()
    };
    let curr = if exact_only {
        filter_exact(current)
    } else {
        current.to_vec()
    };

    let base_keys: HashSet<String> = base.iter().map(edge_key).collect();
    let curr_keys: HashSet<String> = curr.iter().map(edge_key).collect();

    let mut added: Vec<SnapshotEdge> = curr
        .iter()
        .filter(|e| !base_keys.contains(&edge_key(e)))
        .cloned()
        .collect();
    let mut removed: Vec<SnapshotEdge> = base
        .iter()
        .filter(|e| !curr_keys.contains(&edge_key(e)))
        .cloned()
        .collect();
    added.sort();
    removed.sort();

    let summary = DiffSummary {
        added: added.len(),
        removed: removed.len(),
    };
    EdgeDiff {
        added: apply_limit(added, limit),
        removed: apply_limit(removed, limit),
        summary,
        exact_only,
        note: DIFF_HONESTY.to_string(),
        baseline_index_seq: None,
        current_index_seq: None,
        baseline_source: None,
        root_id: None,
        baseline_stale: false,
    }
}

/// Map a store `ReferenceRecord` to a snapshot edge.
pub fn ref_to_edge(r: &ReferenceRecord) -> SnapshotEdge {
    SnapshotEdge {
        name: r.name.clone(),
        path: r.path.clone(),
        line: r.line,
        confidence: r.confidence.as_str().to_string(),
        enclosing: r.enclosing.clone(),
        root_id: r.root_id.clone(),
    }
}

/// Collect all indexed ref edges from the store (confidence window = everything).
pub fn collect_store_edges(store: &Store) -> Result<Vec<SnapshotEdge>> {
    collect_store_edges_in(store, None)
}

/// Collect edges, optionally scoped to one workspace `root_id`.
pub fn collect_store_edges_in(store: &Store, root_id: Option<&str>) -> Result<Vec<SnapshotEdge>> {
    let rows = store.all_refs_for_export(ConfidenceFilter::IncludeDynamic)?;
    let edges: Vec<SnapshotEdge> = rows
        .iter()
        .filter(|r| match root_id {
            None => true,
            Some(rid) => r.root_id == rid,
        })
        .map(ref_to_edge)
        .collect();
    Ok(edges)
}

/// `<root>/.agentgraph/refs.snapshot.json`
pub fn snapshot_path(root: &Path) -> PathBuf {
    root.join(".agentgraph").join(REFS_SNAPSHOT_NAME)
}

/// `<root>/.agentgraph/refs.snapshot.prev.json`
pub fn snapshot_prev_path(root: &Path) -> PathBuf {
    root.join(".agentgraph").join(REFS_SNAPSHOT_PREV_NAME)
}

/// Workspace per-root snapshot: `<root>/.agentgraph/refs.snapshot.<root_id>.json`
pub fn workspace_snapshot_path(root: &Path, root_id: &str) -> PathBuf {
    root.join(".agentgraph")
        .join(format!("refs.snapshot.{root_id}.json"))
}

/// Workspace per-root previous snapshot.
pub fn workspace_snapshot_prev_path(root: &Path, root_id: &str) -> PathBuf {
    root.join(".agentgraph")
        .join(format!("refs.snapshot.{root_id}.prev.json"))
}

fn now_stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map(|s| format!("unix:{s}"))
        .unwrap_or_else(|_| "unknown".into())
}

fn read_snapshot_file(path: &Path) -> Result<RefsSnapshot> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read snapshot {}", path.display()))?;
    let snap: RefsSnapshot = serde_json::from_str(&text)
        .with_context(|| format!("parse snapshot {}", path.display()))?;
    Ok(snap)
}

fn write_snapshot_file(path: &Path, snap: &RefsSnapshot) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(snap)?;
    std::fs::write(path, text).with_context(|| format!("write snapshot {}", path.display()))?;
    Ok(())
}

fn next_index_seq(store: &Store) -> Result<u64> {
    let prev = store
        .get_meta("index_seq")?
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    Ok(prev + 1)
}

/// Write dual sidecar snapshots from the current store + bump `meta.index_seq`.
///
/// Called at the end of a successful full `index()` (including noop early-out).
/// Does **not** run on `index_paths` / watch incremental reindex.
pub fn write_index_snapshot(root: &Path, store: &Store) -> Result<RefsSnapshot> {
    write_index_snapshot_for_root(root, store, "")
}

/// Workspace-aware snapshot write. `root_id == ""` keeps the classic
/// `refs.snapshot.json` (all edges). Non-empty root_id writes **per-root**
/// sidecars `refs.snapshot.<root_id>.json` under that root's `.agentgraph/`.
pub fn write_index_snapshot_for_root(
    root: &Path,
    store: &Store,
    root_id: &str,
) -> Result<RefsSnapshot> {
    let edges = if root_id.is_empty() {
        collect_store_edges(store)?
    } else {
        collect_store_edges_in(store, Some(root_id))?
    };
    let index_seq = next_index_seq(store)?;
    store.set_meta("index_seq", &index_seq.to_string())?;
    store.set_meta("indexed_at", &now_stamp())?;
    // P5: full-index snapshot write clears the dirty-reindex stale flag.
    let _ = set_baseline_stale(store, false);

    let (snap_path, prev_path) = if root_id.is_empty() {
        (snapshot_path(root), snapshot_prev_path(root))
    } else {
        (
            workspace_snapshot_path(root, root_id),
            workspace_snapshot_prev_path(root, root_id),
        )
    };
    // Dual meta: preserve the previous generation before overwriting.
    if snap_path.exists() {
        if let Ok(old) = read_snapshot_file(&snap_path) {
            let _ = write_snapshot_file(&prev_path, &old);
        }
    }
    let snap = RefsSnapshot {
        index_seq,
        indexed_at: Some(now_stamp()),
        edges,
        root_id: root_id.to_string(),
    };
    write_snapshot_file(&snap_path, &snap)?;
    Ok(snap)
}

/// Promote the **current live refs** as the new baseline (both generations).
///
/// Used by `diff --write-snapshot`. After this, an immediate re-diff is empty
/// until further index/watch changes accumulate.
pub fn write_baseline_snapshot(root: &Path, store: &Store) -> Result<RefsSnapshot> {
    write_baseline_snapshot_for_root(root, store, "")
}

/// Workspace-aware baseline promote. Non-empty `root_id` writes per-root sidecars.
pub fn write_baseline_snapshot_for_root(
    root: &Path,
    store: &Store,
    root_id: &str,
) -> Result<RefsSnapshot> {
    let edges = if root_id.is_empty() {
        collect_store_edges(store)?
    } else {
        collect_store_edges_in(store, Some(root_id))?
    };
    let index_seq = store
        .get_meta("index_seq")?
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let snap = RefsSnapshot {
        index_seq,
        indexed_at: Some(now_stamp()),
        edges,
        root_id: root_id.to_string(),
    };
    let (snap_path, prev_path) = if root_id.is_empty() {
        (snapshot_path(root), snapshot_prev_path(root))
    } else {
        (
            workspace_snapshot_path(root, root_id),
            workspace_snapshot_prev_path(root, root_id),
        )
    };
    write_snapshot_file(&snap_path, &snap)?;
    write_snapshot_file(&prev_path, &snap)?;
    // P5: explicit baseline promote locks "now" — clear stale flag.
    let _ = set_baseline_stale(store, false);
    Ok(snap)
}

fn edge_sets_equal(a: &[SnapshotEdge], b: &[SnapshotEdge]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let ka: HashSet<String> = a.iter().map(edge_key).collect();
    let kb: HashSet<String> = b.iter().map(edge_key).collect();
    ka == kb
}

/// Load the baseline snapshot for `diff`.
///
/// Fail-loud when no baseline exists. `root_id` selects workspace per-root sidecars.
pub fn load_baseline(
    root: &Path,
    store: &Store,
    explicit: Option<&Path>,
) -> Result<(RefsSnapshot, String)> {
    load_baseline_for_root(root, store, explicit, "")
}

pub fn load_baseline_for_root(
    root: &Path,
    store: &Store,
    explicit: Option<&Path>,
    root_id: &str,
) -> Result<(RefsSnapshot, String)> {
    if let Some(p) = explicit {
        if !p.exists() {
            bail!(
                "snapshot not found: {} — run `agentgraph index` first, or pass a valid --snapshot path",
                p.display()
            );
        }
        let snap = read_snapshot_file(p)?;
        return Ok((snap, format!("explicit:{}", p.display())));
    }

    let (snap_path, prev_path) = if root_id.is_empty() {
        (snapshot_path(root), snapshot_prev_path(root))
    } else {
        (
            workspace_snapshot_path(root, root_id),
            workspace_snapshot_prev_path(root, root_id),
        )
    };
    if !snap_path.exists() && !prev_path.exists() {
        // Fall back to classic single-root snapshot when workspace sidecar absent.
        let classic = snapshot_path(root);
        let classic_prev = snapshot_prev_path(root);
        if classic.exists() || classic_prev.exists() {
            return load_baseline_for_root(root, store, None, "");
        }
        bail!(
            "no refs snapshot baseline at {} — run `agentgraph index` first \
             (full index writes the baseline; then re-index after edits and run `agentgraph diff`)",
            snap_path.display()
        );
    }

    let live = collect_store_edges_in(
        store,
        if root_id.is_empty() {
            None
        } else {
            Some(root_id)
        },
    )?;
    let snap = if snap_path.exists() {
        read_snapshot_file(&snap_path)?
    } else {
        read_snapshot_file(&prev_path)?
    };

    // Dual-meta selection: when live still matches the latest snapshot and a
    // previous generation exists, compare against the previous full index.
    if snap_path.exists() && prev_path.exists() && edge_sets_equal(&live, &snap.edges) {
        let prev = read_snapshot_file(&prev_path)?;
        return Ok((prev, "previous".into()));
    }
    let source = if snap_path.exists() {
        "snapshot".to_string()
    } else {
        "previous".to_string()
    };
    Ok((snap, source))
}

/// Full diff pipeline: load baseline, collect live edges, set-diff.
pub fn run_diff(
    root: &Path,
    store: &Store,
    exact_only: bool,
    limit: Option<usize>,
    snapshot: Option<&Path>,
) -> Result<EdgeDiff> {
    run_diff_for_root(root, store, exact_only, limit, snapshot, "")
}

/// Workspace-aware diff. Non-empty `root_id` scopes live edges + baseline sidecar.
pub fn run_diff_for_root(
    root: &Path,
    store: &Store,
    exact_only: bool,
    limit: Option<usize>,
    snapshot: Option<&Path>,
    root_id: &str,
) -> Result<EdgeDiff> {
    store.ensure_indexed()?;
    let (baseline, source) = load_baseline_for_root(root, store, snapshot, root_id)?;
    let live = collect_store_edges_in(
        store,
        if root_id.is_empty() {
            None
        } else {
            Some(root_id)
        },
    )?;
    let mut d = diff_edges(&baseline.edges, &live, exact_only, limit);
    d.baseline_index_seq = Some(baseline.index_seq);
    d.baseline_source = Some(source);
    if !root_id.is_empty() {
        d.root_id = Some(root_id.to_string());
    }
    // P5: always report whether dirty reindex drifted after the last full snapshot.
    d.baseline_stale = baseline_stale_flag(store);
    let current_seq = store
        .get_meta("index_seq")?
        .and_then(|s| s.parse::<u64>().ok());
    d.current_index_seq = current_seq;
    d.note = DIFF_HONESTY.to_string();
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(name: &str, path: &str, line: usize, conf: &str, enc: Option<&str>) -> SnapshotEdge {
        SnapshotEdge {
            name: name.into(),
            path: path.into(),
            line,
            confidence: conf.into(),
            enclosing: enc.map(|s| s.to_string()),
            root_id: String::new(),
        }
    }

    #[test]
    fn set_diff_added_removed() {
        let base = vec![
            edge("a", "src/a.ts", 1, "exact", Some("f")),
            edge("b", "src/b.ts", 2, "exact", None),
        ];
        let curr = vec![
            edge("a", "src/a.ts", 1, "exact", Some("f")),
            edge("c", "src/c.ts", 3, "exact", Some("g")),
        ];
        let d = diff_edges(&base, &curr, false, None);
        assert_eq!(d.summary.added, 1);
        assert_eq!(d.summary.removed, 1);
        assert_eq!(d.added[0].name, "c");
        assert_eq!(d.removed[0].name, "b");
        assert!(d.note.contains("indexed edges"));
    }

    #[test]
    fn exact_only_drops_heuristic() {
        let base = vec![edge("a", "src/a.ts", 1, "exact", None)];
        let curr = vec![
            edge("a", "src/a.ts", 1, "exact", None),
            edge("h", "src/h.ts", 4, "heuristic", None),
        ];
        let all = diff_edges(&base, &curr, false, None);
        assert_eq!(all.summary.added, 1);
        let ex = diff_edges(&base, &curr, true, None);
        assert_eq!(ex.summary.added, 0);
        assert!(ex.exact_only);
    }

    #[test]
    fn limit_caps_rows_but_not_summary() {
        let base: Vec<SnapshotEdge> = vec![];
        let curr: Vec<SnapshotEdge> = (0..5)
            .map(|i| edge(&format!("n{i}"), "src/x.ts", i, "exact", None))
            .collect();
        let d = diff_edges(&base, &curr, false, Some(2));
        assert_eq!(d.added.len(), 2);
        assert_eq!(d.summary.added, 5);
    }
}
