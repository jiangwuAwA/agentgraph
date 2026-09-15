pub mod extract;
pub mod parser;
pub mod store;
pub mod walker;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::model::IndexStats;

pub struct Indexer {
    pub root: PathBuf,
    pub db_path: PathBuf,
}

impl Indexer {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let db_path = root.join(".agentgraph").join("index.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self { root, db_path })
    }

    pub fn open_store(&self) -> Result<store::Store> {
        store::Store::open(&self.db_path)
    }

    /// Full or incremental index. Unchanged files (same content hash) are skipped.
    pub fn index(&self, force: bool) -> Result<IndexStats> {
        let mut store = self.open_store()?;
        let files = walker::collect_source_files(&self.root)?;
        let mut indexed = 0usize;
        let mut skipped = 0usize;

        for path in &files {
            let rel = path
                .strip_prefix(&self.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let hash = file_hash(path)?;
            if !force {
                if let Some(prev) = store.file_hash(&rel)? {
                    if prev == hash {
                        skipped += 1;
                        continue;
                    }
                }
            }
            let source = std::fs::read_to_string(path)?;
            let lang = crate::model::Language::from_path(&rel)
                .ok_or_else(|| anyhow::anyhow!("unsupported language: {rel}"))?;
            let parsed = extract::extract_file(&source, lang, &rel)?;
            store.replace_file(&rel, &hash, lang.as_str(), &parsed)?;
            indexed += 1;
        }

        // Drop files that no longer exist on disk.
        let keep: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&self.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        store.prune_missing(&keep)?;

        let stats = store.stats(&self.root.to_string_lossy())?;
        eprintln!(
            "indexed {indexed} file(s), skipped {skipped} unchanged; {} symbols, {} refs",
            stats.symbols, stats.references
        );
        Ok(stats)
    }

    pub fn stats(&self) -> Result<IndexStats> {
        let store = self.open_store()?;
        store.stats(&self.root.to_string_lossy())
    }
}

fn file_hash(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}
