use anyhow::Result;
use serde_json::{json, Value};

use crate::index::store::Store;
use crate::model::{
    is_high_freq_name, ConfidenceFilter, EdgeRole, ImpactNode, ReferenceRecord, SymbolRecord,
    HIGH_FREQ_IMPLEMENTOR_CAP,
};

pub struct Query<'a> {
    store: &'a Store,
}

impl<'a> Query<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub fn find_symbol(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        self.store.find_symbol(name, limit)
    }

    pub fn callers(&self, name: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
        self.store
            .callers_filtered(name, limit, ConfidenceFilter::Default)
    }

    pub fn callers_filtered(
        &self,
        name: &str,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ReferenceRecord>> {
        self.store.callers_filtered(name, limit, filter)
    }

    pub fn impact(&self, name: &str, depth: usize, limit: usize) -> Result<Vec<ImpactNode>> {
        self.store
            .impact_filtered(name, depth, limit, ConfidenceFilter::Default)
    }

    pub fn impact_filtered(
        &self,
        name: &str,
        depth: usize,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ImpactNode>> {
        self.store.impact_filtered(name, depth, limit, filter)
    }

    pub fn related_files(&self, name: &str, limit: usize) -> Result<Vec<(String, usize, String)>> {
        self.store.related_files(name, limit)
    }
}

/// How default `callers` partitions implementor vs call edges (noise governance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallersRoleMode {
    /// Default: `callers` = call+registration; `implementors` separate when present.
    #[default]
    Separate,
    /// `--include-implementors`: merge all roles into one array (old noisy shape).
    IncludeImplementors,
    /// `--implementors-only`: return only the implementors section.
    ImplementorsOnly,
}

/// Safety fetch cap when partitioning roles (legacy helper; product path now
/// uses role-biased SQL in `Store::callers_for_roles` — non-impl first, then
/// unbounded implementors — so Exact callers are never LIMIT-starved).
pub fn role_fetch_cap(limit: usize) -> usize {
    limit.saturating_mul(8).clamp(200, 10_000)
}

const CALLERS_SEPARATE_NOTE: &str =
    "callers=Exact calls+registration; implementors=L1 trait/interface impl edges \
     (not call sites); store keeps all edges; impact still expands implementors; \
     --include-implementors merges; --exact-only pure L0; high-freq names cap implementors";

/// Build CLI/MCP `callers` JSON payload with edge-role separation.
///
/// Locked shape (tests/noise_roles.rs):
/// - zero implementors → **plain array** (back-compat) with `edge_role` + `at` on rows
/// - any implementor → object `{callers, implementors, implementor_count,
///   implementors_truncated, truncated, note}`
/// - `--limit N` applies **per section** in Separate mode
/// - HIGH_FREQ_NAMES: implementors section capped at 20 + `implementors_truncated`
pub fn build_callers_payload(
    query_name: &str,
    hits: Vec<ReferenceRecord>,
    limit: usize,
    mode: CallersRoleMode,
) -> Value {
    let high_freq = is_high_freq_name(query_name);

    let mut calls: Vec<Value> = Vec::new();
    let mut implementors: Vec<Value> = Vec::new();
    let mut implementor_count = 0usize;
    let mut other_count = 0usize;

    for r in &hits {
        let role = r.edge_role();
        let row = r.to_query_json();
        match role {
            EdgeRole::Implementor => {
                implementor_count += 1;
                implementors.push(row);
            }
            _ => {
                other_count += 1;
                if mode != CallersRoleMode::ImplementorsOnly {
                    calls.push(row);
                }
            }
        }
    }

    match mode {
        CallersRoleMode::IncludeImplementors => {
            // Merge-all shape, but under a tight --limit do not let implementor
            // flood path-sort ahead of Exact/registration call sites (noise
            // governance: Exact calls stay visible). Stable non-impl first.
            let mut ordered: Vec<(bool, Value)> = hits
                .iter()
                .map(|r| (r.edge_role() != EdgeRole::Implementor, r.to_query_json()))
                .collect();
            ordered.sort_by_key(|(keep_first, _)| !keep_first);
            let mut all: Vec<Value> = ordered.into_iter().map(|(_, v)| v).collect();
            all.truncate(limit.max(1));
            Value::Array(all)
        }
        CallersRoleMode::ImplementorsOnly => {
            let cap = if high_freq {
                HIGH_FREQ_IMPLEMENTOR_CAP.min(limit.max(1))
            } else {
                limit.max(1)
            };
            let truncated = implementor_count > cap;
            implementors.truncate(cap);
            json!({
                "implementors": implementors,
                "implementor_count": implementor_count,
                "implementors_truncated": truncated,
                "truncated": truncated,
                "note": CALLERS_SEPARATE_NOTE,
            })
        }
        CallersRoleMode::Separate => {
            if implementor_count == 0 {
                // Back-compat: plain array when no implementor flood.
                let mut rows = calls;
                rows.truncate(limit.max(1));
                return Value::Array(rows);
            }
            let impl_cap = if high_freq {
                HIGH_FREQ_IMPLEMENTOR_CAP.min(limit.max(1))
            } else {
                limit.max(1)
            };
            let callers_cap = limit.max(1);
            let impl_truncated = implementor_count > impl_cap;
            let callers_truncated = other_count > callers_cap;
            calls.truncate(callers_cap);
            implementors.truncate(impl_cap);
            json!({
                "callers": calls,
                "implementors": implementors,
                "implementor_count": implementor_count,
                "implementors_truncated": impl_truncated,
                "truncated": impl_truncated || callers_truncated,
                "note": CALLERS_SEPARATE_NOTE,
            })
        }
    }
}

/// CLI/MCP string flag → filter. Default = Exact + Heuristic.
///
/// `--recall` is an alias for `--include-dynamic` (prefer missing nothing
/// over a clean graph). Prefer `--sound` when S is satisfied.
pub fn parse_confidence_flags(exact_only: bool, include_dynamic: bool) -> ConfidenceFilter {
    if exact_only {
        ConfidenceFilter::ExactOnly
    } else if include_dynamic {
        ConfidenceFilter::IncludeDynamic
    } else {
        ConfidenceFilter::Default
    }
}

/// Map explicit CLI/MCP flags; `recall` forces IncludeDynamic unless exact_only.
pub fn parse_query_flags(
    exact_only: bool,
    include_dynamic: bool,
    recall: bool,
) -> ConfidenceFilter {
    if exact_only {
        ConfidenceFilter::ExactOnly
    } else if recall || include_dynamic {
        ConfidenceFilter::IncludeDynamic
    } else {
        ConfidenceFilter::Default
    }
}

/// Resolve callers role-mode from CLI/MCP flags.
pub fn parse_callers_role_mode(
    exact_only: bool,
    include_implementors: bool,
    implementors_only: bool,
) -> Result<CallersRoleMode> {
    if include_implementors && implementors_only {
        anyhow::bail!(
            "--include-implementors and --implementors-only are mutually exclusive \
             (pick merge-all or implementors-only)"
        );
    }
    if exact_only {
        // Exact filter already drops Heuristic implementors; keep Separate shape
        // so a zero-implementor payload stays a plain array.
        return Ok(CallersRoleMode::Separate);
    }
    if implementors_only {
        return Ok(CallersRoleMode::ImplementorsOnly);
    }
    if include_implementors {
        return Ok(CallersRoleMode::IncludeImplementors);
    }
    Ok(CallersRoleMode::Separate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EdgeKind, Evidence};

    fn row(rule: Option<&str>, conf: crate::model::Confidence) -> ReferenceRecord {
        ReferenceRecord {
            name: "x".into(),
            kind: EdgeKind::Call,
            path: "a.rs".into(),
            line: 1,
            enclosing: None,
            module: None,
            resolved: None,
            qualifier: None,
            confidence: conf,
            evidence: rule.map(|r| Evidence {
                rule_id: r.into(),
                snippet: String::new(),
            }),
            root_id: String::new(),
        }
    }

    #[test]
    fn exact_only_wins_over_recall() {
        assert_eq!(
            parse_query_flags(true, true, true),
            ConfidenceFilter::ExactOnly
        );
    }

    #[test]
    fn recall_aliases_include_dynamic() {
        assert_eq!(
            parse_query_flags(false, false, true),
            ConfidenceFilter::IncludeDynamic
        );
        assert_eq!(
            parse_query_flags(false, true, false),
            ConfidenceFilter::IncludeDynamic
        );
    }

    #[test]
    fn default_is_exact_plus_heuristic() {
        assert_eq!(
            parse_query_flags(false, false, false),
            ConfidenceFilter::Default
        );
    }

    #[test]
    fn role_mode_flags() {
        assert_eq!(
            parse_callers_role_mode(false, false, false).unwrap(),
            CallersRoleMode::Separate
        );
        assert_eq!(
            parse_callers_role_mode(false, true, false).unwrap(),
            CallersRoleMode::IncludeImplementors
        );
        assert_eq!(
            parse_callers_role_mode(false, false, true).unwrap(),
            CallersRoleMode::ImplementorsOnly
        );
        assert!(parse_callers_role_mode(false, true, true).is_err());
        assert_eq!(
            parse_callers_role_mode(true, true, false).unwrap(),
            CallersRoleMode::Separate
        );
    }

    #[test]
    fn payload_separates_roles() {
        let hits = vec![
            row(
                Some("rs.di.impl_trait"),
                crate::model::Confidence::Heuristic,
            ),
            row(None, crate::model::Confidence::Exact),
        ];
        let v = build_callers_payload("area", hits, 50, CallersRoleMode::Separate);
        assert!(v.is_object());
        assert_eq!(v["implementor_count"], 1);
    }
}
