//! Minimal MCP (Model Context Protocol) stdio server.
//! Implements initialize / tools/list / tools/call / ping over newline-delimited JSON-RPC 2.0.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::index::{tag_macro_impact_json, tag_macro_ref_json, Indexer};
use crate::query::{parse_query_flags, Query};

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
                "description": "List call sites / references of a symbol. Default includes Exact + Heuristic (L1 DI/factory). Use exact_only=true to drop Heuristic; include_dynamic=true to also return DynamicCandidate. sound=true uses the L2 sound-eligible edge set (mutually exclusive with exact_only/include_dynamic) and reports S-violations.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "limit": {"type": "integer", "default": 50},
                        "exact_only": {"type": "boolean", "default": false},
                        "include_dynamic": {"type": "boolean", "default": false},
                        "recall": {"type": "boolean", "default": false, "description": "Prefer recall over a clean graph (alias for include_dynamic)"},
                        "sound": {"type": "boolean", "default": false},
                        "with_macro": {"type": "boolean", "default": false, "description": "Union optional macro-expanded sidecar hits (origin=macro_expanded). Default off. Mutually exclusive with sound. Not sound-certified."}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "impact",
                "description": "Multi-hop blast radius: who transitively depends on this symbol (call graph BFS). Default Exact + Heuristic; recall/include_dynamic widen for missed-edge safety; sound=true uses L2 S-qualified edges.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "depth": {"type": "integer", "default": 2},
                        "limit": {"type": "integer", "default": 100},
                        "exact_only": {"type": "boolean", "default": false},
                        "include_dynamic": {"type": "boolean", "default": false},
                        "recall": {"type": "boolean", "default": false, "description": "Prefer recall over a clean graph (alias for include_dynamic)"},
                        "sound": {"type": "boolean", "default": false},
                        "with_macro": {"type": "boolean", "default": false, "description": "Union optional macro-expanded sidecar hits (origin=macro_expanded). Default off. Mutually exclusive with sound. Not sound-certified."}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "macro_status",
                "description": "Optional macro-expanded sidecar status (P2): whether <root>/.agentgraph/index.macro.db exists, its path, and file/symbol/ref counts. Default product path does not use this sidecar.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "subset",
                "description": "List language-subset S violations stored at last index (L2). Empty list means --sound may emit its (weakened) eligibility promise; it is still not a runtime call-graph theorem.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
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
                "description": "Optional LLM pass: attach one-line responsibility descriptions to undescribed symbols. Requires OPENAI_API_KEY. Default limit matches CLI (50).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {"type": "integer", "default": 50}
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

/// Shared promise strings (CLI + MCP must not drift — R4 M3).
/// Language-aware (track B): AST-modeled vs lexical-v1 vs mixed — never one
/// global OK for all languages. Source of truth lives in `index::subset`;
/// re-exported here so e2e/CLI assert identity against a single module.
pub use crate::index::subset::{
    select_sound_promise, sound_promise_text, sound_promise_tier, SoundPromiseTier,
    SOUND_PROMISE_DISABLED, SOUND_PROMISE_OK_AST, SOUND_PROMISE_OK_LEXICAL_V1,
    SOUND_PROMISE_OK_MIXED_LEXICAL_V1,
};

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
            let raw = PathBuf::from(r);
            // Relative roots resolve against the server root (not process cwd).
            let candidate = if raw.is_absolute() {
                raw
            } else {
                base.join(&raw)
            };
            // Security: reject roots outside the server's initial root unless
            // AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1 (prompt-injection / path escape).
            // C1: must canonicalize BOTH sides before the prefix test — otherwise
            // `base/../outside` slips through `starts_with` and Indexer::new
            // creates `.agentgraph` outside the jail.
            let allow_any = std::env::var("AGENTGRAPH_MCP_ALLOW_ANY_ROOT")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if !allow_any {
                let cand_canon = candidate.canonicalize().map_err(|e| {
                    tool_error(
                        -32602,
                        &format!(
                            "root '{}' cannot be canonicalized ({e}); set AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1 to override",
                            candidate.display()
                        ),
                    )
                })?;
                let base_canon = base.canonicalize().map_err(|e| {
                    tool_error(
                        -32602,
                        &format!(
                            "server root '{}' cannot be canonicalized ({e})",
                            base.display()
                        ),
                    )
                })?;
                let cand = crate::index::parser::normalize_root(&cand_canon);
                let base_n = crate::index::parser::normalize_root(&base_canon);
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
                let exact_only = args
                    .get("exact_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let include_dynamic = args
                    .get("include_dynamic")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let recall = args
                    .get("recall")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let sound = args.get("sound").and_then(|v| v.as_bool()).unwrap_or(false);
                let with_macro = args
                    .get("with_macro")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                if sound && with_macro {
                    return Err(anyhow::anyhow!(
                        "sound is mutually exclusive with with_macro \
                         (macro sidecar is not sound-certified)"
                    ));
                }
                if sound {
                    if exact_only || include_dynamic || recall {
                        return Err(anyhow::anyhow!(
                            "sound is mutually exclusive with exact_only / include_dynamic / recall \
                             (sound walk uses its own eligibility filter)"
                        ));
                    }
                    let (hits, violations) = store.callers_sound(sym, limit)?;
                    let subset_ok = violations.is_empty();
                    let languages = store.stats(&root.to_string_lossy())?.languages;
                    let (promise_tier, promise) = select_sound_promise(subset_ok, &languages);
                    let mapped: Vec<Value> = hits
                        .into_iter()
                        .map(|r| {
                            let mut v = serde_json::to_value(&r).unwrap_or_default();
                            if let Some(obj) = v.as_object_mut() {
                                obj.insert("at".into(), json!(format!("{}:{}", r.path, r.line)));
                            }
                            v
                        })
                        .collect();
                    let payload = json!({
                        "mode": "sound",
                        "subset_ok": subset_ok,
                        "promise_tier": promise_tier.as_str(),
                        "promise": promise,
                        "promise_languages": languages,
                        "subset_violations": violations,
                        "callers": mapped,
                    });
                    return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                }
                let filter = parse_query_flags(exact_only, include_dynamic, recall);
                let hits = Query::new(&store).callers_filtered(sym, limit, filter)?;
                // Always emit CLI-stable `at` on callers rows (with or without
                // with_macro) so e2e consumers do not see schema flip on the flag.
                let mut mapped: Vec<Value> = hits
                    .into_iter()
                    .map(|r| {
                        let mut v = serde_json::to_value(&r).unwrap_or_default();
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("at".into(), json!(format!("{}:{}", r.path, r.line)));
                        }
                        v
                    })
                    .collect();
                if with_macro {
                    if let Some(side) = indexer.open_macro_store()? {
                        for r in side.callers_filtered(sym, limit, filter)? {
                            mapped.push(tag_macro_ref_json(&r));
                        }
                    }
                }
                Ok(ok_text(serde_json::to_string_pretty(&mapped)?))
            }
            "impact" => {
                let sym = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                let sound = args.get("sound").and_then(|v| v.as_bool()).unwrap_or(false);
                let exact_only = args
                    .get("exact_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let include_dynamic = args
                    .get("include_dynamic")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let recall = args
                    .get("recall")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let with_macro = args
                    .get("with_macro")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                if sound && with_macro {
                    return Err(anyhow::anyhow!(
                        "sound is mutually exclusive with with_macro \
                         (macro sidecar is not sound-certified)"
                    ));
                }
                if sound {
                    if exact_only || include_dynamic || recall {
                        return Err(anyhow::anyhow!(
                            "sound is mutually exclusive with exact_only / include_dynamic / recall"
                        ));
                    }
                    let (hits, violations) = store.impact_sound(sym, depth, limit)?;
                    let subset_ok = violations.is_empty();
                    let languages = store.stats(&root.to_string_lossy())?.languages;
                    let (promise_tier, promise) = select_sound_promise(subset_ok, &languages);
                    let payload = serde_json::json!({
                        "mode": "sound",
                        "subset_ok": subset_ok,
                        "promise_tier": promise_tier.as_str(),
                        "promise": promise,
                        "promise_languages": languages,
                        "subset_violations": violations,
                        "impact": hits,
                    });
                    return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                }
                let filter = parse_query_flags(exact_only, include_dynamic, recall);
                let hits = Query::new(&store).impact_filtered(sym, depth, limit, filter)?;
                if with_macro {
                    let mut mapped: Vec<Value> = hits
                        .iter()
                        .map(|n| serde_json::to_value(n).unwrap_or_default())
                        .collect();
                    if let Some(side) = indexer.open_macro_store()? {
                        for n in side.impact_filtered(sym, depth, limit, filter)? {
                            mapped.push(tag_macro_impact_json(&n));
                        }
                    }
                    return Ok(ok_text(serde_json::to_string_pretty(&mapped)?));
                }
                Ok(ok_text(serde_json::to_string_pretty(&hits)?))
            }
            "macro_status" => {
                let indexer = Indexer::new(&root)?;
                let status = indexer.macro_status()?;
                Ok(ok_text(serde_json::to_string_pretty(&status)?))
            }
            "subset" => {
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let violations = store.subset_violations()?;
                let languages = store.stats(&root.to_string_lossy())?.languages;
                let (promise_tier, promise) =
                    select_sound_promise(violations.is_empty(), &languages);
                let payload = serde_json::json!({
                    "in_subset": violations.is_empty(),
                    "violation_count": violations.len(),
                    "violations": violations,
                    "promise_tier": promise_tier.as_str(),
                    "promise": promise,
                    "promise_languages": languages,
                });
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
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
                let raw = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("path required"))?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                // Accept abs paths under root, `./rel`, and Windows backslashes.
                let lookup = crate::index::parser::rel_path_under_root(
                    std::path::Path::new(raw),
                    &indexer.root,
                )
                .unwrap_or_else(|| raw.replace('\\', "/"));
                let hits = store.importers_of_file(&lookup, limit)?;
                Ok(ok_text(serde_json::to_string_pretty(&hits)?))
            }
            "enrich" => {
                // Default must match CLI Enrich --limit (50) — schema drift trap.
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
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
