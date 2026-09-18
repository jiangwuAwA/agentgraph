//! Track M4-W: multi-root workspace indexing into a single SQLite store.
//!
//! **Design:** one shared `index.db` + `root_id` column on `files`/`symbols`/`refs`
//! (not N separate `.agentgraph` dirs) so agents open one connection.
//!
//! **DB location (documented choice):**
//! - `--workspace-db <path>` wins when set
//! - else `--workspace <manifest.json>` → `<manifest_dir>/.agentgraph/index.db`
//! - else `--workspace-root <dir>…` → **first root's** `<dir>/.agentgraph/index.db`
//!
//! Macro sidecars and (workspace) diff snapshots stay **per-root path** — see
//! `docs/workspace.md`. Nested roots are allowed with a warning; duplicate
//! canonical paths are hard-rejected.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::parser;
use super::store::Store;
use super::Indexer;
use crate::model::{IndexStats, WorkspaceIndexResult, WorkspaceRootInfo, WorkspaceStatus};

/// One workspace project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot {
    pub id: String,
    pub path: PathBuf,
}

/// Manifest JSON: `{ "roots": [ {"id","path"}, … ] }` or a bare array of paths.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum ManifestDoc {
    Wrapped { roots: Vec<ManifestRoot> },
    Array(Vec<ManifestPath>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum ManifestRoot {
    Full { id: String, path: String },
    PathOnly(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum ManifestPath {
    Path(String),
    Full { id: String, path: String },
}

/// Default `root_id` = directory basename (sanitized). Collisions get a short hash suffix.
pub fn default_root_id(path: &Path) -> String {
    let canon = path
        .canonicalize()
        .map(|p| parser::normalize_root(&p))
        .unwrap_or_else(|_| path.to_path_buf());
    let base = canon
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "root".to_string());
    sanitize_root_id(&base)
}

fn sanitize_root_id(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "root".to_string()
    } else {
        cleaned
    }
}

fn short_hash(path: &Path) -> String {
    let mut h = Sha256::new();
    h.update(path.to_string_lossy().as_bytes());
    let dig = h.finalize();
    format!(
        "{:x}",
        dig[0..4].iter().fold(0u32, |a, b| (a << 8) | *b as u32)
    )
}

/// Canonical absolute path string for comparison.
pub fn canon_path_str(path: &Path) -> Result<String> {
    let raw = path
        .canonicalize()
        .with_context(|| format!("workspace root does not exist: {}", path.display()))?;
    Ok(parser::normalize_root(&raw)
        .to_string_lossy()
        .replace('\\', "/"))
}

/// Parse a workspace manifest file.
pub fn parse_manifest(path: &Path) -> Result<Vec<WorkspaceRoot>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read workspace manifest {}", path.display()))?;
    let doc: ManifestDoc = serde_json::from_str(&text)
        .with_context(|| format!("parse workspace manifest {}", path.display()))?;
    let base = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut roots: Vec<WorkspaceRoot> = Vec::new();
    match doc {
        ManifestDoc::Wrapped { roots: items } => {
            for item in items {
                match item {
                    ManifestRoot::Full { id, path: p } => {
                        let abs = resolve_manifest_path(&base, &p);
                        roots.push(WorkspaceRoot {
                            id: sanitize_root_id(&id),
                            path: abs,
                        });
                    }
                    ManifestRoot::PathOnly(p) => {
                        let abs = resolve_manifest_path(&base, &p);
                        let id = default_root_id(&abs);
                        roots.push(WorkspaceRoot { id, path: abs });
                    }
                }
            }
        }
        ManifestDoc::Array(items) => {
            for item in items {
                match item {
                    ManifestPath::Path(p) => {
                        let abs = resolve_manifest_path(&base, &p);
                        let id = default_root_id(&abs);
                        roots.push(WorkspaceRoot { id, path: abs });
                    }
                    ManifestPath::Full { id, path: p } => {
                        let abs = resolve_manifest_path(&base, &p);
                        roots.push(WorkspaceRoot {
                            id: sanitize_root_id(&id),
                            path: abs,
                        });
                    }
                }
            }
        }
    }
    if roots.is_empty() {
        bail!("workspace manifest has no roots: {}", path.display());
    }
    finalize_roots(roots)
}

fn resolve_manifest_path(base: &Path, p: &str) -> PathBuf {
    let raw = PathBuf::from(p);
    if raw.is_absolute() {
        raw
    } else {
        base.join(raw)
    }
}

/// Build roots from repeatable `--workspace-root` dirs (id = basename).
pub fn roots_from_dirs(dirs: &[PathBuf]) -> Result<Vec<WorkspaceRoot>> {
    if dirs.is_empty() {
        bail!("no --workspace-root directories provided");
    }
    let mut roots = Vec::with_capacity(dirs.len());
    for d in dirs {
        if !d.is_dir() {
            bail!("workspace root is not a directory: {}", d.display());
        }
        let id = default_root_id(d);
        roots.push(WorkspaceRoot {
            id,
            path: d.clone(),
        });
    }
    finalize_roots(roots)
}

/// Deduplicate ids, hard-reject duplicate paths, warn on nesting.
/// Returns roots + warnings via `finalize_roots_with_warnings`.
pub fn finalize_roots(roots: Vec<WorkspaceRoot>) -> Result<Vec<WorkspaceRoot>> {
    finalize_roots_with_warnings(roots).map(|(r, _w)| r)
}

/// Same as [`finalize_roots`] but also returns nesting warnings.
pub fn finalize_roots_with_warnings(
    roots: Vec<WorkspaceRoot>,
) -> Result<(Vec<WorkspaceRoot>, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut seen_paths: HashMap<String, String> = HashMap::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut out: Vec<WorkspaceRoot> = Vec::with_capacity(roots.len());

    for r in roots {
        if !r.path.is_dir() {
            bail!("workspace root is not a directory: {}", r.path.display());
        }
        let canon = canon_path_str(&r.path)?;
        if let Some(prev_id) = seen_paths.get(&canon) {
            bail!(
                "duplicate workspace root path '{}' (ids '{prev_id}' and '{}'); \
                 each path may appear only once",
                r.path.display(),
                r.id
            );
        }
        let mut id = sanitize_root_id(&r.id);
        if seen_ids.contains(&id) {
            id = format!("{id}-{}", short_hash(&r.path));
            // If still colliding (extremely unlikely), append index.
            if seen_ids.contains(&id) {
                id = format!("{id}-{}", out.len());
            }
        }
        seen_ids.insert(id.clone());
        seen_paths.insert(canon, id.clone());
        out.push(WorkspaceRoot { id, path: r.path });
    }

    // Nesting: allow + warn (documented; not a hard reject).
    for i in 0..out.len() {
        for j in 0..out.len() {
            if i == j {
                continue;
            }
            let a = canon_path_str(&out[i].path)?;
            let b = canon_path_str(&out[j].path)?;
            if b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/")) {
                warnings.push(format!(
                    "workspace roots nest: '{}' ({}) overlaps '{}' ({}); \
                     relative paths are stored per root_id and may double-count shared files",
                    out[i].id,
                    out[i].path.display(),
                    out[j].id,
                    out[j].path.display()
                ));
            }
        }
    }
    // Dedup warnings (i,j) and (j,i)
    warnings.sort();
    warnings.dedup();
    Ok((out, warnings))
}

/// Resolve the shared workspace SQLite path (documented order).
pub fn resolve_db_path(
    workspace_manifest: Option<&Path>,
    workspace_db: Option<&Path>,
    workspace_roots: &[PathBuf],
    default_root: &Path,
) -> Result<PathBuf> {
    if let Some(db) = workspace_db {
        return Ok(db.to_path_buf());
    }
    if let Some(manifest) = workspace_manifest {
        let dir = manifest
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| default_root.to_path_buf());
        return Ok(dir.join(".agentgraph").join("index.db"));
    }
    if let Some(first) = workspace_roots.first() {
        return Ok(first.join(".agentgraph").join("index.db"));
    }
    Ok(default_root.join(".agentgraph").join("index.db"))
}

/// Resolve query `--workspace-root` filters to `root_id`s recorded in the store
/// (fallback: basename id).
pub fn resolve_filter_root_ids(store: &Store, filters: &[PathBuf]) -> Result<Vec<String>> {
    if filters.is_empty() {
        return Ok(Vec::new());
    }
    let recorded = store.workspace_roots_meta()?;
    let mut ids = Vec::new();
    for f in filters {
        let canon = canon_path_str(f).unwrap_or_else(|_| f.to_string_lossy().replace('\\', "/"));
        let mut matched = None;
        for r in &recorded {
            let rcanon = PathBuf::from(&r.path)
                .canonicalize()
                .map(|p| {
                    parser::normalize_root(&p)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .unwrap_or_else(|_| r.path.replace('\\', "/"));
            if rcanon == canon || r.path == f.to_string_lossy() || r.id == f.to_string_lossy() {
                matched = Some(r.id.clone());
                break;
            }
        }
        let id = matched.unwrap_or_else(|| default_root_id(f));
        ids.push(id);
    }
    Ok(ids)
}

/// SQL root filter for a list of root_ids. Empty list → union all (`None`).
/// Single id → `Some(id)`. Multiple ids are handled by the caller as a union
/// query without filter (rows still tagged) unless we later add IN (…).
pub fn single_root_filter(ids: &[String]) -> Option<&str> {
    if ids.len() == 1 {
        Some(ids[0].as_str())
    } else {
        None
    }
}

/// Index every workspace root into one shared store.
pub fn index_workspace(
    roots: &[WorkspaceRoot],
    db_path: &Path,
    force: bool,
    warnings: &[String],
) -> Result<WorkspaceIndexResult> {
    if roots.is_empty() {
        bail!("workspace index requires at least one root");
    }
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Open once to write workspace meta before per-root index (also migrates schema).
    {
        let store = Store::open(db_path)?;
        let meta: Vec<WorkspaceRootInfo> = roots
            .iter()
            .map(|r| WorkspaceRootInfo {
                id: r.id.clone(),
                path: r.path.to_string_lossy().into_owned(),
                files: 0,
                symbols: 0,
                references: 0,
                subset_violations: 0,
                languages: None,
                exact_refs: 0,
                heuristic_refs: 0,
                dynamic_refs: 0,
                index_seq: None,
                missing: false,
                promise_tier: None,
            })
            .collect();
        // Partial re-index must not drop sibling roots from meta.
        // Merge: keep existing recorded roots not in this batch; overlay this batch.
        let existing = store.workspace_roots_meta().unwrap_or_default();
        let mut merged: Vec<WorkspaceRootInfo> = Vec::new();
        for r in &meta {
            let mut slot = r.clone();
            if let Some(prev) = existing.iter().find(|p| p.id == slot.id) {
                // Preserve counts until live overlay below; keep path/id.
                slot.files = prev.files;
                slot.symbols = prev.symbols;
                slot.references = prev.references;
                slot.subset_violations = prev.subset_violations;
                slot.languages = prev.languages.clone();
                slot.exact_refs = prev.exact_refs;
                slot.heuristic_refs = prev.heuristic_refs;
                slot.dynamic_refs = prev.dynamic_refs;
                slot.index_seq = prev.index_seq;
                slot.promise_tier = prev.promise_tier.clone();
            }
            merged.push(slot);
        }
        for prev in &existing {
            if !merged.iter().any(|m| m.id == prev.id) {
                merged.push(prev.clone());
            }
        }
        store.set_workspace_roots_meta(&merged)?;
    }

    let mut per_root: Vec<WorkspaceRootInfo> = Vec::new();
    let mut languages: Vec<String> = Vec::new();

    for root in roots {
        let indexer = Indexer::with_db_path(&root.path, db_path)?;
        {
            let mut store = indexer.open_store()?;
            store.set_write_root(&root.id);
        }
        let stats: IndexStats = indexer.index_as(force, &root.id)?;
        for lang in &stats.languages {
            if !languages.contains(lang) {
                languages.push(lang.clone());
            }
        }
        per_root.push(WorkspaceRootInfo {
            id: root.id.clone(),
            path: root.path.to_string_lossy().into_owned(),
            files: stats
                .by_root
                .iter()
                .find(|b| b.root_id == root.id)
                .map(|b| b.files)
                .unwrap_or(0),
            symbols: stats
                .by_root
                .iter()
                .find(|b| b.root_id == root.id)
                .map(|b| b.symbols)
                .unwrap_or(0),
            references: stats
                .by_root
                .iter()
                .find(|b| b.root_id == root.id)
                .map(|b| b.references)
                .unwrap_or(0),
            subset_violations: stats
                .by_root
                .iter()
                .find(|b| b.root_id == root.id)
                .map(|b| b.subset_violations)
                .unwrap_or(0),
            languages: Some(stats.languages.clone()),
            exact_refs: 0,
            heuristic_refs: 0,
            dynamic_refs: 0,
            index_seq: None,
            missing: false,
            promise_tier: None,
        });
    }

    // Refresh meta with final per-root counts (merge: do not drop sibling roots).
    {
        let store = Store::open(db_path)?;
        let mut merged = store.workspace_roots_meta().unwrap_or_default();
        for r in &per_root {
            if let Some(slot) = merged.iter_mut().find(|m| m.id == r.id) {
                slot.path = r.path.clone();
                slot.files = r.files;
                slot.symbols = r.symbols;
                slot.references = r.references;
                slot.subset_violations = r.subset_violations;
                if r.languages.is_some() {
                    slot.languages = r.languages.clone();
                }
            } else {
                merged.push(r.clone());
            }
        }
        let live = store.root_status_rows()?;
        for live_r in live {
            if let Some(slot) = merged.iter_mut().find(|m| m.id == live_r.id) {
                slot.files = live_r.files;
                slot.symbols = live_r.symbols;
                slot.references = live_r.references;
                slot.subset_violations = live_r.subset_violations;
                slot.exact_refs = live_r.exact_refs;
                slot.heuristic_refs = live_r.heuristic_refs;
                slot.dynamic_refs = live_r.dynamic_refs;
                slot.index_seq = live_r.index_seq;
                slot.missing = live_r.missing;
                slot.promise_tier = live_r.promise_tier;
                if live_r.languages.is_some() {
                    slot.languages = live_r.languages;
                }
            }
        }
        per_root = merged
            .iter()
            .filter(|m| roots.iter().any(|r| r.id == m.id) || m.files > 0 || !m.path.is_empty())
            .cloned()
            .collect();
        store.set_workspace_roots_meta(&per_root)?;
    }

    // Totals from the store (not sum of per-root stats, which are global counts
    // when a prior single-root index shared the DB — recompute from by_root).
    let totals = {
        let store = Store::open(db_path)?;
        store.stats(db_path.to_string_lossy().as_ref())?
    };

    Ok(WorkspaceIndexResult {
        db_path: db_path.to_string_lossy().into_owned(),
        roots: per_root,
        files: totals.files,
        symbols: totals.symbols,
        references: totals.references,
        languages: totals.languages,
        warnings: warnings.to_vec(),
        note: "single SQLite store + root_id; paths are root-relative; \
               queries without --workspace-root union all roots (rows tagged root_id); \
               macro sidecar remains per-root under <root>/.agentgraph/index.macro.db; \
               not a cross-root type merge"
            .to_string(),
    })
}

/// `workspace status` payload.
pub fn workspace_status(db_path: &Path) -> Result<WorkspaceStatus> {
    if !db_path.exists() {
        bail!(
            "workspace store not found at {} — run `agentgraph index --workspace` / `--workspace-root` first",
            db_path.display()
        );
    }
    let store = Store::open(db_path)?;
    store.ensure_indexed()?;
    let roots = store.root_status_rows()?;
    let stats = store.stats(db_path.to_string_lossy().as_ref())?;
    let workspace = store.is_workspace()? || roots.iter().any(|r| !r.id.is_empty());
    let index_seq = store
        .get_meta("index_seq")?
        .and_then(|s| s.parse::<u64>().ok());
    // Union sound promise = weakest selected root (any violation → disabled).
    let (weakest_root, promise_tier) = {
        let mut worst: Option<&crate::model::WorkspaceRootInfo> = None;
        for r in &roots {
            if r.subset_violations > 0 {
                match worst {
                    Some(w) if w.subset_violations >= r.subset_violations => {}
                    _ => worst = Some(r),
                }
            }
        }
        if let Some(w) = worst {
            (
                Some(w.id.clone()),
                Some(
                    crate::index::subset::SoundPromiseTier::Disabled
                        .as_str()
                        .to_string(),
                ),
            )
        } else if !roots.is_empty() {
            let langs: Vec<String> = stats.languages.clone();
            (
                None,
                Some(
                    crate::index::subset::sound_promise_tier(true, &langs)
                        .as_str()
                        .to_string(),
                ),
            )
        } else {
            (None, None)
        }
    };

    // P4: scoped-sound aggregation by root_id (eligible first + recommendation).
    let violations = store.subset_violations()?;
    let agg = crate::index::subset::scoped_sound_by_root(&roots, &violations, &stats.languages);
    let sound_candidates: Vec<serde_json::Value> = agg
        .sound_candidates
        .iter()
        .map(|c| c.to_payload_json())
        .collect();
    let by_root_buckets: Vec<serde_json::Value> =
        agg.buckets.iter().map(|b| b.to_payload_json()).collect();

    // P5: cheap stale honesty flags (never create sidecar / never refresh baseline).
    let baseline_stale = crate::index::diff::baseline_stale_flag(&store);
    let mut sidecar_roots: Vec<PathBuf> = roots
        .iter()
        .filter(|r| !r.path.is_empty())
        .map(|r| PathBuf::from(&r.path))
        .collect();
    if sidecar_roots.is_empty() {
        // Classic single-root store sitting in a workspace-status call: check
        // `<db_parent_parent>/.agentgraph` heuristic via db path parent chain.
        if let Some(parent) = db_path.parent().and_then(|p| p.parent()) {
            sidecar_roots.push(parent.to_path_buf());
        }
    }
    let (sidecar_exists, sidecar_stale) = crate::index::cheap_sidecar_flags_multi(&sidecar_roots);

    Ok(WorkspaceStatus {
        db_path: db_path.to_string_lossy().into_owned(),
        workspace,
        roots,
        files: stats.files,
        symbols: stats.symbols,
        references: stats.references,
        note: "per-root counts from root_id column; empty root_id = legacy single-root rows; \
               --sound + workspace: subset_ok is per selected root (union = weakest root); \
               macro sidecar is per-root at <root>/.agentgraph/index.macro.db; \
               sound_candidates list scoped --sound roots eligible first; \
               baseline_stale / sidecar_* are honesty flags (no auto-refresh)"
            .to_string(),
        index_seq,
        promise_tier,
        weakest_root,
        sound_candidates,
        recommendation: Some(agg.recommendation.clone()),
        by_root: by_root_buckets,
        baseline_stale,
        sidecar_exists,
        sidecar_stale,
    })
}
