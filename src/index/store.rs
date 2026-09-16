use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use super::extract::ExtractedFile;
use super::subset::{is_sound_eligible, SubsetReport, SubsetViolation};
use crate::model::{
    Confidence, ConfidenceFilter, EdgeKind, Evidence, ImpactNode, IndexStats, ReferenceRecord,
    SymbolKind, SymbolRecord,
};

fn filter_key(f: ConfidenceFilter) -> u8 {
    match f {
        ConfidenceFilter::ExactOnly => 0,
        ConfidenceFilter::Default => 1,
        ConfidenceFilter::IncludeDynamic => 2,
    }
}

/// SQL fragment constraining `refs.confidence` for a query filter.
/// Literals are fixed — never interpolate user input into this.
fn confidence_where(filter: ConfidenceFilter) -> &'static str {
    match filter {
        ConfidenceFilter::ExactOnly => "confidence = 'exact'",
        ConfidenceFilter::Default => "confidence IN ('exact', 'heuristic')",
        ConfidenceFilter::IncludeDynamic => "1=1",
    }
}

#[derive(Default)]
struct QueryCache {
    callers: HashMap<(String, usize, u8), Vec<ReferenceRecord>>,
    impact: HashMap<(String, usize, usize, u8), Vec<ImpactNode>>,
    hits: u64,
    misses: u64,
    max_entries: usize,
}

impl QueryCache {
    fn new() -> Self {
        Self {
            max_entries: 256,
            ..Default::default()
        }
    }

    fn clear(&mut self) {
        self.callers.clear();
        self.impact.clear();
    }

    fn note_miss(&mut self) {
        self.misses += 1;
    }

    fn note_hit(&mut self) {
        self.hits += 1;
    }

    fn evict_if_needed(map_len: usize, max: usize) -> bool {
        map_len >= max
    }
}

pub struct Store {
    conn: Connection,
    cache: RefCell<QueryCache>,
    /// True when refs/symbols may have stale `resolved_symbol_id` after partial relink.
    sid_dirty: std::cell::Cell<bool>,
}

/// File freshness metadata (content hash + mtime/size short-circuit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetaRow {
    pub hash: String,
    pub mtime_ns: i64,
    pub size: i64,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("open db {}", path.display()))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;
            PRAGMA cache_size = -64000;
            PRAGMA temp_store = MEMORY;

            CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                hash TEXT NOT NULL,
                language TEXT NOT NULL,
                mtime_ns INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                parent TEXT,
                description TEXT,
                start_col INTEGER NOT NULL DEFAULT 0,
                end_col INTEGER NOT NULL DEFAULT 0,
                return_type TEXT,
                FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS refs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                path TEXT NOT NULL,
                line INTEGER NOT NULL,
                enclosing TEXT,
                module TEXT,
                resolved TEXT,
                qualifier TEXT,
                resolved_symbol_id INTEGER,
                confidence TEXT NOT NULL DEFAULT 'exact',
                evidence TEXT,
                qual_name TEXT,
                rule_id TEXT,
                FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS subset_violations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                kind TEXT NOT NULL,
                line INTEGER NOT NULL,
                snippet TEXT NOT NULL,
                FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_qname ON symbols(qualified_name);
            CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
            CREATE INDEX IF NOT EXISTS idx_refs_name ON refs(name);
            CREATE INDEX IF NOT EXISTS idx_refs_path ON refs(path);
            CREATE INDEX IF NOT EXISTS idx_subset_path ON subset_violations(path);
            "#,
        )?;
        let _ = conn.execute("ALTER TABLE symbols ADD COLUMN description TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN module TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN resolved TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN qualifier TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN resolved_symbol_id INTEGER", []);
        let _ = conn.execute(
            "ALTER TABLE refs ADD COLUMN confidence TEXT NOT NULL DEFAULT 'exact'",
            [],
        );
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN evidence TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE files ADD COLUMN mtime_ns INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE files ADD COLUMN size INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN qual_name TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN rule_id TEXT", []);
        // R8: original qualifier before Exact upgrade (for revoke when factory collides).
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN pre_qual TEXT", []);
        // Backfill: pre-L1 rows are Exact by definition.
        let _ = conn.execute(
            "UPDATE refs SET confidence = 'exact' WHERE confidence IS NULL",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE symbols ADD COLUMN start_col INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE symbols ADD COLUMN end_col INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute("ALTER TABLE symbols ADD COLUMN return_type TEXT", []);
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_refs_resolved ON refs(resolved);
             CREATE INDEX IF NOT EXISTS idx_refs_resolved_kind ON refs(resolved, kind);
             CREATE INDEX IF NOT EXISTS idx_refs_kind ON refs(kind);
             CREATE INDEX IF NOT EXISTS idx_refs_rsid ON refs(resolved_symbol_id);
             CREATE INDEX IF NOT EXISTS idx_refs_qual_name ON refs(qual_name);
             CREATE INDEX IF NOT EXISTS idx_refs_rule_id ON refs(rule_id);",
        )?;
        Ok(Self {
            conn,
            cache: RefCell::new(QueryCache::new()),
            sid_dirty: std::cell::Cell::new(false),
        })
    }

    pub fn file_hash(&self, path: &str) -> Result<Option<String>> {
        let row = self
            .conn
            .query_row(
                "SELECT hash FROM files WHERE path = ?1",
                params![path],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(row)
    }

    /// Hash + mtime/size for short-circuit skip (perf-plan P0-1).
    pub fn file_meta(&self, path: &str) -> Result<Option<FileMetaRow>> {
        let row = self
            .conn
            .query_row(
                "SELECT hash, mtime_ns, size FROM files WHERE path = ?1",
                params![path],
                |r| {
                    Ok(FileMetaRow {
                        hash: r.get(0)?,
                        mtime_ns: r.get(1)?,
                        size: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// All indexed relative paths (for delete detection / early-out).
    pub fn list_paths(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM files")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for p in rows {
            set.insert(p?);
        }
        Ok(set)
    }

    pub fn sid_dirty(&self) -> bool {
        self.sid_dirty.get()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let v = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(v)
    }

    pub fn dispatch_dirty(&self) -> Result<bool> {
        Ok(self.get_meta("dispatch_dirty")?.as_deref() == Some("1"))
    }

    /// Test helper: drop dispatch rows without clearing dispatch_dirty.
    pub fn clear_dispatch_edges_for_test(&mut self) -> Result<usize> {
        let n = self
            .conn
            .execute("DELETE FROM refs WHERE rule_id = 'ts.event.dispatch'", [])?;
        self.cache.borrow_mut().clear();
        Ok(n)
    }

    /// Refresh mtime/size after a content-hash match (next noop can short-circuit).
    pub fn update_file_meta(&mut self, path: &str, mtime_ns: i64, size: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET mtime_ns = ?1, size = ?2 WHERE path = ?3",
            params![mtime_ns, size, path],
        )?;
        Ok(())
    }

    pub fn begin_batch(&mut self) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    pub fn commit_batch(&mut self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Nested savepoint so a single file failure does not poison the outer batch.
    pub fn begin_savepoint(&mut self, name: &str) -> Result<()> {
        self.conn
            .execute_batch(&format!("SAVEPOINT {}", quote_ident(name)))?;
        Ok(())
    }

    pub fn release_savepoint(&mut self, name: &str) -> Result<()> {
        self.conn
            .execute_batch(&format!("RELEASE {}", quote_ident(name)))?;
        Ok(())
    }

    pub fn rollback_savepoint(&mut self, name: &str) -> Result<()> {
        self.conn
            .execute_batch(&format!("ROLLBACK TO {}", quote_ident(name)))?;
        self.conn
            .execute_batch(&format!("RELEASE {}", quote_ident(name)))?;
        Ok(())
    }

    /// True when the index has at least one file (i.e. has been built).
    pub fn has_index(&self) -> Result<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        Ok(n > 0)
    }

    /// Error if the index is empty — callers/impact/related/importers/export must not silently return [].
    pub fn ensure_indexed(&self) -> Result<()> {
        if !self.has_index()? {
            anyhow::bail!("index is empty — run `agentgraph index` first");
        }
        Ok(())
    }

    /// True when a symbol with this bare name exists (used to gate impact BFS expansion).
    pub fn symbol_name_exists(&self, name: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE name = ?1 LIMIT 1",
            params![name],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Language of the file at `path`, if known.
    pub fn file_language(&self, path: &str) -> Result<Option<String>> {
        let row = self
            .conn
            .query_row(
                "SELECT language FROM files WHERE path = ?1",
                params![path],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(row)
    }

    /// Replace file rows without S-violation payload (tests / legacy).
    pub fn replace_file(
        &mut self,
        path: &str,
        hash: &str,
        language: &str,
        extracted: &ExtractedFile,
    ) -> Result<()> {
        self.replace_file_with_meta(path, hash, language, extracted, 0, 0)
    }

    pub fn replace_file_with_meta(
        &mut self,
        path: &str,
        hash: &str,
        language: &str,
        extracted: &ExtractedFile,
        mtime_ns: i64,
        size: i64,
    ) -> Result<()> {
        let empty = SubsetReport {
            path: path.to_string(),
            language: language.to_string(),
            in_subset: true,
            violations: Vec::new(),
        };
        self.replace_file_with_subset_meta(path, hash, language, extracted, &empty, mtime_ns, size)
    }

    /// Replace file rows. Does NOT open its own transaction (caller uses begin_batch/commit_batch).
    pub fn replace_file_with_subset(
        &mut self,
        path: &str,
        hash: &str,
        language: &str,
        extracted: &ExtractedFile,
        subset: &SubsetReport,
    ) -> Result<()> {
        self.replace_file_with_subset_meta(path, hash, language, extracted, subset, 0, 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn replace_file_with_subset_meta(
        &mut self,
        path: &str,
        hash: &str,
        language: &str,
        extracted: &ExtractedFile,
        subset: &SubsetReport,
        mtime_ns: i64,
        size: i64,
    ) -> Result<()> {
        self.cache.borrow_mut().clear();
        self.sid_dirty.set(true);
        self.set_meta("sid_dirty", "1")?;
        self.set_meta("dispatch_dirty", "1")?;
        // Preserve LLM descriptions for symbols that still exist with same qualified_name.
        let mut old_desc: HashMap<String, String> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT qualified_name, description FROM symbols
                 WHERE path = ?1 AND description IS NOT NULL AND description != ''",
            )?;
            let rows = stmt.query_map(params![path], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (qn, d) = row?;
                old_desc.insert(qn, d);
            }
        }

        self.conn
            .execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        self.conn
            .execute("DELETE FROM refs WHERE path = ?1", params![path])?;
        self.conn.execute(
            "DELETE FROM subset_violations WHERE path = ?1",
            params![path],
        )?;
        self.conn.execute(
            "INSERT INTO files(path, hash, language, mtime_ns, size) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
               hash = excluded.hash,
               language = excluded.language,
               mtime_ns = excluded.mtime_ns,
               size = excluded.size",
            params![path, hash, language, mtime_ns, size],
        )?;
        {
            let mut stmt = self.conn.prepare(
                "INSERT INTO subset_violations(path, kind, line, snippet)
                 VALUES(?1, ?2, ?3, ?4)",
            )?;
            for v in &subset.violations {
                stmt.execute(params![path, v.kind, v.line as i64, v.snippet])?;
            }
        }
        {
            let mut stmt = self.conn.prepare(
                "INSERT INTO symbols(path, name, qualified_name, kind, start_line, end_line, parent, description, start_col, end_col, return_type)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for s in &extracted.symbols {
                let desc = old_desc.get(&s.qualified_name).cloned();
                stmt.execute(params![
                    path,
                    s.name,
                    s.qualified_name,
                    s.kind.as_str(),
                    s.start_line as i64,
                    s.end_line as i64,
                    s.parent,
                    desc,
                    s.start_col as i64,
                    s.end_col as i64,
                    s.return_type,
                ])?;
            }
        }
        {
            let mut stmt = self.conn.prepare(
                "INSERT INTO refs(name, kind, path, line, enclosing, module, resolved, qualifier, confidence, evidence, qual_name, rule_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for r in &extracted.references {
                let evidence_json = r
                    .evidence
                    .as_ref()
                    .and_then(|e| serde_json::to_string(e).ok());
                let rule_id = r.evidence.as_ref().map(|e| e.rule_id.clone());
                let qual_name = match (&r.qualifier, &r.name) {
                    (Some(q), n) if !q.is_empty() => Some(format!("{q}.{n}")),
                    _ => None,
                };
                stmt.execute(params![
                    r.name,
                    r.kind.as_str(),
                    path,
                    r.line as i64,
                    r.enclosing,
                    r.module,
                    r.resolved,
                    r.qualifier,
                    r.confidence.as_str(),
                    evidence_json,
                    qual_name,
                    rule_id,
                ])?;
            }
        }
        Ok(())
    }

    pub fn prune_missing(&mut self, keep_paths: &[String]) -> Result<()> {
        let existing: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT path FROM files")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let keep: std::collections::HashSet<&str> = keep_paths.iter().map(|s| s.as_str()).collect();
        let mut deleted = false;
        for path in existing {
            if !keep.contains(path.as_str()) {
                self.conn
                    .execute("DELETE FROM files WHERE path = ?1", params![path])?;
                deleted = true;
            }
        }
        if deleted {
            self.sid_dirty.set(true);
            self.set_meta("sid_dirty", "1")?;
            // m8: callers/impact cache must not return rows for deleted files.
            self.cache.borrow_mut().clear();
        }
        Ok(())
    }

    pub fn stats(&self, root: &str) -> Result<IndexStats> {
        let files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let symbols: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
        let references: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM refs", [], |r| r.get(0))?;
        let described: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE description IS NOT NULL AND description != ''",
            [],
            |r| r.get(0),
        )?;
        let mut languages = Vec::new();
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT language FROM files ORDER BY language")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            languages.push(row?);
        }
        let mut refs_by_confidence: Vec<(String, usize)> = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT confidence, COUNT(*) FROM refs GROUP BY confidence ORDER BY confidence",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        for row in rows {
            refs_by_confidence.push(row?);
        }
        Ok(IndexStats {
            files: files as usize,
            symbols: symbols as usize,
            references: references as usize,
            languages,
            root: root.to_string(),
            described: described as usize,
            skipped_files: 0,
            failed_files: 0,
            oversized_files: 0,
            noise_skipped_files: 0,
            refs_by_confidence,
        })
    }

    fn escape_like(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    }

    /// Exact match on symbol name or qualified_name (no LIKE).
    pub fn find_symbol_exact(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.path, s.start_line, s.end_line, s.parent, s.description, f.language, s.start_col, s.end_col, s.return_type
             FROM symbols s
             JOIN files f ON f.path = s.path
             WHERE s.name = ?1 OR s.qualified_name = ?1
             ORDER BY
               CASE WHEN s.name = ?1 THEN 0 ELSE 1 END,
               s.path, s.start_line
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![name, limit as i64], |r| {
            Ok(SymbolRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                qualified_name: r.get(2)?,
                kind: SymbolKind::parse(&r.get::<_, String>(3)?),
                path: r.get(4)?,
                language: r.get(9)?,
                start_line: r.get::<_, i64>(5)? as usize,
                end_line: r.get::<_, i64>(6)? as usize,
                parent: r.get(7)?,
                description: r.get(8)?,
                start_col: r.get::<_, i64>(10)? as usize,
                end_col: r.get::<_, i64>(11)? as usize,
                return_type: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Substring (LIKE) fuzzy match on symbol name only.
    pub fn find_symbol_fuzzy(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.path, s.start_line, s.end_line, s.parent, s.description, f.language, s.start_col, s.end_col, s.return_type
             FROM symbols s
             JOIN files f ON f.path = s.path
             WHERE s.name LIKE ?1 ESCAPE '\\'
             ORDER BY s.path, s.start_line
             LIMIT ?2",
        )?;
        let pattern = format!("%{}%", Self::escape_like(name));
        let rows = stmt.query_map(params![pattern, limit as i64], |r| {
            Ok(SymbolRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                qualified_name: r.get(2)?,
                kind: SymbolKind::parse(&r.get::<_, String>(3)?),
                path: r.get(4)?,
                language: r.get(9)?,
                start_line: r.get::<_, i64>(5)? as usize,
                end_line: r.get::<_, i64>(6)? as usize,
                parent: r.get(7)?,
                description: r.get(8)?,
                start_col: r.get::<_, i64>(10)? as usize,
                end_col: r.get::<_, i64>(11)? as usize,
                return_type: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Combined exact + fuzzy (legacy behavior; prefer exact/fuzzy split in new code).
    pub fn find_symbol(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.path, s.start_line, s.end_line, s.parent, s.description, f.language, s.start_col, s.end_col, s.return_type
             FROM symbols s
             JOIN files f ON f.path = s.path
             WHERE s.name = ?1 OR s.qualified_name = ?1 OR s.name LIKE ?2 ESCAPE '\\'
             ORDER BY
               CASE WHEN s.name = ?1 THEN 0 WHEN s.qualified_name = ?1 THEN 1 ELSE 2 END,
               s.path, s.start_line
             LIMIT ?3",
        )?;
        let pattern = format!("%{}%", Self::escape_like(name));
        let rows = stmt.query_map(params![name, pattern, limit as i64], |r| {
            Ok(SymbolRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                qualified_name: r.get(2)?,
                kind: SymbolKind::parse(&r.get::<_, String>(3)?),
                path: r.get(4)?,
                language: r.get(9)?,
                start_line: r.get::<_, i64>(5)? as usize,
                end_line: r.get::<_, i64>(6)? as usize,
                parent: r.get(7)?,
                description: r.get(8)?,
                start_col: r.get::<_, i64>(10)? as usize,
                end_col: r.get::<_, i64>(11)? as usize,
                return_type: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache.borrow().hits
    }

    pub fn cache_misses(&self) -> u64 {
        self.cache.borrow().misses
    }

    pub fn callers(&self, name: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
        self.callers_filtered(name, limit, ConfidenceFilter::Default)
    }

    pub fn callers_filtered(
        &self,
        name: &str,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ReferenceRecord>> {
        let fk = filter_key(filter);
        {
            let mut c = self.cache.borrow_mut();
            if let Some(v) = c.callers.get(&(name.to_string(), limit, fk)).cloned() {
                c.note_hit();
                return Ok(v);
            }
            c.note_miss();
        }
        let result = self.callers_uncached(name, limit, filter)?;
        {
            let mut c = self.cache.borrow_mut();
            if QueryCache::evict_if_needed(c.callers.len(), c.max_entries) {
                c.callers.clear();
            }
            c.callers
                .insert((name.to_string(), limit, fk), result.clone());
        }
        Ok(result)
    }

    fn callers_uncached(
        &self,
        name: &str,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ReferenceRecord>> {
        self.callers_uncached_opt(name, Some(limit), filter)
    }

    /// Same as `callers_uncached` but `limit: None` omits SQL LIMIT (used by
    /// sound walk so post-filter cannot under-approx via early truncation — C5).
    fn callers_uncached_opt(
        &self,
        name: &str,
        limit: Option<usize>,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ReferenceRecord>> {
        // Qualified form `Type.method` / `Type::method` → filter on qualifier+name only.
        let (bare, qual_dot) = if let Some((q, n)) = name.rsplit_once("::") {
            (n.to_string(), format!("{q}.{n}"))
        } else if let Some((q, n)) = name.rsplit_once('.') {
            if q.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            {
                (n.to_string(), name.to_string())
            } else {
                (name.to_string(), String::new())
            }
        } else {
            (name.to_string(), String::new())
        };

        let conf = confidence_where(filter);
        let limit_sql = if limit.is_some() { " LIMIT ?2" } else { "" };
        let sql = if qual_dot.is_empty() {
            format!(
                "SELECT name, kind, path, line, enclosing, module, resolved, qualifier, confidence, evidence
                 FROM refs WHERE name = ?1 AND {conf}
                 ORDER BY path, line{limit_sql}"
            )
        } else {
            // Indexed materialized column (P2-1); fallback expression for legacy rows.
            format!(
                "SELECT name, kind, path, line, enclosing, module, resolved, qualifier, confidence, evidence
                 FROM refs
                 WHERE (qual_name = ?1 OR (qualifier || '.' || name) = ?1 OR (qualifier || '::' || name) = ?1)
                   AND {conf}
                 ORDER BY path, line{limit_sql}"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let key = if qual_dot.is_empty() {
            &bare
        } else {
            &qual_dot
        };
        let rows = match limit {
            Some(l) => stmt.query_map(params![key, l as i64], map_ref)?,
            None => stmt.query_map(params![key], map_ref)?,
        };
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Link refs → symbols.id after a full or incremental index (name + optional qualifier).
    ///
    /// Always starts by clearing all `resolved_symbol_id` values, then fully relinks.
    /// This avoids dangling ids after DELETE+INSERT of a file whose symbols were targets.
    pub fn resolve_symbol_ids(&mut self) -> Result<usize> {
        let n = self.resolve_symbol_ids_core(true, &[])?;
        self.sid_dirty.set(false);
        self.set_meta("sid_dirty", "0")?;
        Ok(n)
    }

    /// Relink only refs touching `dirty_paths` (and inbound links to those paths' old sids).
    pub fn resolve_symbol_ids_for_paths(&mut self, dirty_paths: &[String]) -> Result<usize> {
        if dirty_paths.is_empty() {
            return Ok(0);
        }
        let n = self.resolve_symbol_ids_core(false, dirty_paths)?;
        // Partial relink may leave other paths' inbound links stale if names collide.
        self.sid_dirty.set(true);
        self.set_meta("sid_dirty", "1")?;
        Ok(n)
    }

    /// Merge process-local flag with persisted meta (never clear an in-memory dirty).
    pub fn load_sid_dirty(&self) -> Result<bool> {
        let persisted = self.get_meta("sid_dirty")?.as_deref() == Some("1");
        if persisted {
            self.sid_dirty.set(true);
        }
        Ok(self.sid_dirty.get())
    }

    /// L2 production S-sound: pair `emit(evt)` sites with `on(evt, handler)`
    /// registrations (event name stored in `refs.module`) and insert
    /// Heuristic dispatch edges so runtime emit→handler is in the sound walk.
    ///
    /// **Idempotent:** deletes all prior `ts.event.dispatch` rows first
    /// (Critical fix: incremental index must not accumulate duplicates).
    pub fn link_event_dispatch(&mut self) -> Result<usize> {
        // Join outer batch via SAVEPOINT; standalone uses BEGIN IMMEDIATE.
        let in_tx = !self.conn.is_autocommit();
        if in_tx {
            self.conn.execute_batch("SAVEPOINT dispatch_sp")?;
        } else {
            self.conn.execute_batch("BEGIN IMMEDIATE")?;
        }
        let result = (|| -> Result<usize> {
            self.conn
                .execute("DELETE FROM refs WHERE rule_id = 'ts.event.dispatch'", [])?;

            // (enclosing_or_empty, path, line, event)
            let emits: Vec<(String, String, i64, String)> = {
                let mut stmt = self.conn.prepare(
                    "SELECT COALESCE(enclosing,''), path, line, module FROM refs
                     WHERE module IS NOT NULL AND module != ''
                       AND rule_id = 'ts.event.emit'",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            let mut handlers: HashMap<String, Vec<String>> = HashMap::new();
            {
                let mut stmt = self.conn.prepare(
                    "SELECT name, module FROM refs
                     WHERE module IS NOT NULL AND module != ''
                       AND rule_id = 'ts.event.subscribe'",
                )?;
                let rows =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                for row in rows {
                    let (name, evt) = row?;
                    let list = handlers.entry(evt).or_default();
                    if !list.contains(&name) {
                        list.push(name);
                    }
                }
            }

            let mut created = 0usize;
            let mut stmt = self.conn.prepare_cached(
                "INSERT INTO refs(name, kind, path, line, enclosing, module, resolved, qualifier, confidence, evidence, qual_name, rule_id)
                 VALUES(?1, 'call', ?2, ?3, ?4, ?5, NULL, NULL, 'heuristic', ?6, NULL, 'ts.event.dispatch')",
            )?;
            for (enclosing, path, line, evt) in emits {
                let Some(hs) = handlers.get(&evt) else {
                    continue;
                };
                for h in hs {
                    let evidence = serde_json::json!({
                        "rule_id": "ts.event.dispatch",
                        "snippet": format!("emit('{evt}') → {h}"),
                    })
                    .to_string();
                    stmt.execute(params![h, path, line, enclosing, evt, evidence])?;
                    created += 1;
                }
            }
            Ok(created)
        })();
        match result {
            Ok(created) => {
                if in_tx {
                    self.conn.execute_batch("RELEASE dispatch_sp")?;
                } else {
                    self.conn.execute_batch("COMMIT")?;
                }
                self.cache.borrow_mut().clear();
                self.set_meta("dispatch_dirty", "0")?;
                if created > 0 {
                    self.sid_dirty.set(true);
                    self.set_meta("sid_dirty", "1")?;
                }
                Ok(created)
            }
            Err(e) => {
                if in_tx {
                    let _ = self.conn.execute_batch("ROLLBACK TO dispatch_sp");
                    let _ = self.conn.execute_batch("RELEASE dispatch_sp");
                } else {
                    let _ = self.conn.execute_batch("ROLLBACK");
                }
                let _ = self.set_meta("dispatch_dirty", "1");
                Err(e)
            }
        }
    }

    /// How many `ts.event.dispatch` rows exist (tests / idempotency).
    pub fn event_dispatch_count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM refs WHERE rule_id = 'ts.event.dispatch'",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Full resolve when dirty (export safety). Returns refs linked.
    pub fn ensure_sids_for_export(&mut self) -> Result<usize> {
        // Honor persisted flag across process restarts (R4 M5).
        self.load_sid_dirty()?;
        if self.sid_dirty.get() {
            self.resolve_symbol_ids()
        } else {
            Ok(0)
        }
    }

    pub fn assert_no_dangling_sids(&self) -> Result<()> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM refs r
             WHERE r.resolved_symbol_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM symbols s WHERE s.id = r.resolved_symbol_id)",
            [],
            |r| r.get(0),
        )?;
        if n > 0 {
            anyhow::bail!("{n} dangling resolved_symbol_id(s)");
        }
        Ok(())
    }

    fn resolve_symbol_ids_core(&mut self, full: bool, dirty_paths: &[String]) -> Result<usize> {
        if full {
            // Drop every previous link so incremental reindex cannot leave dangling sids.
            self.conn
                .execute("UPDATE refs SET resolved_symbol_id = NULL", [])?;
        } else {
            // Snapshot old symbol ids on dirty paths, then NULL those + refs pointing at them.
            let mut old_ids: Vec<i64> = Vec::new();
            for p in dirty_paths {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id FROM symbols WHERE path = ?1")?;
                let rows = stmt.query_map(params![p], |r| r.get::<_, i64>(0))?;
                for id in rows {
                    old_ids.push(id?);
                }
            }
            for p in dirty_paths {
                self.conn.execute(
                    "UPDATE refs SET resolved_symbol_id = NULL WHERE path = ?1",
                    params![p],
                )?;
            }
            for id in &old_ids {
                self.conn.execute(
                    "UPDATE refs SET resolved_symbol_id = NULL WHERE resolved_symbol_id = ?1",
                    params![id],
                )?;
            }
            // Also re-prefer: refs whose bare name matches a symbol **now** on a
            // dirty path (new symbols may steal inbound links from same-name elsewhere).
            for p in dirty_paths {
                self.conn.execute(
                    "UPDATE refs SET resolved_symbol_id = NULL
                     WHERE name IN (SELECT name FROM symbols WHERE path = ?1)",
                    params![p],
                )?;
            }
        }

        // In-memory bulk relink (P1): two SQL scans + executemany, not per-row SELECT.
        // Correlated UPDATE with refs.path is not portable in SQLite UPDATE subqueries.
        struct Sym {
            id: i64,
            name: String,
            qname: String,
            path: String,
        }
        let mut by_qname: HashMap<String, Vec<Sym>> = HashMap::new();
        let mut by_name: HashMap<String, Vec<Sym>> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, name, qualified_name, path FROM symbols")?;
            let rows = stmt.query_map([], |r| {
                Ok(Sym {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    qname: r.get(2)?,
                    path: r.get(3)?,
                })
            })?;
            for row in rows {
                let s = row?;
                by_name.entry(s.name.clone()).or_default().push(Sym {
                    id: s.id,
                    name: s.name.clone(),
                    qname: s.qname.clone(),
                    path: s.path.clone(),
                });
                by_qname.entry(s.qname.clone()).or_default().push(Sym {
                    id: s.id,
                    name: s.name.clone(),
                    qname: s.qname.clone(),
                    path: s.path.clone(),
                });
            }
        }

        let pick = |cands: &Vec<Sym>, path: &str| -> Option<i64> {
            // Prefer same path, then lexicographically smallest path.
            let mut best: Option<&Sym> = None;
            for c in cands {
                let better = match best {
                    None => true,
                    Some(b) => {
                        let b_same = b.path == path;
                        let c_same = c.path == path;
                        if c_same != b_same {
                            c_same
                        } else {
                            c.path < b.path
                        }
                    }
                };
                if better {
                    best = Some(c);
                }
            }
            best.map(|c| c.id)
        };

        let pending: Vec<(i64, String, Option<String>, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT id, name, qualifier, path FROM refs WHERE resolved_symbol_id IS NULL",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut updates: Vec<(i64, i64)> = Vec::with_capacity(pending.len());
        for (rid, name, qual, rpath) in pending {
            let mut sid = None;
            if let Some(q) = qual.as_deref() {
                if !q.is_empty() {
                    let q1 = format!("{q}.{name}");
                    let q2 = format!("{q}::{name}");
                    if let Some(cands) = by_qname.get(&q1) {
                        sid = pick(cands, &rpath);
                    }
                    if sid.is_none() {
                        if let Some(cands) = by_qname.get(&q2) {
                            sid = pick(cands, &rpath);
                        }
                    }
                }
            }
            if sid.is_none() {
                if let Some(cands) = by_name.get(&name) {
                    sid = pick(cands, &rpath);
                }
            }
            if let Some(sid) = sid {
                updates.push((sid, rid));
            }
        }
        {
            let mut upd = self
                .conn
                .prepare_cached("UPDATE refs SET resolved_symbol_id = ?1 WHERE id = ?2")?;
            // One explicit transaction for thousands of point updates.
            self.conn.execute_batch("BEGIN")?;
            for (sid, rid) in updates {
                upd.execute(params![sid, rid])?;
            }
            self.conn.execute_batch("COMMIT")?;
        }
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM refs WHERE resolved_symbol_id IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Cross-file / cross-function qualifier upgrade:
    /// for `s := f()` define edges, if `f` has a return type, rewrite call
    /// refs in the same file whose qualifier is the variable `s` to that type.
    pub fn resolve_qualifiers(&mut self) -> Result<usize> {
        // (path, var) -> type from define join function return_type.
        // Skip ambiguous factory names (multiple distinct return_types, or
        // multiple function symbols sharing the name — R6 + R7).
        let mut map: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        let mut ambiguous: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        // Factory names with >1 function symbol in the **same package dir** (R10 C4).
        let mut multi_fn_names: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT name, path FROM symbols WHERE kind = 'function'")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut counts: std::collections::HashMap<(String, String), usize> =
                std::collections::HashMap::new();
            for n in rows {
                let (name, path) = n?;
                let dir = package_dir_of(&path);
                *counts.entry((dir, name)).or_default() += 1;
            }
            for ((dir, name), c) in counts {
                if c > 1 {
                    multi_fn_names.insert((dir, name));
                }
            }
        }
        {
            let mut stmt = self.conn.prepare(
                "SELECT r.path, r.name, s.return_type, r.module, s.path
                 FROM refs r
                 JOIN symbols s ON s.name = r.module AND s.kind = 'function'
                 WHERE r.kind = 'define' AND r.module IS NOT NULL
                   AND s.return_type IS NOT NULL AND s.return_type != ''",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (path, var, ty, module, sym_path) = row?;
                // R10 C4: only same-package (directory) factories may type a define.
                if package_dir_of(&path) != package_dir_of(&sym_path) {
                    continue;
                }
                let key = (path, var);
                if multi_fn_names.contains(&(package_dir_of(&sym_path), module.clone())) {
                    map.remove(&key);
                    ambiguous.insert(key);
                    continue;
                }
                if let Some(prev) = map.get(&key) {
                    if prev != &ty {
                        ambiguous.insert(key);
                    }
                } else if !ambiguous.contains(&key) {
                    map.insert(key, ty);
                }
            }
            for k in &ambiguous {
                map.remove(k);
            }
        }

        // Also: New* constructors already set qualifier to type in extract.
        // Upgrade call refs: qualifier is var name matching define map.
        // P0-5: prefetch type names once instead of per-row COUNT(*).
        let mut type_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT name FROM symbols
                 WHERE kind IN ('class','struct','interface','enum','trait')",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for n in rows {
                type_names.insert(n?);
            }
        }
        // R8 revoke: restore sticky upgrades when the factory is now ambiguous
        // or no longer uniquely typed.
        let mut revoked = 0usize;
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, path, pre_qual, qualifier FROM refs
                 WHERE kind = 'call' AND pre_qual IS NOT NULL",
            )?;
            let pending: Vec<(i64, String, String, String)> = {
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            drop(stmt);
            let mut restore = self.conn.prepare_cached(
                "UPDATE refs SET qualifier = ?1, qual_name = NULL, pre_qual = NULL WHERE id = ?2",
            )?;
            let mut reup = self.conn.prepare_cached(
                "UPDATE refs SET qualifier = ?1, qual_name = ?1 || '.' || name WHERE id = ?2",
            )?;
            for (id, path, pre, cur) in pending {
                let key = (path.clone(), pre.clone());
                match map.get(&key) {
                    // R9: factory type changed → re-upgrade sticky qualifier.
                    Some(ty) if !ambiguous.contains(&key) => {
                        if ty != &cur {
                            reup.execute(params![ty, id])?;
                            // R10 M3: qualifier change invalidates sid links.
                            self.conn.execute(
                                "UPDATE refs SET resolved_symbol_id = NULL WHERE id = ?1",
                                params![id],
                            )?;
                            revoked += 1;
                        }
                    }
                    _ => {
                        restore.execute(params![pre, id])?;
                        self.conn.execute(
                            "UPDATE refs SET resolved_symbol_id = NULL WHERE id = ?1",
                            params![id],
                        )?;
                        revoked += 1;
                    }
                }
            }
        }
        let mut updated = 0usize;
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, path, qualifier FROM refs
                 WHERE kind = 'call' AND qualifier IS NOT NULL AND pre_qual IS NULL",
            )?;
            let pending: Vec<(i64, String, String)> = {
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            drop(stmt);
            let mut upd = self.conn.prepare_cached(
                "UPDATE refs SET qualifier = ?1, qual_name = ?1 || '.' || name, pre_qual = qualifier WHERE id = ?2",
            )?;
            for (id, path, qual) in pending {
                // If qualifier already looks like a known type (class/struct), skip.
                if type_names.contains(&qual) {
                    continue;
                }
                if ambiguous.contains(&(path.clone(), qual.clone())) {
                    continue;
                }
                if let Some(ty) = map.get(&(path.clone(), qual.clone())) {
                    if ty != &qual {
                        upd.execute(params![ty, id])?;
                        updated += 1;
                    }
                }
            }
        }
        if revoked > 0 || updated > 0 {
            // R10 M3/M4: sid links and query cache must follow qualifier rewrites.
            self.sid_dirty.set(true);
            self.set_meta("sid_dirty", "1")?;
            self.cache.borrow_mut().clear();
        }
        Ok(updated + revoked)
    }

    /// Parse/extract failed: keep path with a parse_error S violation (R12 M3).
    pub fn record_parse_error(&mut self, path: &str, reason: &str) -> Result<()> {
        self.cache.borrow_mut().clear();
        self.sid_dirty.set(true);
        self.set_meta("sid_dirty", "1")?;
        self.set_meta("dispatch_dirty", "1")?;
        self.conn
            .execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        self.conn
            .execute("DELETE FROM refs WHERE path = ?1", params![path])?;
        self.conn.execute(
            "DELETE FROM subset_violations WHERE path = ?1",
            params![path],
        )?;
        self.conn.execute(
            "INSERT INTO files(path, hash, language, mtime_ns, size) VALUES(?1, 'parse-error', 'unknown', 0, 0)
             ON CONFLICT(path) DO UPDATE SET hash = 'parse-error'",
            params![path],
        )?;
        self.conn.execute(
            "INSERT INTO subset_violations(path, kind, line, snippet) VALUES(?1, 'parse_error', 1, ?2)",
            params![path, reason.chars().take(200).collect::<String>()],
        )?;
        Ok(())
    }

    pub fn importers_of_file(&self, file_path: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, kind, path, line, enclosing, module, resolved, qualifier, confidence, evidence
             FROM refs
             WHERE resolved = ?1 AND kind = 'import'
             ORDER BY path, line
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![file_path, limit as i64], map_ref)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn all_symbols_for_export(&self) -> Result<Vec<SymbolRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.path, s.start_line, s.end_line, s.parent, s.description, f.language, s.start_col, s.end_col, s.return_type
             FROM symbols s JOIN files f ON f.path = s.path
             ORDER BY s.path, s.start_line",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SymbolRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                qualified_name: r.get(2)?,
                kind: SymbolKind::parse(&r.get::<_, String>(3)?),
                path: r.get(4)?,
                language: r.get(9)?,
                start_line: r.get::<_, i64>(5)? as usize,
                end_line: r.get::<_, i64>(6)? as usize,
                parent: r.get(7)?,
                description: r.get(8)?,
                start_col: r.get::<_, i64>(10)? as usize,
                end_col: r.get::<_, i64>(11)? as usize,
                return_type: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Export refs; `filter` drops edges outside the confidence window
    /// (SCIP default excludes DynamicCandidate).
    pub fn all_refs_for_export(&self, filter: ConfidenceFilter) -> Result<Vec<ReferenceRecord>> {
        let conf = confidence_where(filter);
        let sql = format!(
            "SELECT name, kind, path, line, enclosing, module, resolved, qualifier, confidence, evidence
             FROM refs WHERE {conf} ORDER BY path, line"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_ref)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// True BFS impact. `limit` caps total output; per-layer fetch uses a larger slice.
    ///
    /// Frontier expansion via `enclosing` only happens when a symbol with that leaf name
    /// actually exists in the index (and preferably in the same language as the citing
    /// file). This prevents cross-language last_segment(name) collisions from linking
    /// e.g. Python `validate_email` impact into an unrelated TypeScript `authenticate`.
    pub fn impact(&self, name: &str, depth: usize, limit: usize) -> Result<Vec<ImpactNode>> {
        self.impact_filtered(name, depth, limit, ConfidenceFilter::Default)
    }

    pub fn impact_filtered(
        &self,
        name: &str,
        depth: usize,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ImpactNode>> {
        let fk = filter_key(filter);
        {
            let mut c = self.cache.borrow_mut();
            if let Some(v) = c.impact.get(&(name.to_string(), depth, limit, fk)).cloned() {
                c.note_hit();
                return Ok(v);
            }
            c.note_miss();
        }
        let result = self.impact_uncached(name, depth, limit, filter)?;
        {
            let mut c = self.cache.borrow_mut();
            if QueryCache::evict_if_needed(c.impact.len(), c.max_entries) {
                c.impact.clear();
            }
            c.impact
                .insert((name.to_string(), depth, limit, fk), result.clone());
        }
        Ok(result)
    }

    fn impact_uncached(
        &self,
        name: &str,
        depth: usize,
        limit: usize,
        filter: ConfidenceFilter,
    ) -> Result<Vec<ImpactNode>> {
        let mut visited_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut visited_ref: std::collections::HashSet<(String, i64, String)> =
            std::collections::HashSet::new();
        let mut out: Vec<ImpactNode> = Vec::new();
        let mut frontier: std::collections::VecDeque<(String, usize)> =
            std::collections::VecDeque::new();
        frontier.push_back((name.to_string(), 0));
        visited_names.insert(name.to_string());

        while let Some((current, d)) = frontier.pop_front() {
            if d >= depth || out.len() >= limit {
                continue;
            }
            // No SQL LIMIT before expansion (R5 M3): output limit ≠ frontier fetch.
            let refs = self.callers_uncached_opt(&current, None, filter)?;
            for r in refs {
                if out.len() >= limit {
                    break;
                }
                let key = (r.name.clone(), r.line as i64, r.path.clone());
                if !visited_ref.insert(key) {
                    continue;
                }
                out.push(ImpactNode {
                    name: r.name.clone(),
                    path: r.path.clone(),
                    line: r.line,
                    kind: r.kind,
                    depth: d + 1,
                    enclosing: r.enclosing.clone(),
                    resolved: r.resolved.clone(),
                    confidence: r.confidence,
                });
                // Only expand via last segment of enclosing when that leaf is a real
                // symbol, ideally in the same language as the referring file.
                if let Some(enc) = r.enclosing.clone() {
                    let leaf = last_segment(&enc);
                    if !leaf.is_empty() && visited_names.insert(leaf.clone()) {
                        let ref_lang = self.file_language(&r.path)?;
                        if self.expandable_enclosing(&leaf, ref_lang.as_deref())? {
                            frontier.push_back((leaf, d + 1));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Gate for impact BFS: expand enclosing only if a symbol named `leaf` exists,
    /// preferring a match in the same language as the referring file.
    fn expandable_enclosing(&self, leaf: &str, ref_lang: Option<&str>) -> Result<bool> {
        if let Some(lang) = ref_lang {
            let n_same: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM symbols s
                 JOIN files f ON f.path = s.path
                 WHERE s.name = ?1 AND f.language = ?2
                 LIMIT 1",
                params![leaf, lang],
                |r| r.get(0),
            )?;
            if n_same > 0 {
                return Ok(true);
            }
            // Cross-language: only expand if a same-language hit is impossible
            // (no same-lang symbol). Skip bare-name collisions across languages
            // when a same-lang candidate set exists for the *current* frontier name.
            // Practical MVP: do NOT expand across languages — require same language.
            return Ok(false);
        }
        // Unknown file language: fall back to "leaf must exist as any symbol".
        self.symbol_name_exists(leaf)
    }

    pub fn related_files(&self, name: &str, limit: usize) -> Result<Vec<(String, usize, String)>> {
        let mut scores: HashMap<String, (usize, String)> = HashMap::new();

        let mut stmt = self.conn.prepare(
            "SELECT path, kind, description FROM symbols WHERE name = ?1 OR qualified_name = ?1",
        )?;
        let defs = stmt.query_map(params![name], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut def_paths = Vec::new();
        for row in defs {
            let (path, kind, desc) = row?;
            def_paths.push(path.clone());
            let e = scores.entry(path).or_insert((0, String::new()));
            e.0 += 10;
            e.1 = if desc.is_some() {
                format!("definition:{kind}+llm")
            } else {
                format!("definition:{kind}")
            };
        }

        for dp in &def_paths {
            let imps = self.importers_of_file(dp, limit * 10)?;
            for r in imps {
                let e = scores.entry(r.path.clone()).or_insert((0, String::new()));
                e.0 += 3;
                if e.1.is_empty() {
                    e.1 = format!("imports:{dp}");
                }
            }
        }

        let refs = self.callers(name, limit * 20)?;
        for r in refs {
            let e = scores.entry(r.path.clone()).or_insert((0, String::new()));
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = format!("reference:{}", r.kind.as_str());
            }
        }

        let mut list: Vec<(String, usize, String)> = scores
            .into_iter()
            .map(|(p, (s, reason))| (p, s, reason))
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        list.truncate(limit);
        Ok(list)
    }

    #[allow(clippy::type_complexity)]
    pub fn symbols_needing_description(
        &self,
        limit: usize,
    ) -> Result<Vec<(i64, String, String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, path, start_line, end_line
             FROM symbols
             WHERE (description IS NULL OR description = '')
               AND kind IN ('function','method','class','struct','trait','interface')
             ORDER BY path, start_line
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                format!(
                    "{}:{}-{}",
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?
                ),
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn set_description(&mut self, id: i64, description: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE symbols SET description = ?1 WHERE id = ?2",
            params![description, id],
        )?;
        Ok(())
    }

    /// All stored S-violations (from last index of each file).
    pub fn subset_violations(&self) -> Result<Vec<SubsetViolation>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, kind, line, snippet FROM subset_violations ORDER BY path, line",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SubsetViolation {
                path: r.get(0)?,
                kind: r.get(1)?,
                line: r.get::<_, i64>(2)? as usize,
                snippet: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn subset_violation_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM subset_violations", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    fn ref_is_sound(&self, r: &ReferenceRecord) -> bool {
        let rule = r.evidence.as_ref().map(|e| e.rule_id.as_str());
        is_sound_eligible(r.confidence, rule)
    }

    /// Callers restricted to the sound over-approximation edge set (L2).
    /// Fetches **all** confidence-window rows then filters — never LIMIT-truncates
    /// before the sound filter (C5 under-approx bug).
    pub fn callers_sound(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<(Vec<ReferenceRecord>, Vec<SubsetViolation>)> {
        let all = self.callers_uncached_opt(name, None, ConfidenceFilter::IncludeDynamic)?;
        let hits: Vec<ReferenceRecord> = all
            .into_iter()
            .filter(|r| self.ref_is_sound(r))
            .take(limit)
            .collect();
        let violations = self.subset_violations()?;
        Ok((hits, violations))
    }

    /// Impact BFS over the sound edge set only (L2 `impact --sound`).
    pub fn impact_sound(
        &self,
        name: &str,
        depth: usize,
        limit: usize,
    ) -> Result<(Vec<ImpactNode>, Vec<SubsetViolation>)> {
        let mut visited_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut visited_ref: std::collections::HashSet<(String, i64, String)> =
            std::collections::HashSet::new();
        let mut out: Vec<ImpactNode> = Vec::new();
        let mut frontier: std::collections::VecDeque<(String, usize)> =
            std::collections::VecDeque::new();
        frontier.push_back((name.to_string(), 0));
        visited_names.insert(name.to_string());

        while let Some((current, d)) = frontier.pop_front() {
            if d >= depth || out.len() >= limit {
                continue;
            }
            // No SQL LIMIT before sound filter (C5).
            let refs =
                self.callers_uncached_opt(&current, None, ConfidenceFilter::IncludeDynamic)?;
            for r in refs {
                if !self.ref_is_sound(&r) {
                    continue;
                }
                if out.len() >= limit {
                    break;
                }
                let key = (r.name.clone(), r.line as i64, r.path.clone());
                if !visited_ref.insert(key) {
                    continue;
                }
                out.push(ImpactNode {
                    name: r.name.clone(),
                    path: r.path.clone(),
                    line: r.line,
                    kind: r.kind,
                    depth: d + 1,
                    enclosing: r.enclosing.clone(),
                    resolved: r.resolved.clone(),
                    confidence: r.confidence,
                });
                if let Some(enc) = r.enclosing.clone() {
                    let leaf = last_segment(&enc);
                    if !leaf.is_empty() && visited_names.insert(leaf.clone()) {
                        let ref_lang = self.file_language(&r.path)?;
                        if self.expandable_enclosing(&leaf, ref_lang.as_deref())? {
                            frontier.push_back((leaf, d + 1));
                        }
                    }
                }
            }
        }
        let violations = self.subset_violations()?;
        Ok((out, violations))
    }
}

fn last_segment(s: &str) -> String {
    s.rsplit(['.', ':']).next().unwrap_or(s).to_string()
}

/// Directory of a repo-relative path (Go package approximation). Root files → "".
fn package_dir_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

/// Minimal identifier quoting for SAVEPOINT names (alphanumeric + underscore only).
fn quote_ident(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "sp".to_string()
    } else if cleaned.chars().next().unwrap().is_ascii_digit() {
        format!("sp_{cleaned}")
    } else {
        cleaned
    }
}

fn map_ref(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReferenceRecord> {
    let confidence_s: String = r.get(8)?;
    let evidence_s: Option<String> = r.get(9)?;
    let evidence = evidence_s.and_then(|s| serde_json::from_str::<Evidence>(&s).ok());
    Ok(ReferenceRecord {
        name: r.get(0)?,
        kind: EdgeKind::parse(&r.get::<_, String>(1)?),
        path: r.get(2)?,
        line: r.get::<_, i64>(3)? as usize,
        enclosing: r.get(4)?,
        module: r.get(5)?,
        resolved: r.get(6)?,
        qualifier: r.get(7)?,
        confidence: Confidence::parse(&confidence_s),
        evidence,
    })
}
