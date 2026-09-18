//! High-level agent query recipes (priority-3): `blast_radius` + `who_calls`.
//!
//! These wrap existing store/query primitives into agent-friendly JSON payloads
//! that pick a confidence window automatically, separate implementor noise, and
//! always carry honesty fields (`window`, `subset_ok`, `promise_tier`,
//! `recommendation`, `note`).
//!
//! **Non-claims (honesty):** recipes never promise a complete runtime graph,
//! zero missed dynamic edges, ecosystem sound, or macro-complete graphs.
//! Prefer raw flags (`callers`/`impact` + `--sound`/`--exact-only`/…)
//! when you need full control; prefer these recipes when an agent needs a
//! default-safe answer plus a short recommendation.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{json, Value};

use crate::index::store::Store;
use crate::index::subset::select_sound_promise;
use crate::index::{union_impact, Indexer, UnionOptions};
use crate::model::{is_high_freq_name, ConfidenceFilter, MacroSidecarStatus, ReferenceRecord};
use crate::query::{build_callers_payload, CallersRoleMode};

/// Shared honesty note on every recipe payload (CLI + MCP).
pub const RECIPE_NOTE: &str =
    "not a complete runtime graph (indexed L0/L1 candidates; recipe chooses a window — it does not prove runtime completeness)";

/// Auto-window decision for `blast_radius`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastWindowDecision {
    /// `sound` | `default`
    pub window: &'static str,
    pub subset_ok: bool,
    pub use_sound: bool,
    pub recommendation: String,
}

/// Decide blast_radius confidence window from subset status.
///
/// - `subset_ok` → `impact --sound` walk (`window=sound`)
/// - else → default impact (Exact+Heuristic), **never** blind `--recall`
pub fn decide_blast_window(subset_ok: bool, root_label: Option<&str>) -> BlastWindowDecision {
    if subset_ok {
        BlastWindowDecision {
            window: "sound",
            subset_ok: true,
            use_sound: true,
            recommendation:
                "subset_ok: sound window over S-qualified edges; still review implementor/registration edge_role tags (not a complete runtime graph)"
                    .into(),
        }
    } else {
        let root_part = match root_label {
            Some(r) if !r.is_empty() => format!(" in root '{r}'"),
            _ => String::new(),
        };
        BlastWindowDecision {
            window: "default",
            subset_ok: false,
            use_sound: false,
            recommendation: format!(
                "sound disabled because unsafe/S-violated{root_part}; use default (Exact+Heuristic) + review implementors — not a complete runtime graph"
            ),
        }
    }
}

/// Gate `include_macro=true` on sidecar health.
///
/// Allowed only when status exists && !stale && !nested && expanded_root present,
/// and never under a sound window (sound && macro are mutually exclusive).
/// Returns `(allowed, Option<refusal_reason>)`.
pub fn decide_include_macro(
    requested: bool,
    status: Option<&MacroSidecarStatus>,
    sound_window: bool,
) -> (bool, Option<String>) {
    if !requested {
        return (false, None);
    }
    if sound_window {
        return (
            false,
            Some(
                "include_macro refused: sound window selected; macro sidecar is not sound-certified (mutually exclusive)"
                    .into(),
            ),
        );
    }
    let Some(st) = status else {
        return (
            false,
            Some("include_macro refused: macro sidecar status unavailable".into()),
        );
    };
    if !st.exists {
        return (
            false,
            Some(
                "include_macro refused: macro sidecar missing (run `agentgraph index --macro-expanded-root <expanded_tree>` first)"
                    .into(),
            ),
        );
    }
    if st.stale {
        return (
            false,
            Some(
                "include_macro refused: macro sidecar stale (main source fingerprint changed; run `agentgraph macro rebuild`)"
                    .into(),
            ),
        );
    }
    if st.expanded_root_nested {
        return (
            false,
            Some(
                "include_macro refused: expanded_root nests with --root (main-walker hazard; relocate expanded tree)"
                    .into(),
            ),
        );
    }
    if st.expanded_root_missing {
        return (
            false,
            Some(
                "include_macro refused: recorded expanded_root missing on disk (restore tree then `agentgraph macro rebuild`)"
                    .into(),
            ),
        );
    }
    (true, None)
}

/// Inputs for `build_blast_radius_payload` (keeps clippy arity green).
pub struct BlastRadiusPayloadInput {
    pub symbol: String,
    pub depth: usize,
    pub limit: usize,
    pub nodes: Vec<Value>,
    pub window: BlastWindowDecision,
    pub promise_tier: String,
    pub languages: Vec<String>,
    pub include_macro: bool,
    pub include_macro_reason: Option<String>,
    pub stale: Option<bool>,
}

/// Build the locked `blast_radius` JSON payload.
pub fn build_blast_radius_payload(input: BlastRadiusPayloadInput) -> Value {
    let BlastRadiusPayloadInput {
        symbol,
        depth,
        limit,
        nodes,
        window,
        promise_tier,
        languages,
        include_macro,
        include_macro_reason,
        stale,
    } = input;
    let mut v = json!({
        "tool": "blast_radius",
        "symbol": symbol,
        "depth": depth,
        "limit": limit,
        "window": window.window,
        "subset_ok": window.subset_ok,
        "promise_tier": promise_tier,
        "promise_languages": languages,
        "nodes": nodes,
        "include_macro": include_macro,
        "recommendation": window.recommendation,
        "note": RECIPE_NOTE,
    });
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "include_macro_reason".into(),
            match include_macro_reason {
                Some(r) => Value::String(r),
                None => Value::Null,
            },
        );
        obj.insert(
            "stale".into(),
            match stale {
                Some(s) => Value::Bool(s),
                None => Value::Null,
            },
        );
        // Alias for muscle-memory with raw `impact` tool.
        obj.insert(
            "impact".into(),
            obj.get("nodes").cloned().unwrap_or(Value::Null),
        );
    }
    v
}

/// Build the locked `who_calls` JSON payload.
///
/// Reuses `build_callers_payload` (noise governance). Always surfaces
/// `callers` + `implementors` sections + `high_freq_name` + recommendation.
pub fn build_who_calls_payload(
    symbol: &str,
    noisy: bool,
    limit: usize,
    hits: &[ReferenceRecord],
    subset_ok: bool,
    promise_tier: &str,
) -> Value {
    let high_freq = is_high_freq_name(symbol);
    let mode = if noisy {
        CallersRoleMode::IncludeImplementors
    } else {
        CallersRoleMode::Separate
    };
    let payload = build_callers_payload(symbol, hits.to_vec(), limit, mode);

    let mut callers: Vec<Value> = Vec::new();
    let mut implementors: Vec<Value> = Vec::new();
    let mut implementor_count = 0usize;
    let mut implementors_truncated = false;

    match &payload {
        Value::Array(rows) => {
            for row in rows {
                if row.get("edge_role").and_then(|r| r.as_str()) == Some("implementor") {
                    implementor_count += 1;
                }
            }
            if noisy {
                // Merged old-noisy shape: everything lives under callers.
                callers = rows.clone();
            } else {
                // Separate back-compat: plain array = zero implementors.
                callers = rows.clone();
            }
        }
        Value::Object(obj) => {
            if let Some(c) = obj.get("callers").and_then(|c| c.as_array()) {
                callers = c.clone();
            }
            if let Some(i) = obj.get("implementors").and_then(|i| i.as_array()) {
                implementors = i.clone();
            }
            implementor_count = obj
                .get("implementor_count")
                .and_then(|n| n.as_u64())
                .unwrap_or(implementors.len() as u64) as usize;
            implementors_truncated = obj
                .get("implementors_truncated")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
        }
        _ => {}
    }

    let recommendation = if noisy {
        format!(
            "noisy=true merges implementors into callers (old shape); prefer noisy=false to separate call sites — high_freq_name={high_freq}"
        )
    } else if implementor_count > 0 {
        format!(
            "implementors separated/collapsed from callers (count={implementor_count}{}); review callers first, then implementors — high_freq_name={high_freq}",
            if implementors_truncated {
                ", truncated"
            } else {
                ""
            }
        )
    } else {
        format!(
            "no implementor flood on this symbol; callers = Exact+Heuristic reference sites — high_freq_name={high_freq}"
        )
    };

    json!({
        "tool": "who_calls",
        "symbol": symbol,
        "noisy": noisy,
        "limit": limit,
        "window": "default",
        "subset_ok": subset_ok,
        "promise_tier": promise_tier,
        "high_freq_name": high_freq,
        "callers": callers,
        "implementors": implementors,
        "implementor_count": implementor_count,
        "implementors_truncated": implementors_truncated,
        "payload": payload,
        "recommendation": recommendation,
        "note": RECIPE_NOTE,
    })
}

/// Shared blast_radius args (CLI + MCP).
pub struct BlastRadiusArgs {
    pub symbol: String,
    pub depth: usize,
    pub limit: usize,
    pub include_macro: bool,
    pub root_id: Option<String>,
}

/// Shared who_calls args (CLI + MCP).
pub struct WhoCallsArgs {
    pub symbol: String,
    pub noisy: bool,
    pub limit: usize,
    pub root_id: Option<String>,
}

/// Macro sidecar is per-root. Workspace multi-root + include_macro without a
/// single root filter is refused (same policy as CLI `--with-macro`).
fn refuse_macro_workspace(
    store: &Store,
    include_macro: bool,
    root_filter: Option<&str>,
) -> Option<String> {
    if !include_macro {
        return None;
    }
    let is_ws = store.is_workspace().unwrap_or(false)
        || store
            .workspace_roots_meta()
            .map(|r| r.iter().any(|x| !x.id.is_empty()))
            .unwrap_or(false);
    if !is_ws {
        return None;
    }
    let roots = store.workspace_roots_meta().unwrap_or_default();
    let multi = roots.iter().filter(|r| !r.id.is_empty()).count() > 1;
    if multi && root_filter.is_none() {
        return Some(
            "include_macro refused: workspace multi-root requires a single root_id filter (macro sidecar is per-root)"
                .into(),
        );
    }
    None
}

/// Execute blast_radius against an open store + indexer.
pub fn run_blast_radius(
    store: &Store,
    indexer: &Indexer,
    root_label: &str,
    args: &BlastRadiusArgs,
) -> Result<Value> {
    store.ensure_indexed()?;
    let rf = args.root_id.as_deref();
    let violations = store.subset_violations_in(rf)?;
    let languages = store.stats(root_label)?.languages;
    let subset_ok = violations.is_empty();
    let (promise_tier, _promise) = select_sound_promise(subset_ok, &languages);
    let decision = decide_blast_window(subset_ok, rf);

    let mut include_macro = args.include_macro;
    let mut include_macro_reason: Option<String> = None;
    let mut stale: Option<bool> = None;

    if include_macro {
        if let Some(ws_reason) = refuse_macro_workspace(store, true, rf) {
            include_macro = false;
            include_macro_reason = Some(ws_reason);
        } else {
            let status = indexer.macro_status()?;
            stale = Some(status.stale);
            let (ok, reason) = decide_include_macro(true, Some(&status), decision.use_sound);
            if !ok {
                include_macro = false;
                include_macro_reason = reason;
            }
        }
    }

    let mut nodes: Vec<Value> = if decision.use_sound {
        let (hits, _viol) = store.impact_sound_in(&args.symbol, args.depth, args.limit, rf)?;
        hits.iter().map(|n| n.to_query_json()).collect()
    } else {
        let hits = store.impact_filtered_in(
            &args.symbol,
            args.depth,
            args.limit,
            ConfidenceFilter::Default,
            rf,
        )?;
        hits.iter().map(|n| n.to_query_json()).collect()
    };

    if include_macro {
        if let Some(side) = indexer.open_macro_store()? {
            let status = indexer.macro_status()?;
            let opts = UnionOptions {
                dedup: true,
                ignore_sidecar: false,
            };
            let (layout, _map, _present) = indexer.macro_crate_layout()?;
            let side_hits = side.impact_filtered(
                &args.symbol,
                args.depth,
                args.limit,
                ConfidenceFilter::Default,
            )?;
            let expanded_root = status
                .expanded_root
                .clone()
                .map(PathBuf::from)
                .unwrap_or_else(|| indexer.root.clone());
            let (rows, _stats) = union_impact(
                nodes,
                &side_hits,
                &expanded_root,
                &indexer.root,
                &layout,
                opts,
            );
            nodes = rows;
        }
    }

    Ok(build_blast_radius_payload(BlastRadiusPayloadInput {
        symbol: args.symbol.clone(),
        depth: args.depth,
        limit: args.limit,
        nodes,
        window: decision,
        promise_tier: promise_tier.as_str().to_string(),
        languages: languages.clone(),
        include_macro,
        include_macro_reason,
        stale,
    }))
}

/// Execute who_calls against an open store.
pub fn run_who_calls(store: &Store, root_label: &str, args: &WhoCallsArgs) -> Result<Value> {
    store.ensure_indexed()?;
    let rf = args.root_id.as_deref();
    let violations = store.subset_violations_in(rf)?;
    let languages = store.stats(root_label)?.languages;
    let subset_ok = violations.is_empty();
    let (promise_tier, _p) = select_sound_promise(subset_ok, &languages);
    let hits = store.callers_for_roles(&args.symbol, args.limit, ConfidenceFilter::Default, rf)?;
    Ok(build_who_calls_payload(
        &args.symbol,
        args.noisy,
        args.limit,
        &hits,
        subset_ok,
        promise_tier.as_str(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EdgeKind, Evidence};

    fn row(rule: Option<&str>) -> ReferenceRecord {
        ReferenceRecord {
            name: "x".into(),
            kind: EdgeKind::Call,
            path: "a.rs".into(),
            line: 1,
            enclosing: None,
            module: None,
            resolved: None,
            qualifier: None,
            confidence: crate::model::Confidence::Heuristic,
            evidence: rule.map(|r| Evidence {
                rule_id: r.into(),
                snippet: String::new(),
            }),
            root_id: String::new(),
        }
    }

    #[test]
    fn note_is_honest() {
        assert!(RECIPE_NOTE.contains("not a complete runtime graph"));
    }

    #[test]
    fn who_calls_separate_vs_noisy() {
        let hits = vec![row(Some("rs.di.impl_trait")), row(None)];
        let quiet = build_who_calls_payload("area", false, 50, &hits, true, "ast_modeled");
        assert_eq!(quiet["implementor_count"], 1);
        let noisy = build_who_calls_payload("area", true, 50, &hits, true, "ast_modeled");
        assert_eq!(noisy["noisy"], true);
        assert!(noisy["callers"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn blast_payload_builder_accepts_input_struct() {
        let v = build_blast_radius_payload(BlastRadiusPayloadInput {
            symbol: "x".into(),
            depth: 3,
            limit: 10,
            nodes: vec![],
            window: decide_blast_window(true, None),
            promise_tier: "ast_modeled".into(),
            languages: vec!["typescript".into()],
            include_macro: false,
            include_macro_reason: None,
            stale: None,
        });
        assert_eq!(v["window"], "sound");
        assert!(v["stale"].is_null());
    }
}
