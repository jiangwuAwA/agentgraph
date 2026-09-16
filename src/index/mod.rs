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
    mtime_ns: i64,
    size: i64,
}

struct ParsedFile {
    rel: String,
    hash: String,
    lang_str: &'static str,
    extracted: extract::ExtractedFile,
    mtime_ns: i64,
    size: i64,
    subset: subset::SubsetReport,
}

struct FailedFile {
    rel: String,
    reason: String,
}

fn trust_mtime() -> bool {
    // AGENTGRAPH_TRUST_MTIME=0 → always content-hash (mtime is only a hint).
    !matches!(
        std::env::var("AGENTGRAPH_TRUST_MTIME").as_deref(),
        Ok("0") | Ok("false") | Ok("FALSE")
    )
}

fn trace_enabled() -> bool {
    matches!(
        std::env::var("AGENTGRAPH_TRACE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

fn emit_trace(timings: &serde_json::Value) {
    if trace_enabled() {
        eprintln!("agentgraph_trace {timings}");
    }
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
    ///
    /// Perf-plan P0: mtime/size short-circuit, parallel hash, dirty early-out,
    /// incremental sid relink. Content hash remains the source of truth when
    /// metadata mismatches.
    pub fn index(&self, force: bool) -> Result<IndexStats> {
        let mut store = self.open_store()?;
        let t0 = std::time::Instant::now();
        let collected = walker::collect_source_files_with_stats(&self.root)?;
        let walk_ms = t0.elapsed().as_millis();

        let known: std::collections::HashSet<String> =
            collected.files.iter().map(|f| f.rel.clone()).collect();

        // Phase 1: metadata short-circuit, then read+hash only candidates (parallel).
        let t_hash = std::time::Instant::now();
        let mut to_parse: Vec<FileWork> = Vec::new();
        let mut skipped = 0usize;
        let mut failed_read = 0usize;
        let mut meta_skipped = 0usize;

        // Candidates needing open+hash: new, force, or mtime/size mismatch.
        struct Candidate {
            path: PathBuf,
            rel: String,
            mtime_ns: i64,
            size: i64,
            lang: Language,
        }
        let mut candidates = Vec::new();
        for f in &collected.files {
            let Some(lang) = Language::from_path(&f.rel) else {
                continue;
            };
            if !force && trust_mtime() {
                if let Ok(Some(prev)) = store.file_meta(&f.rel) {
                    if prev.mtime_ns == f.mtime_ns && prev.size == f.size && f.mtime_ns != 0 {
                        skipped += 1;
                        meta_skipped += 1;
                        continue;
                    }
                }
            }
            candidates.push(Candidate {
                path: f.path.clone(),
                rel: f.rel.clone(),
                mtime_ns: f.mtime_ns,
                size: f.size,
                lang,
            });
        }

        // Parallel read+hash (P0-2).
        #[allow(clippy::type_complexity)]
        let hashed: Vec<Result<(String, String, i64, i64, Language, Vec<u8>), String>> = candidates
            .into_par_iter()
            .map(|c| match std::fs::read(&c.path) {
                Ok(bytes) => {
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    let hash = format!("{:x}", hasher.finalize());
                    Ok((c.rel, hash, c.mtime_ns, c.size, c.lang, bytes))
                }
                Err(e) => Err(format!("{}: {e}", c.rel)),
            })
            .collect();

        for item in hashed {
            match item {
                Err(msg) => {
                    eprintln!("skip {msg}");
                    failed_read += 1;
                }
                Ok((rel, hash, mtime_ns, size, lang, bytes)) => {
                    if !force {
                        if let Ok(Some(prev)) = store.file_meta(&rel) {
                            if prev.hash == hash {
                                skipped += 1;
                                // Refresh mtime so next noop can short-circuit.
                                let _ = store.update_file_meta(&rel, mtime_ns, size);
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
                    to_parse.push(FileWork {
                        rel,
                        hash,
                        source,
                        lang,
                        mtime_ns,
                        size,
                    });
                }
            }
        }
        let hash_ms = t_hash.elapsed().as_millis();

        // Deleted paths (for early-out and prune).
        let db_paths = store.list_paths()?;
        let deleted: Vec<String> = db_paths.difference(&known).cloned().collect();

        // P0-3: nothing dirty → skip transaction + full resolve.
        // Still repair dispatch if a prior run left dispatch_dirty (C2).
        if !force && to_parse.is_empty() && deleted.is_empty() {
            if store.dispatch_dirty()? {
                store.link_event_dispatch()?;
            }
            let mut stats = store.stats(&self.root.to_string_lossy())?;
            stats.skipped_files = skipped;
            stats.failed_files = failed_read;
            stats.oversized_files = collected.oversized_skipped;
            emit_trace(&serde_json::json!({
                "phase": "noop_early_out",
                "walk_ms": walk_ms,
                "read_hash_ms": hash_ms,
                "skipped": skipped,
                "meta_skipped": meta_skipped,
                "dirty": 0,
                "files": collected.files.len(),
            }));
            eprintln!(
                "index noop: skipped {skipped} unchanged ({} meta), failed {failed_read}; {} symbols, {} refs",
                meta_skipped, stats.symbols, stats.references
            );
            return Ok(stats);
        }

        // Phase 2: parallel parse/extract.
        let t_parse = std::time::Instant::now();
        let known_ref = &known;
        let parsed: Vec<Result<ParsedFile, FailedFile>> = to_parse
            .into_par_iter()
            .map(|fw| {
                let lang_str = fw.lang.as_str();
                match extract::extract_file(&fw.source, fw.lang, &fw.rel, known_ref) {
                    Ok(extracted) => {
                        let subset = subset::scan_subset(&fw.source, fw.lang, &fw.rel);
                        Ok(ParsedFile {
                            rel: fw.rel,
                            hash: fw.hash,
                            lang_str,
                            extracted,
                            mtime_ns: fw.mtime_ns,
                            size: fw.size,
                            subset,
                        })
                    }
                    Err(e) => Err(FailedFile {
                        rel: fw.rel,
                        reason: format!("{e:#}"),
                    }),
                }
            })
            .collect();
        let parse_ms = t_parse.elapsed().as_millis();

        let mut dirty_paths: Vec<String> = Vec::new();
        let mut indexed = 0usize;
        let mut failed_parse = 0usize;
        let t_db = std::time::Instant::now();
        store.begin_batch()?;
        for item in parsed {
            match item {
                Ok(pf) => {
                    // Per-file savepoint: a DB error for one file must not poison
                    // the outer transaction or leave partial rows for that path.
                    store.begin_savepoint("file_sp")?;
                    match store.replace_file_with_subset_meta(
                        &pf.rel,
                        &pf.hash,
                        pf.lang_str,
                        &pf.extracted,
                        &pf.subset,
                        pf.mtime_ns,
                        pf.size,
                    ) {
                        Ok(()) => {
                            store.release_savepoint("file_sp")?;
                            indexed += 1;
                            dirty_paths.push(pf.rel);
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
        let db_ms = t_db.elapsed().as_millis();

        // L2: emit↔on dispatch closure (corpus-wide) before sid link.
        let dispatched = store.link_event_dispatch()?;
        if dispatched > 0 {
            eprintln!("event dispatch edges: {dispatched}");
        }

        // Incremental sid + always run qualifier pass when dirty (P0-4/5).
        let t_sid = std::time::Instant::now();
        let linked = if force {
            store.resolve_symbol_ids()?
        } else if !dirty_paths.is_empty() || !deleted.is_empty() {
            let mut dirty = dirty_paths.clone();
            dirty.extend(deleted.iter().cloned());
            store.resolve_symbol_ids_for_paths(&dirty)?
        } else {
            0
        };
        let sid_ms = t_sid.elapsed().as_millis();

        let t_qual = std::time::Instant::now();
        let upgraded = if force || indexed > 0 || !deleted.is_empty() {
            store.resolve_qualifiers()?
        } else {
            0
        };
        let qual_ms = t_qual.elapsed().as_millis();

        let t_stats = std::time::Instant::now();
        let mut stats = store.stats(&self.root.to_string_lossy())?;
        stats.skipped_files = skipped;
        stats.failed_files = failed_read + failed_parse;
        stats.oversized_files = collected.oversized_skipped;
        let stats_ms = t_stats.elapsed().as_millis();

        emit_trace(&serde_json::json!({
            "walk_ms": walk_ms,
            "read_hash_ms": hash_ms,
            "parse_extract_ms": parse_ms,
            "db_replace_ms": db_ms,
            "resolve_sids_ms": sid_ms,
            "resolve_qualifiers_ms": qual_ms,
            "stats_ms": stats_ms,
            "files": collected.files.len(),
            "dirty": indexed,
            "skipped": skipped,
            "meta_skipped": meta_skipped,
            "deleted": deleted.len(),
        }));

        let conf_summary: Vec<String> = stats
            .refs_by_confidence
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        eprintln!(
            "indexed {indexed} file(s), skipped {skipped} unchanged ({} meta), failed {}; oversize-skip {}; {} symbols, {} refs [{}], {} described",
            meta_skipped,
            stats.failed_files,
            collected.oversized_skipped,
            stats.symbols,
            stats.references,
            conf_summary.join(","),
            stats.described
        );
        eprintln!("resolved_symbol_id on {linked} ref(s); upgraded {upgraded} qualifier(s)");
        Ok(stats)
    }

    /// Path-scoped incremental index (watch UX / perf-plan P1-1).
    ///
    /// Falls back to full `index(false)` when the path set is large or unknown.
    pub fn index_paths(&self, paths: &[PathBuf]) -> Result<IndexStats> {
        const MAX_SCOPED: usize = 64;
        if paths.is_empty() || paths.len() > MAX_SCOPED {
            return self.index(false);
        }
        let mut store = self.open_store()?;
        let mut to_parse = Vec::new();
        let mut failed_read = 0usize;
        let mut deleted: Vec<String> = Vec::new();

        for path in paths {
            let rel = path
                .strip_prefix(&self.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            if !path.exists() {
                deleted.push(rel);
                continue;
            }
            let Some(lang) = Language::from_path(&rel) else {
                continue;
            };
            let meta = std::fs::metadata(path).ok();
            let mtime_ns = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i64)
                .unwrap_or(0);
            let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
            // mtime short-circuit (same as full index); AGENTGRAPH_TRUST_MTIME=0 forces hash.
            if trust_mtime() {
                if let Ok(Some(prev)) = store.file_meta(&rel) {
                    if prev.mtime_ns == mtime_ns && prev.size == size && mtime_ns != 0 {
                        continue;
                    }
                }
            }
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
            if let Ok(Some(prev)) = store.file_meta(&rel) {
                if prev.hash == hash {
                    let _ = store.update_file_meta(&rel, mtime_ns, size);
                    continue;
                }
            }
            let source = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    failed_read += 1;
                    continue;
                }
            };
            to_parse.push(FileWork {
                rel,
                hash,
                source,
                lang,
                mtime_ns,
                size,
            });
        }

        if to_parse.is_empty() && deleted.is_empty() {
            let mut stats = store.stats(&self.root.to_string_lossy())?;
            stats.failed_files = failed_read;
            return Ok(stats);
        }

        let known: std::collections::HashSet<String> = walker::collect_source_files(&self.root)?
            .iter()
            .map(|p| {
                p.strip_prefix(&self.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        let mut dirty_paths: Vec<String> = Vec::new();
        store.begin_batch()?;
        for fw in to_parse {
            let lang_str = fw.lang.as_str();
            match extract::extract_file(&fw.source, fw.lang, &fw.rel, &known) {
                Ok(extracted) => {
                    let subset = subset::scan_subset(&fw.source, fw.lang, &fw.rel);
                    store.begin_savepoint("file_sp")?;
                    match store.replace_file_with_subset_meta(
                        &fw.rel,
                        &fw.hash,
                        lang_str,
                        &extracted,
                        &subset,
                        fw.mtime_ns,
                        fw.size,
                    ) {
                        Ok(()) => {
                            store.release_savepoint("file_sp")?;
                            dirty_paths.push(fw.rel);
                        }
                        Err(e) => {
                            store.rollback_savepoint("file_sp")?;
                            eprintln!("db error {}: {e:#}", fw.rel);
                        }
                    }
                }
                Err(e) => eprintln!("parse fail {}: {e:#}", fw.rel),
            }
        }
        if !deleted.is_empty() {
            // Drop deleted paths from DB via prune of remaining tree.
            let keep: Vec<String> = known.into_iter().collect();
            store.prune_missing(&keep)?;
        }
        store.commit_batch()?;
        // Rebuild emit↔on dispatch BEFORE sid resolve so new dispatch refs get sids.
        store.link_event_dispatch()?;
        let mut dirty = dirty_paths.clone();
        dirty.extend(deleted.iter().cloned());
        if !dirty.is_empty() {
            store.resolve_symbol_ids_for_paths(&dirty)?;
            store.resolve_qualifiers()?;
        }
        let mut stats = store.stats(&self.root.to_string_lossy())?;
        stats.failed_files = failed_read;
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
            let mut pending_paths: std::collections::HashSet<PathBuf> =
                std::collections::HashSet::new();

            loop {
                let tick = Duration::from_millis(debounce.as_millis().max(10) as u64);
                match raw_rx.recv_timeout(tick) {
                    Ok(Ok(ev)) => {
                        if is_source_event(&ev, &root) {
                            pending = true;
                            last_event = Instant::now();
                            for p in ev.paths {
                                pending_paths.insert(p);
                            }
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                if pending && last_event.elapsed() >= debounce {
                    pending = false;
                    let paths: Vec<PathBuf> = pending_paths.drain().collect();
                    let indexer = Indexer {
                        root: root.clone(),
                        db_path: db_path.clone(),
                    };
                    let result = if paths.is_empty() {
                        indexer.index(false)
                    } else {
                        indexer.index_paths(&paths)
                    };
                    match result {
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
        // Normalize UNC prefixes so `\\?\C:\...` event paths still match root.
        let p_n = parser::normalize_root(p);
        let r_n = parser::normalize_root(root);
        if !p_n.starts_with(&r_n) && !p.starts_with(root) {
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
