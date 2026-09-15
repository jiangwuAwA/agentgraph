use anyhow::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use crate::model::Language;

/// Walk the repo, respecting .gitignore, collecting supported source files.
pub fn collect_source_files(root: &Path) -> Result<Vec<PathBuf>> {
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
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if rel_str.starts_with(".agentgraph") || rel_str.contains(".min.") {
            continue;
        }
        // Skip if any path segment is a noise dir
        let noisy = rel_str.split('/').any(|seg| {
            matches!(
                seg,
                "node_modules"
                    | "target"
                    | "dist"
                    | "build"
                    | "vendor"
                    | ".venv"
                    | "__pycache__"
                    | ".git"
                    | "third_party"
                    | "testdata"
            )
        });
        if noisy {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            if meta.len() > 1_500_000 {
                continue;
            }
        }

        if Language::from_path(&rel_str).is_some() {
            out.push(path.to_path_buf());
        }
    }

    out.sort();
    Ok(out)
}
