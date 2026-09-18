//! Minimal MCP (Model Context Protocol) stdio server.
//! Implements initialize / tools/list / tools/call / ping over newline-delimited JSON-RPC 2.0.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::index::{union_callers, union_impact, Indexer, UnionOptions};
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
                "description": "List call sites / references of a symbol. Default includes Exact + Heuristic (L1 DI/factory). Use exact_only=true to drop Heuristic; include_dynamic=true to also return DynamicCandidate. sound=true uses the L2 sound-eligible edge set (mutually exclusive with exact_only/include_dynamic) and reports S-violations. with_macro unions optional sidecar hits (origin=macro_expanded, paths mapped when possible); de-dup ON by default; exact_only+with_macro ignores the sidecar; not sound.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "limit": {"type": "integer", "default": 50},
                        "exact_only": {"type": "boolean", "default": false},
                        "include_dynamic": {"type": "boolean", "default": false},
                        "recall": {"type": "boolean", "default": false, "description": "Prefer recall over a clean graph (alias for include_dynamic)"},
                        "sound": {"type": "boolean", "default": false},
                        "with_macro": {"type": "boolean", "default": false, "description": "Union optional macro-expanded sidecar hits (origin=macro_expanded). Default off. Paths mapped to source when possible; de-dup ON (same name+enclosing+mapped_path as main Exact/Heuristic drops sidecar row). exact_only ignores sidecar. Mutually exclusive with sound. limit applies per store; without de-dup union may return ~2N rows. Not sound-certified."},
                        "no_macro_dedup": {"type": "boolean", "default": false, "description": "Debug: keep duplicate sidecar rows under with_macro (default de-dup ON)."}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "impact",
                "description": "Multi-hop blast radius: who transitively depends on this symbol (call graph BFS). Default Exact + Heuristic; recall/include_dynamic widen for missed-edge safety; sound=true uses L2 S-qualified edges. with_macro unions optional sidecar (mapped path + de-dup ON; not sound).",
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
                        "with_macro": {"type": "boolean", "default": false, "description": "Union optional macro-expanded sidecar hits (origin=macro_expanded). Default off. Mapped paths + de-dup ON. exact_only ignores sidecar. Mutually exclusive with sound. limit applies per store; without de-dup union may return ~2N rows. Not sound-certified."},
                        "no_macro_dedup": {"type": "boolean", "default": false, "description": "Debug: keep duplicate sidecar rows under with_macro (default de-dup ON)."}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "macro_status",
                "description": "Optional macro-expanded sidecar status (P2/M1): exists, path, counts, origin, expanded_root(_missing/_nested), subset_violation_count, stale (source fingerprint), path_map_present/path_map, dedup_stats, rebuild_policy. Default product path does not use this sidecar. Not sound.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "macro_rebuild",
                "description": "Re-index the recorded expanded_root into the macro sidecar (idempotent; Track M1). Does not run cargo-expand. Fails when expanded_root is missing/nested. Not sound.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "force": {"type": "boolean", "default": true, "description": "Force full re-parse of the expanded tree"}
                    }
                }
            },
            {
                "name": "subset",
                "description": "List language-subset S violations stored at last index (L2). Empty list means --sound may emit its (weakened) eligibility promise; it is still not a runtime call-graph theorem. After watch/index_paths, violations reflect current disk (S re-cert on dirty files).",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "graph_diff",
                "description": "Indexed-edge set difference vs the snapshot baseline written at index time (Track M4). added/removed ref rows (name+path+line+confidence+enclosing). Honesty: indexed edges only; NOT a runtime call-graph diff. Fails when no baseline exists (run index first). exact_only filters to Exact edges; write_snapshot promotes current live refs as the new baseline.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "exact_only": {"type": "boolean", "default": false, "description": "Only Exact (L0) edges in the set difference"},
                        "limit": {"type": "integer", "description": "Cap rows per side (summary stays full)"},
                        "snapshot": {"type": "string", "description": "Explicit snapshot JSON path (optional)"},
                        "write_snapshot": {"type": "boolean", "default": false, "description": "Promote current live refs as baseline after the diff"}
                    }
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
                let no_macro_dedup = args
                    .get("no_macro_dedup")
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
                // M1: exact_only + with_macro ignores sidecar (plain array).
                if with_macro && exact_only {
                    return Ok(ok_text(serde_json::to_string_pretty(&mapped)?));
                }
                if with_macro {
                    if let Some(side) = indexer.open_macro_store()? {
                        let opts = UnionOptions {
                            dedup: !no_macro_dedup,
                            ignore_sidecar: false,
                        };
                        let (layout, _map, path_map_present) = indexer.macro_crate_layout()?;
                        let status = indexer.macro_status()?;
                        if status.stale {
                            eprintln!(
                                "warn: macro sidecar is stale; unioning existing rows — run macro rebuild"
                            );
                        }
                        let side_hits = side.callers_filtered(sym, limit, filter)?;
                        let expanded_root = status
                            .expanded_root
                            .clone()
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| root.clone());
                        let (rows, stats) = union_callers(
                            std::mem::take(&mut mapped),
                            &side_hits,
                            &expanded_root,
                            &root,
                            &layout,
                            opts,
                        );
                        let _ = indexer.store_dedup_stats(&stats);
                        let payload = json!({
                            "callers": rows,
                            "sidecar_present": true,
                            "stale": status.stale,
                            "origin": "macro_expanded",
                            "path_map_present": path_map_present,
                            "dedup_stats": stats,
                            "note": "sidecar union is optional candidates (not sound); de-dup ON unless no_macro_dedup",
                        });
                        return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
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
                let no_macro_dedup = args
                    .get("no_macro_dedup")
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
                    // Always emit `at` on impact rows (caller symmetry).
                    let mapped: Vec<Value> = hits
                        .into_iter()
                        .map(|n| {
                            let mut v = serde_json::to_value(&n).unwrap_or_default();
                            if let Some(obj) = v.as_object_mut() {
                                obj.insert("at".into(), json!(format!("{}:{}", n.path, n.line)));
                            }
                            v
                        })
                        .collect();
                    let payload = serde_json::json!({
                        "mode": "sound",
                        "subset_ok": subset_ok,
                        "promise_tier": promise_tier.as_str(),
                        "promise": promise,
                        "promise_languages": languages,
                        "subset_violations": violations,
                        "impact": mapped,
                    });
                    return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                }
                let filter = parse_query_flags(exact_only, include_dynamic, recall);
                let hits = Query::new(&store).impact_filtered(sym, depth, limit, filter)?;
                // Always emit `at` on impact rows (with or without with_macro) so
                // e2e consumers do not see schema flip on the flag — mirrors callers.
                let mut mapped: Vec<Value> = hits
                    .iter()
                    .map(|n| {
                        let mut v = serde_json::to_value(n).unwrap_or_default();
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("at".into(), json!(format!("{}:{}", n.path, n.line)));
                        }
                        v
                    })
                    .collect();
                if with_macro && exact_only {
                    return Ok(ok_text(serde_json::to_string_pretty(&mapped)?));
                }
                if with_macro {
                    if let Some(side) = indexer.open_macro_store()? {
                        let opts = UnionOptions {
                            dedup: !no_macro_dedup,
                            ignore_sidecar: false,
                        };
                        let (layout, _map, path_map_present) = indexer.macro_crate_layout()?;
                        let status = indexer.macro_status()?;
                        if status.stale {
                            eprintln!(
                                "warn: macro sidecar is stale; unioning existing rows — run macro rebuild"
                            );
                        }
                        let side_hits = side.impact_filtered(sym, depth, limit, filter)?;
                        let expanded_root = status
                            .expanded_root
                            .clone()
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| root.clone());
                        let (rows, stats) = union_impact(
                            std::mem::take(&mut mapped),
                            &side_hits,
                            &expanded_root,
                            &root,
                            &layout,
                            opts,
                        );
                        let _ = indexer.store_dedup_stats(&stats);
                        let payload = json!({
                            "impact": rows,
                            "sidecar_present": true,
                            "stale": status.stale,
                            "origin": "macro_expanded",
                            "path_map_present": path_map_present,
                            "dedup_stats": stats,
                            "note": "sidecar union is optional candidates (not sound); de-dup ON unless no_macro_dedup",
                        });
                        return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                    }
                }
                Ok(ok_text(serde_json::to_string_pretty(&mapped)?))
            }
            "macro_status" => {
                let indexer = Indexer::new(&root)?;
                let status = indexer.macro_status()?;
                Ok(ok_text(serde_json::to_string_pretty(&status)?))
            }
            "macro_rebuild" => {
                let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(true);
                let indexer = Indexer::new(&root)?;
                let result = indexer.macro_rebuild(force)?;
                let status = indexer.macro_status()?;
                let payload = json!({
                    "macro_sidecar": result,
                    "status": status,
                    "note": "sidecar rebuilt from recorded expanded_root (idempotent; not sound)",
                });
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
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
            "graph_diff" => {
                let exact_only = args
                    .get("exact_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);
                let write_snapshot = args
                    .get("write_snapshot")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let snap_arg = args
                    .get("snapshot")
                    .and_then(|v| v.as_str())
                    .map(std::path::PathBuf::from);
                let indexer = Indexer::new(&root)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let d = crate::index::diff::run_diff(
                    &indexer.root,
                    &store,
                    exact_only,
                    limit,
                    snap_arg.as_deref(),
                )?;
                if write_snapshot {
                    let snap = crate::index::diff::write_baseline_snapshot(&indexer.root, &store)?;
                    eprintln!(
                        "graph_diff: wrote baseline snapshot ({} edges)",
                        snap.edges.len()
                    );
                }
                Ok(ok_text(serde_json::to_string_pretty(&d)?))
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
