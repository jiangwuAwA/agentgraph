//! High-level agent query recipes (priority-3): `blast_radius` + `who_calls`.
//!
//! These wrap existing store/query primitives into agent-friendly JSON payloads
//! that pick a confidence window automatically, separate implementor noise, and
//! always carry honesty fields (`window`, `subset_ok`, `promise_tier`,
//! `recommendation`, `note`).
//!
//! **P0-4:** when auto-window is default/disabled, `recommendation` also names
//! next legal commands — scoped `sound_candidates` (eligible roots/dirs first)
//! plus example `impact <sym> --sound --workspace-root <id>` — or an honest
//! "no eligible root; use default + review implementors" (never blind
//! `--recall` as the default). CLI + MCP share this payload builder.
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
use crate::index::subset::{
    scoped_sound_by_root, scoped_sound_by_top_dir, select_sound_promise, top_dir_of_path,
    SoundAggregation, SoundScopeKind, SubsetViolation,
};
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

/// P0-4 machine-readable next-step guidance for default/disabled windows.
///
/// Stable payload keys (勿改名): `sound_candidates`, `example_command`,
/// `by_root` / `by_top_dir`, `baseline_stale`, `sidecar_stale`.
#[derive(Debug, Clone, Default)]
pub struct ScopedSoundGuidance {
    /// Candidates from `aggregate_scoped_sound` / subset helpers (eligible first).
    pub sound_candidates: Vec<Value>,
    /// Workspace-root buckets when scope is Root.
    pub by_root: Option<Value>,
    /// Top-level dir buckets when scope is TopDir (single-root path hint).
    pub by_top_dir: Option<Value>,
    /// Example legal command when an eligible root exists.
    pub example_command: Option<String>,
    /// True when at least one candidate is sound-eligible.
    pub has_eligible: bool,
    /// Honesty flags (P5 / P1-2) — mentioned in recommendation when true.
    pub baseline_stale: bool,
    pub sidecar_stale: bool,
    /// P1-2: cheap sidecar existence (default payload key stability).
    pub sidecar_exists: bool,
    /// One-liner to append to `recommendation` when window is default/disabled.
    pub guidance: String,
}

/// Aggregate scoped-sound candidates for blast_radius guidance (P0-4).
///
/// Workspace multi-root → `scoped_sound_by_root` over **all** roots (so a dirty
/// union still surfaces clean sibling roots). Single-root store →
/// `scoped_sound_by_top_dir` path hints.
pub fn scoped_sound_aggregation(
    store: &Store,
    root_filter: Option<&str>,
    languages: &[String],
    violations: &[SubsetViolation],
) -> SoundAggregation {
    let roots = store.root_status_rows().unwrap_or_default();
    let is_workspace =
        store.is_workspace().unwrap_or(false) || roots.iter().any(|r| !r.id.is_empty());
    if is_workspace {
        // Always aggregate all roots: union guidance needs clean siblings even
        // when this call selected one root or is a dirty union.
        let all_violations = store
            .subset_violations()
            .unwrap_or_else(|_| violations.to_vec());
        return scoped_sound_by_root(&roots, &all_violations, languages);
    }
    let mut keys: Vec<(String, Option<String>)> = Vec::new();
    if let Ok(dirs) = store.distinct_file_top_dirs(root_filter) {
        for d in dirs {
            keys.push((d, None));
        }
    }
    for v in violations {
        let d = top_dir_of_path(&v.path);
        let key = if d.is_empty() {
            "(root)".to_string()
        } else {
            d
        };
        if !keys.iter().any(|(k, _)| *k == key) {
            keys.push((key, None));
        }
    }
    scoped_sound_by_top_dir(&keys, violations, languages)
}

/// Build next-legal-command text + payload fields when window is default/disabled.
///
/// Honesty rules:
/// - Eligible roots first; example `impact <sym> --sound --workspace-root <id>`
/// - Single-root: `by_top_dir` path hint only (`--sound` stays store-wide)
/// - No eligible root: say so; suggest default + review implementors
/// - **Never** suggest blind `--recall` as the default next step
/// - Dirty union is **never** labeled `window=sound`
pub fn build_scoped_sound_guidance(
    symbol: &str,
    agg: &SoundAggregation,
    baseline_stale: bool,
    sidecar_stale: bool,
) -> ScopedSoundGuidance {
    let sound_candidates: Vec<Value> = agg
        .sound_candidates
        .iter()
        .map(|c| c.to_payload_json())
        .collect();
    let eligible: Vec<&crate::index::subset::SoundCandidate> = agg
        .sound_candidates
        .iter()
        .filter(|c| c.sound_eligible)
        .collect();

    let buckets_json = json!(agg
        .buckets
        .iter()
        .map(|b| b.to_payload_json())
        .collect::<Vec<_>>());
    let (by_root, by_top_dir) = match agg.scope {
        SoundScopeKind::Root => (Some(buckets_json), None),
        SoundScopeKind::TopDir => (None, Some(buckets_json)),
    };

    let mut example_command = None;
    let mut parts: Vec<String> = Vec::new();

    if !eligible.is_empty() {
        let keys: Vec<&str> = eligible.iter().map(|c| c.key.as_str()).collect();
        let ineligible: Vec<&str> = agg
            .sound_candidates
            .iter()
            .filter(|c| !c.sound_eligible)
            .map(|c| c.key.as_str())
            .collect();
        let avoid_part = if ineligible.is_empty() {
            String::new()
        } else {
            format!("; avoid {}", ineligible.join(", "))
        };
        match agg.scope {
            SoundScopeKind::Root => {
                let first = keys[0];
                let cmd = format!("impact {symbol} --sound --workspace-root {first}");
                example_command = Some(cmd.clone());
                parts.push(format!(
                    "scoped sound candidates (eligible first): {}{avoid_part}; e.g. `{cmd}` — do not claim union --sound on dirty roots",
                    keys.join(", ")
                ));
            }
            SoundScopeKind::TopDir => {
                parts.push(format!(
                    "by_top_dir sound-eligible path hint (single-root: --sound is still store-wide; \
                     re-index a clean path/parent for a scoped store): {}{avoid_part}",
                    keys.join(", ")
                ));
                parts.push(format!(
                    "next legal step: default blast_radius / impact (Exact+Heuristic) + review implementors; \
                     for a scoped sound trial re-index a clean top dir then `impact {symbol} --sound` on that store"
                ));
            }
        }
    } else {
        let label = match agg.scope {
            SoundScopeKind::Root => "workspace root",
            SoundScopeKind::TopDir => "top-level dir",
        };
        parts.push(format!(
            "no sound-eligible {label} — every scope has S violations"
        ));
        parts.push(
            "next legal step: default blast_radius / impact (Exact+Heuristic) + review implementors — \
             do NOT pass blind --recall as the default window"
                .to_string(),
        );
    }

    if baseline_stale {
        parts.push(
            "baseline_stale=true — dirty reindex after last full snapshot; run `agentgraph index` \
             to refresh baseline (never auto-refreshed)"
                .into(),
        );
    }
    if sidecar_stale {
        parts.push(
            "sidecar_stale=true — macro sidecar fingerprint stale; run `agentgraph macro rebuild` \
             if you use --with-macro (sidecar edges are not sound-certified)"
                .into(),
        );
    }

    ScopedSoundGuidance {
        sound_candidates,
        by_root,
        by_top_dir,
        example_command,
        has_eligible: !eligible.is_empty(),
        baseline_stale,
        sidecar_stale,
        // Overwritten by run_blast_radius with a live cheap sidecar_exists read.
        sidecar_exists: false,
        guidance: parts.join("; "),
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
    /// P0-4 scoped-sound next-step guidance (empty on sound window).
    pub scoped_sound: ScopedSoundGuidance,
}

/// Build the locked `blast_radius` JSON payload.
///
/// Stable keys (勿改名): `window`, `promise_tier`, `subset_ok`,
/// `recommendation`, `note`, `sound_candidates`.
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
        scoped_sound,
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
        // Always present (stable key); empty when window=sound or no store scopes.
        "sound_candidates": scoped_sound.sound_candidates,
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
        // P0-4 optional scoped-sound fields (present when applicable / when true).
        if let Some(cmd) = &scoped_sound.example_command {
            obj.insert("example_command".into(), Value::String(cmd.clone()));
        } else {
            obj.insert("example_command".into(), Value::Null);
        }
        if let Some(b) = &scoped_sound.by_root {
            obj.insert("by_root".into(), b.clone());
        }
        if let Some(b) = &scoped_sound.by_top_dir {
            obj.insert("by_top_dir".into(), b.clone());
        }
        // Honesty flags: always emit booleans when guidance was computed
        // (sound window still attaches cheap flags for key stability).
        obj.insert(
            "baseline_stale".into(),
            Value::Bool(scoped_sound.baseline_stale),
        );
        obj.insert(
            "sidecar_stale".into(),
            Value::Bool(scoped_sound.sidecar_stale),
        );
        // P1-2: default payload always carries sidecar_exists (cheap meta read).
        obj.insert(
            "sidecar_exists".into(),
            Value::Bool(scoped_sound.sidecar_exists),
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
        // P1-2 default honesty flags (overwritten by run_who_calls with live cheap reads).
        "baseline_stale": false,
        "sidecar_exists": false,
        "sidecar_stale": false,
    })
}

/// Shared blast_radius args (CLI + MCP).
///
/// P2-1: `include_macro` is tri-state:
/// - `Some(true)` — explicit `--include-macro` / MCP `include_macro: true`
/// - `Some(false)` — explicit `--no-include-macro` / MCP `include_macro: false`
/// - `None` — not explicit; resolve from repo/project `macro_default` config
pub struct BlastRadiusArgs {
    pub symbol: String,
    pub depth: usize,
    pub limit: usize,
    pub include_macro: Option<bool>,
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
pub fn refuse_macro_workspace(
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
///
/// CLI + MCP share this builder (P0-4). When auto-window is default/disabled,
/// the payload gains scoped-sound next-step guidance (`sound_candidates`,
/// example command / by_top_dir hint, stale honesty flags).
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
    let mut decision = decide_blast_window(subset_ok, rf);

    // P5 cheap honesty flags (never create sidecar / never refresh baseline).
    let baseline_stale = crate::index::diff::baseline_stale_flag(store);
    let mut sidecar_roots: Vec<PathBuf> = store
        .workspace_roots_meta()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !r.path.is_empty())
        .map(|r| PathBuf::from(r.path))
        .collect();
    if sidecar_roots.is_empty() {
        sidecar_roots.push(indexer.root.clone());
    }
    let (sidecar_exists, sidecar_stale) = crate::index::cheap_sidecar_flags_multi(&sidecar_roots);

    // P0-4: default/disabled window → next legal commands + scoped candidates.
    let scoped_sound = if !decision.use_sound {
        let agg = scoped_sound_aggregation(store, rf, &languages, &violations);
        let mut guidance =
            build_scoped_sound_guidance(&args.symbol, &agg, baseline_stale, sidecar_stale);
        guidance.sidecar_exists = sidecar_exists;
        decision.recommendation = format!("{}; {}", decision.recommendation, guidance.guidance);
        guidance
    } else {
        // Sound window: keep keys stable, no scoped-command spam.
        ScopedSoundGuidance {
            baseline_stale,
            sidecar_stale,
            sidecar_exists,
            ..ScopedSoundGuidance::default()
        }
    };

    // P2-1: CLI/MCP explicit wins; else repo/project macro_default may request.
    let mut cfg_roots: Vec<PathBuf> = vec![indexer.root.clone()];
    if let Ok(ws) = store.workspace_roots_meta() {
        // Prefer the filtered workspace root's config when a single root is selected.
        if let Some(rf_id) = rf {
            if let Some(r) = ws.iter().find(|r| r.id == rf_id) {
                if !r.path.is_empty() {
                    cfg_roots.insert(0, PathBuf::from(&r.path));
                }
            }
        }
        for r in ws.into_iter().filter(|r| !r.path.is_empty()) {
            let p = PathBuf::from(r.path);
            if !cfg_roots.contains(&p) {
                cfg_roots.push(p);
            }
        }
    }
    let macro_cfg = crate::config::load_macro_default_config_for_roots(&cfg_roots);
    let (request_macro, cfg_success_reason) =
        crate::config::resolve_macro_include_request(args.include_macro, &macro_cfg);

    let mut include_macro = request_macro;
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
            } else {
                // Allowed: CLI explicit keeps reason null; repo config tags source.
                include_macro_reason = cfg_success_reason;
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
        scoped_sound,
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
    let mut payload = build_who_calls_payload(
        &args.symbol,
        args.noisy,
        args.limit,
        &hits,
        subset_ok,
        promise_tier.as_str(),
    );
    // P1-2: default who_calls payload carries live honesty flags (cheap meta).
    crate::index::insert_stale_flags(&mut payload, store, std::path::Path::new(root_label))?;
    Ok(payload)
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
            scoped_sound: ScopedSoundGuidance::default(),
        });
        assert_eq!(v["window"], "sound");
        assert!(v["stale"].is_null());
        assert!(v.get("sound_candidates").is_some());
        assert!(v["sound_candidates"].as_array().unwrap().is_empty());
        assert!(v["recommendation"].as_str().unwrap().contains("subset_ok"));
    }
}
