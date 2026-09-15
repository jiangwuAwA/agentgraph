use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::index::{llm, Indexer};
use crate::query::Query;

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
    /// Find symbol definitions by exact or fuzzy name
    Find {
        name: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// List call sites / references of a symbol
    Callers {
        name: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Blast radius: who transitively depends on this symbol
    Impact {
        name: String,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
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
    /// Run as an MCP server over stdio
    Mcp,
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
        Commands::Find { name, limit } => {
            let store = indexer.open_store()?;
            let q = Query::new(&store);
            let hits = q.find_symbol(&name, limit)?;
            if hits.is_empty() {
                bail!("no symbol matching '{name}' — run `agentgraph index` first?");
            }
            println!("{}", serde_json::to_string_pretty(&hits)?);
        }
        Commands::Callers { name, limit } => {
            let store = indexer.open_store()?;
            let q = Query::new(&store);
            let hits = q.callers(&name, limit)?;
            println!("{}", serde_json::to_string_pretty(&hits)?);
        }
        Commands::Impact { name, depth, limit } => {
            let store = indexer.open_store()?;
            let q = Query::new(&store);
            let hits = q.impact(&name, depth, limit)?;
            println!("{}", serde_json::to_string_pretty(&hits)?);
        }
        Commands::Related { name, limit } => {
            let store = indexer.open_store()?;
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
            let hits = store.importers_of_file(&path.replace('\\', "/"), limit)?;
            println!("{}", serde_json::to_string_pretty(&hits)?);
        }
        Commands::Enrich { limit } => {
            let cfg = llm::LlmConfig::from_env()?;
            let mut store = indexer.open_store()?;
            let report = llm::enrich(&indexer.root, &mut store, &cfg, limit)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::Mcp => {
            crate::mcp::server::run_stdio(root)?;
        }
    }
    Ok(())
}
