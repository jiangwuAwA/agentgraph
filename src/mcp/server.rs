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
                "description": "Find symbol definitions by exact or fuzzy name. Returns path, line range, kind. Optional workspace_db / root_id filter for multi-root stores (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Symbol name or qualified name"},
                        "limit": {"type": "integer", "default": 20},
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional; multi-root)"},
                        "root_id": {"type": "string", "description": "Filter rows to this workspace root_id (optional; default = union all roots)"}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "callers",
                "description": "List call sites / references of a symbol. Default includes Exact + Heuristic (L1 DI/factory). Noise governance: when implementor edges (trait/interface impls) are present the payload is an object {callers, implementors, implementor_count, implementors_truncated, truncated, note}; plain array when zero implementors. Rows carry edge_role (call|implementor|registration|dynamic). include_implementors=true merges all roles (old noisy shape); implementors_only=true returns implementors only; exact_only=true stays pure Exact calls. sound=true uses the L2 sound-eligible edge set (mutually exclusive with exact_only/include_dynamic/include_implementors/implementors_only). with_macro unions optional sidecar hits; de-dup ON by default; not sound. Optional workspace_db / root_id filter (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "limit": {"type": "integer", "default": 50},
                        "exact_only": {"type": "boolean", "default": false},
                        "include_dynamic": {"type": "boolean", "default": false},
                        "recall": {"type": "boolean", "default": false, "description": "Prefer recall over a clean graph (alias for include_dynamic)"},
                        "sound": {"type": "boolean", "default": false},
                        "include_implementors": {"type": "boolean", "default": false, "description": "Merge implementor edges into the callers array (old noisy default). Mutually exclusive with implementors_only."},
                        "implementors_only": {"type": "boolean", "default": false, "description": "Return only implementor edges. Mutually exclusive with include_implementors."},
                        "with_macro": {"type": "boolean", "default": false, "description": "Union optional macro-expanded sidecar hits (origin=macro_expanded). Default off. Paths mapped to source when possible; de-dup ON (same name+enclosing+mapped_path as main Exact/Heuristic drops sidecar row). exact_only ignores sidecar. Mutually exclusive with sound. limit applies per store; without de-dup union may return ~2N rows. Not sound-certified."},
                        "no_macro_dedup": {"type": "boolean", "default": false, "description": "Debug: keep duplicate sidecar rows under with_macro (default de-dup ON)."},
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional; multi-root)"},
                        "root_id": {"type": "string", "description": "Filter rows to this workspace root_id (optional; default = union all roots)"}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "impact",
                "description": "Multi-hop blast radius: who transitively depends on this symbol (call graph BFS). Default Exact + Heuristic; recall/include_dynamic widen for missed-edge safety; sound=true uses L2 S-qualified edges. Rows carry edge_role (call|implementor|registration|dynamic); implementor edges still expand (blast radius) but are tagged. with_macro unions optional sidecar (mapped path + de-dup ON; not sound). Optional workspace_db / root_id filter (default off).",
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
                        "no_macro_dedup": {"type": "boolean", "default": false, "description": "Debug: keep duplicate sidecar rows under with_macro (default de-dup ON)."},
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional; multi-root)"},
                        "root_id": {"type": "string", "description": "Filter rows to this workspace root_id (optional; default = union all roots)"}
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
                "description": "List language-subset S violations stored at last index (L2). Empty list means --sound may emit its (weakened) eligibility promise; it is still not a runtime call-graph theorem. After watch/index_paths, violations reflect current disk (S re-cert on dirty files). Optional workspace_db / root_id filter (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional)"},
                        "root_id": {"type": "string", "description": "Filter violations to this workspace root_id (optional)"}
                    }
                }
            },
            {
                "name": "graph_diff",
                "description": "Indexed-edge set difference vs the snapshot baseline written at index time (Track M4). added/removed ref rows (name+path+line+confidence+enclosing+root_id). Honesty: indexed edges only; NOT a runtime call-graph diff. Fails when no baseline exists (run index first). exact_only filters to Exact edges; write_snapshot promotes current live refs as the new baseline. Optional workspace_db / root_id filter (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "exact_only": {"type": "boolean", "default": false, "description": "Only Exact (L0) edges in the set difference"},
                        "limit": {"type": "integer", "description": "Cap rows per side (summary stays full)"},
                        "snapshot": {"type": "string", "description": "Explicit snapshot JSON path (optional)"},
                        "write_snapshot": {"type": "boolean", "default": false, "description": "Promote current live refs as baseline after the diff"},
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional)"},
                        "root_id": {"type": "string", "description": "Filter edges to this workspace root_id (optional)"}
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
                "name": "workspace_status",
                "description": "Multi-root workspace status (Track M4-W): db_path, per-root counts (files/symbols/refs/subset_violations), root_id list. Optional workspace_db / workspace_root args. Single SQLite store + root_id — not N separate connections.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional)"},
                        "workspace_root": {"type": "string", "description": "Project root used to resolve the default workspace DB (optional)"}
                    }
                }
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

/// Optional workspace store handle: `workspace_db` path + `root_id` filter.
/// Default off — empty filter means union all roots; absent db means classic root store.
fn open_mcp_store(
    root: &PathBuf,
    args: &Value,
) -> anyhow::Result<(crate::index::Indexer, Option<String>)> {
    let mut indexer = Indexer::new(root)?;
    let mut root_filter: Option<String> = None;
    if let Some(db) = args.get("workspace_db").and_then(|v| v.as_str()) {
        indexer.db_path = PathBuf::from(db);
    }
    if let Some(rid) = args
        .get("root_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
    {
        // Accept either a recorded root_id or a workspace root path.
        let store = indexer.open_store()?;
        let ids = crate::index::workspace::resolve_filter_root_ids(&store, &[PathBuf::from(&rid)])?;
        root_filter = ids.into_iter().next();
    }
    Ok((indexer, root_filter))
}

fn mcp_print_rows(value: Value, store: &crate::index::store::Store) -> anyhow::Result<Value> {
    let mut v = value;
    let workspace = store.is_workspace().unwrap_or(false)
        || store
            .workspace_roots_meta()
            .map(|r| r.iter().any(|x| !x.id.is_empty()))
            .unwrap_or(false);
    crate::model::ensure_workspace_root_ids(&mut v, workspace);
    if workspace {
        if let Ok(roots) = store.workspace_roots_meta() {
            crate::model::inject_root_paths(&mut v, &roots);
        }
    }
    Ok(v)
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
            "workspace_status" => {
                let db = if let Some(p) = args.get("workspace_db").and_then(|v| v.as_str()) {
                    PathBuf::from(p)
                } else if let Some(r) = args.get("workspace_root").and_then(|v| v.as_str()) {
                    let raw = PathBuf::from(r);
                    let abs = if raw.is_absolute() {
                        raw
                    } else {
                        root.join(&raw)
                    };
                    abs.join(".agentgraph").join("index.db")
                } else {
                    root.join(".agentgraph").join("index.db")
                };
                let status = crate::index::workspace::workspace_status(&db)
                    .map_err(|e| anyhow::anyhow!("{e:#}"))?;
                Ok(ok_text(serde_json::to_string_pretty(&status)?))
            }
            "find_symbol" => {
                let sym = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let rf = root_filter.as_deref();
                let exact = store.find_symbol_exact_in(sym, limit, rf)?;
                let hits = if exact.is_empty() {
                    store.find_symbol_fuzzy_in(sym, limit, rf)?
                } else {
                    exact
                };
                let payload = mcp_print_rows(serde_json::to_value(&hits)?, &store)?;
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
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
                let include_implementors = args
                    .get("include_implementors")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let implementors_only = args
                    .get("implementors_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let rf = root_filter.as_deref();
                let role_mode = crate::query::parse_callers_role_mode(
                    exact_only,
                    include_implementors,
                    implementors_only,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                if sound && with_macro {
                    return Err(anyhow::anyhow!(
                        "sound is mutually exclusive with with_macro \
                         (macro sidecar is not sound-certified)"
                    ));
                }
                if with_macro {
                    let roots = store.workspace_roots_meta().unwrap_or_default();
                    let multi = roots.iter().filter(|r| !r.id.is_empty()).count() > 1;
                    if multi && rf.is_none() {
                        return Err(anyhow::anyhow!(
                            "with_macro + workspace multi-root requires a single root_id filter \
                             (macro sidecar is per-root)"
                        ));
                    }
                }
                if sound {
                    if exact_only
                        || include_dynamic
                        || recall
                        || include_implementors
                        || implementors_only
                    {
                        return Err(anyhow::anyhow!(
                            "sound is mutually exclusive with exact_only / include_dynamic / recall \
                             / include_implementors / implementors_only \
                             (sound walk uses its own eligibility filter)"
                        ));
                    }
                    let (hits, violations) = store.callers_sound_in(sym, limit, rf)?;
                    let subset_ok = violations.is_empty();
                    let languages = store.stats(&root.to_string_lossy())?.languages;
                    let (promise_tier, promise) = select_sound_promise(subset_ok, &languages);
                    let mapped: Vec<Value> = hits.iter().map(|r| r.to_query_json()).collect();
                    let payload = json!({
                        "mode": "sound",
                        "subset_ok": subset_ok,
                        "promise_tier": promise_tier.as_str(),
                        "promise": promise,
                        "promise_languages": languages,
                        "subset_violations": violations,
                        "callers": mapped,
                    });
                    let payload = mcp_print_rows(payload, &store)?;
                    return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                }
                let filter = parse_query_flags(exact_only, include_dynamic, recall);
                let hits = store.callers_for_roles(sym, limit, filter, rf)?;
                let role_payload =
                    crate::query::build_callers_payload(sym, hits.clone(), limit, role_mode);
                let mut mapped: Vec<Value> = hits.iter().map(|r| r.to_query_json()).collect();
                // M1: exact_only + with_macro ignores sidecar (plain array).
                if with_macro && exact_only {
                    let payload = mcp_print_rows(role_payload, &store)?;
                    return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
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
                            "note": "sidecar union is optional candidates (not sound); de-dup ON unless no_macro_dedup; main rows carry edge_role",
                        });
                        let payload = mcp_print_rows(payload, &store)?;
                        return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                    }
                }
                let payload = mcp_print_rows(role_payload, &store)?;
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
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
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let rf = root_filter.as_deref();
                if sound && with_macro {
                    return Err(anyhow::anyhow!(
                        "sound is mutually exclusive with with_macro \
                         (macro sidecar is not sound-certified)"
                    ));
                }
                if with_macro {
                    let roots = store.workspace_roots_meta().unwrap_or_default();
                    let multi = roots.iter().filter(|r| !r.id.is_empty()).count() > 1;
                    if multi && rf.is_none() {
                        return Err(anyhow::anyhow!(
                            "with_macro + workspace multi-root requires a single root_id filter \
                             (macro sidecar is per-root)"
                        ));
                    }
                }
                if sound {
                    if exact_only || include_dynamic || recall {
                        return Err(anyhow::anyhow!(
                            "sound is mutually exclusive with exact_only / include_dynamic / recall"
                        ));
                    }
                    let (hits, violations) = store.impact_sound_in(sym, depth, limit, rf)?;
                    let subset_ok = violations.is_empty();
                    let languages = store.stats(&root.to_string_lossy())?.languages;
                    let (promise_tier, promise) = select_sound_promise(subset_ok, &languages);
                    // Always emit `at` on impact rows (caller symmetry); edge_role tagged.
                    let mapped: Vec<Value> = hits.iter().map(|n| n.to_query_json()).collect();
                    let payload = serde_json::json!({
                        "mode": "sound",
                        "subset_ok": subset_ok,
                        "promise_tier": promise_tier.as_str(),
                        "promise": promise,
                        "promise_languages": languages,
                        "subset_violations": violations,
                        "impact": mapped,
                    });
                    let payload = mcp_print_rows(payload, &store)?;
                    return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                }
                let filter = parse_query_flags(exact_only, include_dynamic, recall);
                let hits = store.impact_filtered_in(sym, depth, limit, filter, rf)?;
                // Always emit `at` on impact rows (with or without with_macro) so
                // e2e consumers do not see schema flip on the flag — mirrors callers.
                let mut mapped: Vec<Value> = hits.iter().map(|n| n.to_query_json()).collect();
                if with_macro && exact_only {
                    let payload = mcp_print_rows(json!(mapped), &store)?;
                    return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
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
                        let payload = mcp_print_rows(payload, &store)?;
                        return Ok(ok_text(serde_json::to_string_pretty(&payload)?));
                    }
                }
                let payload = mcp_print_rows(json!(mapped), &store)?;
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
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
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let rf = root_filter.as_deref();
                let violations = store.subset_violations_in(rf)?;
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
                    "root_id": root_filter,
                });
                let payload = mcp_print_rows(payload, &store)?;
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
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let rid = root_filter.clone().unwrap_or_default();
                let d = crate::index::diff::run_diff_for_root(
                    &indexer.root,
                    &store,
                    exact_only,
                    limit,
                    snap_arg.as_deref(),
                    &rid,
                )?;
                if write_snapshot {
                    let snap = crate::index::diff::write_baseline_snapshot_for_root(
                        &indexer.root,
                        &store,
                        &rid,
                    )?;
                    eprintln!(
                        "graph_diff: wrote baseline snapshot ({} edges, root_id={})",
                        snap.edges.len(),
                        if rid.is_empty() {
                            "(all)"
                        } else {
                            rid.as_str()
                        }
                    );
                }
                let payload = mcp_print_rows(serde_json::to_value(&d)?, &store)?;
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
