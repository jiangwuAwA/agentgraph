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
                "name": "blast_radius",
                "description": "High-level blast-radius recipe (agent-friendly). Auto confidence window: sound when the selected store/root is subset_ok (S-qualified modeled edges); otherwise default Exact+Heuristic impact — never blind recall. Response always includes nodes (edge_role tagged), window (sound|default), promise_tier, subset_ok, sound_candidates[] (stable key; eligible roots first when window is default), stale (macro sidecar if relevant), include_macro + include_macro_reason, recommendation (short zh/en sentence; when window is default/disabled it names next legal scoped-sound commands e.g. impact <sym> --sound --workspace-root <id>, or honest no-eligible-root guidance — never blind --recall), note (not a complete runtime graph). include_macro=true is accepted only when macro sidecar exists && !stale && !nested; otherwise refused with a reason in the payload. P2-1: when include_macro is omitted, repo/project macro_default (file/env) may auto-enable if_fresh (reason repo_config_if_fresh); global default remains OFF. Explicit include_macro true/false wins over config. Optional workspace_db / root_id filter (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string", "description": "Query symbol name (alias: name)"},
                        "name": {"type": "string", "description": "Alias for symbol (CLI parity)"},
                        "depth": {"type": "integer", "default": 3},
                        "limit": {"type": "integer", "default": 100},
                        "include_macro": {"type": "boolean", "description": "Explicit include_macro. Omit to resolve from repo/project macro_default (P2-1; global default OFF). true/false wins over config. When true, only accepted when macro sidecar exists && !stale && !nested; refused with reason otherwise. Mutually exclusive with a sound window (not sound-certified)."},
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional; multi-root)"},
                        "root_id": {"type": "string", "description": "Filter rows to this workspace root_id (optional; default = union all roots)"}
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "graph",
                "description": "Self-contained HTML neighborhood graph for a symbol (CLI `agentgraph graph` parity — no shell-out). Returns JSON with `html` (offline, no CDN) plus honesty fields: window (sound|default|disabled), subset_ok, promise_tier, recommendation, note (not a complete runtime graph), node_count/edge_count, html_bytes, sha256. sound=true uses L2 S-qualified edges; when subset_ok=false the payload does **not** claim sound (honest disabled HTML still returned). sound + with_macro mutually exclusive (also exclusive with exact_only/include_dynamic). auto_window=true picks sound vs default like blast_radius (never blind recall). String-only by default; optional `out` writes HTML under the workspace root jail. include_recommendation default true. Optional workspace_db / root_id filter (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string", "description": "Query symbol name (alias: name)"},
                        "name": {"type": "string", "description": "Alias for symbol (CLI parity)"},
                        "depth": {"type": "integer", "default": 3, "description": "Impact BFS depth (default 3)"},
                        "direction": {"type": "string", "enum": ["impact", "callers", "both"], "default": "impact"},
                        "sound": {"type": "boolean", "default": false, "description": "L2 sound-eligible walk; disabled honesty UX when subset_ok=false. Mutually exclusive with with_macro / exact_only / include_dynamic."},
                        "with_macro": {"type": "boolean", "default": false, "description": "Union optional macro sidecar candidates (MACRO badge). Default off. Mutually exclusive with sound. Not sound-certified."},
                        "exact_only": {"type": "boolean", "default": false},
                        "include_dynamic": {"type": "boolean", "default": false},
                        "auto_window": {"type": "boolean", "default": false, "description": "Reuse blast_radius auto-window: sound when subset_ok else default — never blind recall."},
                        "include_recommendation": {"type": "boolean", "default": true},
                        "out": {"type": "string", "description": "Optional relative/absolute HTML path under the workspace root jail; omitted → string-only (path=null)"},
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional; multi-root)"},
                        "root_id": {"type": "string", "description": "Filter rows to this workspace root_id (optional; default = union all roots)"}
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "who_calls",
                "description": "High-level who-calls recipe (agent-friendly). noisy=false (default) uses the store callers payload builder: implementors separated/collapsed from call sites + high-frequency names demoted (cap implementors). noisy=true merges implementors into callers (old noisy shape). Always returns callers + implementors sections, edge_role tags, high_freq_name, promise_tier, subset_ok, recommendation, note (not a complete runtime graph). Window is default Exact+Heuristic — not a runtime call graph. Optional workspace_db / root_id filter (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string", "description": "Query symbol name (alias: name)"},
                        "name": {"type": "string", "description": "Alias for symbol (CLI parity)"},
                        "noisy": {"type": "boolean", "default": false, "description": "true = merge implementors into callers (old noisy shape); false = separate/collapse implementors"},
                        "limit": {"type": "integer", "default": 50},
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional; multi-root)"},
                        "root_id": {"type": "string", "description": "Filter rows to this workspace root_id (optional; default = union all roots)"}
                    },
                    "required": ["symbol"]
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
                "description": "List language-subset S violations stored at last index (L2). Empty list means --sound may emit its (weakened) eligibility promise; it is still not a runtime call-graph theorem. After watch/index_paths, violations reflect current disk (S re-cert on dirty files). P4: always includes sound_candidates[] (eligible first) + recommendation + by_root/by_top_dir for scoped --sound when global promise is disabled. P5: baseline_stale / sidecar_exists / sidecar_stale (never creates sidecar). Optional workspace_db / root_id filter (default off).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace_db": {"type": "string", "description": "Explicit shared workspace SQLite path (optional)"},
                        "root_id": {"type": "string", "description": "Filter violations to this workspace root_id (optional)"},
                        "by_root": {"type": "boolean", "default": false, "description": "Force by_root buckets (workspace stores always include them; single-root trees get by_top_dir instead)"}
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
                // P1-2: default stats payload includes honesty flags (cheap meta reads).
                let (indexer, _root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let mut stats = store.stats(&indexer.root.to_string_lossy())?;
                let (b, se, ss) = crate::index::cheap_honesty_flags(&store, &indexer.root)?;
                stats.baseline_stale = b;
                stats.sidecar_exists = se;
                stats.sidecar_stale = ss;
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
                    let mut payload = json!({
                        "mode": "sound",
                        "subset_ok": subset_ok,
                        "promise_tier": promise_tier.as_str(),
                        "promise": promise,
                        "promise_languages": languages,
                        "subset_violations": violations,
                        "callers": mapped,
                    });
                    // P1-2: sound callers object carries default honesty flags.
                    crate::index::insert_stale_flags(&mut payload, &store, &indexer.root)?;
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
            "blast_radius" => {
                let sym = args
                    .get("symbol")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("symbol (or name) required"))?;
                let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                let include_macro = args.get("include_macro").and_then(|v| v.as_bool());
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let recipe_args = crate::query::recipes::BlastRadiusArgs {
                    symbol: sym.to_string(),
                    depth,
                    limit,
                    include_macro,
                    root_id: root_filter.clone(),
                };
                let payload = crate::query::recipes::run_blast_radius(
                    &store,
                    &indexer,
                    &root.to_string_lossy(),
                    &recipe_args,
                )?;
                let payload = mcp_print_rows(payload, &store)?;
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
            }
            "graph" => {
                let sym = args
                    .get("symbol")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("symbol (or name) required"))?;
                let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
                let direction_s = args
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("impact");
                let direction =
                    crate::viz::GraphDirection::parse(direction_s).ok_or_else(|| {
                        anyhow::anyhow!(
                            "direction must be impact|callers|both (got '{direction_s}')"
                        )
                    })?;
                let sound = args.get("sound").and_then(|v| v.as_bool()).unwrap_or(false);
                // P2-1: tri-state — explicit with_macro/include_macro wins; absent → repo config.
                let with_macro_arg = args
                    .get("with_macro")
                    .or_else(|| args.get("include_macro"))
                    .and_then(|v| v.as_bool());
                let exact_only = args
                    .get("exact_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let include_dynamic = args
                    .get("include_dynamic")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let auto_window = args
                    .get("auto_window")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let include_recommendation = args
                    .get("include_recommendation")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let out = args
                    .get("out")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let gargs = crate::viz::GraphHtmlArgs {
                    symbol: sym.to_string(),
                    depth,
                    direction,
                    sound,
                    with_macro: with_macro_arg,
                    exact_only,
                    include_dynamic,
                    include_recommendation,
                    auto_window,
                    out,
                    root_id: root_filter.clone(),
                };
                let payload = crate::viz::run_graph_html(&store, &indexer, &root, &gargs)?;
                let payload = mcp_print_rows(payload, &store)?;
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
            }
            "who_calls" => {
                let sym = args
                    .get("symbol")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("symbol (or name) required"))?;
                let noisy = args.get("noisy").and_then(|v| v.as_bool()).unwrap_or(false);
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                let (indexer, root_filter) = open_mcp_store(&root, &args)?;
                let store = indexer.open_store()?;
                store.ensure_indexed()?;
                let recipe_args = crate::query::recipes::WhoCallsArgs {
                    symbol: sym.to_string(),
                    noisy,
                    limit,
                    root_id: root_filter.clone(),
                };
                let payload = crate::query::recipes::run_who_calls(
                    &store,
                    &root.to_string_lossy(),
                    &recipe_args,
                )?;
                let payload = mcp_print_rows(payload, &store)?;
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
                    let mut payload = serde_json::json!({
                        "mode": "sound",
                        "subset_ok": subset_ok,
                        "promise_tier": promise_tier.as_str(),
                        "promise": promise,
                        "promise_languages": languages,
                        "subset_violations": violations,
                        "impact": mapped,
                    });
                    // P1-2: sound impact object carries default honesty flags.
                    crate::index::insert_stale_flags(&mut payload, &store, &indexer.root)?;
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
                // P5 honesty flags already on MacroSidecarStatus (sidecar_* + baseline_stale).
                let mut payload = serde_json::to_value(&status)?;
                if let Some(obj) = payload.as_object_mut() {
                    let macro_cfg = crate::config::load_macro_default_config(&indexer.root);
                    obj.insert(
                        "macro_default".into(),
                        serde_json::json!(macro_cfg.policy.as_str()),
                    );
                    obj.insert(
                        "macro_default_source".into(),
                        serde_json::json!(macro_cfg.source_label()),
                    );
                    obj.insert(
                        "macro_default_requests_include".into(),
                        serde_json::json!(macro_cfg.requests_include()),
                    );
                }
                Ok(ok_text(serde_json::to_string_pretty(&payload)?))
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
                let force_by_root = args
                    .get("by_root")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let violations = store.subset_violations_in(rf)?;
                let languages = store.stats(&root.to_string_lossy())?.languages;
                let (promise_tier, promise) =
                    select_sound_promise(violations.is_empty(), &languages);
                // P4: scoped-sound helpers (same shape as CLI subset).
                let roots = store.root_status_rows().unwrap_or_default();
                let is_workspace = store.is_workspace().unwrap_or(false)
                    || roots.iter().any(|r| !r.id.is_empty())
                    || force_by_root;
                let agg = if is_workspace {
                    crate::index::subset::scoped_sound_by_root(&roots, &violations, &languages)
                } else {
                    let mut keys: Vec<(String, Option<String>)> = Vec::new();
                    if let Ok(dirs) = store.distinct_file_top_dirs(rf) {
                        for d in dirs {
                            keys.push((d, None));
                        }
                    }
                    for v in &violations {
                        let d = crate::index::subset::top_dir_of_path(&v.path);
                        let key = if d.is_empty() { "(root)".into() } else { d };
                        if !keys.iter().any(|(k, _)| *k == key) {
                            keys.push((key, None));
                        }
                    }
                    crate::index::subset::scoped_sound_by_top_dir(&keys, &violations, &languages)
                };
                let agg_payload = agg.to_payload_json();
                let baseline_stale = crate::index::diff::baseline_stale_flag(&store);
                let mut sidecar_roots: Vec<std::path::PathBuf> = roots
                    .iter()
                    .filter(|r| !r.path.is_empty())
                    .map(|r| std::path::PathBuf::from(&r.path))
                    .collect();
                if sidecar_roots.is_empty() {
                    sidecar_roots.push(indexer.root.clone());
                }
                let (sidecar_exists, sidecar_stale) =
                    crate::index::cheap_sidecar_flags_multi(&sidecar_roots);
                let mut payload = serde_json::json!({
                    "in_subset": violations.is_empty(),
                    "violation_count": violations.len(),
                    "violations": violations,
                    "promise_tier": promise_tier.as_str(),
                    "promise": promise,
                    "promise_languages": languages,
                    "root_id": root_filter,
                    "by_root": agg_payload.get("by_root").cloned().unwrap_or(serde_json::Value::Null),
                    "by_top_dir": agg_payload.get("by_top_dir").cloned().unwrap_or(serde_json::Value::Null),
                    "sound_candidates": agg_payload.get("sound_candidates").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "recommendation": agg_payload.get("recommendation").cloned().unwrap_or_else(|| serde_json::json!(agg.recommendation.clone())),
                    "baseline_stale": baseline_stale,
                    "sidecar_exists": sidecar_exists,
                    "sidecar_stale": sidecar_stale,
                    "note": "sound_candidates are scoped --sound hints (eligible first); global workspace --sound = weakest root",
                });
                let by_root_empty = payload
                    .get("by_root")
                    .and_then(|v| v.as_array())
                    .map(|a| a.is_empty())
                    .unwrap_or(false);
                if by_root_empty {
                    if let Some(obj) = payload.as_object_mut() {
                        // Keep by_root null when empty for non-workspace clarity.
                        obj.insert("by_root".into(), serde_json::Value::Null);
                    }
                }
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
                if d.baseline_stale {
                    eprintln!(
                        "graph_diff: baseline_stale=true — dirty reindex after last full snapshot \
                         (baseline not auto-refreshed)"
                    );
                }
                let mut payload = mcp_print_rows(serde_json::to_value(&d)?, &store)?;
                // P1-2: ensure sidecar flags present even if EdgeDiff serialization lags.
                if payload.get("sidecar_exists").is_none() {
                    crate::index::insert_stale_flags(&mut payload, &store, &indexer.root)?;
                }
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
