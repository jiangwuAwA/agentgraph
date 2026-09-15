use anyhow::{anyhow, Result};
use tree_sitter::{Language as TsLanguage, Parser};

use crate::model::Language;

/// Precomputed newline offsets for O(log n) byte→line lookup.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset where each 1-based line starts.
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

    /// 1-based line number for a byte offset.
    pub fn line_of(&self, byte_offset: usize) -> usize {
        let off = byte_offset.min(self.source_len);
        self.line_starts.partition_point(|&s| s <= off)
    }

    /// 0-based column (byte offset within the line).
    pub fn col_of(&self, byte_offset: usize) -> usize {
        let off = byte_offset.min(self.source_len);
        let line = self.line_of(off);
        let start = self.line_starts[line.saturating_sub(1)];
        off.saturating_sub(start)
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
