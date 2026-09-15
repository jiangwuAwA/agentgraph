//! Minimal MCP (Model Context Protocol) stdio server.
//! Implements initialize / tools/list / tools/call / ping over newline-delimited JSON-RPC 2.0.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::index::Indexer;
use crate::query::Query;

pub fn run_stdio(root: PathBuf) -> Result<()> {
    let state = Mutex::new(ServerState { root });

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                write_msg(
                    &mut out,
                    &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("parse error: {e}")}}),
                )?;
                continue;
            }
        };

        // Notifications have no id
        if msg.get("id").is_none() {
            continue;
        }
        let id = msg["id"].clone();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));

        let result = match method {
            "initialize" => handle_initialize(),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(tools_list()),
            "tools/call" => handle_tools_call(&state, &params),
            _ => Err(json!({
                "code": -32601,
                "message": format!("method not found: {method}")
            })),
        };

        let response = match result {
            Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
            Err(e) => json!({"jsonrpc":"2.0","id":id,"error":e}),
        };
        write_msg(&mut out, &response)?;
    }
    Ok(())
}

fn write_msg(out: &mut impl Write, msg: &Value) -> Result<()> {
    let s = serde_json::to_string(msg)?;
    out.write_all(s.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn handle_initialize() -> Result<Value, Value> {
    Ok(json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "agentgraph",
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "index",
                "description": "Build or refresh the code symbol/call graph index for a repository (incremental). Call this once before other queries.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "root": {"type": "string", "description": "Project root path (optional; defaults to server root)"},
                        "force": {"type": "boolean", "description": "Force full reindex"}
                    }
                }
            },
            {
                "name": "find_symbol",
                "description": "Find symbol definitions by exact or fuzzy name. Returns path, line range, kind.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Symbol name or qualified name"},
                        "limit": {"type": "integer", "default": 20}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "callers",
                "description": "List call sites / references of a symbol. Use before editing a function to see who depends on it.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "limit": {"type": "integer", "default": 50}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "impact",
                "description": "Multi-hop blast radius: who transitively depends on this symbol (call graph BFS).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "depth": {"type": "integer", "default": 2},
                        "limit": {"type": "integer", "default": 100}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "related_files",
                "description": "Files most related to a symbol (definition + reference sites). Scope retrieval/context before reading code.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "limit": {"type": "integer", "default": 10}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "stats",
                "description": "Show current index statistics (file/symbol/reference counts).",
                "inputSchema": {"type": "object", "properties": {}}
            },
            {
                "name": "importers",
                "description": "List files that import a given file (module-level resolved import edges).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Repo-relative file path"},
                        "limit": {"type": "integer", "default": 50}
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "enrich",
                "description": "Optional LLM pass: attach one-line responsibility descriptions to undescribed symbols. Requires OPENAI_API_KEY.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {"type": "integer", "default": 30}
                    }
                }
            }
        ]
    })
}

struct ServerState {
    root: PathBuf,
}

fn tool_error(code: i64, message: &str) -> Value {
    json!({"code": code, "message": message})
}

fn ok_text(text: String) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": false
    })
}

fn err_text(message: String) -> Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

fn handle_tools_call(state: &Mutex<ServerState>, params: &Value) -> Result<Value, Value> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| tool_error(-32602, "missing tool name"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let root = {
        let st = state.lock().unwrap();
        let base = st.root.clone();
        if let Some(r) = args.get("root").and_then(|v| v.as_str()) {
            let candidate = PathBuf::from(r);
            // Security: reject roots outside the server's initial root unless
            // AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1 (prompt-injection / path escape).
            let allow_any = std::env::var("AGENTGRAPH_MCP_ALLOW_ANY_ROOT")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if !allow_any {
                let cand = crate::index::parser::normalize_root(&candidate);
                let base_n = crate::index::parser::normalize_root(&base);
                if cand != base_n && !cand.starts_with(&base_n) {
                    return Err(tool_error(
                        -32602,
                        &format!(
                            "root '{}' is outside server root '{}'; set AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1 to override",
                            cand.display(),
                            base_n.display()
                        ),
                    ));
                }
                cand
            } else {
                candidate
            }
        } else {
            base
        }
    };

    let result = (|| -> anyhow::Result<Value> {
        match name {
            "index" => {
                let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
                let indexer = Indexer::new(&root)?;
                let stats = indexer.index(force)?;
                Ok(ok_text(serde_json::to_string_pretty(&stats)?))
            }
            "stats" => {
                let indexer = Indexer::new(&root)?;
                let stats = indexer.stats()?;
                Ok(ok_text(serde_json::to_string_pretty(&stats)?))
            }
            "find_symbol" => {
                let sym = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let hits = Query::new(&store).find_symbol(sym, limit)?;
                Ok(ok_text(serde_json::to_string_pretty(&hits)?))
            }
            "callers" => {
                let sym = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let hits = Query::new(&store).callers(sym, limit)?;
                Ok(ok_text(serde_json::to_string_pretty(&hits)?))
            }
            "impact" => {
                let sym = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let hits = Query::new(&store).impact(sym, depth, limit)?;
                Ok(ok_text(serde_json::to_string_pretty(&hits)?))
            }
            "related_files" => {
                let sym = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let hits = Query::new(&store).related_files(sym, limit)?;
                let mapped: Vec<Value> = hits
                    .into_iter()
                    .map(|(path, score, reason)| {
                        json!({"path": path, "score": score, "reason": reason})
                    })
                    .collect();
                Ok(ok_text(serde_json::to_string_pretty(&mapped)?))
            }
            "importers" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("path required"))?
                    .replace('\\', "/");
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let hits = store.importers_of_file(&path, limit)?;
                Ok(ok_text(serde_json::to_string_pretty(&hits)?))
            }
            "enrich" => {
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(30) as usize;
                let cfg = crate::index::llm::LlmConfig::from_env()?;
                let indexer = Indexer::new(&root)?;
                let mut store = indexer.open_store()?;
                let report = crate::index::llm::enrich(&indexer.root, &mut store, &cfg, limit)?;
                Ok(ok_text(serde_json::to_string_pretty(&report)?))
            }
            other => Err(anyhow::anyhow!("unknown tool: {other}")),
        }
    })();

    match result {
        Ok(v) => Ok(v),
        Err(e) => Ok(err_text(format!("{e:#}"))),
    }
}
