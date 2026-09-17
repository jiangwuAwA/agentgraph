//! Track M1: expanded-sidecar path mapping + query de-dup policy.
//!
//! Maps expanded shadow-tree relative paths back to source-tree paths
//! (prefix strip / crate-root alignment / explicit pairs from sidecar meta).
//! Also owns the `--with-macro` union de-dup table from
//! `docs/product-boundary-migration.md` §1.4.
//!
//! Honesty: sidecar rows stay `origin=macro_expanded` and are **not** sound.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::model::{Confidence, DedupStats, ImpactNode, ReferenceRecord};

/// One explicit expanded→source path mapping (exact file or prefix pair).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathMapPair {
    /// Expanded relative path or path prefix (`/` separators).
    pub expanded: String,
    /// Source relative path or path prefix (`/` separators).
    pub source: String,
    /// When true, `expanded`/`source` are prefixes; otherwise exact paths.
    #[serde(default)]
    pub prefix: bool,
}

/// Serialized form stored in sidecar `meta.path_map`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathMap {
    #[serde(default)]
    pub pairs: Vec<PathMapPair>,
}

impl PathMap {
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

/// Heuristics + explicit overrides for expanded → source path mapping.
#[derive(Debug, Clone)]
pub struct CrateLayout {
    /// Explicit pairs (from sidecar meta / operator). Longest prefix wins.
    pub pairs: Vec<PathMapPair>,
    /// Rust workspace template; `{crate}` is the first path component.
    pub rust_crate_src: String,
    /// Leading directory names that mean "expanded shadow view".
    pub expand_dir_names: Vec<String>,
}

impl Default for CrateLayout {
    fn default() -> Self {
        Self {
            pairs: Vec::new(),
            rust_crate_src: "crates/{crate}/src".to_string(),
            expand_dir_names: vec![
                "expanded-view".into(),
                "expanded_view".into(),
                "real-expanded".into(),
                "real_expanded".into(),
                "expand-shadow".into(),
                "expand_shadow".into(),
                "expanded".into(),
                "shadow".into(),
            ],
        }
    }
}

impl CrateLayout {
    pub fn from_path_map(map: &PathMap) -> Self {
        Self {
            pairs: map.pairs.clone(),
            ..Default::default()
        }
    }
}

fn norm_slashes(s: &str) -> String {
    s.replace('\\', "/")
}

fn strip_drive_prefix(s: &str) -> &str {
    // `D:/foo` / `d:\foo` → keep as-is for now; absolute handling uses roots.
    s
}

/// True when `rel` looks like an absolute filesystem path (Unix or Windows).
fn looks_absolute(s: &str) -> bool {
    s.starts_with('/') || s.starts_with("\\\\") || {
        let b = s.as_bytes();
        b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'/' || b[2] == b'\\')
    }
}

fn path_under_exists(source_root: &Path, rel: &str) -> bool {
    if rel.is_empty() || looks_absolute(rel) {
        return false;
    }
    source_root.join(rel).exists()
}

fn components(rel: &str) -> Vec<&str> {
    rel.split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect()
}

/// Lexically resolve `../` / `./` in a `/`-separated relative path.
fn lex_normalize(rel: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for c in components(rel) {
        if c == ".." {
            out.pop()?; // None → escapes above root
        } else {
            out.push(c);
        }
    }
    Some(out.join("/"))
}

fn strip_expand_dirs(rel: &str, layout: &CrateLayout) -> String {
    let mut comps = components(rel);
    while let Some(first) = comps.first().copied() {
        let lower = first.to_ascii_lowercase();
        if layout
            .expand_dir_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&lower))
        {
            comps.remove(0);
        } else {
            break;
        }
    }
    comps.join("/")
}

/// Align a (possibly expand-dir-stripped) expanded relative path onto the
/// source workspace crate layout.
///
/// Examples:
/// - `event-engine/lib.rs` → `crates/event-engine/src/lib.rs`
/// - `event-engine/src/lib.rs` → `crates/event-engine/src/lib.rs`
/// - `crates/event-engine/lib.rs` → `crates/event-engine/src/lib.rs`
/// - `src/core.rs` → identity (`src/core.rs`)
fn crate_align(rel: &str, layout: &CrateLayout) -> Option<String> {
    let comps = components(rel);
    if comps.is_empty() {
        return None;
    }
    // Already a workspace crate path missing `src/`:
    // `crates/foo/lib.rs` → `crates/foo/src/lib.rs`
    if comps.len() >= 3 && comps[0] == "crates" && comps[2] != "src" {
        let crate_name = comps[1];
        let rest = &comps[2..];
        let joined = rest.join("/");
        // Only rewrite when rest looks like crate entry/module files.
        if joined.ends_with(".rs") || rest[0] == "src" {
            if rest[0] == "src" {
                return Some(rel.trim_start_matches("./").to_string());
            }
            let mapped = format!("crates/{crate_name}/src/{joined}");
            return Some(mapped);
        }
    }
    // `crates/foo/src/...` already aligned.
    if comps.len() >= 4 && comps[0] == "crates" && comps[2] == "src" {
        return Some(comps.join("/"));
    }
    // Flat expanded crate dir: `foo/lib.rs`, `foo/src/lib.rs`, `foo/mod.rs`,
    // `foo/bar/mod.rs`.
    if comps.len() >= 2 {
        let crate_name = comps[0];
        // Skip obvious non-crate top-level source dirs.
        if matches!(crate_name, "src" | "tests" | "target" | "docs" | "fixtures") {
            if crate_name == "src" {
                return Some(comps.join("/"));
            }
            return None;
        }
        let rest = &comps[1..];
        if rest[0] == "src" {
            // `foo/src/lib.rs` → `crates/foo/src/lib.rs` (do not double `src/`).
            let joined = rest.join("/");
            return Some(format!("crates/{crate_name}/{joined}"));
        }
        let joined = rest.join("/");
        if joined.ends_with(".rs") {
            let tmpl = layout.rust_crate_src.replace("{crate}", crate_name);
            return Some(format!("{tmpl}/{joined}"));
        }
    }
    // Bare `lib.rs` / module at expanded root — cannot guess crate.
    None
}

fn apply_pairs(rel: &str, layout: &CrateLayout) -> Option<String> {
    // Exact pairs first.
    for p in layout.pairs.iter().filter(|p| !p.prefix) {
        if rel == p.expanded {
            return Some(p.source.clone());
        }
    }
    // Prefix pairs, longest expanded prefix first.
    let mut prefixes: Vec<&PathMapPair> = layout.pairs.iter().filter(|p| p.prefix).collect();
    prefixes.sort_by_key(|p| std::cmp::Reverse(p.expanded.len()));
    for p in prefixes {
        let exp = p.expanded.trim_end_matches('/');
        if rel == exp {
            return Some(p.source.trim_end_matches('/').to_string());
        }
        let with_slash = format!("{exp}/");
        if let Some(rest) = rel.strip_prefix(with_slash.as_str()) {
            let src = p.source.trim_end_matches('/');
            return Some(format!("{src}/{rest}"));
        }
    }
    None
}

/// Map an expanded shadow-tree relative path onto a source-tree relative path.
///
/// Order:
/// 1. Explicit pairs from `crate_layout.pairs` (sidecar meta / operator)
/// 2. Absolute path under `expanded_root` → strip that prefix, then remap
/// 3. Identity when the path already exists under `source_root`
/// 4. Strip known expand-dir leading components
/// 5. `../` lexical resolution against `expanded_root` then against roots
/// 6. Rust crate-root alignment heuristics
///
/// Returns `None` when no honest mapping can be produced (caller keeps
/// `origin=macro_expanded`, `mapped=false`).
pub fn map_expanded_path(
    expanded_rel: &str,
    expanded_root: &Path,
    source_root: &Path,
    crate_layout: &CrateLayout,
) -> Option<String> {
    let raw = norm_slashes(expanded_rel);
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // Absolute / root-prefixed expanded paths → make relative to expanded_root.
    let mut rel = raw.to_string();
    if looks_absolute(&rel) || rel.contains(':') {
        let exp_norm = norm_slashes(&expanded_root.to_string_lossy());
        let exp_norm = exp_norm.trim_end_matches('/');
        // Case-insensitive drive compare on Windows-style prefixes.
        if rel.len() > exp_norm.len() {
            let (a, b) = (
                rel[..exp_norm.len()].to_ascii_lowercase(),
                exp_norm.to_ascii_lowercase(),
            );
            if a == b && rel.as_bytes().get(exp_norm.len()) == Some(&b'/') {
                rel = rel[exp_norm.len() + 1..].to_string();
            }
        }
        if looks_absolute(&rel) {
            // Still absolute: try strip_prefix via Path.
            if let Some(r) = parser_rel(Path::new(&raw), expanded_root) {
                rel = r;
            } else {
                // Windows drive letter path that does not share expanded_root:
                // try the source root as well (shadow may record source paths).
                if let Some(r) = parser_rel(Path::new(&raw), source_root) {
                    return Some(r);
                }
                return None;
            }
        }
    }
    let _ = strip_drive_prefix; // keep helper referenced for clarity
    rel = norm_slashes(&rel);

    // `../` relative forms (sibling roots).
    if rel.contains("../") || rel.starts_with("../") {
        // Against expanded_root first.
        if let Ok(joined) = std::fs::canonicalize(expanded_root.join(&rel)) {
            if let Some(r) = parser_rel(&joined, source_root) {
                return Some(r);
            }
            if let Some(r) = parser_rel(&joined, expanded_root) {
                rel = r;
            }
        } else if let Some(lex) = lex_normalize(&rel) {
            // Lexical: `../src-root/src/core.rs` won't map by normalize alone
            // unless the path re-enters a known root name — fall through.
            rel = lex;
        }
    }

    // 1. Explicit pairs (exact + prefix).
    if let Some(mapped) = apply_pairs(&rel, crate_layout) {
        return Some(mapped);
    }

    // 3. Identity when present under source.
    if path_under_exists(source_root, &rel) {
        return Some(rel.clone());
    }

    // 4. Strip expand-dir prefixes.
    let stripped = strip_expand_dirs(&rel, crate_layout);
    if stripped != rel {
        if let Some(mapped) = apply_pairs(&stripped, crate_layout) {
            return Some(mapped);
        }
        if path_under_exists(source_root, &stripped) {
            return Some(stripped.clone());
        }
        rel = stripped;
    }

    // After strip, `../src/...` style may have become `src/...`.
    if let Some(lex) = lex_normalize(&rel) {
        if lex != rel {
            if path_under_exists(source_root, &lex) {
                return Some(lex.clone());
            }
            if let Some(mapped) = apply_pairs(&lex, crate_layout) {
                return Some(mapped);
            }
            rel = lex;
        }
    }

    // 6. Crate-root alignment — only accept when the source tree actually has
    // the target crate (or the mapped file). Prevents inventing
    // `crates/<unknown>/src/...` for unmappable shadow-only paths.
    if let Some(mapped) = crate_align(&rel, crate_layout) {
        if path_under_exists(source_root, &mapped) {
            return Some(mapped);
        }
        let comps = components(&mapped);
        if comps.len() >= 2 && comps[0] == "crates" {
            let crate_dir = source_root.join("crates").join(comps[1]);
            if crate_dir.is_dir() {
                return Some(mapped);
            }
        }
        // First-component crate dir already present under source root
        // (non-workspace layout).
        let rel_comps = components(&rel);
        if let Some(first) = rel_comps.first() {
            if source_root.join(first).is_dir() && !matches!(*first, "src" | "tests") {
                return Some(mapped);
            }
        }
        return None;
    }

    None
}

fn parser_rel(path: &Path, root: &Path) -> Option<String> {
    crate::index::parser::rel_path_under_root(path, root)
}

/// Discover explicit path-map pairs by sampling expanded files.
///
/// Records exact pairs when a mapped source path exists, and crate-level
/// prefix pairs when multiple files share a crate alignment.
pub fn discover_path_map(
    expanded_root: &Path,
    source_root: &Path,
    layout: &CrateLayout,
) -> PathMap {
    let mut pairs: Vec<PathMapPair> = layout.pairs.clone();
    let mut prefix_hits: HashMap<(String, String), usize> = HashMap::new();

    let Ok(files) = crate::index::walker::collect_source_files(expanded_root) else {
        return PathMap { pairs };
    };

    for abs in files {
        let Ok(rel_path) = abs.strip_prefix(expanded_root) else {
            continue;
        };
        let rel = norm_slashes(&rel_path.to_string_lossy());
        if rel.is_empty() {
            continue;
        }
        let Some(mapped) = map_expanded_path(&rel, expanded_root, source_root, layout) else {
            continue;
        };
        let exists = path_under_exists(source_root, &mapped);
        if exists && !pairs.iter().any(|p| !p.prefix && p.expanded == rel) {
            pairs.push(PathMapPair {
                expanded: rel.clone(),
                source: mapped.clone(),
                prefix: false,
            });
        }
        // Prefix generalization: first component (crate) → source prefix.
        if let Some((exp_crate, src_prefix)) = crate_prefix_pair(&rel, &mapped) {
            *prefix_hits
                .entry((exp_crate.clone(), src_prefix.clone()))
                .or_insert(0) += 1;
        }
    }

    for ((exp_prefix, src_prefix), n) in prefix_hits {
        if n < 1 {
            continue;
        }
        let already = pairs.iter().any(|p| {
            p.prefix && p.expanded.trim_end_matches('/') == exp_prefix.trim_end_matches('/')
        });
        if !already {
            pairs.push(PathMapPair {
                expanded: exp_prefix,
                source: src_prefix,
                prefix: true,
            });
        }
    }

    PathMap { pairs }
}

fn crate_prefix_pair(expanded_rel: &str, mapped: &str) -> Option<(String, String)> {
    let e = components(expanded_rel);
    let m = components(mapped);
    if e.is_empty() || m.is_empty() {
        return None;
    }
    // Strip expand-dir from expanded for the prefix key.
    // Map `foo/...` → `crates/foo/src/...` when mapped starts with crates/foo/src.
    if m.len() >= 3 && m[0] == "crates" && m[2] == "src" {
        let exp_crate = e[0];
        // If expanded still has expand-dir prefix, use the next component.
        let exp_crate = if matches!(
            exp_crate.to_ascii_lowercase().as_str(),
            "expanded-view"
                | "expanded_view"
                | "real-expanded"
                | "real_expanded"
                | "expand-shadow"
                | "expand_shadow"
                | "expanded"
                | "shadow"
        ) {
            e.get(1).copied().unwrap_or(exp_crate)
        } else {
            exp_crate
        };
        return Some((
            format!("{exp_crate}/"),
            format!("crates/{}/{}/", m[1], m[2]),
        ));
    }
    // Identity prefix: same first component after strip.
    None
}

/// Serialize a path map for sidecar `meta.path_map`.
pub fn path_map_to_meta(map: &PathMap) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".into())
}

/// Parse sidecar `meta.path_map`. Missing/invalid → empty map.
pub fn path_map_from_meta(raw: Option<&str>) -> PathMap {
    let Some(s) = raw else {
        return PathMap::default();
    };
    serde_json::from_str(s).unwrap_or_default()
}

/// Dedup key: name + enclosing + mapped_path (or original path when unmapped).
pub fn dedup_key(name: &str, enclosing: Option<&str>, path: &str) -> String {
    format!("{name}\u{0}{}\u{0}{path}", enclosing.unwrap_or(""))
}

fn json_str<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

fn main_key_from_json(v: &serde_json::Value) -> Option<(String, String, String)> {
    let name = json_str(v, "name")?.to_string();
    let enclosing = json_str(v, "enclosing").unwrap_or("").to_string();
    let path = json_str(v, "path")?.to_string();
    Some((name, enclosing, path))
}

fn confidence_from_json(v: &serde_json::Value) -> Confidence {
    match json_str(v, "confidence").unwrap_or("exact") {
        "heuristic" => Confidence::Heuristic,
        "dynamic_candidate" => Confidence::DynamicCandidate,
        _ => Confidence::Exact,
    }
}

/// Tag a sidecar ReferenceRecord for JSON union (origin + mapped path + at).
///
/// Origin stays `macro_expanded` (non-sound). When `mapped_path` is `Some`,
/// `path`/`at`/`mapped_path` point at the source path; original expanded path
/// is preserved as `expanded_path`.
pub fn tag_macro_ref_json(
    r: &ReferenceRecord,
    mapped_path: Option<&str>,
    mapped: bool,
) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap_or_default();
    let display_path = mapped_path.unwrap_or(r.path.as_str()).to_string();
    if let Some(obj) = v.as_object_mut() {
        if let Some(mp) = mapped_path {
            obj.insert("expanded_path".into(), serde_json::json!(r.path.clone()));
            obj.insert("path".into(), serde_json::json!(mp));
            obj.insert("mapped_path".into(), serde_json::json!(mp));
        }
        obj.insert(
            "at".into(),
            serde_json::json!(format!("{display_path}:{}", r.line)),
        );
        obj.insert("origin".into(), serde_json::json!("macro_expanded"));
        obj.insert("mapped".into(), serde_json::json!(mapped));
    }
    v
}

/// Tag a sidecar ImpactNode for JSON union (origin + mapped path + at).
pub fn tag_macro_impact_json(
    n: &ImpactNode,
    mapped_path: Option<&str>,
    mapped: bool,
) -> serde_json::Value {
    let mut v = serde_json::to_value(n).unwrap_or_default();
    let display_path = mapped_path.unwrap_or(n.path.as_str()).to_string();
    if let Some(obj) = v.as_object_mut() {
        if let Some(mp) = mapped_path {
            obj.insert("expanded_path".into(), serde_json::json!(n.path.clone()));
            obj.insert("path".into(), serde_json::json!(mp));
            obj.insert("mapped_path".into(), serde_json::json!(mp));
        }
        obj.insert(
            "at".into(),
            serde_json::json!(format!("{display_path}:{}", n.line)),
        );
        obj.insert("origin".into(), serde_json::json!("macro_expanded"));
        obj.insert("mapped".into(), serde_json::json!(mapped));
    }
    v
}

/// Options for a `--with-macro` union.
#[derive(Debug, Clone, Copy)]
pub struct UnionOptions {
    /// Default ON. `--no-macro-dedup` sets this false (debug).
    pub dedup: bool,
    /// `--exact-only` ignores the sidecar entirely (spec §1.4).
    pub ignore_sidecar: bool,
}

impl Default for UnionOptions {
    fn default() -> Self {
        Self {
            dedup: true,
            ignore_sidecar: false,
        }
    }
}

/// Outcome of a mapped + de-duped sidecar union.
#[derive(Debug, Clone)]
pub struct UnionOutcome {
    pub rows: Vec<serde_json::Value>,
    pub dedup_stats: DedupStats,
    pub path_map_present: bool,
    pub sidecar_present: bool,
    pub stale: bool,
}

impl UnionOutcome {
    pub fn empty_main(rows: Vec<serde_json::Value>) -> Self {
        Self {
            rows,
            dedup_stats: DedupStats::default(),
            path_map_present: false,
            sidecar_present: false,
            stale: false,
        }
    }
}

fn map_record_path(
    path: &str,
    expanded_root: &Path,
    source_root: &Path,
    layout: &CrateLayout,
) -> Option<String> {
    map_expanded_path(path, expanded_root, source_root, layout)
}

/// Union main callers JSON rows with sidecar ReferenceRecords.
///
/// De-dup table (docs/product-boundary-migration.md §1.4):
/// - same name+enclosing+mapped_path as main Exact → drop sidecar (`merged_exact`)
/// - same key as main Heuristic → keep main Heuristic (`merged_heuristic`)
/// - unmappable path → keep sidecar, `mapped=false`, `origin=macro_expanded`
/// - sidecar-only symbols (fmt/clone/…) → kept as candidates
/// - `dedup=false` (`--no-macro-dedup`) keeps every sidecar row
pub fn union_callers(
    main_rows: Vec<serde_json::Value>,
    side_hits: &[ReferenceRecord],
    expanded_root: &Path,
    source_root: &Path,
    layout: &CrateLayout,
    opts: UnionOptions,
) -> (Vec<serde_json::Value>, DedupStats) {
    let mut stats = DedupStats {
        main_rows: main_rows.len(),
        sidecar_rows: if opts.ignore_sidecar {
            0
        } else {
            side_hits.len()
        },
        ..DedupStats::default()
    };
    if opts.ignore_sidecar {
        return (main_rows, stats);
    }

    let mut main_exact: HashSet<(String, String, String)> = HashSet::new();
    let mut main_heuristic: HashSet<(String, String, String)> = HashSet::new();
    for v in &main_rows {
        if let Some((n, e, p)) = main_key_from_json(v) {
            match confidence_from_json(v) {
                Confidence::Exact => {
                    main_exact.insert((n, e, p));
                }
                Confidence::Heuristic => {
                    main_heuristic.insert((n, e, p));
                }
                Confidence::DynamicCandidate => {}
            }
        }
    }

    let mut rows = main_rows;
    for r in side_hits {
        let mapped_path = map_record_path(&r.path, expanded_root, source_root, layout);
        let mapped = mapped_path.is_some();
        let path_for_key = mapped_path.clone().unwrap_or_else(|| r.path.clone());
        let enclosing = r.enclosing.clone();
        let key = (
            r.name.clone(),
            enclosing.clone().unwrap_or_default(),
            path_for_key.clone(),
        );

        if opts.dedup && mapped {
            if main_exact.contains(&key) {
                stats.merged_exact += 1;
                continue;
            }
            if main_heuristic.contains(&key) {
                stats.merged_heuristic += 1;
                continue;
            }
        }

        if !mapped {
            stats.unmapped += 1;
        }
        stats.kept_sidecar += 1;
        rows.push(tag_macro_ref_json(r, mapped_path.as_deref(), mapped));
    }
    (rows, stats)
}

/// Union main impact JSON rows with sidecar ImpactNodes (same §1.4 policy).
pub fn union_impact(
    main_rows: Vec<serde_json::Value>,
    side_hits: &[ImpactNode],
    expanded_root: &Path,
    source_root: &Path,
    layout: &CrateLayout,
    opts: UnionOptions,
) -> (Vec<serde_json::Value>, DedupStats) {
    let mut stats = DedupStats {
        main_rows: main_rows.len(),
        sidecar_rows: if opts.ignore_sidecar {
            0
        } else {
            side_hits.len()
        },
        ..DedupStats::default()
    };
    if opts.ignore_sidecar {
        return (main_rows, stats);
    }

    let mut main_exact: HashSet<(String, String, String)> = HashSet::new();
    let mut main_heuristic: HashSet<(String, String, String)> = HashSet::new();
    for v in &main_rows {
        if let Some((n, e, p)) = main_key_from_json(v) {
            match confidence_from_json(v) {
                Confidence::Exact => {
                    main_exact.insert((n, e, p));
                }
                Confidence::Heuristic => {
                    main_heuristic.insert((n, e, p));
                }
                Confidence::DynamicCandidate => {}
            }
        }
    }

    let mut rows = main_rows;
    for n in side_hits {
        let mapped_path = map_record_path(&n.path, expanded_root, source_root, layout);
        let mapped = mapped_path.is_some();
        let path_for_key = mapped_path.clone().unwrap_or_else(|| n.path.clone());
        let key = (
            n.name.clone(),
            n.enclosing.clone().unwrap_or_default(),
            path_for_key.clone(),
        );
        if opts.dedup && mapped {
            if main_exact.contains(&key) {
                stats.merged_exact += 1;
                continue;
            }
            if main_heuristic.contains(&key) {
                stats.merged_heuristic += 1;
                continue;
            }
        }
        if !mapped {
            stats.unmapped += 1;
        }
        stats.kept_sidecar += 1;
        rows.push(tag_macro_impact_json(n, mapped_path.as_deref(), mapped));
    }
    (rows, stats)
}

/// Compute a source-tree fingerprint (aggregate of rel+mtime_ns+size).
///
/// Stored in sidecar `meta.source_fingerprint` at build/rebuild time.
/// Status/query compares the current main fingerprint to detect staleness.
pub fn source_fingerprint(source_root: &Path) -> String {
    use sha2::{Digest, Sha256};
    let Ok(collected) = crate::index::walker::collect_source_files_with_stats(source_root) else {
        return String::new();
    };
    let mut entries: Vec<(String, i64, i64)> = collected
        .files
        .iter()
        .map(|f| (norm_slashes(&f.rel), f.mtime_ns, f.size))
        .collect();
    entries.sort();
    let mut h = Sha256::new();
    for (rel, mtime_ns, size) in entries {
        h.update(rel.as_bytes());
        h.update([0u8]);
        h.update(mtime_ns.to_le_bytes());
        h.update(size.to_le_bytes());
    }
    format!("{:x}", h.finalize())
}

/// Keep a PathBuf import used by tests through this module's public surface.
#[allow(dead_code)]
fn _pathbuf_ty(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_key_separates_enclosing() {
        assert_ne!(
            dedup_key("helper", Some("a"), "src/x.rs"),
            dedup_key("helper", Some("b"), "src/x.rs")
        );
        assert_eq!(
            dedup_key("helper", None, "src/x.rs"),
            dedup_key("helper", Some(""), "src/x.rs")
        );
    }

    #[test]
    fn crate_align_event_engine_lib() {
        let layout = CrateLayout::default();
        assert_eq!(
            crate_align("event-engine/lib.rs", &layout).as_deref(),
            Some("crates/event-engine/src/lib.rs")
        );
        assert_eq!(
            crate_align("event-engine/src/lib.rs", &layout).as_deref(),
            Some("crates/event-engine/src/lib.rs")
        );
        assert_eq!(
            crate_align("crates/event-engine/lib.rs", &layout).as_deref(),
            Some("crates/event-engine/src/lib.rs")
        );
    }

    #[test]
    fn map_requires_source_crate_dir_for_align() {
        let tmp = std::env::temp_dir().join(format!("ag-macro-map-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let exp = tmp.join("exp");
        std::fs::create_dir_all(src.join("crates/event-engine/src")).unwrap();
        std::fs::create_dir_all(exp.join("event-engine")).unwrap();
        let mapped = map_expanded_path("event-engine/lib.rs", &exp, &src, &CrateLayout::default());
        assert_eq!(mapped.as_deref(), Some("crates/event-engine/src/lib.rs"));
        // Unknown crate: no invent.
        let mapped2 = map_expanded_path("nope/lib.rs", &exp, &src, &CrateLayout::default());
        assert_eq!(mapped2, None);
    }

    #[test]
    fn path_map_meta_roundtrip() {
        let map = PathMap {
            pairs: vec![PathMapPair {
                expanded: "event-engine/".into(),
                source: "crates/event-engine/src/".into(),
                prefix: true,
            }],
        };
        let s = path_map_to_meta(&map);
        let back = path_map_from_meta(Some(&s));
        assert_eq!(back.pairs, map.pairs);
        assert!(path_map_from_meta(None).is_empty());
        assert!(path_map_from_meta(Some("not-json")).is_empty());
    }
}
