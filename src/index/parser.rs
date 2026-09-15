use anyhow::{anyhow, Result};
use tree_sitter::{Language as TsLanguage, Parser};

use crate::model::Language;

pub fn ts_language(lang: Language) -> TsLanguage {
    match lang {
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
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

/// 0-based line -> 1-based line number
pub fn line_of(byte_offset: usize, source: &str) -> usize {
    let mut line = 1usize;
    for (i, ch) in source.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}
