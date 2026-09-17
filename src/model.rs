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
