use anyhow::Result;
use ignore::WalkBuilder;
use std::path::PathBuf;

use crate::model::Language;

/// Walk the repo, respecting .gitignore, collecting supported source files.
pub fn collect_source_files(root: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel_str = rel.to_string_lossy();

        // Skip our own index and common noise.
        if rel_str.starts_with(".agentgraph")
            || rel_str.contains("node_modules/")
            || rel_str.contains("\\node_modules\\")
            || rel_str.contains("/target/")
            || rel_str.contains("\\target\\")
            || rel_str.contains(".min.")
        {
            continue;
        }

        if Language::from_path(&rel_str).is_some() {
            out.push(path.to_path_buf());
        }
    }

    out.sort();
    Ok(out)
}
