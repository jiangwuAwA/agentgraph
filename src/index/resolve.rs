//! Module-path resolution for relative imports (TS/JS/Python/Go/Rust).

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
    let exts = [
        ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs",
    ];
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
        let hit = nf.ends_with(&file_suffix)
            || {
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
        let strong = parent == import_path || parent.ends_with(&suffix.trim_end_matches('/').to_string())
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

    let cleaned = use_path
        .trim_start_matches("crate::")
        .trim_start_matches("super::")
        .trim_start_matches("self::");
    let segs: Vec<&str> = cleaned
        .split("::")
        .filter(|s| !s.is_empty() && *s != "*")
        .collect();
    if segs.is_empty() {
        return None;
    }

    // For super::, walk up from the current file's directory.
    let from_dir = file_dir(from_file);
    let base_dirs: Vec<String> = if use_path.starts_with("super::") {
        let mut dirs = vec![
            normalize_repo_path(&from_dir.to_string_lossy().replace('\\', "/")),
        ];
        if let Some(parent) = from_dir.parent() {
            dirs.push(normalize_repo_path(
                &parent.to_string_lossy().replace('\\', "/"),
            ));
        }
        dirs
    } else if use_path.starts_with("self::") {
        vec![normalize_repo_path(
            &from_dir.to_string_lossy().replace('\\', "/"),
        )]
    } else {
        // crate:: → try from src/ and from root
        vec!["src".to_string(), String::new()]
    };

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
