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

        let mut known: std::collections::HashSet<String> =
            collected.files.iter().map(|f| f.rel.clone()).collect();
        // R23: oversized / minified sources mint S violations (record_parse_error)
        // but are NOT in `files` (walker skips them). They must stay in the
        // keep-set — otherwise prune_missing CASCADE-deletes the violation rows
        // in the same pass and `--sound` wrongly claims in_subset=true.
        for p in collected
            .oversized_paths
            .iter()
            .chain(collected.minified_paths.iter())
        {
            known.insert(p.clone());
        }

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
        // R24: unreadable / non-UTF-8 sources must mint parse_error (keep-set
        // already holds them via walker; without a violation `--sound` claims
        // in_subset=true for a file that was never certified).
        let mut read_or_utf8_failed: Vec<String> = Vec::new();
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

        // Parallel read+hash (P0-2). Err carries (rel, message) so R24 can mint
        // a parse_error without re-parsing a formatted skip line.
        #[allow(clippy::type_complexity)]
        let hashed: Vec<
            Result<(String, String, i64, i64, Language, Vec<u8>), (String, String)>,
        > = candidates
            .into_par_iter()
            .map(|c| match std::fs::read(&c.path) {
                Ok(bytes) => {
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    let hash = format!("{:x}", hasher.finalize());
                    Ok((c.rel, hash, c.mtime_ns, c.size, c.lang, bytes))
                }
                Err(e) => Err((c.rel, format!("{e}"))),
            })
            .collect();

        for item in hashed {
            match item {
                Err((rel, e)) => {
                    eprintln!("skip {rel}: {e}");
                    failed_read += 1;
                    read_or_utf8_failed.push(rel);
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
                            read_or_utf8_failed.push(rel);
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

        // R24: unreadable / non-UTF-8 sources mint parse_error (not silent).
        for p in &read_or_utf8_failed {
            let _ = store.record_parse_error(p, "read/UTF-8 failure — cannot certify S");
        }

        // R13 M3: oversized / minified sources mint S violations (not silent).
        for p in &collected.oversized_paths {
            let _ = store.record_parse_error(p, "oversized source (>1.5MiB) not certified in S");
        }
        for p in &collected.minified_paths {
            let _ = store.record_parse_error(p, "minified bundle (.min.) not certified in S");
        }

        // Deleted paths (for early-out and prune).
        let db_paths = store.list_paths()?;
        let deleted: Vec<String> = db_paths.difference(&known).cloned().collect();

        // P0-3: nothing dirty → skip transaction + full resolve.
        // Still repair dispatch if a prior run left dispatch_dirty (C2).
        // R24: also early-out when the only work was minting parse_error rows
        // for read/UTF-8 failures (they are already written above).
        if !force && to_parse.is_empty() && deleted.is_empty() {
            if store.dispatch_dirty()? {
                store.link_event_dispatch()?;
            }
            let mut stats = store.stats(&self.root.to_string_lossy())?;
            stats.skipped_files = skipped;
            stats.failed_files = failed_read;
            stats.oversized_files = collected.oversized_skipped;
            stats.noise_skipped_files = collected.noise_skipped;
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
                    // R12 M3: failed parse must not leave stale empty violations.
                    store.record_parse_error(&f.rel, &f.reason)?;
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
        stats.noise_skipped_files = collected.noise_skipped;
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
            "indexed {indexed} file(s), skipped {skipped} unchanged ({} meta), failed {}; oversize-skip {}; noise-skip {}; {} symbols, {} refs [{}], {} described",
            meta_skipped,
            stats.failed_files,
            collected.oversized_skipped,
            collected.noise_skipped,
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

        // Directory lifecycle (rename / copy-in): event paths may be folders with
        // no source extension. Scoped index cannot see children by path alone —
        // fall back to a full reindex so new locations are picked up.
        if paths.iter().any(|p| p.is_dir()) {
            return self.index(false);
        }

        for path in paths {
            // Resolve UNC / macOS /var / Windows short-name forms even when the
            // leaf is already deleted (canonicalize parent + re-join).
            let Some(rel) = parser::rel_path_under_root(path, &self.root) else {
                eprintln!(
                    "index_paths: skip {} (outside root {})",
                    path.display(),
                    self.root.display()
                );
                continue;
            };
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
            // R24: watch path must honor the same size / .min. gates as full
            // index. A file that grows past 1.5MiB (or is renamed to *.min.*)
            // must mint an S violation — not be fully parsed and certified.
            if size > 1_500_000 {
                let _ =
                    store.record_parse_error(&rel, "oversized source (>1.5MiB) not certified in S");
                continue;
            }
            if rel.contains(".min.") {
                let _ =
                    store.record_parse_error(&rel, "minified bundle (.min.) not certified in S");
                continue;
            }
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
                    let _ = store.record_parse_error(&rel, &format!("read error: {e}"));
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
                    eprintln!("skip {rel}: not valid UTF-8");
                    failed_read += 1;
                    let _ = store.record_parse_error(&rel, "read/UTF-8 failure — cannot certify S");
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

        // Nothing dirty on the event paths themselves: uncertified / failed
        // rows were already written above. Sibling oversized/minified only
        // need minting when some other file is dirty (walker runs below).
        if to_parse.is_empty() && deleted.is_empty() {
            if store.dispatch_dirty()? {
                store.link_event_dispatch()?;
            }
            let mut stats = store.stats(&self.root.to_string_lossy())?;
            stats.failed_files = failed_read;
            return Ok(stats);
        }

        // Use walker's already-resolved rel paths (same rule as full index).
        // Re-stripping with naive strip_prefix + unwrap_or(abs) would put
        // absolute paths into the keep set and prune every store row.
        // R23: also keep oversized/minified paths so S violations survive prune.
        let collected = walker::collect_source_files_with_stats(&self.root)?;
        let mut known: std::collections::HashSet<String> =
            collected.files.iter().map(|f| f.rel.clone()).collect();
        for p in collected
            .oversized_paths
            .iter()
            .chain(collected.minified_paths.iter())
        {
            known.insert(p.clone());
        }
        // R24: mint oversized/minified violations on the watch path (full index
        // already does this; scoped index only kept them in the prune set).
        for p in &collected.oversized_paths {
            let _ = store.record_parse_error(p, "oversized source (>1.5MiB) not certified in S");
        }
        for p in &collected.minified_paths {
            let _ = store.record_parse_error(p, "minified bundle (.min.) not certified in S");
        }

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
                Err(e) => {
                    eprintln!("parse fail {}: {e:#}", fw.rel);
                    // R24: parse failure on the watch path must not leave the
                    // previous symbols/violations as if the file were still OK.
                    store.record_parse_error(&fw.rel, &format!("{e:#}"))?;
                }
            }
        }
        // Always prune against the live tree walk, not only when the event
        // listed a vanished path. File rename A→B may deliver only the To
        // path; From is then absent from `deleted` and would leave a stale
        // duplicate under A (same content, new path).
        {
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
///
/// Also accepts Remove / rename (ModifyKind::Name) of non-source paths under
/// root: Windows/notify often reports a **directory** path with no extension
/// when a tree is deleted or renamed. Filtering those out left a stale graph.
///
/// Path form must match `rel_path_under_root` (both-sides canonicalize):
/// macOS notify reports `/var/...` while Indexer root is `/private/var/...`,
/// Windows may report `RUNNER~1` short names or `\\?\` UNC. One-sided
/// `starts_with` dropped those events *before* `index_paths` could recover
/// them — recreating the stale-graph class of bugs R21 fixed downstream.
fn is_source_event(ev: &notify::Event, root: &Path) -> bool {
    use notify::event::ModifyKind;
    use notify::EventKind;
    if !matches!(
        ev.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }
    // Remove or rename: path may be a directory that still needs prune/full walk.
    let dir_lifecycle = matches!(ev.kind, EventKind::Remove(_))
        || matches!(ev.kind, EventKind::Modify(ModifyKind::Name(_)));
    ev.paths.iter().any(|p| {
        // Both-sides resolve (parent-canonicalize for deleted leaves) — same
        // rule as index_paths / walker. Reject outside-root before any filter.
        if parser::rel_path_under_root(p, root).is_none() {
            return false;
        }
        // Ignore our own index db and junk.
        let s = p.to_string_lossy().replace('\\', "/");
        if s.contains("/.agentgraph/") || s.contains("/.agentgraph") {
            return false;
        }
        if Language::from_path(&s).is_some() {
            return true;
        }
        // Extensionless Remove/rename under root is likely a directory (or a
        // rename of one). Source files with other extensions already matched
        // above; non-source files (README.md, images) stay filtered.
        dir_lifecycle && std::path::Path::new(&*s).extension().is_none()
    })
}

#[cfg(test)]
mod path_event_tests {
    use super::is_source_event;
    use crate::index::parser;
    use notify::event::{ModifyKind, RenameMode};
    use notify::{Event, EventKind};
    use std::path::{Path, PathBuf};

    fn temp_base(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentgraph-r22-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real").join("src")).unwrap();
        dir
    }

    /// Create a directory alias (`mklink /J` on Windows, symlink on Unix) so
    /// event paths can differ lexically from the canonical root — the same
    /// class as macOS `/var` → `/private/var`.
    fn make_alias(target: &Path, alias: &Path) {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, alias).expect("symlink alias");
        }
        #[cfg(windows)]
        {
            let ok = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    alias.to_str().expect("utf8 alias"),
                    target.to_str().expect("utf8 target"),
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ok {
                std::os::windows::fs::symlink_dir(target, alias).expect("junction/symlink alias");
            }
        }
        // Alias must actually resolve; otherwise the scenario is invalid.
        assert!(
            alias.join("src").is_dir(),
            "alias {} must resolve to target {}",
            alias.display(),
            target.display()
        );
    }

    fn modify_rename(path: PathBuf) -> Event {
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any))).add_path(path)
    }

    fn remove(path: PathBuf) -> Event {
        Event::new(EventKind::Remove(notify::event::RemoveKind::Any)).add_path(path)
    }

    /// Lexical mismatch (symlink/junction alias) must still accept source
    /// events — otherwise watch never calls index_paths and the graph goes stale.
    #[test]
    fn is_source_event_accepts_alias_form_under_root() {
        let base = temp_base("alias");
        let real = base.join("real");
        let alias = base.join("link");
        make_alias(&real, &alias);

        // Canonicalize the way Indexer::new does (normalize strips `\\?\`).
        let root = parser::normalize_root(&real.canonicalize().expect("canonicalize real"));
        let event_path = alias.join("src").join("a.ts");
        // Sanity: event path must NOT lexically start with root.
        assert!(
            !event_path.starts_with(&root),
            "test setup broken: {} unexpectedly starts with {}",
            event_path.display(),
            root.display()
        );

        assert!(
            is_source_event(&modify_rename(event_path.clone()), &root),
            "alias-form source rename must be accepted (root={}, event={})",
            root.display(),
            event_path.display()
        );
        assert!(
            is_source_event(&remove(event_path.clone()), &root),
            "alias-form source remove must be accepted (root={}, event={})",
            root.display(),
            event_path.display()
        );
    }

    /// Deleted leaf under an alias-form parent: parent still exists via the
    /// real path, so parent-canonicalize + re-join must classify it under root.
    #[test]
    fn is_source_event_accepts_deleted_leaf_via_alias_parent() {
        let base = temp_base("alias-del");
        let real = base.join("real");
        let alias = base.join("link");
        make_alias(&real, &alias);

        let root = parser::normalize_root(&real.canonicalize().expect("canonicalize real"));
        let leaf = alias.join("src").join("gone.ts");
        // File never created (or already gone) — Remove of a vanished path.
        assert!(!leaf.exists());
        assert!(
            is_source_event(&remove(leaf.clone()), &root),
            "deleted leaf under live alias parent must be accepted (root={}, event={})",
            root.display(),
            leaf.display()
        );
    }

    /// Directory Remove with no extension under an alias form (Windows notify
    /// reports dir paths on tree delete) must still enter the reindex path.
    #[test]
    fn is_source_event_accepts_alias_directory_remove() {
        let base = temp_base("alias-dir");
        let real = base.join("real");
        let alias = base.join("link");
        make_alias(&real, &alias);

        let root = parser::normalize_root(&real.canonicalize().expect("canonicalize real"));
        let dir = alias.join("src");
        assert!(
            is_source_event(&remove(dir.clone()), &root),
            "alias-form directory remove must be accepted (root={}, event={})",
            root.display(),
            dir.display()
        );
    }

    /// Outside-root paths must stay rejected even with the stronger check.
    #[test]
    fn is_source_event_rejects_outside_root() {
        let base = temp_base("outside");
        let real = base.join("real");
        std::fs::create_dir_all(base.join("other")).unwrap();
        std::fs::write(base.join("other/b.ts"), "x").unwrap();
        let root = parser::normalize_root(&real.canonicalize().expect("canonicalize real"));
        let outside = base.join("other").join("b.ts");
        assert!(
            !is_source_event(&modify_rename(outside.clone()), &root),
            "outside-root event must be rejected: {}",
            outside.display()
        );
    }

    /// Lexical match still works (fast path) — no regression for same-form paths.
    #[test]
    fn is_source_event_accepts_lexical_match() {
        let base = temp_base("lexical");
        let real = base.join("real");
        std::fs::write(real.join("src/a.ts"), "export function helper() {}\n").unwrap();
        let root = parser::normalize_root(&real.canonicalize().expect("canonicalize real"));
        let p = root.join("src").join("a.ts");
        assert!(is_source_event(&modify_rename(p.clone()), &root));
    }
}
