use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::index::{llm, macro_map, union_callers, union_impact, Indexer, UnionOptions};
use crate::model::ConfidenceFilter;
use crate::query::{parse_query_flags, Query};
use crate::viz::{
    add_macro_caller_rows, add_macro_impact_rows, build_callers_graph, build_impact_graph,
    merge_graphs, render_graph_html, GraphDirection, GraphFlags,
};

#[derive(Parser, Debug)]
#[command(
    name = "agentgraph",
    version,
    about = "Agent-native code understanding: symbol graph, call graph, impact analysis"
)]
pub struct Cli {
    /// Project root to index (defaults to cwd)
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    /// Workspace manifest JSON: `{ "roots": [{"id","path"},…] }` or array of paths.
    /// Shared store defaults to `<manifest_dir>/.agentgraph/index.db`.
    #[arg(long, global = true)]
    pub workspace: Option<PathBuf>,
    /// Workspace project root (repeatable). On `index`: roots to ingest.
    /// On queries: filter rows to that `root_id`.
    #[arg(long, global = true)]
    pub workspace_root: Vec<PathBuf>,
    /// Explicit shared workspace SQLite path (overrides manifest/first-root defaults).
    #[arg(long, global = true)]
    pub workspace_db: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

/// `agentgraph workspace …` (Track M4-W multi-root status).
#[derive(Subcommand, Debug)]
pub enum WorkspaceCmd {
    /// Print per-root workspace index status (root_id, counts, subset violations)
    Status,
}

/// Optional P2 macro-expanded sidecar (CLI default OFF). Track M1: path map,
/// de-dup, fingerprint/stale, rebuild.
#[derive(Subcommand, Debug)]
pub enum MacroCmd {
    /// Print sidecar status (path, counts, stale, path_map, dedup_stats, …)
    Status,
    /// Re-index the recorded expanded_root into the sidecar (idempotent)
    Rebuild {
        /// Force full re-parse of the expanded tree (default true for rebuild)
        #[arg(long, default_value_t = true)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Build or refresh the local index (incremental by content hash).
    /// Multi-root: `index --workspace <manifest.json>` or repeated `--workspace-root <dir>`.
    Index {
        /// Re-parse every file even if unchanged
        #[arg(long)]
        force: bool,
        /// Optional: also index an expanded shadow tree into sidecar
        /// `.agentgraph/index.macro.db` (does not replace the main index)
        #[arg(long)]
        macro_expanded_root: Option<PathBuf>,
    },
    /// Workspace multi-root commands (single SQLite store + root_id)
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCmd,
    },
    /// Show index statistics
    Stats,
    /// Find symbol definitions by exact name (falls back to fuzzy on no match)
    Find {
        name: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Force fuzzy (substring) search; default is exact-first with automatic fuzzy fallback
        #[arg(long, default_value_t = false)]
        fuzzy: bool,
    },
    /// List call sites / references of a symbol
    Callers {
        name: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Only Exact (L0) edges; exclude Heuristic/DynamicCandidate.
        /// Combined with --with-macro: sidecar is **ignored** (exact-only semantics).
        #[arg(long, default_value_t = false)]
        exact_only: bool,
        /// Also include DynamicCandidate edges (higher noise)
        #[arg(long, default_value_t = false)]
        include_dynamic: bool,
        /// Prefer recall: same as --include-dynamic (use when you fear missed edges)
        #[arg(long, default_value_t = false)]
        recall: bool,
        /// L2: only sound-eligible edges + S-violation report
        #[arg(long, default_value_t = false)]
        sound: bool,
        /// Union optional macro-expanded sidecar hits (tagged origin=macro_expanded,
        /// paths mapped back to source when possible). Default OFF; absent sidecar
        /// is treated as empty. De-dup ON by default (see --no-macro-dedup).
        /// Not sound — see docs/macro-sidecar.md. `--limit` applies per store;
        /// without de-dup the union may return up to ~2N rows.
        #[arg(long, default_value_t = false)]
        with_macro: bool,
        /// Debug: keep duplicate sidecar rows under --with-macro (default de-dup ON)
        #[arg(long, default_value_t = false)]
        no_macro_dedup: bool,
        /// Noise governance: merge implementor edges into the callers array
        /// (old noisy default). Mutually exclusive with --implementors-only.
        /// Default (without this flag) separates `{callers, implementors}` when
        /// any implementor is present; plain array when none.
        #[arg(long, default_value_t = false)]
        include_implementors: bool,
        /// Noise governance: return only implementor/registration-implementor
        /// edges (trait/interface impls). Mutually exclusive with --include-implementors.
        #[arg(long, default_value_t = false)]
        implementors_only: bool,
    },
    /// High-level blast-radius recipe: auto sound vs default window + honesty payload
    ///
    /// Picks `impact --sound` when the selected store/root is `subset_ok`;
    /// otherwise default Exact+Heuristic impact (never blind `--recall`).
    /// Response always includes `window`, `subset_ok`, `promise_tier`,
    /// `recommendation`, and `note` (not a complete runtime graph).
    BlastRadius {
        /// Query symbol name
        name: String,
        /// BFS depth (recipe default 3)
        #[arg(long, default_value_t = 3)]
        depth: usize,
        /// Cap impact nodes
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Include macro sidecar only when safe (exists && !stale && !nested)
        #[arg(long, default_value_t = false)]
        include_macro: bool,
    },
    /// High-level who-calls recipe: implementors separated by default; --noisy merges
    ///
    /// Default (noisy=false) reuses the store callers payload builder:
    /// implementors are separated/collapsed + high-freq names demoted.
    /// `--noisy` restores the old merged shape. Response carries `callers`,
    /// `implementors`, `high_freq_name`, `promise_tier`, `recommendation`.
    WhoCalls {
        /// Query symbol name
        name: String,
        /// Cap callers / implementors sections
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Old noisy shape: merge implementors into callers
        #[arg(long, default_value_t = false)]
        noisy: bool,
    },
    /// Blast radius: who transitively depends on this symbol
    Impact {
        name: String,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Only Exact (L0) edges; exclude Heuristic/DynamicCandidate.
        /// Combined with --with-macro: sidecar is **ignored** (exact-only semantics).
        #[arg(long, default_value_t = false)]
        exact_only: bool,
        /// Also include DynamicCandidate edges (higher noise)
        #[arg(long, default_value_t = false)]
        include_dynamic: bool,
        /// Prefer recall: same as --include-dynamic (use when you fear missed edges)
        #[arg(long, default_value_t = false)]
        recall: bool,
        /// L2: walk only sound-eligible edges; report S-violations; never claims sound outside S
        #[arg(long, default_value_t = false)]
        sound: bool,
        /// Union optional macro-expanded sidecar hits (tagged origin=macro_expanded,
        /// paths mapped when possible). Default OFF; absent sidecar is empty.
        /// De-dup ON by default (see --no-macro-dedup). Not sound.
        /// `--limit` applies per store; without de-dup union may return ~2N rows.
        #[arg(long, default_value_t = false)]
        with_macro: bool,
        /// Debug: keep duplicate sidecar rows under --with-macro (default de-dup ON)
        #[arg(long, default_value_t = false)]
        no_macro_dedup: bool,
    },
    /// Files most related to a symbol (for retrieval scoping)
    Related {
        name: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Files that import a given file (module-level edges)
    Importers {
        /// Repo-relative file path, e.g. src/auth.ts
        path: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Optional LLM pass: one-line responsibility descriptions for symbols
    Enrich {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Poll and reindex when source files change
    Watch {
        #[arg(long, default_value_t = 5)]
        interval: u64,
    },
    /// Scan the index for language-subset S violations (L2)
    ///
    /// Always includes `sound_candidates` + `recommendation` (scoped --sound).
    /// Workspace stores always include `by_root` buckets; single-root trees
    /// include `by_top_dir`. `--by-root` forces root buckets when needed.
    Subset {
        /// Force/include `by_root` aggregation (always on for workspace stores).
        #[arg(long, default_value_t = false)]
        by_root: bool,
    },
    /// Compare indexed edge set against the snapshot baseline written at `index` time.
    ///
    /// Honesty: indexed edges only (name+path+line+confidence+enclosing);
    /// **not** a runtime call-graph diff. No baseline → fail-loud (run `index` first).
    Diff {
        /// Only Exact (L0) edges participate in the set difference
        #[arg(long, default_value_t = false)]
        exact_only: bool,
        /// Cap rows returned per side (summary counters stay full)
        #[arg(long)]
        limit: Option<usize>,
        /// Explicit snapshot JSON (default: <root>/.agentgraph/refs.snapshot[.prev].json)
        #[arg(long)]
        snapshot: Option<PathBuf>,
        /// After printing the diff, promote current live refs as the new baseline
        #[arg(long, default_value_t = false)]
        write_snapshot: bool,
    },
    /// Export index as SCIP protobuf binary (official scip CLI) or LSIF JSONL
    Export {
        #[arg(value_parser = ["scip", "scip-json", "lsif"])]
        format: String,
        /// Output file path
        #[arg(short, long)]
        out: PathBuf,
        /// Only export Exact edges (drop Heuristic/DynamicCandidate)
        #[arg(long, default_value_t = false)]
        exact_only: bool,
        /// Also export DynamicCandidate edges (default excludes them)
        #[arg(long, default_value_t = false)]
        include_dynamic: bool,
        /// Prefer recall: same as --include-dynamic (怕漏时导出更宽窗口)
        #[arg(long, default_value_t = false)]
        recall: bool,
    },
    /// Run as an MCP server over stdio
    Mcp,
    /// Measure callers/impact latency percentiles in-process (PLAN query SLO)
    BenchQuery {
        #[arg(long, default_value_t = 200)]
        samples: usize,
        /// Symbol name prefix used to synthesize query targets (default: helper)
        #[arg(long, default_value = "helper")]
        prefix: String,
        /// Also include this exact hot symbol (high fan-in), e.g. run
        #[arg(long)]
        hot: Option<String>,
        /// Skip query cache (cold path) — measures uncached SQL
        #[arg(long, default_value_t = false)]
        cold: bool,
    },
    /// Optional macro-expanded sidecar commands (CLI default OFF)
    Macro {
        #[command(subcommand)]
        command: MacroCmd,
    },
    /// Render a local self-contained HTML code-graph around a symbol
    ///
    /// Primary view is impact-style BFS (outgoing blast radius). Open the
    /// written HTML in a browser — no network required. Shows indexed L0/L1
    /// candidates, not a complete runtime graph.
    Graph {
        /// Query symbol name (center of the neighborhood)
        name: String,
        /// BFS depth for impact (ignored for pure callers view)
        #[arg(long, default_value_t = 2)]
        depth: usize,
        /// Output HTML path (default: <root>/.agentgraph/graph.html)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Neighborhood direction
        #[arg(long, value_enum, default_value_t = GraphDirArg::Impact)]
        direction: GraphDirArg,
        /// Alias for `--direction impact` (blast radius)
        #[arg(long, default_value_t = false)]
        impact: bool,
        /// Only Exact (L0) edges; exclude Heuristic/DynamicCandidate
        #[arg(long, default_value_t = false)]
        exact_only: bool,
        /// Also include DynamicCandidate edges (higher noise)
        #[arg(long, default_value_t = false)]
        include_dynamic: bool,
        /// Union optional macro-expanded sidecar hits (MACRO badge + mapped path)
        #[arg(long, default_value_t = false)]
        with_macro: bool,
        /// Debug: keep duplicate sidecar rows under --with-macro (default de-dup ON)
        #[arg(long, default_value_t = false)]
        no_macro_dedup: bool,
        /// L2: render only sound-eligible edges; header shows subset_ok + promise_tier.
        /// Mutually exclusive with --with-macro (and with --exact-only / --include-dynamic).
        /// When subset_ok=false the HTML is still written but is **not** labeled a
        /// sound graph (disabled honesty UX); process exits non-zero.
        #[arg(long, default_value_t = false)]
        sound: bool,
    },
}

/// CLI enum for `graph --direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GraphDirArg {
    /// Outgoing blast radius (default)
    Impact,
    /// Direct callers / reference sites
    Callers,
    /// Union of impact + callers
    Both,
}

impl From<GraphDirArg> for GraphDirection {
    fn from(v: GraphDirArg) -> Self {
        match v {
            GraphDirArg::Impact => GraphDirection::Impact,
            GraphDirArg::Callers => GraphDirection::Callers,
            GraphDirArg::Both => GraphDirection::Both,
        }
    }
}

/// Build main callers rows as JSON (`at=path:line` + `edge_role`).
fn main_callers_json(hits: &[crate::model::ReferenceRecord]) -> Vec<serde_json::Value> {
    hits.iter().map(|r| r.to_query_json()).collect()
}

/// Build main impact rows as JSON (`at=path:line`).
fn main_impact_json(hits: &[crate::model::ImpactNode]) -> Vec<serde_json::Value> {
    hits.iter().map(|n| n.to_query_json()).collect()
}

/// Workspace store? (`meta.workspace=1` or any non-empty root_id).
fn store_is_workspace(store: &crate::index::store::Store) -> bool {
    if store.is_workspace().unwrap_or(false) {
        return true;
    }
    store
        .workspace_roots_meta()
        .map(|r| r.iter().any(|x| !x.id.is_empty()))
        .unwrap_or(false)
}

/// Cheap P5 honesty flags for a store root (and optional workspace roots).
/// Never creates a macro sidecar; never refreshes the diff baseline.
fn attach_stale_flags(
    payload: &mut serde_json::Value,
    store: &crate::index::store::Store,
    classic_root: &std::path::Path,
) -> Result<()> {
    let baseline_stale = crate::index::diff::baseline_stale_flag(store);
    let mut roots: Vec<PathBuf> = store
        .workspace_roots_meta()?
        .into_iter()
        .filter(|r| !r.path.is_empty())
        .map(|r| PathBuf::from(r.path))
        .collect();
    if roots.is_empty() {
        roots.push(classic_root.to_path_buf());
    }
    let (sidecar_exists, sidecar_stale) = crate::index::cheap_sidecar_flags_multi(&roots);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("baseline_stale".into(), serde_json::json!(baseline_stale));
        obj.insert("sidecar_exists".into(), serde_json::json!(sidecar_exists));
        obj.insert("sidecar_stale".into(), serde_json::json!(sidecar_stale));
    }
    Ok(())
}

/// stderr one-liner when stale honesty flags are set (P5).
fn warn_stale_flags(baseline_stale: bool, sidecar_stale: bool, context: &str) {
    if baseline_stale {
        eprintln!(
            "warn: baseline_stale=true ({context}) — dirty reindex after last full snapshot; \
             run `agentgraph index` to refresh baseline or `diff --write-snapshot` to lock current"
        );
    }
    if sidecar_stale {
        eprintln!(
            "warn: sidecar_stale=true ({context}) — main source fingerprint changed since \
             sidecar build; run `agentgraph macro rebuild` if you use --with-macro"
        );
    }
}

/// Build scoped-sound aggregation for `subset` payloads (P4).
fn subset_sound_aggregation(
    store: &crate::index::store::Store,
    root_filter: Option<&str>,
    languages: &[String],
    violations: &[crate::index::subset::SubsetViolation],
    force_by_root: bool,
) -> crate::index::subset::SoundAggregation {
    let roots = store.root_status_rows().unwrap_or_default();
    let is_workspace = store_is_workspace(store) || roots.iter().any(|r| !r.id.is_empty());
    if is_workspace || force_by_root {
        // When a single root filter is active, still aggregate all roots so
        // Agents see the full candidate list; filter only the violation list.
        return crate::index::subset::scoped_sound_by_root(&roots, violations, languages);
    }
    // Single-root tree: aggregate by top-level path segment.
    let mut keys: Vec<(String, Option<String>)> = Vec::new();
    if let Ok(dirs) = store.distinct_file_top_dirs(root_filter) {
        for d in dirs {
            keys.push((d, None));
        }
    }
    // Include top dirs that only appear on violations (oversized / parse_error).
    for v in violations {
        let d = crate::index::subset::top_dir_of_path(&v.path);
        let key = if d.is_empty() {
            "(root)".to_string()
        } else {
            d
        };
        if !keys.iter().any(|(k, _)| *k == key) {
            keys.push((key, None));
        }
    }
    crate::index::subset::scoped_sound_by_top_dir(&keys, violations, languages)
}

/// Tag query JSON rows with `root_id` / `root_path` when the store is workspace.
fn print_query_json(value: &serde_json::Value, store: &crate::index::store::Store) -> Result<()> {
    let mut v = value.clone();
    let workspace = store_is_workspace(store);
    crate::model::ensure_workspace_root_ids(&mut v, workspace);
    if workspace {
        if let Ok(roots) = store.workspace_roots_meta() {
            crate::model::inject_root_paths(&mut v, &roots);
        }
    }
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

/// Extract sidecar union rows from a with-macro payload (array or wrapped object).
#[allow(dead_code)]
pub fn union_rows_payload(v: &serde_json::Value) -> Option<&serde_json::Value> {
    if v.is_array() {
        Some(v)
    } else {
        v.get("callers").or_else(|| v.get("impact"))
    }
}

/// Macro sidecar is **per-root**. Workspace multi-root + `--with-macro` without
/// a single-root filter is rejected (clear error; sidecars live under each
/// `<root>/.agentgraph/index.macro.db`).
fn guard_macro_workspace(
    store: &crate::index::store::Store,
    with_macro: bool,
    root_filter: Option<&str>,
) -> Result<()> {
    if !with_macro {
        return Ok(());
    }
    if !store_is_workspace(store) {
        return Ok(());
    }
    let roots = store.workspace_roots_meta().unwrap_or_default();
    let multi = roots.iter().filter(|r| !r.id.is_empty()).count() > 1;
    if multi && root_filter.is_none() {
        bail!(
            "--with-macro + workspace multi-root requires a single --workspace-root filter \
             (macro sidecar is per-root at <root>/.agentgraph/index.macro.db; \
             union across roots has no shared sidecar — see docs/macro-sidecar.md)"
        );
    }
    Ok(())
}

/// Resolve which store the CLI should open for this invocation.
fn resolve_cli_db(
    root: &std::path::Path,
    workspace: Option<&PathBuf>,
    workspace_db: Option<&PathBuf>,
    workspace_root: &[PathBuf],
) -> Result<PathBuf> {
    crate::index::workspace::resolve_db_path(
        workspace.map(|p| p.as_path()),
        workspace_db.map(|p| p.as_path()),
        workspace_root,
        root,
    )
}

/// Root_id filter for queries (empty vec → union all roots, rows still tagged).
fn resolve_query_root_filter(
    store: &crate::index::store::Store,
    workspace_root: &[PathBuf],
) -> Result<Option<String>> {
    if workspace_root.is_empty() {
        return Ok(None);
    }
    let ids = crate::index::workspace::resolve_filter_root_ids(store, workspace_root)?;
    if ids.is_empty() {
        return Ok(None);
    }
    if ids.len() == 1 {
        return Ok(Some(ids.into_iter().next().unwrap()));
    }
    // Multiple roots selected → union (no SQL filter); rows carry root_id.
    Ok(None)
}

pub fn run(cli: Cli) -> Result<()> {
    let root = match cli.root {
        Some(r) => r,
        None => std::env::current_dir()?,
    };
    let workspace_flag = cli.workspace.clone();
    let workspace_roots = cli.workspace_root.clone();
    let workspace_db_flag = cli.workspace_db.clone();
    let workspace_mode =
        workspace_flag.is_some() || workspace_db_flag.is_some() || !workspace_roots.is_empty();

    // Workspace index/status/queries open the **shared** store, not --root/.agentgraph
    // (unless --root is the only handle and no workspace flags are set).
    let db_path = resolve_cli_db(
        &root,
        workspace_flag.as_ref(),
        workspace_db_flag.as_ref(),
        &workspace_roots,
    )?;
    let indexer = {
        let mut ix = Indexer::new(&root)?;
        if workspace_mode {
            ix.db_path = db_path.clone();
        }
        ix
    };

    match cli.command {
        Commands::Workspace { command } => match command {
            WorkspaceCmd::Status => {
                let db = if workspace_mode {
                    db_path.clone()
                } else {
                    indexer.db_path.clone()
                };
                let status = crate::index::workspace::workspace_status(&db)?;
                warn_stale_flags(
                    status.baseline_stale,
                    status.sidecar_stale,
                    "workspace status",
                );
                println!("{}", serde_json::to_string_pretty(&status)?);
            }
        },
        Commands::Index {
            force,
            macro_expanded_root,
        } => {
            // Multi-root workspace index path (M4-W).
            // Trigger: --workspace manifest and/or any --workspace-root.
            if workspace_flag.is_some() || !workspace_roots.is_empty() {
                if macro_expanded_root.is_some() {
                    bail!(
                        "index --workspace / --workspace-root cannot be combined with --macro-expanded-root                          (macro sidecar is per-root; index each root separately for sidecars)"
                    );
                }
                let roots = if let Some(manifest) = workspace_flag.as_ref() {
                    crate::index::workspace::parse_manifest(manifest)?
                } else {
                    crate::index::workspace::roots_from_dirs(&workspace_roots)?
                };
                let (roots, warnings) =
                    crate::index::workspace::finalize_roots_with_warnings(roots)?;
                for w in &warnings {
                    eprintln!("warn: {w}");
                }
                let result =
                    crate::index::workspace::index_workspace(&roots, &db_path, force, &warnings)?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                return Ok(());
            }
            if let Some(exp) = macro_expanded_root {
                // Validate nesting BEFORE main index — a nested expanded tree would
                // otherwise be ingested by the main walker in the same command.
                indexer.validate_macro_expanded_root(&exp)?;
                // Main index first (source L0/L1 stays the default product).
                let main = indexer.index(force)?;
                let side = indexer.index_macro_expanded(&exp, force)?;
                let payload = serde_json::json!({
                    "main": main,
                    "macro_sidecar": side,
                    "note": "sidecar is optional dual-index candidates (not sound); \
                             callers/impact ignore it unless --with-macro; \
                             de-dup ON by default; origin=macro_expanded",
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                let stats = indexer.index(force)?;
                println!("{}", serde_json::to_string_pretty(&stats)?);
            }
        }
        Commands::Stats => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let mut stats = store.stats(&indexer.root.to_string_lossy())?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            if let Some(rid) = &root_filter {
                if let Some(br) = stats.by_root.iter().find(|b| &b.root_id == rid) {
                    stats.files = br.files;
                    stats.symbols = br.symbols;
                    stats.references = br.references;
                }
                stats.root = format!("{}#{}", stats.root, rid);
            }
            // P5: cheap stale honesty flags (no sidecar create / no baseline write).
            stats.baseline_stale = crate::index::diff::baseline_stale_flag(&store);
            let mut sidecar_roots: Vec<PathBuf> = store
                .workspace_roots_meta()?
                .into_iter()
                .filter(|r| !r.path.is_empty())
                .map(|r| PathBuf::from(r.path))
                .collect();
            if sidecar_roots.is_empty() {
                sidecar_roots.push(indexer.root.clone());
            }
            let (se, ss) = crate::index::cheap_sidecar_flags_multi(&sidecar_roots);
            stats.sidecar_exists = se;
            stats.sidecar_stale = ss;
            warn_stale_flags(stats.baseline_stale, stats.sidecar_stale, "stats");
            println!("{}", serde_json::to_string_pretty(&stats)?);
        }
        Commands::Find { name, limit, fuzzy } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let rf = root_filter.as_deref();
            let hits = if fuzzy {
                store.find_symbol_fuzzy_in(&name, limit, rf)?
            } else {
                let exact = store.find_symbol_exact_in(&name, limit, rf)?;
                if exact.is_empty() {
                    let fuzzy_hits = store.find_symbol_fuzzy_in(&name, limit, rf)?;
                    if !fuzzy_hits.is_empty() {
                        eprintln!(
                            "note: no exact match for '{name}'; showing fuzzy results (pass --fuzzy to skip exact)"
                        );
                    }
                    fuzzy_hits
                } else {
                    exact
                }
            };
            if hits.is_empty() {
                bail!("no symbol matching '{name}' — run `agentgraph index` first?");
            }
            print_query_json(&serde_json::json!(hits), &store)?;
        }
        Commands::Callers {
            name,
            limit,
            exact_only,
            include_dynamic,
            recall,
            sound,
            with_macro,
            no_macro_dedup,
            include_implementors,
            implementors_only,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let rf = root_filter.as_deref();
            let role_mode = crate::query::parse_callers_role_mode(
                exact_only,
                include_implementors,
                implementors_only,
            )?;
            if sound && with_macro {
                bail!(
                    "--sound is mutually exclusive with --with-macro \
                     (macro sidecar is not sound-certified; no subset_ok claim for expanded-only rows)"
                );
            }
            guard_macro_workspace(&store, with_macro, rf)?;
            if sound {
                if exact_only || include_dynamic || recall {
                    bail!(
                        "--sound is mutually exclusive with --exact-only / --include-dynamic / --recall \
                         (sound walk uses its own eligibility filter)"
                    );
                }
                if include_implementors || implementors_only {
                    bail!(
                        "--sound is mutually exclusive with --include-implementors / --implementors-only \
                         (sound walk uses its own eligibility filter; roles are tagged on impact/HTML)"
                    );
                }
                let (hits, violations) = store.callers_sound_in(&name, limit, rf)?;
                let subset_ok = violations.is_empty();
                let languages = store.stats(&indexer.root.to_string_lossy())?.languages;
                let (promise_tier, promise) =
                    crate::index::subset::select_sound_promise(subset_ok, &languages);
                let mapped: Vec<serde_json::Value> =
                    hits.iter().map(|r| r.to_query_json()).collect();
                // S-qualified modeled edges; disabled when S violated.
                // Promise tier is language-aware (AST vs lexical v1 vs mixed).
                let payload = serde_json::json!({
                    "mode": "sound",
                    "subset_ok": subset_ok,
                    "promise_tier": promise_tier.as_str(),
                    "promise": promise,
                    "promise_languages": languages,
                    "subset_violations": violations,
                    "callers": mapped,
                });
                print_query_json(&payload, &store)?;
                return Ok(());
            }
            let filter = parse_query_flags(exact_only, include_dynamic, recall);
            // Widened fetch so role partition does not starve Exact calls.
            let hits = store.callers_for_roles(&name, limit, filter, rf)?;
            let role_payload =
                crate::query::build_callers_payload(&name, hits.clone(), limit, role_mode);
            let main_rows = main_callers_json(&hits);

            // M1: --exact-only --with-macro → ignore sidecar (spec §1.4).
            let ignore_sidecar = exact_only;
            if with_macro && ignore_sidecar {
                print_query_json(&role_payload, &store)?;
                return Ok(());
            }
            if with_macro {
                let opts = UnionOptions {
                    dedup: !no_macro_dedup,
                    ignore_sidecar: false,
                };
                let (layout, _map, path_map_present) = indexer.macro_crate_layout()?;
                let side = indexer.open_macro_store()?;
                if let Some(side) = side {
                    let status = indexer.macro_status()?;
                    if status.stale {
                        eprintln!(
                            "warn: macro sidecar is stale \
                             (main source fingerprint changed since sidecar build); \
                             unioning existing rows — run `agentgraph macro rebuild`"
                        );
                    }
                    warn_stale_flags(status.baseline_stale, status.stale, "callers --with-macro");
                    let side_hits = side.callers_filtered(&name, limit, filter)?;
                    let expanded_root = status
                        .expanded_root
                        .clone()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| indexer.root.clone());
                    // Union against role-tagged main rows (edge_role + at).
                    let (rows, stats) = union_callers(
                        main_rows,
                        &side_hits,
                        &expanded_root,
                        &indexer.root,
                        &layout,
                        opts,
                    );
                    let _ = indexer.store_dedup_stats(&stats);
                    let payload = serde_json::json!({
                        "callers": rows,
                        "sidecar_present": true,
                        "stale": status.stale,
                        "origin": "macro_expanded",
                        "path_map_present": path_map_present,
                        "dedup_stats": stats,
                        "note": "sidecar union is optional candidates (not sound); de-dup ON unless --no-macro-dedup; main rows carry edge_role",
                    });
                    print_query_json(&payload, &store)?;
                    return Ok(());
                }
                // Absent sidecar → fall through to role payload.
            }
            print_query_json(&role_payload, &store)?;
            let _ = main_rows; // used above when with_macro sidecar present
        }
        Commands::BlastRadius {
            name,
            depth,
            limit,
            include_macro,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let args = crate::query::recipes::BlastRadiusArgs {
                symbol: name,
                depth,
                limit,
                include_macro,
                root_id: root_filter.clone(),
            };
            let payload = crate::query::recipes::run_blast_radius(
                &store,
                &indexer,
                &indexer.root.to_string_lossy(),
                &args,
            )?;
            print_query_json(&payload, &store)?;
        }
        Commands::WhoCalls { name, limit, noisy } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let args = crate::query::recipes::WhoCallsArgs {
                symbol: name,
                noisy,
                limit,
                root_id: root_filter.clone(),
            };
            let payload = crate::query::recipes::run_who_calls(
                &store,
                &indexer.root.to_string_lossy(),
                &args,
            )?;
            print_query_json(&payload, &store)?;
        }
        Commands::Impact {
            name,
            depth,
            limit,
            exact_only,
            include_dynamic,
            recall,
            sound,
            with_macro,
            no_macro_dedup,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let rf = root_filter.as_deref();
            if sound && with_macro {
                bail!(
                    "--sound is mutually exclusive with --with-macro \
                     (macro sidecar is not sound-certified; no subset_ok claim for expanded-only rows)"
                );
            }
            guard_macro_workspace(&store, with_macro, rf)?;
            if sound {
                if exact_only || include_dynamic || recall {
                    bail!(
                        "--sound is mutually exclusive with --exact-only / --include-dynamic / --recall"
                    );
                }
                let (hits, violations) = store.impact_sound_in(&name, depth, limit, rf)?;
                let subset_ok = violations.is_empty();
                let languages = store.stats(&indexer.root.to_string_lossy())?.languages;
                let (promise_tier, promise) =
                    crate::index::subset::select_sound_promise(subset_ok, &languages);
                let mapped: Vec<serde_json::Value> =
                    hits.iter().map(|n| n.to_query_json()).collect();
                let payload = serde_json::json!({
                    "mode": "sound",
                    "subset_ok": subset_ok,
                    "promise_tier": promise_tier.as_str(),
                    "promise": promise,
                    "promise_languages": languages,
                    "subset_violations": violations,
                    "impact": mapped,
                });
                print_query_json(&payload, &store)?;
                return Ok(());
            }
            let filter = parse_query_flags(exact_only, include_dynamic, recall);
            let hits = store.impact_filtered_in(&name, depth, limit, filter, rf)?;

            let ignore_sidecar = exact_only;
            if with_macro && ignore_sidecar {
                print_query_json(&serde_json::json!(main_impact_json(&hits)), &store)?;
                return Ok(());
            }
            if with_macro {
                let main_rows = main_impact_json(&hits);
                let opts = UnionOptions {
                    dedup: !no_macro_dedup,
                    ignore_sidecar: false,
                };
                let (layout, _map, path_map_present) = indexer.macro_crate_layout()?;
                if let Some(side) = indexer.open_macro_store()? {
                    let status = indexer.macro_status()?;
                    if status.stale {
                        eprintln!(
                            "warn: macro sidecar is stale \
                             (main source fingerprint changed since sidecar build); \
                             unioning existing rows — run `agentgraph macro rebuild`"
                        );
                    }
                    warn_stale_flags(status.baseline_stale, status.stale, "impact --with-macro");
                    let side_hits = side.impact_filtered(&name, depth, limit, filter)?;
                    let expanded_root = status
                        .expanded_root
                        .clone()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| indexer.root.clone());
                    let (rows, stats) = union_impact(
                        main_rows,
                        &side_hits,
                        &expanded_root,
                        &indexer.root,
                        &layout,
                        opts,
                    );
                    let _ = indexer.store_dedup_stats(&stats);
                    let payload = serde_json::json!({
                        "impact": rows,
                        "sidecar_present": true,
                        "stale": status.stale,
                        "origin": "macro_expanded",
                        "path_map_present": path_map_present,
                        "dedup_stats": stats,
                        "note": "sidecar union is optional candidates (not sound); de-dup ON unless --no-macro-dedup",
                    });
                    print_query_json(&payload, &store)?;
                } else {
                    print_query_json(&serde_json::json!(main_rows), &store)?;
                }
            } else {
                print_query_json(&serde_json::json!(main_impact_json(&hits)), &store)?;
            }
        }
        Commands::Related { name, limit } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let q = Query::new(&store);
            let hits = q.related_files(&name, limit)?;
            let mapped: Vec<serde_json::Value> = hits
                .into_iter()
                .map(|(path, score, reason)| {
                    serde_json::json!({"path": path, "score": score, "reason": reason})
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&mapped)?);
        }
        Commands::Importers { path, limit } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            // Accept abs paths under root, `./rel`, and Windows backslashes.
            let lookup = crate::index::parser::rel_path_under_root(
                std::path::Path::new(&path),
                &indexer.root,
            )
            .unwrap_or_else(|| path.replace('\\', "/"));
            let hits = store.importers_of_file(&lookup, limit)?;
            println!("{}", serde_json::to_string_pretty(&hits)?);
        }
        Commands::Enrich { limit } => {
            let cfg = llm::LlmConfig::from_env()?;
            let mut store = indexer.open_store()?;
            let report = llm::enrich(&indexer.root, &mut store, &cfg, limit)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::Watch { interval } => {
            // Prefer fsnotify; fall back to polling if watcher setup fails.
            let debounce = std::time::Duration::from_millis(interval.saturating_mul(50).max(50));
            match indexer.watch_events(debounce) {
                Ok((rx, _handle)) => {
                    eprintln!(
                        "fsnotify watching {} (debounce {debounce:?}; Ctrl+C to stop)",
                        indexer.root.display()
                    );
                    for stats in rx {
                        eprintln!(
                            "reindexed: {} files / {} symbols / {} refs",
                            stats.files, stats.symbols, stats.references
                        );
                    }
                }
                Err(e) => {
                    eprintln!("fsnotify unavailable ({e:#}); falling back to poll");
                    indexer.watch(interval)?;
                }
            }
        }
        Commands::Export {
            format,
            out,
            exact_only,
            include_dynamic,
            recall,
        } => {
            let mut store = indexer.open_store()?;
            store.ensure_indexed()?;
            // perf-plan P0-4: never export stale sid links.
            store.ensure_sids_for_export()?;
            let filter = parse_query_flags(exact_only, include_dynamic, recall);
            match format.as_str() {
                "scip" => {
                    crate::index::export::export_scip_filtered(&store, &indexer.root, &out, filter)?
                }
                "scip-json" => crate::index::export::export_scip_json_filtered(
                    &store,
                    &indexer.root,
                    &out,
                    filter,
                )?,
                "lsif" => {
                    crate::index::export::export_lsif_filtered(&store, &indexer.root, &out, filter)?
                }
                other => bail!("unknown export format: {other}"),
            }
            println!("wrote {format} → {}", out.display());
        }
        Commands::Subset { by_root } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let rf = root_filter.as_deref();
            let violations = store.subset_violations_in(rf)?;
            let languages = store.stats(&indexer.root.to_string_lossy())?.languages;
            let (promise_tier, promise) =
                crate::index::subset::select_sound_promise(violations.is_empty(), &languages);
            // P4: scoped-sound aggregation (workspace → by_root; single-root → by_top_dir).
            let agg = subset_sound_aggregation(&store, rf, &languages, &violations, by_root);
            let agg_payload = agg.to_payload_json();
            // Legacy per-root summary kept for older consumers; enriched buckets
            // live under by_root / sound_candidates.
            let per_root: Vec<serde_json::Value> = store
                .root_status_rows()?
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "root_id": r.id,
                        "path": r.path,
                        "subset_ok": r.subset_violations == 0,
                        "subset_violations": r.subset_violations,
                        "promise_tier": r.promise_tier,
                    })
                })
                .collect();
            let mut payload = serde_json::json!({
                "in_subset": violations.is_empty(),
                "violation_count": violations.len(),
                "violations": violations,
                "promise_tier": promise_tier.as_str(),
                "promise": promise,
                "promise_languages": languages,
                "root_id": root_filter,
                "by_root": if per_root.is_empty() && !agg_payload.get("by_root").map(|v| v.is_array()).unwrap_or(false) {
                    serde_json::Value::Null
                } else {
                    agg_payload
                        .get("by_root")
                        .cloned()
                        .filter(|v| v.is_array() && !v.as_array().map(|a| a.is_empty()).unwrap_or(true))
                        .unwrap_or_else(|| serde_json::json!(per_root))
                },
                "by_top_dir": agg_payload.get("by_top_dir").cloned().unwrap_or(serde_json::Value::Null),
                "sound_candidates": agg_payload.get("sound_candidates").cloned().unwrap_or_else(|| serde_json::json!([])),
                "recommendation": agg_payload.get("recommendation").cloned().unwrap_or_else(|| serde_json::json!(agg.recommendation.clone())),
                "note": "in_subset=true is required for the L2 soundness claim on impact/callers --sound; promise_tier is language-aware (ast_modeled vs lexical_v1 vs mixed_lexical_v1); workspace union subset_ok = weakest selected root; sound_candidates are scoped --sound hints (eligible first) — global --sound still uses weakest selected root",
            });
            attach_stale_flags(&mut payload, &store, &indexer.root)?;
            print_query_json(&payload, &store)?;
            if !violations.is_empty() {
                std::process::exit(2);
            }
        }
        Commands::Diff {
            exact_only,
            limit,
            snapshot,
            write_snapshot,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let rid = root_filter.clone().unwrap_or_default();
            let d = crate::index::diff::run_diff_for_root(
                &indexer.root,
                &store,
                exact_only,
                limit,
                snapshot.as_deref(),
                &rid,
            )?;
            if write_snapshot {
                let snap = crate::index::diff::write_baseline_snapshot_for_root(
                    &indexer.root,
                    &store,
                    &rid,
                )?;
                eprintln!(
                    "wrote baseline snapshot ({} edges, index_seq={}, root_id={})",
                    snap.edges.len(),
                    snap.index_seq,
                    if rid.is_empty() {
                        "(all)".to_string()
                    } else {
                        rid.clone()
                    }
                );
            }
            // P5: stderr one-liner when dirty reindex drifted after last full snapshot.
            if d.baseline_stale {
                eprintln!(
                    "warn: baseline_stale=true — dirty reindex after last full snapshot \
                     (baseline not auto-refreshed); run `agentgraph index` or \
                     `diff --write-snapshot` to lock current edges"
                );
            }
            print_query_json(&serde_json::to_value(&d)?, &store)?;
        }
        Commands::BenchQuery {
            samples,
            prefix,
            hot,
            cold,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            // When --hot is set, run a **dedicated hot-only track** so p95 is
            // not diluted by thousands of helper names (Critical C1).
            let names: Vec<String> = if let Some(h) = &hot {
                vec![h.clone()]
            } else {
                let mut v = Vec::new();
                let hits = store.find_symbol_fuzzy(&prefix, samples.max(20))?;
                for s in hits {
                    if !v.contains(&s.name) {
                        v.push(s.name);
                    }
                }
                if v.is_empty() {
                    bail!("no symbols matching prefix '{prefix}' — index first?");
                }
                v
            };
            let hot_limit = if hot.is_some() { 5000usize } else { 20 };
            let mut callers_ms = Vec::with_capacity(samples);
            let mut impact_ms = Vec::with_capacity(samples);
            if !cold {
                let _ = store.callers_filtered(&names[0], hot_limit, ConfidenceFilter::Default)?;
                let _ = store.impact_filtered(&names[0], 2, 50, ConfidenceFilter::Default)?;
            }
            for i in 0..samples {
                let n = names[i % names.len()].clone();
                let limit = hot_limit;
                if cold {
                    let t = std::time::Instant::now();
                    let s2 = indexer.open_store()?;
                    let _ = s2.callers_filtered(&n, limit, ConfidenceFilter::Default)?;
                    callers_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                    let t = std::time::Instant::now();
                    let _ = s2.impact_filtered(&n, 2, 50, ConfidenceFilter::Default)?;
                    impact_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                } else {
                    let t = std::time::Instant::now();
                    let _ = store.callers_filtered(&n, limit, ConfidenceFilter::Default)?;
                    callers_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                    let t = std::time::Instant::now();
                    let _ = store.impact_filtered(&n, 2, 50, ConfidenceFilter::Default)?;
                    impact_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                }
            }
            callers_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            impact_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let pct = |v: &[f64], q: f64| -> f64 {
                let idx = ((v.len() as f64 * q).ceil() as usize).saturating_sub(1);
                v[idx.min(v.len() - 1)]
            };
            let payload = serde_json::json!({
                "samples": samples,
                "symbols": names.len(),
                "hot": hot,
                "cold": cold,
                "callers_ms": {
                    "p50": pct(&callers_ms, 0.50),
                    "p95": pct(&callers_ms, 0.95),
                    "p99": pct(&callers_ms, 0.99),
                    "max": callers_ms.last().copied().unwrap_or(0.0),
                },
                "impact_ms": {
                    "p50": pct(&impact_ms, 0.50),
                    "p95": pct(&impact_ms, 0.95),
                    "p99": pct(&impact_ms, 0.99),
                    "max": impact_ms.last().copied().unwrap_or(0.0),
                },
                "slo_ms": 50,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
            let c95 = pct(&callers_ms, 0.95);
            let i95 = pct(&impact_ms, 0.95);
            if c95 >= 50.0 || i95 >= 50.0 {
                bail!("query p95 SLO fail: callers={c95:.2}ms impact={i95:.2}ms (budget 50ms)");
            }
        }
        Commands::Mcp => {
            crate::mcp::server::run_stdio(indexer.root)?;
        }
        Commands::Macro { command } => match command {
            MacroCmd::Status => {
                let status = indexer.macro_status()?;
                let mut payload = serde_json::to_value(&status)?;
                if workspace_mode || store_is_workspace(&indexer.open_store()?) {
                    if let Some(obj) = payload.as_object_mut() {
                        obj.insert(
                            "workspace_note".into(),
                            serde_json::json!(
                                "macro sidecar is per-root at <root>/.agentgraph/index.macro.db; \
                                 workspace multi-root queries need --workspace-root to select which sidecar \
                                 (union across roots without a filter is rejected)"
                            ),
                        );
                    }
                }
                // P5: ensure honesty flags + stderr one-liners.
                if let Some(obj) = payload.as_object_mut() {
                    obj.entry("sidecar_exists")
                        .or_insert(serde_json::json!(status.exists));
                    obj.entry("sidecar_stale")
                        .or_insert(serde_json::json!(status.stale));
                    obj.entry("baseline_stale")
                        .or_insert(serde_json::json!(status.baseline_stale));
                }
                warn_stale_flags(status.baseline_stale, status.stale, "macro status");
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            MacroCmd::Rebuild { force } => {
                if workspace_mode {
                    bail!(
                        "macro rebuild + workspace multi-root is not supported in one command; \
                         rebuild sidecars per classic --root (sidecar is per-root .agentgraph/index.macro.db)"
                    );
                }
                let result = indexer.macro_rebuild(force)?;
                let status = indexer.macro_status()?;
                let payload = serde_json::json!({
                    "macro_sidecar": result,
                    "status": status,
                    "note": "sidecar rebuilt from recorded expanded_root (idempotent; not sound)",
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        },
        Commands::Graph {
            name,
            depth,
            out,
            direction,
            impact,
            exact_only,
            include_dynamic,
            with_macro,
            no_macro_dedup,
            sound,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let root_filter = resolve_query_root_filter(&store, &workspace_roots)?;
            let rf = root_filter.as_deref();
            if sound && with_macro {
                bail!(
                    "--sound is mutually exclusive with --with-macro \
                     (macro sidecar is not sound-certified; no subset_ok claim for expanded-only rows)"
                );
            }
            if sound && (exact_only || include_dynamic) {
                bail!(
                    "--sound is mutually exclusive with --exact-only / --include-dynamic \
                     (sound walk uses its own eligibility filter)"
                );
            }
            guard_macro_workspace(&store, with_macro, rf)?;
            let direction: GraphDirection = if impact {
                GraphDirection::Impact
            } else {
                direction.into()
            };
            let filter = parse_query_flags(exact_only, include_dynamic, false);
            let flags = GraphFlags {
                exact_only,
                include_dynamic,
                with_macro,
                sound,
                direction,
            };
            // Render cap is MAX_GRAPH_NODES; query limit is slightly higher so BFS
            // has room before the viz cap truncates.
            let query_limit = crate::viz::MAX_GRAPH_NODES.saturating_add(50).max(100);
            let ignore_sidecar = exact_only && with_macro;
            let opts = UnionOptions {
                dedup: !no_macro_dedup,
                ignore_sidecar,
            };

            // Track M4: sound-eligible neighborhood + S status on the page.
            // Track M4-W polish: --workspace-root scopes graph queries + root_id badges.
            if sound {
                let languages = store.stats(&indexer.root.to_string_lossy())?.languages;
                let (mut data, subset_ok, promise_tier) = match direction {
                    GraphDirection::Callers => {
                        let (hits, violations) = store.callers_sound_in(&name, query_limit, rf)?;
                        let subset_ok = violations.is_empty();
                        let (tier, _p) =
                            crate::index::subset::select_sound_promise(subset_ok, &languages);
                        (
                            build_callers_graph(&name, &hits, flags.clone()),
                            subset_ok,
                            tier.as_str().to_string(),
                        )
                    }
                    GraphDirection::Both => {
                        let (ih, iv) = store.impact_sound_in(&name, depth, query_limit, rf)?;
                        let (ch, cv) = store.callers_sound_in(&name, query_limit, rf)?;
                        let subset_ok = iv.is_empty() && cv.is_empty();
                        let (tier, _p) =
                            crate::index::subset::select_sound_promise(subset_ok, &languages);
                        let a = build_impact_graph(&name, &ih, flags.clone(), depth);
                        let b = build_callers_graph(&name, &ch, flags.clone());
                        let mut d = merge_graphs(a, b);
                        d.depth = depth;
                        (d, subset_ok, tier.as_str().to_string())
                    }
                    GraphDirection::Impact => {
                        let (hits, violations) =
                            store.impact_sound_in(&name, depth, query_limit, rf)?;
                        let subset_ok = violations.is_empty();
                        let (tier, _p) =
                            crate::index::subset::select_sound_promise(subset_ok, &languages);
                        (
                            build_impact_graph(&name, &hits, flags.clone(), depth),
                            subset_ok,
                            tier.as_str().to_string(),
                        )
                    }
                };
                data.flags.sound = true;
                data.subset_ok = Some(subset_ok);
                data.promise_tier = Some(promise_tier);
                data.root_filter = root_filter.clone();
                if !subset_ok {
                    data.empty_note = Some(
                        "S violated — sound walk disabled; this page is NOT a sound graph. \
                         Best-effort sound-eligible candidates only (promise_tier=disabled)."
                            .to_string(),
                    );
                }

                let out_path = match out {
                    Some(p) => p,
                    None => indexer.root.join(".agentgraph").join("graph.html"),
                };
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let html = render_graph_html(&data);
                std::fs::write(&out_path, html)?;
                println!(
                    "wrote graph → {} ({} nodes / {} edges, direction={}, depth={}, sound=true, subset_ok={})",
                    out_path.display(),
                    data.nodes.len(),
                    data.edges.len(),
                    direction.as_str(),
                    depth,
                    subset_ok
                );
                if !subset_ok {
                    eprintln!(
                        "note: subset_ok=false — HTML written but **not** a sound graph \
                         (promise_tier=disabled; see docs/sound-subset.md)"
                    );
                    std::process::exit(2);
                }
                if data.nodes.len() <= 1 && data.edges.is_empty() {
                    eprintln!(
                        "note: no sound-eligible indexed edges for '{name}' — empty graph written \
                         (S-qualified walk; not a complete runtime graph)"
                    );
                }
                return Ok(());
            }

            let mut data = match direction {
                GraphDirection::Callers => {
                    let hits = store.callers_filtered_in(&name, query_limit, filter, rf)?;
                    let mut d = build_callers_graph(&name, &hits, flags.clone());
                    if with_macro && !opts.ignore_sidecar {
                        if let Some(side) = indexer.open_macro_store()? {
                            let (layout, _, _) = indexer.macro_crate_layout()?;
                            let status = indexer.macro_status()?;
                            let side_hits = side.callers_filtered(&name, query_limit, filter)?;
                            let expanded_root = status
                                .expanded_root
                                .clone()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| indexer.root.clone());
                            let (main_rows, _) = (main_callers_json(&hits), ());
                            // Re-run union for viz: use mapped ReferenceRecords via JSON side.
                            // Map each side hit path for the graph nodes.
                            let mut mapped_records = Vec::new();
                            for r in &side_hits {
                                let mapped = macro_map::map_expanded_path(
                                    &r.path,
                                    &expanded_root,
                                    &indexer.root,
                                    &layout,
                                );
                                let mut r2 = r.clone();
                                if let Some(mp) = mapped {
                                    r2.path = mp;
                                }
                                mapped_records.push(r2);
                            }
                            // Optional de-dup against main keys.
                            if opts.dedup {
                                let mut main_keys = std::collections::HashSet::new();
                                for v in &main_rows {
                                    if let (Some(n), Some(p)) =
                                        (v["name"].as_str(), v["path"].as_str())
                                    {
                                        let e = v["enclosing"].as_str().unwrap_or("");
                                        main_keys.insert(format!("{n}\u{0}{e}\u{0}{p}"));
                                    }
                                }
                                mapped_records.retain(|r| {
                                    let e = r.enclosing.clone().unwrap_or_default();
                                    !main_keys
                                        .contains(&format!("{}\u{0}{e}\u{0}{}", r.name, r.path))
                                });
                            }
                            add_macro_caller_rows(&mut d, &name, &mapped_records);
                        }
                    }
                    d
                }
                GraphDirection::Both => {
                    let impact_hits =
                        store.impact_filtered_in(&name, depth, query_limit, filter, rf)?;
                    let callers_hits = store.callers_filtered_in(&name, query_limit, filter, rf)?;
                    let a = build_impact_graph(&name, &impact_hits, flags.clone(), depth);
                    let b = build_callers_graph(&name, &callers_hits, flags.clone());
                    let mut d = merge_graphs(a, b);
                    d.depth = depth;
                    if with_macro && !opts.ignore_sidecar {
                        if let Some(side) = indexer.open_macro_store()? {
                            let (layout, _, _) = indexer.macro_crate_layout()?;
                            let status = indexer.macro_status()?;
                            let expanded_root = status
                                .expanded_root
                                .clone()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| indexer.root.clone());
                            let si = side.impact_filtered(&name, depth, query_limit, filter)?;
                            let sc = side.callers_filtered(&name, query_limit, filter)?;
                            let map_imp: Vec<crate::model::ImpactNode> = si
                                .iter()
                                .map(|n| {
                                    let mut n2 = n.clone();
                                    if let Some(mp) = macro_map::map_expanded_path(
                                        &n.path,
                                        &expanded_root,
                                        &indexer.root,
                                        &layout,
                                    ) {
                                        n2.path = mp;
                                    }
                                    n2
                                })
                                .collect();
                            let map_cal: Vec<crate::model::ReferenceRecord> = sc
                                .iter()
                                .map(|r| {
                                    let mut r2 = r.clone();
                                    if let Some(mp) = macro_map::map_expanded_path(
                                        &r.path,
                                        &expanded_root,
                                        &indexer.root,
                                        &layout,
                                    ) {
                                        r2.path = mp;
                                    }
                                    r2
                                })
                                .collect();
                            add_macro_impact_rows(&mut d, &name, &map_imp);
                            add_macro_caller_rows(&mut d, &name, &map_cal);
                        }
                    }
                    d
                }
                GraphDirection::Impact => {
                    let hits = store.impact_filtered_in(&name, depth, query_limit, filter, rf)?;
                    let mut d = build_impact_graph(&name, &hits, flags.clone(), depth);
                    if with_macro && !opts.ignore_sidecar {
                        if let Some(side) = indexer.open_macro_store()? {
                            let (layout, _, _) = indexer.macro_crate_layout()?;
                            let status = indexer.macro_status()?;
                            let expanded_root = status
                                .expanded_root
                                .clone()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| indexer.root.clone());
                            let side_hits =
                                side.impact_filtered(&name, depth, query_limit, filter)?;
                            let map_imp: Vec<crate::model::ImpactNode> = side_hits
                                .iter()
                                .map(|n| {
                                    let mut n2 = n.clone();
                                    if let Some(mp) = macro_map::map_expanded_path(
                                        &n.path,
                                        &expanded_root,
                                        &indexer.root,
                                        &layout,
                                    ) {
                                        n2.path = mp;
                                    }
                                    n2
                                })
                                .collect();
                            add_macro_impact_rows(&mut d, &name, &map_imp);
                        }
                    }
                    d
                }
            };

            // Non-sound graph path: subset_ok stays unset on the page.
            data.root_filter = root_filter.clone();
            let _ = &mut data;

            let out_path = match out {
                Some(p) => p,
                None => indexer.root.join(".agentgraph").join("graph.html"),
            };
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let html = render_graph_html(&data);
            std::fs::write(&out_path, html)?;
            println!(
                "wrote graph → {} ({} nodes / {} edges, direction={}, depth={})",
                out_path.display(),
                data.nodes.len(),
                data.edges.len(),
                direction.as_str(),
                depth
            );
            if data.nodes.len() <= 1 && data.edges.is_empty() {
                eprintln!(
                    "note: no indexed edges for '{name}' — empty graph written \
                     (L0/L1 candidates only; not a complete runtime graph)"
                );
            }
        }
    }
    Ok(())
}
