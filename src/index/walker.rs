use anyhow::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use crate::model::Language;

/// Result of a source-file walk, including silent skips that used to vanish.
#[derive(Debug, Clone)]
pub struct CollectResult {
    pub files: Vec<PathBuf>,
    /// Supported source files skipped because they exceeded the size cap (1.5 MiB).
    pub oversized_skipped: usize,
}

/// Walk the repo, respecting .gitignore, collecting supported source files.
pub fn collect_source_files(root: &Path) -> Result<Vec<PathBuf>> {
    Ok(collect_source_files_with_stats(root)?.files)
}

/// Same as [`collect_source_files`] but also reports oversized-file skips (m10).
pub fn collect_source_files_with_stats(root: &Path) -> Result<CollectResult> {
    let mut out = Vec::new();
    let mut oversized_skipped = 0usize;
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

        let is_source = Language::from_path(&rel_str).is_some();
        if let Ok(meta) = entry.metadata() {
            if meta.len() > 1_500_000 {
                if is_source {
                    oversized_skipped += 1;
                }
                continue;
            }
        }

        if is_source {
            out.push(path.to_path_buf());
        }
    }

    out.sort();
    Ok(CollectResult {
        files: out,
        oversized_skipped,
    })
}
