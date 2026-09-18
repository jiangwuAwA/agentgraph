use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
    Python,
    Go,
    Rust,
}

impl Language {
    pub fn from_path(path: &str) -> Option<Self> {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".tsx") {
            Some(Language::Tsx)
        } else if lower.ends_with(".ts") || lower.ends_with(".mts") || lower.ends_with(".cts") {
            Some(Language::TypeScript)
        } else if lower.ends_with(".jsx") {
            Some(Language::Jsx)
        } else if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs") {
            Some(Language::JavaScript)
        } else if lower.ends_with(".py") || lower.ends_with(".pyi") {
            Some(Language::Python)
        } else if lower.ends_with(".go") {
            Some(Language::Go)
        } else if lower.ends_with(".rs") {
            Some(Language::Rust)
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::JavaScript => "javascript",
            Language::Jsx => "jsx",
            Language::Python => "python",
            Language::Go => "go",
            Language::Rust => "rust",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    TypeAlias,
    Variable,
    Module,
    Struct,
    Enum,
    Trait,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Variable => "variable",
            SymbolKind::Module => "module",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "function" => SymbolKind::Function,
            "method" => SymbolKind::Method,
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "type_alias" => SymbolKind::TypeAlias,
            "variable" => SymbolKind::Variable,
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "trait" => SymbolKind::Trait,
            _ => SymbolKind::Module,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Call,
    Import,
    Define,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Call => "call",
            EdgeKind::Import => "import",
            EdgeKind::Define => "define",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "import" => EdgeKind::Import,
            "define" => EdgeKind::Define,
            _ => EdgeKind::Call,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRecord {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub path: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub start_col: usize,
    #[serde(default)]
    pub end_col: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    /// Workspace multi-root id. Empty for classic single-root stores.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root_id: String,
}

/// How sure we are that a ref edge is real.
///
/// - `Exact`: L0 syntactic certainty (direct call / import / define).
/// - `Heuristic`: DI / factory / subscription patterns — likely, not proven.
/// - `DynamicCandidate`: reflection / string / computed access — high noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    #[default]
    Exact,
    Heuristic,
    DynamicCandidate,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::Exact => "exact",
            Confidence::Heuristic => "heuristic",
            Confidence::DynamicCandidate => "dynamic_candidate",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "heuristic" => Confidence::Heuristic,
            "dynamic_candidate" => Confidence::DynamicCandidate,
            _ => Confidence::Exact,
        }
    }
}

/// Query-time confidence window for callers / impact / export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfidenceFilter {
    /// Only Exact (L0 syntactic certainty).
    ExactOnly,
    /// Exact + Heuristic (default for callers/impact).
    #[default]
    Default,
    /// Everything including DynamicCandidate.
    IncludeDynamic,
}

impl ConfidenceFilter {
    /// Stored string values accepted by this filter (for SQL / matching).
    pub fn accepted_strs(self) -> &'static [&'static str] {
        match self {
            ConfidenceFilter::ExactOnly => &["exact"],
            ConfidenceFilter::Default => &["exact", "heuristic"],
            ConfidenceFilter::IncludeDynamic => &["exact", "heuristic", "dynamic_candidate"],
        }
    }
}

impl Confidence {
    pub fn included_in(self, filter: ConfidenceFilter) -> bool {
        filter.accepted_strs().contains(&self.as_str())
    }
}

/// Why a non-Exact edge exists (rule id + source fragment).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub rule_id: String,
    pub snippet: String,
}

/// Query-time edge role for noise governance (L1 callers vs implementors).
///
/// Classified from `confidence` + `evidence.rule_id` — **no DB migration**.
/// Store keeps all edges; `impact` still expands implementor edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRole {
    /// Default Exact / unknown-reference edge (direct call / import / define).
    #[default]
    Call,
    /// Trait / interface implementor side (`impl Trait for Type`, dyn method).
    Implementor,
    /// DI / Nest / inventory / linkme / framework registration site.
    Registration,
    /// DynamicCandidate / reflection / string-key family.
    Dynamic,
}

impl EdgeRole {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeRole::Call => "call",
            EdgeRole::Implementor => "implementor",
            EdgeRole::Registration => "registration",
            EdgeRole::Dynamic => "dynamic",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "implementor" => EdgeRole::Implementor,
            "registration" => EdgeRole::Registration,
            "dynamic" => EdgeRole::Dynamic,
            _ => EdgeRole::Call,
        }
    }
}

/// L1 rule ids that emit **implementor** edges (not call sites).
const IMPLEMENTOR_RULES: &[&str] = &[
    "rs.di.impl_trait",
    "rs.di.dyn_trait_method",
    "go.di.interface_impl",
    "go.di.interface_impl_v2",
    "go.di.interface_assert",
];

/// L1 rule ids that emit **registration** / inventory / nest / framework edges.
const REGISTRATION_RULES: &[&str] = &[
    "ts.di.register",
    "ts.di.bind",
    "ts.di.to",
    "ts.di.decorator",
    "ts.nest.module_providers",
    "ts.nest.module_controllers",
    "ts.nest.module_imports",
    "ts.nest.module_exports",
    "ts.nest.ctor_inject",
    "ts.event.subscribe",
    "ts.event.dispatch",
    "ts.framework.register",
    "py.di.depends",
    "py.di.inject",
    "py.di.entry_points",
    "py.framework.init_subclass",
    "go.di.handler_map",
    "go.di.route_register",
    "rs.di.inventory_submit",
    "rs.di.linkme_distributed_slice",
];

/// Classify a ref edge into an [`EdgeRole`] at query time.
///
/// Mapping table (locked by `tests/noise_roles.rs`):
/// - implementor: `rs.di.impl_trait`, `rs.di.dyn_trait_method`, `go.di.interface_impl*`
/// - registration: nest / inventory / linkme / framework / DI register
/// - dynamic: any DynamicCandidate confidence
/// - call: default Exact (and unknown Heuristic ids — still references, not implementors)
pub fn edge_role_for(confidence: Confidence, rule_id: Option<&str>) -> EdgeRole {
    if matches!(confidence, Confidence::DynamicCandidate) {
        return EdgeRole::Dynamic;
    }
    if let Some(rid) = rule_id {
        if IMPLEMENTOR_RULES.contains(&rid) {
            return EdgeRole::Implementor;
        }
        if REGISTRATION_RULES.contains(&rid) {
            return EdgeRole::Registration;
        }
    }
    EdgeRole::Call
}

/// High-frequency method names that flood `callers` via implementor edges
/// (Display/Debug/Clone/Drop/… + generic service names). Constant for now;
/// configurable list is backlog (docs/noise-governance.md).
pub const HIGH_FREQ_NAMES: &[&str] = &[
    "fmt",
    "debug",
    "clone",
    "drop",
    "default",
    "eq",
    "hash",
    "new",
    "into",
    "from",
    "as_ref",
    "to_string",
    "get",
    "set",
    "call",
    "execute",
    "run",
    "handle",
];

/// Cap on the `implementors` section for [`HIGH_FREQ_NAMES`] under default `callers`.
pub const HIGH_FREQ_IMPLEMENTOR_CAP: usize = 20;

/// True when `name` is in the high-frequency demote set (ASCII case-insensitive).
pub fn is_high_freq_name(name: &str) -> bool {
    HIGH_FREQ_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceRecord {
    pub name: String,
    pub kind: EdgeKind,
    pub path: String,
    pub line: usize,
    pub enclosing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    #[serde(default)]
    pub confidence: Confidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,
    /// Workspace multi-root id. Empty for classic single-root stores.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root_id: String,
}

impl ReferenceRecord {
    /// Query-time edge role (not persisted; derived from confidence + evidence).
    pub fn edge_role(&self) -> EdgeRole {
        edge_role_for(
            self.confidence,
            self.evidence.as_ref().map(|e| e.rule_id.as_str()),
        )
    }

    /// JSON row for callers/impact payloads (`at=path:line` + `edge_role`).
    pub fn to_query_json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                "at".into(),
                serde_json::json!(format!("{}:{}", self.path, self.line)),
            );
            obj.insert(
                "edge_role".into(),
                serde_json::json!(self.edge_role().as_str()),
            );
        }
        v
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    pub references: usize,
    pub languages: Vec<String>,
    pub root: String,
    #[serde(default)]
    pub described: usize,
    /// Unchanged (hash-skip) files during incremental index.
    #[serde(default)]
    pub skipped_files: usize,
    #[serde(default)]
    pub failed_files: usize,
    /// Supported source files skipped because they exceeded the 1.5 MiB cap.
    #[serde(default)]
    pub oversized_files: usize,
    /// Supported source files skipped due to noise-dir filter (e.g. testdata/).
    #[serde(default)]
    pub noise_skipped_files: usize,
    /// Edge counts by confidence (`exact` / `heuristic` / `dynamic_candidate`).
    #[serde(default)]
    pub refs_by_confidence: Vec<(String, usize)>,
    /// Per-root counts when the store holds a multi-root workspace (M4-W).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_root: Vec<RootIndexStats>,
}

/// Per-root index counters (workspace multi-root).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootIndexStats {
    pub root_id: String,
    pub files: usize,
    pub symbols: usize,
    pub references: usize,
    #[serde(default)]
    pub subset_violations: usize,
}

/// One project root inside a workspace manifest / `--workspace-root` list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRootInfo {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub files: usize,
    #[serde(default)]
    pub symbols: usize,
    #[serde(default)]
    pub references: usize,
    #[serde(default)]
    pub subset_violations: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    /// Exact-confidence ref edges for this `root_id`.
    #[serde(default)]
    pub exact_refs: usize,
    /// Heuristic-confidence ref edges for this `root_id`.
    #[serde(default)]
    pub heuristic_refs: usize,
    /// DynamicCandidate ref edges for this `root_id`.
    #[serde(default)]
    pub dynamic_refs: usize,
    /// `meta.index_seq` at last full index that touched this store/root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_seq: Option<u64>,
    /// True when the recorded root path is missing on disk.
    #[serde(default)]
    pub missing: bool,
    /// Per-root sound-promise tier note (weakest = disabled when any violation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promise_tier: Option<String>,
}

/// `agentgraph workspace status` / MCP `workspace_status` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStatus {
    pub db_path: String,
    #[serde(default)]
    pub workspace: bool,
    #[serde(default)]
    pub roots: Vec<WorkspaceRootInfo>,
    #[serde(default)]
    pub files: usize,
    #[serde(default)]
    pub symbols: usize,
    #[serde(default)]
    pub references: usize,
    #[serde(default)]
    pub note: String,
    /// Store-level full-index counter (`meta.index_seq`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_seq: Option<u64>,
    /// Weakest selected-root promise tier (union sound = weakest root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promise_tier: Option<String>,
    /// Root id with the weakest S gate (violations first; empty when clean).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weakest_root: Option<String>,
}

/// Result of `index --workspace` / `--workspace-root`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceIndexResult {
    pub db_path: String,
    pub roots: Vec<WorkspaceRootInfo>,
    pub files: usize,
    pub symbols: usize,
    pub references: usize,
    pub languages: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactNode {
    pub name: String,
    pub path: String,
    pub line: usize,
    pub kind: EdgeKind,
    pub depth: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(default)]
    pub confidence: Confidence,
    /// Workspace multi-root id. Empty for classic single-root stores.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root_id: String,
    /// Query-time edge role (noise governance). Optional for legacy JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_role: Option<EdgeRole>,
}

impl ImpactNode {
    /// JSON row for impact payloads (`at=path:line`; `edge_role` when known).
    pub fn to_query_json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                "at".into(),
                serde_json::json!(format!("{}:{}", self.path, self.line)),
            );
        }
        v
    }
}

/// Query-time sidecar de-dup counters (Track M1). Serde-default for old JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupStats {
    /// Sidecar rows dropped because they matched a main Exact row (same key).
    #[serde(default)]
    pub merged_exact: usize,
    /// Sidecar rows dropped because main already had a Heuristic row for the key.
    #[serde(default)]
    pub merged_heuristic: usize,
    /// Sidecar rows kept after de-dup (candidates, including unmapped).
    #[serde(default)]
    pub kept_sidecar: usize,
    /// Kept sidecar rows whose path could not be mapped back to source.
    #[serde(default)]
    pub unmapped: usize,
    /// Main-store row count at union time.
    #[serde(default)]
    pub main_rows: usize,
    /// Sidecar-store row count at union time (before de-dup).
    #[serde(default)]
    pub sidecar_rows: usize,
}

/// Optional macro-expanded sidecar (P2 / Track M1). CLI default OFF.
/// See docs/macro-sidecar.md. Not sound.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroSidecarStatus {
    pub exists: bool,
    pub path: String,
    #[serde(default)]
    pub files: usize,
    #[serde(default)]
    pub symbols: usize,
    #[serde(default)]
    pub refs: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_root: Option<String>,
    /// True when a recorded `expanded_root` no longer exists on disk (stale sidecar).
    #[serde(default)]
    pub expanded_root_missing: bool,
    /// True when the recorded `expanded_root` currently nests with `--root`
    /// (equal / under / contains) after a move or junction — main-walker hazard.
    #[serde(default)]
    pub expanded_root_nested: bool,
    /// S-violation count **inside the sidecar store only**. Does **not** claim
    /// main `subset_ok`; see docs/macro-sidecar.md.
    #[serde(default)]
    pub subset_violation_count: usize,
    /// True when main source fingerprint no longer matches the fingerprint
    /// recorded at sidecar build time (or the recorded fingerprint is missing
    /// after an upgrade). `--with-macro` warns and still unions.
    #[serde(default)]
    pub stale: bool,
    /// Fingerprint of the main source tree at last sidecar build/rebuild.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<String>,
    /// Explicit expanded→source pairs recorded in sidecar meta (if any).
    #[serde(default)]
    pub path_map_present: bool,
    /// Serialized path-map pairs (empty when none).
    #[serde(default)]
    pub path_map: Vec<(String, String)>,
    /// Last `--with-macro` de-dup counters written back to sidecar meta.
    #[serde(default)]
    pub dedup_stats: DedupStats,
    /// Sidecars are operator-rebuilt (`macro rebuild`); no auto cargo-expand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebuild_policy: Option<String>,
}

impl Default for MacroSidecarStatus {
    fn default() -> Self {
        Self {
            exists: false,
            path: String::new(),
            files: 0,
            symbols: 0,
            refs: 0,
            origin: None,
            expanded_root: None,
            expanded_root_missing: false,
            expanded_root_nested: false,
            subset_violation_count: 0,
            stale: false,
            source_fingerprint: None,
            path_map_present: false,
            path_map: Vec::new(),
            dedup_stats: DedupStats::default(),
            rebuild_policy: Some("manual".into()),
        }
    }
}

/// Result of `index --macro-expanded-root` / `macro rebuild` (sidecar build).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroIndexResult {
    pub files: usize,
    pub symbols: usize,
    pub references: usize,
    pub languages: Vec<String>,
    pub path: String,
    pub expanded_root: String,
    pub origin: String,
    /// Main source fingerprint recorded at this sidecar build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<String>,
    /// True when explicit path-map pairs were written to sidecar meta.
    #[serde(default)]
    pub path_map_present: bool,
    /// Fingerprint match after this build (should be false immediately after).
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichReport {
    pub attempted: usize,
    pub described: usize,
    pub skipped: usize,
    pub failed: usize,
    pub model: String,
}

/// True when a JSON object looks like a query-row payload that should carry
/// `root_id` under workspace multi-root stores.
fn looks_like_query_row(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    obj.contains_key("root_id")
        || (obj.contains_key("path")
            && (obj.contains_key("name")
                || obj.contains_key("line")
                || obj.contains_key("kind")
                || obj.contains_key("snippet")
                || obj.contains_key("enclosing")))
}

/// Force-include `root_id` on default query JSON rows when the store is a
/// workspace multi-root DB. Empty/missing → `"default"`. Classic single-root
/// path leaves rows unchanged (`root_id` omitted when empty).
///
/// Recurses into arrays and wrapper objects (`{callers,implementors,…}`,
/// `{mode: sound, callers|impact|violations}`, `{added,removed}`).
pub fn ensure_workspace_root_ids(value: &mut serde_json::Value, workspace: bool) {
    if !workspace {
        return;
    }
    match value {
        serde_json::Value::Object(obj) => {
            if looks_like_query_row(obj) {
                let rid = obj.get("root_id").and_then(|v| v.as_str()).unwrap_or("");
                let out = if rid.is_empty() { "default" } else { rid };
                obj.insert("root_id".into(), serde_json::json!(out));
            }
            let keys: Vec<String> = obj.keys().cloned().collect();
            for k in keys {
                if let Some(v) = obj.get_mut(&k) {
                    ensure_workspace_root_ids(v, true);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                ensure_workspace_root_ids(item, true);
            }
        }
        _ => {}
    }
}

/// Enrich find/callers/impact JSON rows with `root_path` from workspace meta
/// so agents can disambiguate the same relative path under two roots.
pub fn inject_root_paths(value: &mut serde_json::Value, roots: &[WorkspaceRootInfo]) {
    if roots.is_empty() {
        return;
    }
    match value {
        serde_json::Value::Object(obj) => {
            if let Some(rid) = obj.get("root_id").and_then(|v| v.as_str()) {
                let rid = if rid == "default" { "" } else { rid };
                if !obj.contains_key("root_path") {
                    if let Some(meta) = roots
                        .iter()
                        .find(|r| r.id == rid || (!rid.is_empty() && r.id == rid))
                    {
                        if !meta.path.is_empty() {
                            obj.insert("root_path".into(), serde_json::json!(meta.path));
                        }
                    }
                }
            }
            let keys: Vec<String> = obj.keys().cloned().collect();
            for k in keys {
                if let Some(v) = obj.get_mut(&k) {
                    inject_root_paths(v, roots);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                inject_root_paths(item, roots);
            }
        }
        _ => {}
    }
}
