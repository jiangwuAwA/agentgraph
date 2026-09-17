use anyhow::{anyhow, Result};
use tree_sitter::{Language as TsLanguage, Parser};

use crate::model::Language;

/// Precomputed line starts for O(log n) byte→line / UTF-16 column lookup.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<usize>,
    source_len: usize,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            line_starts,
            source_len: source.len(),
        }
    }

    pub fn line_of(&self, byte_offset: usize) -> usize {
        let off = byte_offset.min(self.source_len);
        self.line_starts.partition_point(|&s| s <= off)
    }

    /// 0-based UTF-16 code-unit column (SCIP/LSIF position encoding).
    pub fn col_utf16(&self, source: &str, byte_offset: usize) -> usize {
        let off = byte_offset.min(self.source_len);
        let line = self.line_of(off);
        let start = self.line_starts[line.saturating_sub(1)];
        source[start..off.min(source.len())]
            .chars()
            .map(|c| c.len_utf16())
            .sum()
    }
}

pub fn ts_language(lang: Language) -> TsLanguage {
    match lang {
        Language::TypeScript | Language::JavaScript => {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        }
        Language::Tsx | Language::Jsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
    }
}

pub fn parse(source: &str, lang: Language) -> Result<tree_sitter::Tree> {
    let mut parser = Parser::new();
    let tsl = ts_language(lang);
    parser
        .set_language(&tsl)
        .map_err(|e| anyhow!("set_language failed: {e}"))?;
    parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("parse returned no tree"))
}

/// Strip Windows `\\?\` UNC prefix so paths are portable for display/MCP.
pub fn normalize_root(path: &std::path::Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{stripped}"));
    }
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        return std::path::PathBuf::from(stripped.to_string());
    }
    path.to_path_buf()
}

/// Compute `path` relative to `root` with `/` separators.
///
/// Direct `normalize_root` + `strip_prefix` first. On mismatch (macOS `/var` vs
/// `/private/var` symlink forms, Windows 8.3 short names, UNC, etc.) resolve
/// both sides and retry — same pattern as the MCP root jail. Returns `None`
/// when `path` is not under `root` even after resolution (caller must skip).
///
/// Deleted leaves cannot be canonicalized; their **parent** still can, so
/// resolve the parent and re-join the file name (watch delete/rename).
pub fn rel_path_under_root(path: &std::path::Path, root: &std::path::Path) -> Option<String> {
    let p_n = normalize_root(path);
    let r_n = normalize_root(root);
    if let Ok(rel) = p_n.strip_prefix(&r_n) {
        return Some(rel.to_string_lossy().replace('\\', "/"));
    }
    let r_c = root.canonicalize().ok()?;
    let r_cn = normalize_root(&r_c);
    let p_resolved = resolve_for_prefix_match(path)?;
    let p_cn = normalize_root(&p_resolved);
    let rel = p_cn.strip_prefix(&r_cn).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Canonicalize `path` if it exists; otherwise canonicalize its parent and
/// re-join the last component (deleted file/dir under a live parent).
fn resolve_for_prefix_match(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Ok(c) = path.canonicalize() {
        return Some(c);
    }
    let parent = path.parent()?;
    let name = path.file_name()?;
    let parent_c = parent.canonicalize().ok()?;
    Some(parent_c.join(name))
}
