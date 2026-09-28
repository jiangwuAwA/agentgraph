//! Module-path resolution for relative imports (TS/JS/Python/Go/Rust)
//! plus workspace package-name aliases (partial package map — not full TS resolution).

use super::workspace::{PackageAliasEntry, PackageAliasMap};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Normalize a repo-relative path (`/` separators, no `.`/`..` left).
pub fn normalize_repo_path(path: &str) -> String {
    let p = Path::new(path);
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

fn file_dir(rel_path: &str) -> PathBuf {
    Path::new(rel_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
}

fn join_rel(base_dir: &Path, spec: &str) -> String {
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return String::new();
    }
    let joined = base_dir.join(spec);
    normalize_repo_path(&joined.to_string_lossy().replace('\\', "/"))
}

fn last_segment(s: &str) -> &str {
    s.rsplit(['.', ':']).next().unwrap_or(s)
}

/// Resolve a TypeScript/JavaScript import specifier to a repo-relative file path.
pub fn resolve_typescript_import(
    from_file: &str,
    specifier: &str,
    known_files: &HashSet<String>,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let dir = file_dir(from_file);
    let base = join_rel(&dir, specifier);
    if base.is_empty() {
        return None;
    }
    let exts = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];
    let mut candidates: Vec<String> = Vec::with_capacity(exts.len() * 2 + 4);
    candidates.push(base.clone());
    for e in exts {
        candidates.push(format!("{base}{e}"));
    }
    for e in exts {
        candidates.push(format!("{base}/index{e}"));
    }
    candidates.push(format!("{base}.d.ts"));

    candidates.into_iter().find(|c| known_files.contains(c))
}

/// Resolve a Python import to a repo-relative `.py` path.
/// `level`: 0 = absolute, 1 = `from .mod`, 2 = `from ..mod`, etc.
pub fn resolve_python_import(
    from_file: &str,
    module: &str,
    level: usize,
    known_files: &HashSet<String>,
) -> Option<String> {
    let dir = file_dir(from_file);
    let base_dir = if level > 0 {
        let mut d = dir.clone();
        for _ in 1..level {
            d = d.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        }
        d
    } else {
        PathBuf::new()
    };

    let rel_mod = if level > 0 {
        if module.is_empty() {
            normalize_repo_path(&base_dir.to_string_lossy().replace('\\', "/"))
        } else {
            let joined = base_dir.join(module.replace('.', "/"));
            normalize_repo_path(&joined.to_string_lossy().replace('\\', "/"))
        }
    } else {
        normalize_repo_path(&module.replace('.', "/"))
    };

    if rel_mod.is_empty() {
        return None;
    }

    [
        format!("{rel_mod}.py"),
        format!("{rel_mod}/__init__.py"),
        format!("{rel_mod}.pyi"),
    ]
    .into_iter()
    .find(|c| known_files.contains(c))
}

/// Resolve a Go import path. Conservative: only match when a known file's
/// directory path ends with the full import-path tail as a whole path segment
/// sequence (avoids `pkg/util` matching every `.../pkg/util/`).
pub fn resolve_go_import(
    _from_file: &str,
    import_path: &str,
    known_files: &HashSet<String>,
) -> Option<String> {
    if import_path.is_empty() {
        return None;
    }
    // Skip stdlib-looking imports without a dot in the first segment (no domain).
    let first = import_path.split('/').next().unwrap_or("");
    if !first.contains('.') {
        return None;
    }
    let mut best: Option<(usize, String)> = None; // (len, path)
    let suffix = format!("/{import_path}/");
    let file_suffix = format!("/{import_path}.go");
    for f in known_files {
        if !f.ends_with(".go") {
            continue;
        }
        let nf = f.replace('\\', "/");
        // directory form: .../github.com/foo/bar/pkg/... when import is github.com/foo/bar
        // We require the import path to appear as a complete path tail of some prefix.
        // Match: path ends with /import_path/.go  OR contains /import_path/ as dir and then a file under it.
        let hit = nf.ends_with(&file_suffix) || {
            // file under package dir: .../{import_path}/file.go or .../{import_path}/sub/file.go
            if let Some(pos) = nf.find(&suffix) {
                // ensure segment boundary already handled by leading /
                let _ = pos;
                true
            } else {
                false
            }
        };
        // Stronger: the directory of the file must end with import_path
        let parent = nf.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let strong = parent == import_path
            || parent.ends_with(&suffix.trim_end_matches('/').to_string())
            || parent.ends_with(&format!("/{import_path}"));
        if hit && strong {
            let score = nf.len();
            if best.as_ref().map(|(l, _)| score < *l).unwrap_or(true) {
                best = Some((score, f.clone()));
            }
        } else if strong {
            let score = nf.len();
            if best.as_ref().map(|(l, _)| score < *l).unwrap_or(true) {
                best = Some((score, f.clone()));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Resolve a Rust `use` path. Only local module paths (`crate::` / `super::` /
/// `self::`) or unambiguous `src/` matches; never external crate names.
pub fn resolve_rust_use(
    from_file: &str,
    use_path: &str,
    known_files: &HashSet<String>,
) -> Option<String> {
    let is_local = use_path.starts_with("crate::")
        || use_path.starts_with("super::")
        || use_path.starts_with("self::");
    if !is_local {
        return None; // external crate — do not guess local files
    }

    // Strip the root prefix without collapsing repeated `super::` segments.
    // `super::super::foo` must count TWO levels up, not strip both and lose the walk.
    let mut rest = use_path;
    let from_dir = file_dir(from_file);
    let base_dirs: Vec<String> = if rest.starts_with("crate::") {
        rest = &rest["crate::".len()..];
        vec!["src".to_string(), String::new()]
    } else if rest.starts_with("self::") {
        rest = &rest["self::".len()..];
        vec![normalize_repo_path(
            &from_dir.to_string_lossy().replace('\\', "/"),
        )]
    } else {
        // Count leading `super::` segments (N). Rust: one `super` = parent module
        // of the current file. File `path/to/mod.rs` IS module `path/to` (parent dir
        // is one up); file `path/to/file.rs` is module `path/to/file` (parent dir is
        // the containing directory). Then each extra `super::` walks one more parent.
        let mut n = 0usize;
        while rest.starts_with("super::") {
            rest = &rest["super::".len()..];
            n += 1;
        }
        if n == 0 {
            return None;
        }
        let is_mod = from_file.ends_with("mod.rs");
        let mut d = if is_mod {
            match from_dir.parent() {
                Some(p) => p.to_path_buf(),
                None => PathBuf::new(),
            }
        } else {
            from_dir.clone()
        };
        // d is now the directory of the parent module (1 super).
        for _ in 1..n {
            match d.parent() {
                Some(p) => d = p.to_path_buf(),
                None => break,
            }
        }
        vec![normalize_repo_path(&d.to_string_lossy().replace('\\', "/"))]
    };

    let segs: Vec<&str> = rest
        .split("::")
        .filter(|s| !s.is_empty() && *s != "*")
        .collect();
    if segs.is_empty() {
        return None;
    }

    let path = segs.join("/");
    for base in &base_dirs {
        let prefix = if base.is_empty() {
            path.clone()
        } else {
            format!("{base}/{path}")
        };
        for cand in [
            format!("{prefix}.rs"),
            format!("{prefix}/mod.rs"),
            format!("src/{prefix}.rs"),
            format!("src/{prefix}/mod.rs"),
        ] {
            if known_files.contains(&cand) {
                return Some(cand);
            }
        }
    }

    // Last resort: unique file ending with /path.rs under src/
    let suffix = format!("/{path}.rs");
    let mod_suffix = format!("/{path}/mod.rs");
    let mut best: Option<String> = None;
    for f in known_files {
        let nf = f.replace('\\', "/");
        if !nf.starts_with("src/") {
            continue;
        }
        if nf.ends_with(&suffix) || nf.ends_with(&mod_suffix) {
            match &best {
                None => best = Some(f.clone()),
                Some(b) if f.len() < b.len() => best = Some(f.clone()),
                _ => {}
            }
        }
    }
    best
}

#[allow(dead_code)]
fn _last_segment_export() {
    let _ = last_segment("a.b");
}

// ---------------------------------------------------------------------------
// Package-name import → workspace alias (next-cut A)
//
// Partial package map: explicit sources only (CLI / tsconfig paths /
// package.json name). Not full TypeScript module resolution.
// ---------------------------------------------------------------------------

/// Split a package specifier into `(package_name, optional_subpath)`.
///
/// `@demo/registry` → (`@demo/registry`, None)
/// `@demo/registry/client` → (`@demo/registry`, Some("client"))
/// `lodash/get` → (`lodash`, Some("get"))
/// `./relative` / `../relative` are not package specifiers → (`./relative`, None)
pub fn split_package_specifier(spec: &str) -> (&str, Option<&str>) {
    let s = spec.trim();
    if s.is_empty() || s.starts_with('.') || s.starts_with('/') {
        return (s, None);
    }
    if let Some(rest) = s.strip_prefix('@') {
        // scoped package: @scope/name[/sub]
        let mut it = rest.splitn(3, '/');
        let scope = it.next().unwrap_or("");
        let name = it.next().unwrap_or("");
        let sub = it.next();
        if scope.is_empty() || name.is_empty() {
            return (s, None);
        }
        let pkg_len = 1 + scope.len() + 1 + name.len(); // @scope/name
        let pkg = &s[..pkg_len.min(s.len())];
        (pkg, sub.filter(|x| !x.is_empty()))
    } else {
        match s.split_once('/') {
            Some((pkg, sub)) if !pkg.is_empty() && !sub.is_empty() && !pkg.contains('.') => {
                // Unscoped: require the first segment to look like a package name
                // (no dots — avoids treating host/path Go-style or file paths as pkgs).
                (pkg, Some(sub))
            }
            Some((pkg, sub)) if !pkg.is_empty() && !sub.is_empty() && pkg.contains('@') => {
                (pkg, Some(sub))
            }
            Some((pkg, sub)) if !pkg.is_empty() && sub.is_empty() => (pkg, None),
            _ => {
                // Bare package or dotted unscoped name (`lodash`, `node:fs` handled elsewhere).
                (s, None)
            }
        }
    }
}

/// Result of resolving a package specifier through workspace aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageResolveHit {
    /// Package name that matched (subpath stripped).
    pub package: String,
    /// Mapped workspace `root_id`.
    pub root_id: String,
    /// Root-relative barrel/entry when known.
    pub entry: Option<String>,
    /// Workspace-relative file when `root_path` + `entry` are known
    /// (`packages/registry/src/index.ts`).
    pub workspace_file: Option<String>,
}

/// Limited extension table for wildcard / extension-less path probing (F6).
const ALIAS_FILE_EXTS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx"];
const ALIAS_INDEX_FILES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.d.ts",
    "index.js",
    "index.jsx",
];

/// Probe `root_abs` + `rel` with the limited extension table.
/// Returns the root-relative path of the first existing candidate.
/// Never invents a path when nothing exists on disk.
fn probe_alias_file(root_abs: &Path, rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    let rel = rel.trim_start_matches('/').to_string();
    if rel.is_empty() {
        return None;
    }
    let exact = root_abs.join(&rel);
    if exact.is_file() {
        return Some(rel);
    }
    let has_known_ext = ALIAS_FILE_EXTS.iter().any(|e| rel.ends_with(e));
    if !has_known_ext {
        for ext in ALIAS_FILE_EXTS {
            let cand = format!("{rel}{ext}");
            if root_abs.join(&cand).is_file() {
                return Some(cand);
            }
        }
    }
    for idx in ALIAS_INDEX_FILES {
        let cand = format!("{rel}/{idx}");
        if root_abs.join(&cand).is_file() {
            return Some(cand);
        }
    }
    None
}

fn workspace_display_file(root_path: &str, root_rel: &str) -> String {
    let rp = root_path
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    let display_root = if Path::new(&rp).is_absolute() {
        let parts: Vec<&str> = rp.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
        } else {
            parts.last().map(|s| s.to_string()).unwrap_or(rp)
        }
    } else {
        rp
    };
    let e = root_rel
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();
    format!("{display_root}/{e}")
}

/// Resolve a package import specifier via the workspace package alias map.
///
/// Supports exact package keys and **tsconfig-style wildcards** (`@/*` -> `src/*`):
/// the longest matching prefix wins; `*` in the alias entry is replaced by the
/// unmatched suffix. For wildcards the concrete leaf is probed on disk with a
/// **limited extension table** (`.ts`/`.tsx`/`.d.ts`/`.js`/`.jsx` + `/index.*`);
/// when nothing exists the hit keeps `root_id` only (no invented path).
/// Returns `None` for relative imports and unknown packages.
/// When only `root_id` is known (no entry/file), still returns a hit so
/// queries can connect the import to the mapped workspace root.
pub fn resolve_package_import(
    specifier: &str,
    aliases: &PackageAliasMap,
) -> Option<PackageResolveHit> {
    if specifier.is_empty() || specifier.starts_with('.') || specifier.starts_with('/') {
        return None;
    }
    let (pkg, _sub) = split_package_specifier(specifier);
    if let Some(entry) = aliases.get(pkg) {
        return Some(hit_from_entry(pkg.to_string(), entry, None));
    }
    // Wildcard aliases: `@/*` -> `src/*`, longest prefix before `*` wins.
    let mut best: Option<(usize, String, String, &PackageAliasEntry)> = None;
    for (key, entry) in aliases {
        let Some(prefix) = key.strip_suffix('*') else {
            continue;
        };
        if prefix.is_empty() {
            continue;
        }
        if let Some(rest) = specifier.strip_prefix(prefix) {
            if rest.is_empty() {
                continue;
            }
            let score = prefix.len();
            if best.as_ref().map(|(s, _, _, _)| score > *s).unwrap_or(true) {
                best = Some((score, key.clone(), rest.to_string(), entry));
            }
        }
    }
    let (_score, key, rest, entry) = best?;
    Some(hit_from_entry(key, entry, Some(rest)))
}

fn hit_from_entry(
    key: String,
    entry: &PackageAliasEntry,
    wild_rest: Option<String>,
) -> PackageResolveHit {
    let package = if key.ends_with('*') {
        key.trim_end_matches('*').to_string()
    } else {
        key.clone()
    };

    // Wildcard mapping: substitute `*`, then probe concrete files. Fail -> no file.
    if let Some(rest) = wild_rest {
        let pattern = entry.entry.clone().unwrap_or_else(|| "*".to_string());
        let candidate_rel = if pattern.contains('*') {
            pattern.replacen('*', rest.as_str(), 1)
        } else {
            format!("{}/{}", pattern.trim_end_matches('/'), rest)
        };
        let candidate_rel = candidate_rel.replace('\\', "/");
        // Only probe when root_path is an existing directory (verifiable).
        let probed = entry.root_path.as_deref().and_then(|rp| {
            let rp_path = Path::new(rp);
            if rp_path.is_dir() {
                probe_alias_file(rp_path, &candidate_rel)
            } else {
                None
            }
        });
        return match probed {
            Some(found) => {
                let workspace_file = entry
                    .root_path
                    .as_deref()
                    .map(|rp| workspace_display_file(rp, &found));
                PackageResolveHit {
                    package,
                    root_id: entry.root_id.clone(),
                    entry: Some(found),
                    workspace_file,
                }
            }
            None => PackageResolveHit {
                package,
                root_id: entry.root_id.clone(),
                entry: None,
                workspace_file: None,
            },
        };
    }

    // Exact package key: declared barrel/entry is trusted as-is (no FS probe).
    let workspace_file = match (&entry.root_path, &entry.entry) {
        (Some(rp), Some(e)) => Some(workspace_display_file(rp, e)),
        _ => None,
    };
    PackageResolveHit {
        package,
        root_id: entry.root_id.clone(),
        entry: entry.entry.clone(),
        workspace_file,
    }
}

/// Build the `refs.resolved` display value for a package import hit.
///
/// Prefers workspace-relative barrel path, then root-relative entry, then
/// `@alias:<pkg>→<root_id>` so the import remains queryable by root.
pub fn package_resolve_display(hit: &PackageResolveHit) -> String {
    if let Some(wf) = &hit.workspace_file {
        return wf.clone();
    }
    if let Some(e) = &hit.entry {
        return e.clone();
    }
    format!("@alias:{}→{}", hit.package, hit.root_id)
}
