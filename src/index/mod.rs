pub mod diff;
pub mod export;
pub mod extract;
pub mod llm;
pub mod macro_map;
pub mod parser;
pub mod resolve;
pub mod rules;
pub mod store;
pub mod subset;
pub mod walker;
pub mod workspace;

use anyhow::Result;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub use macro_map::{
    map_expanded_path, path_map_from_meta, path_map_to_meta, source_fingerprint,
    tag_macro_impact_json, tag_macro_ref_json, union_callers, union_impact, CrateLayout, PathMap,
    PathMapPair, UnionOptions,
};

use crate::model::{IndexStats, Language, MacroIndexResult, MacroSidecarStatus};

pub use crate::model::DedupStats;

/// Sidecar DB file name under `<root>/.agentgraph/` (P2 optional, CLI default OFF).
pub const MACRO_SIDECAR_DB_NAME: &str = "index.macro.db";

/// Absolute path of the optional macro-expanded sidecar for a project root.
pub fn macro_sidecar_path(root: &Path) -> PathBuf {
    root.join(".agentgraph").join(MACRO_SIDECAR_DB_NAME)
}

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

    /// Indexer bound to an explicit shared SQLite path (workspace multi-root).
    pub fn with_db_path(root: impl AsRef<Path>, db_path: impl AsRef<Path>) -> Result<Self> {
        let root = parser::normalize_root(&root.as_ref().canonicalize()?);
        let db_path = db_path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self { root, db_path })
    }

    pub fn open_store(&self) -> Result<store::Store> {
        store::Store::open(&self.db_path)
    }

    /// `<root>/.agentgraph/index.macro.db` (optional P2 sidecar).
    pub fn macro_sidecar_path(&self) -> PathBuf {
        macro_sidecar_path(&self.root)
    }

    /// Open the macro-expanded sidecar if the file exists. Never creates it.
    /// Absent sidecar → `None` (queries treat as empty; default stays source L0/L1).
    pub fn open_macro_store(&self) -> Result<Option<store::Store>> {
        let path = self.macro_sidecar_path();
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(store::Store::open(&path)?))
    }

    /// Reject expanded roots that nest with the main project root (either direction).
    ///
    /// Under-root expanded trees are ingested by the main walker (graph pollution +
    /// possible main `subset_ok` flip). Ancestor expanded roots make the sidecar
    /// re-index the whole parent with wrong relative paths. Keep the shadow as a
    /// **sibling** directory — see docs/macro-sidecar.md.
    pub fn validate_macro_expanded_root(&self, expanded_root: &Path) -> Result<PathBuf> {
        // Relative expanded roots resolve against **--root**, not the process cwd.
        // Otherwise `index --macro-expanded-root expand-shadow` can silently
        // dual-index an unrelated cwd-relative tree (or bypass nesting checks).
        let joined = if expanded_root.is_absolute() {
            expanded_root.to_path_buf()
        } else {
            self.root.join(expanded_root)
        };
        let expanded_raw = joined
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("macro expanded root '{}': {e}", joined.display()))?;
        let expanded = parser::normalize_root(&expanded_raw);
        let main = parser::normalize_root(&self.root);
        if !expanded.is_dir() {
            anyhow::bail!(
                "macro expanded root is not a directory: {}",
                expanded.display()
            );
        }
        if expanded == main {
            anyhow::bail!(
                "macro expanded root equals project root '{}'; \
                 point --macro-expanded-root at a sibling shadow tree, not --root",
                main.display()
            );
        }
        if expanded.starts_with(&main) {
            anyhow::bail!(
                "macro expanded root '{}' is under project root '{}'; \
                 the main walker would ingest expanded sources and pollute the source graph \
                 (keep the expanded shadow tree outside --root, as a sibling directory)",
                expanded.display(),
                main.display()
            );
        }
        if main.starts_with(&expanded) {
            anyhow::bail!(
                "macro expanded root '{}' contains project root '{}'; \
                 use a sibling shadow tree, not an ancestor of --root",
                expanded.display(),
                main.display()
            );
        }
        Ok(expanded)
    }

    /// Index an expanded shadow tree into the sidecar DB.
    /// Does **not** touch `<root>/.agentgraph/index.db`. Not sound — dual-index
    /// candidates only (Track M1: path map + fingerprint + de-dup at query).
    ///
    /// Validates nesting **before** any main-index side effects when called from CLI;
    /// also re-validates here (defense in depth). Writes `meta.source_fingerprint`
    /// (main source aggregate) and discovered `meta.path_map` pairs.
    pub fn index_macro_expanded(
        &self,
        expanded_root: &Path,
        force: bool,
    ) -> Result<MacroIndexResult> {
        let expanded = self.validate_macro_expanded_root(expanded_root)?;
        let db_path = self.macro_sidecar_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Reuse the extract/index pipeline, but write only into the sidecar path.
        let side = Indexer {
            root: expanded.clone(),
            db_path: db_path.clone(),
        };
        let stats = side.index(force)?;

        // M1: fingerprint of the **main** source tree at sidecar build time +
        // discovered path-map pairs (heuristics + existence-checked exact pairs).
        let fingerprint = macro_map::source_fingerprint(&self.root);
        let layout = CrateLayout::default();
        let path_map = macro_map::discover_path_map(&expanded, &self.root, &layout);
        {
            let store = store::Store::open(&db_path)?;
            store.set_meta("sidecar_kind", "macro_expanded")?;
            store.set_meta("origin", "macro_expanded")?;
            store.set_meta("expanded_root", &expanded.to_string_lossy())?;
            store.set_meta("source_fingerprint", &fingerprint)?;
            store.set_meta("path_map", &macro_map::path_map_to_meta(&path_map))?;
            store.set_meta("rebuild_policy", "manual")?;
        }
        Ok(MacroIndexResult {
            files: stats.files,
            symbols: stats.symbols,
            references: stats.references,
            languages: stats.languages,
            path: db_path.to_string_lossy().into_owned(),
            expanded_root: expanded.to_string_lossy().into_owned(),
            origin: "macro_expanded".to_string(),
            source_fingerprint: Some(fingerprint),
            path_map_present: !path_map.is_empty(),
            stale: false,
        })
    }

    /// Rebuild the sidecar from the **recorded** `expanded_root` (M1 `macro rebuild`).
    /// Idempotent. Fails loudly when no expanded_root was recorded or when the
    /// recorded root currently nests with `--root` (R27).
    pub fn macro_rebuild(&self, force: bool) -> Result<MacroIndexResult> {
        let status = self.macro_status()?;
        if !status.exists {
            anyhow::bail!(
                "macro rebuild: no sidecar at {}; \
                 run `index --macro-expanded-root <sibling-shadow>` first",
                status.path
            );
        }
        let expanded_root = status.expanded_root.ok_or_else(|| {
            anyhow::anyhow!(
                "macro rebuild: sidecar has no recorded expanded_root; \
                 rebuild with `index --macro-expanded-root <sibling-shadow>`"
            )
        })?;
        if status.expanded_root_nested {
            anyhow::bail!(
                "macro rebuild: expanded_root '{expanded_root}' currently nests with --root \
                 (R27); move the shadow tree outside --root before rebuild"
            );
        }
        if status.expanded_root_missing {
            anyhow::bail!(
                "macro rebuild: expanded_root '{expanded_root}' does not exist on disk; \
                 restore the shadow tree or re-run index --macro-expanded-root"
            );
        }
        // force=true: explicit rebuild always re-parses the expanded tree.
        self.index_macro_expanded(Path::new(&expanded_root), force)
    }

    /// Layout + path map used at query time for `--with-macro` unions.
    pub fn macro_crate_layout(&self) -> Result<(CrateLayout, PathMap, bool)> {
        let path = self.macro_sidecar_path();
        if !path.exists() {
            return Ok((CrateLayout::default(), PathMap::default(), false));
        }
        let store = store::Store::open(&path)?;
        let map = macro_map::path_map_from_meta(store.get_meta("path_map")?.as_deref());
        let present = !map.is_empty();
        let layout = CrateLayout::from_path_map(&map);
        Ok((layout, map, present))
    }

    /// Current vs recorded sidecar source fingerprint → stale flag.
    ///
    /// Missing recorded fingerprint (pre-M1 sidecar) → `stale=true` is **not**
    /// forced; we report `stale=false` but leave `source_fingerprint: None` so
    /// operators can `macro rebuild` to upgrade. Present + mismatch → stale.
    pub fn macro_sidecar_stale(&self, recorded: Option<&str>) -> bool {
        match recorded {
            None => false,
            Some("") => false,
            Some(rec) => {
                let current = macro_map::source_fingerprint(&self.root);
                current != rec
            }
        }
    }

    /// Write last union de-dup stats into sidecar meta (for `macro status`).
    pub fn store_dedup_stats(&self, stats: &DedupStats) -> Result<()> {
        let path = self.macro_sidecar_path();
        if !path.exists() {
            return Ok(());
        }
        let store = store::Store::open(&path)?;
        let json = serde_json::to_string(stats)?;
        store.set_meta("dedup_stats", &json)?;
        Ok(())
    }

    /// Whether the sidecar exists + path + counts. Does not create the DB.
    pub fn macro_status(&self) -> Result<MacroSidecarStatus> {
        let path = self.macro_sidecar_path();
        let path_str = path.to_string_lossy().into_owned();
        if !path.exists() {
            return Ok(MacroSidecarStatus {
                path: path_str,
                rebuild_policy: Some("manual".into()),
                ..MacroSidecarStatus::default()
            });
        }
        let store = store::Store::open(&path)?;
        let stats = store.stats(&self.root.to_string_lossy())?;
        let origin = match store.get_meta("origin")? {
            Some(o) => Some(o),
            None => store.get_meta("sidecar_kind")?,
        };
        let expanded_root = store.get_meta("expanded_root")?;
        let expanded_root_missing = expanded_root
            .as_ref()
            .map(|p| !Path::new(p).exists())
            .unwrap_or(false);
        // Re-validate nesting on every status: a sibling expanded tree that was
        // later moved/junctioned under --root is a main-walker pollution hazard
        // even though the sidecar file itself still exists.
        let expanded_root_nested = expanded_root
            .as_ref()
            .filter(|_| !expanded_root_missing)
            .map(|p| self.expanded_root_nests_with_main(Path::new(p)))
            .unwrap_or(false);
        // Sidecar-only S honesty (unsafe/eval in the expanded tree). Does not
        // claim or flip main subset_ok.
        let subset_violation_count = store.subset_violations()?.len();

        let source_fingerprint = store.get_meta("source_fingerprint")?;
        let stale = self.macro_sidecar_stale(source_fingerprint.as_deref());

        let path_map = macro_map::path_map_from_meta(store.get_meta("path_map")?.as_deref());
        let path_map_present = !path_map.is_empty();
        let path_map_pairs: Vec<(String, String)> = path_map
            .pairs
            .iter()
            .map(|p| {
                let tag = if p.prefix { "prefix" } else { "exact" };
                (format!("{}:{tag}", p.expanded), p.source.clone())
            })
            .collect();

        let dedup_stats: DedupStats = match store.get_meta("dedup_stats")? {
            Some(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            None => DedupStats::default(),
        };

        Ok(MacroSidecarStatus {
            exists: true,
            path: path_str,
            files: stats.files,
            symbols: stats.symbols,
            refs: stats.references,
            origin,
            expanded_root,
            expanded_root_missing,
            expanded_root_nested,
            subset_violation_count,
            stale,
            source_fingerprint,
            path_map_present,
            path_map: path_map_pairs,
            dedup_stats,
            rebuild_policy: Some("manual".into()),
        })
    }

    /// True when `expanded` (after canonicalize) equals / is under / contains main root.
    fn expanded_root_nests_with_main(&self, expanded: &Path) -> bool {
        let Ok(raw) = expanded.canonicalize() else {
            return false;
        };
        let exp = parser::normalize_root(&raw);
        let main = parser::normalize_root(&self.root);
        exp == main || exp.starts_with(&main) || main.starts_with(&exp)
    }

    /// Full or incremental index. Unchanged files (same content hash) are skipped.
    /// Parse/extract runs in parallel; DB writes are batched in one transaction.
    ///
    /// Perf-plan P0: mtime/size short-circuit, parallel hash, dirty early-out,
    /// incremental sid relink. Content hash remains the source of truth when
    /// metadata mismatches.
    ///
    /// Classic single-root path: `root_id = ""`.
    pub fn index(&self, force: bool) -> Result<IndexStats> {
        self.index_as(force, "")
    }

    /// Workspace multi-root: stamp rows with `root_id` and scope path ops to it.
    pub fn index_as(&self, force: bool, root_id: &str) -> Result<IndexStats> {
        let mut store = self.open_store()?;
        store.set_write_root(root_id);
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
            // M4: full index (including noop) refreshes the diff baseline snapshot.
            let _ = diff::write_index_snapshot_for_root(&self.root, &store, root_id);
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
        // M4: dual sidecar snapshot + meta.index_seq for `agentgraph diff`.
        if let Err(e) = diff::write_index_snapshot_for_root(&self.root, &store, root_id) {
            eprintln!("warn: failed to write refs snapshot: {e:#}");
        }
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
        // M4 S re-cert: dirty-file subset refresh so `--sound` / `subset` reflect
        // current disk after watch / index_paths (not only last full index).
        if !dirty_paths.is_empty() || !deleted.is_empty() {
            let mut recert: Vec<String> = dirty_paths.clone();
            recert.extend(deleted.iter().cloned());
            if let Err(e) = store.refresh_subset_for_paths(&recert) {
                eprintln!("warn: S re-cert refresh failed: {e:#}");
            }
        }
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
