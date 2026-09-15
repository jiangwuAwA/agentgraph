//! Module-path resolution for relative imports (TS/Python/Go/Rust).

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
    let joined = if spec.starts_with("./") || spec.starts_with("../") {
        base_dir.join(spec)
    } else {
        // bare or absolute-from-root style — only resolve relative-looking
        return String::new();
    };
    normalize_repo_path(&joined.to_string_lossy().replace('\\', "/"))
}

fn exists_as(rel: &str, candidates: &[String]) -> Option<String> {
    if candidates.iter().any(|c| c == rel) {
        return Some(rel.to_string());
    }
    None
}

/// Resolve a TypeScript/JavaScript import specifier to a repo-relative file path.
/// `known_files` is the set of indexed file paths (repo-relative, `/` separators).
pub fn resolve_typescript_import(
    from_file: &str,
    specifier: &str,
    known_files: &std::collections::HashSet<String>,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None; // package import — no local edge
    }
    let dir = file_dir(from_file);
    let base = join_rel(&dir, specifier);
    if base.is_empty() {
        return None;
    }
    let exts = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];
    let mut candidates: Vec<String> = Vec::new();
    candidates.push(base.clone());
    for e in exts {
        candidates.push(format!("{base}{e}"));
    }
    for e in exts {
        candidates.push(format!("{base}/index{e}"));
    }
    // also .d.ts
    candidates.push(format!("{base}.d.ts"));

    for c in &candidates {
        if let Some(hit) = exists_as(c, &[]) {
            if known_files.contains(&hit) {
                return Some(hit);
            }
        }
        // exists_as with empty list only exact-matches via contains of candidates later
    }
    for c in &candidates {
        if known_files.contains(c) {
            return Some(c.clone());
        }
    }
    None
}

/// Resolve a Python import (`from .mod import x` / `import pkg.a`) to a repo-relative `.py` path.
pub fn resolve_python_import(
    from_file: &str,
    module: &str,
    level: usize,
    known_files: &std::collections::HashSet<String>,
) -> Option<String> {
    // level > 0 means relative import
    let dir = file_dir(from_file);
    let mut base_dir = dir.clone();
    for _ in 0..level.saturating_sub(1).max(if level > 0 { level - 1 } else { 0 }) {
        base_dir = base_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
    }
    // level=1 means current package; level=2 means parent, etc.
    if level > 0 {
        let mut d = dir.clone();
        for _ in 1..level {
            d = d.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        }
        base_dir = d;
    }

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

    let candidates = [
        format!("{rel_mod}.py"),
        format!("{rel_mod}/__init__.py"),
        format!("{rel_mod}.pyi"),
    ];
    for c in &candidates {
        if known_files.contains(c) {
            return Some(c.clone());
        }
    }
    None
}

/// Resolve a Go import path to a local file if it looks like a module-relative path
/// (best-effort: match by package directory under root). `from_file` unused for stdlib.
pub fn resolve_go_import(
    _from_file: &str,
    import_path: &str,
    known_files: &std::collections::HashSet<String>,
) -> Option<String> {
    // Prefer matching known files whose directory ends with the import path tail.
    // e.g. import "github.com/foo/bar/pkg/x" → files under pkg/x
    let tail_parts: Vec<&str> = import_path.rsplit('/').take(3).collect();
    let tail = tail_parts.iter().rev().cloned().collect::<Vec<_>>().join("/");
    let mut best: Option<String> = None;
    for f in known_files {
        if !f.ends_with(".go") {
            continue;
        }
        if f.replace('\\', "/").contains(&format!("/{tail}/"))
            || f.ends_with(&format!("/{tail}.go"))
        {
            // pick shortest match
            match &best {
                None => best = Some(f.clone()),
                Some(b) if f.len() < b.len() => best = Some(f.clone()),
                _ => {}
            }
        }
    }
    best
}

/// Resolve a Rust `use` path to a local .rs file (best-effort by module path segments).
pub fn resolve_rust_use(
    _from_file: &str,
    use_path: &str,
    known_files: &std::collections::HashSet<String>,
) -> Option<String> {
    // crate::foo::bar → foo/bar.rs or foo/bar/mod.rs
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
    let path = segs.join("/");
    let candidates = [
        format!("src/{path}.rs"),
        format!("src/{path}/mod.rs"),
        format!("{path}.rs"),
        format!("{path}/mod.rs"),
        format!("src/{path}.rs"),
    ];
    for c in &candidates {
        if known_files.contains(c) {
            return Some(c.clone());
        }
    }
    // search any file ending with /path.rs
    let suffix = format!("/{path}.rs");
    let mod_suffix = format!("/{path}/mod.rs");
    let mut best: Option<String> = None;
    for f in known_files {
        let nf = f.replace('\\', "/");
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

// silence unused warning for helper
#[allow(dead_code)]
fn _use_exists_as() {
    let _ = exists_as("x", &[]);
}
