use anyhow::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use super::parser::normalize_root;
use crate::model::Language;

/// One collectible source file with freshness metadata (perf-plan P0-1).
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    /// Repo-relative path with `/` separators.
    pub rel: String,
    pub mtime_ns: i64,
    pub size: i64,
}

/// Result of a source-file walk, including silent skips that used to vanish.
#[derive(Debug, Clone)]
pub struct CollectResult {
    pub files: Vec<SourceFile>,
    /// Supported source files skipped because they exceeded the size cap (1.5 MiB).
    pub oversized_skipped: usize,
    /// Supported source files skipped by the noise-dir filter (testdata, …).
    pub noise_skipped: usize,
    /// Paths skipped as oversized (mint S violations — R13 M3).
    pub oversized_paths: Vec<String>,
    /// Paths skipped as `.min.` bundles (mint S violations — R13 M3).
    pub minified_paths: Vec<String>,
}

/// Walk the repo, respecting .gitignore, collecting supported source files.
pub fn collect_source_files(root: &Path) -> Result<Vec<PathBuf>> {
    Ok(collect_source_files_with_stats(root)?
        .files
        .into_iter()
        .map(|f| f.path)
        .collect())
}

fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Same as [`collect_source_files`] but also reports oversized-file skips (m10).
pub fn collect_source_files_with_stats(root: &Path) -> Result<CollectResult> {
    let mut out = Vec::new();
    let mut oversized_skipped = 0usize;
    let mut noise_skipped = 0usize;
    let mut oversized_paths = Vec::new();
    let mut minified_paths = Vec::new();
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
        let p_n = normalize_root(path);
        let root_n = normalize_root(root);
        let rel = p_n.strip_prefix(&root_n).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if rel_str.starts_with(".agentgraph") {
            continue;
        }
        let is_source = Language::from_path(&rel_str).is_some();
        if rel_str.contains(".min.") {
            if is_source {
                minified_paths.push(rel_str);
            }
            continue;
        }
        // Skip if any path segment is a noise dir — **count** source skips (R4 m).
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
            if is_source {
                noise_skipped += 1;
            }
            continue;
        }

        let meta = entry.metadata().ok();
        if let Some(meta) = &meta {
            if meta.len() > 1_500_000 {
                if is_source {
                    oversized_skipped += 1;
                    oversized_paths.push(rel_str);
                }
                continue;
            }
        }

        if is_source {
            let (mtime_ns, size) = match &meta {
                Some(m) => (mtime_ns(m), m.len() as i64),
                None => (0, 0),
            };
            out.push(SourceFile {
                path: path.to_path_buf(),
                rel: rel_str,
                mtime_ns,
                size,
            });
        }
    }

    Ok(CollectResult {
        files: out,
        oversized_skipped,
        noise_skipped,
        oversized_paths,
        minified_paths,
    })
}
