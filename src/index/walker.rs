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

    const NOISE: &[&str] = &[
        "node_modules/",
        "\\node_modules\\",
        "/target/",
        "\\target\\",
        "/dist/",
        "\\dist\\",
        "/build/",
        "\\build\\",
        "/vendor/",
        "\\vendor\\",
        "/.venv/",
        "\\.venv\\",
        "/__pycache__/",
        "\\__pycache__\\",
        "/.git/",
        "\\.git\\",
        "/third_party/",
        "\\third_party\\",
        "/testdata/",
        "\\testdata\\",
        "/fixtures/generated/",
    ];

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
        if NOISE.iter().any(|n| rel_str.contains(&n.replace('\\', "/"))) {
            continue;
        }

        // Skip obviously generated / huge files by size
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
