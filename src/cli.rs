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

    #[command(subcommand)]
    pub command: Commands,
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
    /// Build or refresh the local index (incremental by content hash)
    Index {
        /// Re-parse every file even if unchanged
        #[arg(long)]
        force: bool,
        /// Optional: also index an expanded shadow tree into sidecar
        /// `.agentgraph/index.macro.db` (does not replace the main index)
        #[arg(long)]
        macro_expanded_root: Option<PathBuf>,
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
    Subset,
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

/// Build main callers rows as JSON (`at=path:line`, no origin tag).
fn main_callers_json(hits: &[crate::model::ReferenceRecord]) -> Vec<serde_json::Value> {
    hits.iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert(
                    "at".into(),
                    serde_json::json!(format!("{}:{}", r.path, r.line)),
                );
            }
            v
        })
        .collect()
}

/// Build main impact rows as JSON (`at=path:line`).
fn main_impact_json(hits: &[crate::model::ImpactNode]) -> Vec<serde_json::Value> {
    hits.iter()
        .map(|n| {
            let mut v = serde_json::to_value(n).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert(
                    "at".into(),
                    serde_json::json!(format!("{}:{}", n.path, r_line(n))),
                );
            }
            v
        })
        .collect()
}

fn r_line(n: &crate::model::ImpactNode) -> usize {
    n.line
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

pub fn run(cli: Cli) -> Result<()> {
    let root = match cli.root {
        Some(r) => r,
        None => std::env::current_dir()?,
    };
    let indexer = Indexer::new(&root)?;

    match cli.command {
        Commands::Index {
            force,
            macro_expanded_root,
        } => {
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
            let stats = indexer.stats()?;
            println!("{}", serde_json::to_string_pretty(&stats)?);
        }
        Commands::Find { name, limit, fuzzy } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let hits = if fuzzy {
                // Explicit --fuzzy: substring search only.
                store.find_symbol_fuzzy(&name, limit)?
            } else {
                // Exact first; if empty, automatic fuzzy fallback with a stderr note.
                let exact = store.find_symbol_exact(&name, limit)?;
                if exact.is_empty() {
                    let fuzzy_hits = store.find_symbol_fuzzy(&name, limit)?;
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
            println!("{}", serde_json::to_string_pretty(&hits)?);
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
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            if sound && with_macro {
                bail!(
                    "--sound is mutually exclusive with --with-macro \
                     (macro sidecar is not sound-certified; no subset_ok claim for expanded-only rows)"
                );
            }
            if sound {
                if exact_only || include_dynamic || recall {
                    bail!(
                        "--sound is mutually exclusive with --exact-only / --include-dynamic / --recall \
                         (sound walk uses its own eligibility filter)"
                    );
                }
                let (hits, violations) = store.callers_sound(&name, limit)?;
                let subset_ok = violations.is_empty();
                let languages = store.stats(&indexer.root.to_string_lossy())?.languages;
                let (promise_tier, promise) =
                    crate::index::subset::select_sound_promise(subset_ok, &languages);
                let mapped: Vec<serde_json::Value> = hits
                    .into_iter()
                    .map(|r| {
                        let mut v = serde_json::to_value(&r).unwrap_or_default();
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert(
                                "at".into(),
                                serde_json::json!(format!("{}:{}", r.path, r.line)),
                            );
                        }
                        v
                    })
                    .collect();
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
                println!("{}", serde_json::to_string_pretty(&payload)?);
                return Ok(());
            }
            let q = Query::new(&store);
            let filter = parse_query_flags(exact_only, include_dynamic, recall);
            let hits = q.callers_filtered(&name, limit, filter)?;
            let main_rows = main_callers_json(&hits);

            // M1: --exact-only --with-macro → ignore sidecar (spec §1.4).
            let ignore_sidecar = exact_only;
            if with_macro && ignore_sidecar {
                println!("{}", serde_json::to_string_pretty(&main_rows)?);
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
                    let side_hits = side.callers_filtered(&name, limit, filter)?;
                    let expanded_root = status
                        .expanded_root
                        .clone()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| indexer.root.clone());
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
                        "note": "sidecar union is optional candidates (not sound); de-dup ON unless --no-macro-dedup",
                    });
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                    return Ok(());
                }
                // Absent sidecar → empty union (graceful; plain array, no error).
            }
            println!("{}", serde_json::to_string_pretty(&main_rows)?);
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
            if sound && with_macro {
                bail!(
                    "--sound is mutually exclusive with --with-macro \
                     (macro sidecar is not sound-certified; no subset_ok claim for expanded-only rows)"
                );
            }
            if sound {
                if exact_only || include_dynamic || recall {
                    bail!(
                        "--sound is mutually exclusive with --exact-only / --include-dynamic / --recall"
                    );
                }
                let (hits, violations) = store.impact_sound(&name, depth, limit)?;
                let subset_ok = violations.is_empty();
                let languages = store.stats(&indexer.root.to_string_lossy())?.languages;
                let (promise_tier, promise) =
                    crate::index::subset::select_sound_promise(subset_ok, &languages);
                let payload = serde_json::json!({
                    "mode": "sound",
                    "subset_ok": subset_ok,
                    "promise_tier": promise_tier.as_str(),
                    "promise": promise,
                    "promise_languages": languages,
                    "subset_violations": violations,
                    "impact": hits,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
                return Ok(());
            }
            let q = Query::new(&store);
            let filter = parse_query_flags(exact_only, include_dynamic, recall);
            let hits = q.impact_filtered(&name, depth, limit, filter)?;

            let ignore_sidecar = exact_only;
            if with_macro && ignore_sidecar {
                println!("{}", serde_json::to_string_pretty(&hits)?);
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
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&main_rows)?);
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&hits)?);
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
        Commands::Subset => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let violations = store.subset_violations()?;
            let languages = store.stats(&indexer.root.to_string_lossy())?.languages;
            let (promise_tier, promise) =
                crate::index::subset::select_sound_promise(violations.is_empty(), &languages);
            let payload = serde_json::json!({
                "in_subset": violations.is_empty(),
                "violation_count": violations.len(),
                "violations": violations,
                "promise_tier": promise_tier.as_str(),
                "promise": promise,
                "promise_languages": languages,
                "note": "in_subset=true is required for the L2 soundness claim on impact/callers --sound; promise_tier is language-aware (ast_modeled vs lexical_v1 vs mixed_lexical_v1)",
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
            if !violations.is_empty() {
                std::process::exit(2);
            }
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
                println!("{}", serde_json::to_string_pretty(&status)?);
            }
            MacroCmd::Rebuild { force } => {
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
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
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
                sound: false,
                direction,
            };
            // Render cap is MAX_GRAPH_NODES; query limit is slightly higher so BFS
            // has room before the viz cap truncates.
            let query_limit = crate::viz::MAX_GRAPH_NODES.saturating_add(50).max(100);
            let q = Query::new(&store);
            let ignore_sidecar = exact_only && with_macro;
            let opts = UnionOptions {
                dedup: !no_macro_dedup,
                ignore_sidecar,
            };

            let mut data = match direction {
                GraphDirection::Callers => {
                    let hits = q.callers_filtered(&name, query_limit, filter)?;
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
                    let impact_hits = q.impact_filtered(&name, depth, query_limit, filter)?;
                    let callers_hits = q.callers_filtered(&name, query_limit, filter)?;
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
                    let hits = q.impact_filtered(&name, depth, query_limit, filter)?;
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

            // Optional sound flags are not part of graph CLI today; leave subset_ok unset.
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
