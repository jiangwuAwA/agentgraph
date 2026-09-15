pub mod export;
pub mod extract;
pub mod llm;
pub mod parser;
pub mod resolve;
pub mod rules;
pub mod store;
pub mod subset;
pub mod walker;

use anyhow::Result;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::model::{IndexStats, Language};

pub struct Indexer {
    pub root: PathBuf,
    pub db_path: PathBuf,
}

struct FileWork {
    rel: String,
    hash: String,
    source: String,
    lang: Language,
}

struct ParsedFile {
    rel: String,
    hash: String,
    lang_str: &'static str,
    lang: Language,
    source_for_subset: String,
    extracted: extract::ExtractedFile,
}

struct FailedFile {
    rel: String,
    reason: String,
}

impl Indexer {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = parser::normalize_root(&root.as_ref().canonicalize()?);
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
    /// Parse/extract runs in parallel; DB writes are batched in one transaction.
    pub fn index(&self, force: bool) -> Result<IndexStats> {
        let mut store = self.open_store()?;
        let files = walker::collect_source_files(&self.root)?;

        let known: std::collections::HashSet<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&self.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        // Phase 1: read + hash, skip unchanged / non-utf8.
        let mut to_parse: Vec<FileWork> = Vec::new();
        let mut skipped = 0usize;
        let mut failed_read = 0usize;

        for path in &files {
            let rel = path
                .strip_prefix(&self.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("skip {rel}: read error: {e}");
                    failed_read += 1;
                    continue;
                }
            };
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let hash = format!("{:x}", hasher.finalize());

            if !force {
                if let Ok(Some(prev)) = store.file_hash(&rel) {
                    if prev == hash {
                        skipped += 1;
                        continue;
                    }
                }
            }

            let source = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("skip {rel}: not valid UTF-8");
                    failed_read += 1;
                    continue;
                }
            };

            let Some(lang) = Language::from_path(&rel) else {
                continue;
            };
            to_parse.push(FileWork {
                rel,
                hash,
                source,
                lang,
            });
        }

        // Phase 2: parallel parse/extract.
        let known_ref = &known;
        let parsed: Vec<Result<ParsedFile, FailedFile>> = to_parse
            .into_par_iter()
            .map(|fw| {
                let lang_str = fw.lang.as_str();
                match extract::extract_file(&fw.source, fw.lang, &fw.rel, known_ref) {
                    Ok(extracted) => Ok(ParsedFile {
                        rel: fw.rel,
                        hash: fw.hash,
                        lang_str,
                        lang: fw.lang,
                        source_for_subset: fw.source,
                        extracted,
                    }),
                    Err(e) => Err(FailedFile {
                        rel: fw.rel,
                        reason: format!("{e:#}"),
                    }),
                }
            })
            .collect();

        let mut indexed = 0usize;
        let mut failed_parse = 0usize;
        store.begin_batch()?;
        for item in parsed {
            match item {
                Ok(pf) => {
                    // Per-file savepoint: a DB error for one file must not poison
                    // the outer transaction or leave partial rows for that path.
                    store.begin_savepoint("file_sp")?;
                    let subset = subset::scan_subset(&pf.source_for_subset, pf.lang, &pf.rel);
                    match store.replace_file_with_subset(
                        &pf.rel,
                        &pf.hash,
                        pf.lang_str,
                        &pf.extracted,
                        &subset,
                    ) {
                        Ok(()) => {
                            store.release_savepoint("file_sp")?;
                            indexed += 1;
                        }
                        Err(e) => {
                            store.rollback_savepoint("file_sp")?;
                            eprintln!("db error {}: {e:#}", pf.rel);
                            failed_parse += 1;
                        }
                    }
                }
                Err(f) => {
                    eprintln!("parse fail {}: {}", f.rel, f.reason);
                    failed_parse += 1;
                }
            }
        }

        let keep: Vec<String> = known.into_iter().collect();
        store.prune_missing(&keep)?;
        store.commit_batch()?;
        let linked = store.resolve_symbol_ids()?;
        let upgraded = store.resolve_qualifiers()?;
        eprintln!("resolved_symbol_id on {linked} ref(s); upgraded {upgraded} qualifier(s)");

        let mut stats = store.stats(&self.root.to_string_lossy())?;
        stats.skipped_files = skipped;
        stats.failed_files = failed_read + failed_parse;
        let conf_summary: Vec<String> = stats
            .refs_by_confidence
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        eprintln!(
            "indexed {indexed} file(s), skipped {skipped} unchanged, failed {}; {} symbols, {} refs [{}], {} described",
            stats.failed_files,
            stats.symbols,
            stats.references,
            conf_summary.join(","),
            stats.described
        );
        Ok(stats)
    }

    pub fn stats(&self) -> Result<IndexStats> {
        let store = self.open_store()?;
        store.stats(&self.root.to_string_lossy())
    }

    /// Polling fallback watch (no fsnotify). Prefer `watch_events`.
    pub fn watch(&self, interval_secs: u64) -> Result<()> {
        eprintln!(
            "watching {} every {interval_secs}s (Ctrl+C to stop)",
            self.root.display()
        );
        let mut last_sig: Option<u64> = None;
        loop {
            let sig = self.fingerprint()?;
            if last_sig != Some(sig) {
                match self.index(false) {
                    Ok(s) => eprintln!("reindexed: {} files / {} symbols", s.files, s.symbols),
                    Err(e) => eprintln!("index error: {e:#}"),
                }
                last_sig = Some(sig);
            }
            std::thread::sleep(std::time::Duration::from_secs(interval_secs.max(1)));
        }
    }

    /// fsnotify-backed watch. Receiver yields after each successful incremental reindex.
    /// Debounce coalesces event bursts.
    pub fn watch_events(
        &self,
        debounce: std::time::Duration,
    ) -> Result<(
        std::sync::mpsc::Receiver<IndexStats>,
        std::thread::JoinHandle<()>,
    )> {
        use notify::{RecommendedWatcher, RecursiveMode, Watcher};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let root = self.root.clone();
        let db_path = self.db_path.clone();
        let (tx, rx) = mpsc::channel::<IndexStats>();
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();

        let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        let debounce = if debounce.is_zero() {
            Duration::from_millis(50)
        } else {
            debounce
        };

        let handle = std::thread::spawn(move || {
            let _watcher = watcher;
            let mut pending = false;
            let mut last_event = Instant::now();

            loop {
                let tick = Duration::from_millis(debounce.as_millis().max(10) as u64);
                match raw_rx.recv_timeout(tick) {
                    Ok(Ok(ev)) => {
                        if is_source_event(&ev, &root) {
                            pending = true;
                            last_event = Instant::now();
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                if pending && last_event.elapsed() >= debounce {
                    pending = false;
                    let indexer = Indexer {
                        root: root.clone(),
                        db_path: db_path.clone(),
                    };
                    match indexer.index(false) {
                        Ok(stats) => {
                            if tx.send(stats).is_err() {
                                break;
                            }
                        }
                        Err(e) => eprintln!("watch reindex error: {e:#}"),
                    }
                }
            }
        });

        Ok((rx, handle))
    }

    fn fingerprint(&self) -> Result<u64> {
        let files = walker::collect_source_files(&self.root)?;
        let mut h = 0u64;
        for p in files {
            if let Ok(meta) = std::fs::metadata(&p) {
                if let Ok(mtime) = meta.modified() {
                    if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                        // Include sub-second precision so same-second edits are detected.
                        h = h
                            .wrapping_mul(31)
                            .wrapping_add(d.as_secs())
                            .wrapping_add(d.subsec_nanos() as u64)
                            .wrapping_add(meta.len());
                    }
                }
            }
        }
        Ok(h)
    }
}

/// True if the fs event touches a supported source file under `root`.
fn is_source_event(ev: &notify::Event, root: &Path) -> bool {
    use notify::EventKind;
    if !matches!(
        ev.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }
    ev.paths.iter().any(|p| {
        if !p.starts_with(root) {
            return false;
        }
        // Ignore our own index db and junk.
        let s = p.to_string_lossy().replace('\\', "/");
        if s.contains("/.agentgraph/") || s.contains("/.agentgraph") {
            return false;
        }
        Language::from_path(&s).is_some()
    })
}
