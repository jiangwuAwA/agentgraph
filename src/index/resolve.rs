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
