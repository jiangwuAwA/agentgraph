use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::index::{llm, Indexer};
use crate::model::ConfidenceFilter;
use crate::query::{parse_confidence_flags, Query};

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

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Build or refresh the local index (incremental by content hash)
    Index {
        /// Re-parse every file even if unchanged
        #[arg(long)]
        force: bool,
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
        /// Only Exact (L0) edges; exclude Heuristic/DynamicCandidate
        #[arg(long, default_value_t = false)]
        exact_only: bool,
        /// Also include DynamicCandidate edges (higher noise)
        #[arg(long, default_value_t = false)]
        include_dynamic: bool,
        /// L2: only sound-eligible edges + S-violation report
        #[arg(long, default_value_t = false)]
        sound: bool,
    },
    /// Blast radius: who transitively depends on this symbol
    Impact {
        name: String,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Only Exact (L0) edges; exclude Heuristic/DynamicCandidate
        #[arg(long, default_value_t = false)]
        exact_only: bool,
        /// Also include DynamicCandidate edges (higher noise)
        #[arg(long, default_value_t = false)]
        include_dynamic: bool,
        /// L2: walk only sound-eligible edges; report S-violations; never claims sound outside S
        #[arg(long, default_value_t = false)]
        sound: bool,
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
}

pub fn run(cli: Cli) -> Result<()> {
    let root = match cli.root {
        Some(r) => r,
        None => std::env::current_dir()?,
    };
    let indexer = Indexer::new(&root)?;

    match cli.command {
        Commands::Index { force } => {
            let stats = indexer.index(force)?;
            println!("{}", serde_json::to_string_pretty(&stats)?);
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
            sound,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            if sound {
                if exact_only || include_dynamic {
                    bail!(
                        "--sound is mutually exclusive with --exact-only / --include-dynamic \
                         (sound walk uses its own eligibility filter)"
                    );
                }
                let (hits, violations) = store.callers_sound(&name, limit)?;
                let subset_ok = violations.is_empty();
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
                // Production S: modeled dispatch + registrations; disabled when S violated.
                let payload = serde_json::json!({
                    "mode": "sound",
                    "subset_ok": subset_ok,
                    "promise": if subset_ok {
                        "S satisfied. Sound walk over-approximates modeled runtime edges (direct, literal-key, emit↔on dispatch, DI/route registration). Not a claim outside S; registration ≠ HTTP ServeHTTP."
                    } else {
                        "S violated — eligibility claim disabled; results are best-effort sound-eligible edges only."
                    },
                    "subset_violations": violations,
                    "callers": mapped,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
                return Ok(());
            }
            let q = Query::new(&store);
            let filter = parse_confidence_flags(exact_only, include_dynamic);
            let hits = q.callers_filtered(&name, limit, filter)?;
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
            println!("{}", serde_json::to_string_pretty(&mapped)?);
        }
        Commands::Impact {
            name,
            depth,
            limit,
            exact_only,
            include_dynamic,
            sound,
        } => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            if sound {
                if exact_only || include_dynamic {
                    bail!("--sound is mutually exclusive with --exact-only / --include-dynamic");
                }
                let (hits, violations) = store.impact_sound(&name, depth, limit)?;
                let subset_ok = violations.is_empty();
                let payload = serde_json::json!({
                    "mode": "sound",
                    "subset_ok": subset_ok,
                    "promise": if subset_ok {
                        "S satisfied. Sound walk over-approximates modeled runtime edges (direct, literal-key, emit↔on dispatch, DI/route registration). Not a claim outside S; registration ≠ HTTP ServeHTTP."
                    } else {
                        "S violated — eligibility claim disabled; results are best-effort sound-eligible edges only."
                    },
                    "subset_violations": violations,
                    "impact": hits,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
                return Ok(());
            }
            let q = Query::new(&store);
            let filter = parse_confidence_flags(exact_only, include_dynamic);
            let hits = q.impact_filtered(&name, depth, limit, filter)?;
            println!("{}", serde_json::to_string_pretty(&hits)?);
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
            let hits = store.importers_of_file(&path.replace('\\', "/"), limit)?;
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
        } => {
            let mut store = indexer.open_store()?;
            store.ensure_indexed()?;
            // perf-plan P0-4: never export stale sid links.
            store.ensure_sids_for_export()?;
            let filter = parse_confidence_flags(exact_only, include_dynamic);
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
                "lsif" => crate::index::export::export_lsif(&store, &indexer.root, &out)?,
                other => bail!("unknown export format: {other}"),
            }
            println!("wrote {format} → {}", out.display());
        }
        Commands::Subset => {
            let store = indexer.open_store()?;
            store.ensure_indexed()?;
            let violations = store.subset_violations()?;
            let payload = serde_json::json!({
                "in_subset": violations.is_empty(),
                "violation_count": violations.len(),
                "violations": violations,
                "note": "in_subset=true is required for the L2 soundness claim on impact/callers --sound",
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
                    let s2 = indexer.open_store()?;
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
    }
    Ok(())
}
