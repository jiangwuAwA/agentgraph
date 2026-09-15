use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use super::extract::ExtractedFile;
use crate::model::{EdgeKind, ImpactNode, IndexStats, ReferenceRecord, SymbolKind, SymbolRecord};

#[derive(Default)]
struct QueryCache {
    callers: HashMap<(String, usize), Vec<ReferenceRecord>>,
    impact: HashMap<(String, usize, usize), Vec<ImpactNode>>,
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
                language TEXT NOT NULL
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
                FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_qname ON symbols(qualified_name);
            CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
            CREATE INDEX IF NOT EXISTS idx_refs_name ON refs(name);
            CREATE INDEX IF NOT EXISTS idx_refs_path ON refs(path);
            "#,
        )?;
        let _ = conn.execute("ALTER TABLE symbols ADD COLUMN description TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN module TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN resolved TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN qualifier TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN resolved_symbol_id INTEGER", []);
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
             CREATE INDEX IF NOT EXISTS idx_refs_kind ON refs(kind);",
        )?;
        Ok(Self {
            conn,
            cache: RefCell::new(QueryCache::new()),
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

    /// Replace file rows. Does NOT open its own transaction (caller uses begin_batch/commit_batch).
    pub fn replace_file(
        &mut self,
        path: &str,
        hash: &str,
        language: &str,
        extracted: &ExtractedFile,
    ) -> Result<()> {
        self.cache.borrow_mut().clear();
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
            "INSERT INTO files(path, hash, language) VALUES(?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET hash = excluded.hash, language = excluded.language",
            params![path, hash, language],
        )?;
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
                "INSERT INTO refs(name, kind, path, line, enclosing, module, resolved, qualifier)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for r in &extracted.references {
                stmt.execute(params![
                    r.name,
                    r.kind.as_str(),
                    path,
                    r.line as i64,
                    r.enclosing,
                    r.module,
                    r.resolved,
                    r.qualifier,
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
        for path in existing {
            if !keep.contains(path.as_str()) {
                self.conn
                    .execute("DELETE FROM files WHERE path = ?1", params![path])?;
            }
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
        Ok(IndexStats {
            files: files as usize,
            symbols: symbols as usize,
            references: references as usize,
            languages,
            root: root.to_string(),
            described: described as usize,
            skipped_files: 0,
            failed_files: 0,
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
        {
            let mut c = self.cache.borrow_mut();
            if let Some(v) = c.callers.get(&(name.to_string(), limit)).cloned() {
                c.note_hit();
                return Ok(v);
            }
            c.note_miss();
        }
        let result = self.callers_uncached(name, limit)?;
        {
            let mut c = self.cache.borrow_mut();
            if QueryCache::evict_if_needed(c.callers.len(), c.max_entries) {
                c.callers.clear();
            }
            c.callers.insert((name.to_string(), limit), result.clone());
        }
        Ok(result)
    }

    fn callers_uncached(&self, name: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
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

        let sql = if qual_dot.is_empty() {
            "SELECT name, kind, path, line, enclosing, module, resolved, qualifier
             FROM refs WHERE name = ?1 ORDER BY path, line LIMIT ?2"
        } else {
            // Exclusive: when a type qualifier is present, do not mix bare-name hits.
            "SELECT name, kind, path, line, enclosing, module, resolved, qualifier
             FROM refs
             WHERE (qualifier || '.' || name) = ?1
                OR (qualifier || '::' || name) = ?1
             ORDER BY path, line LIMIT ?2"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if qual_dot.is_empty() {
            stmt.query_map(params![bare, limit as i64], map_ref)?
        } else {
            stmt.query_map(params![qual_dot, limit as i64], map_ref)?
        };
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Link refs → symbols.id after a full or incremental index (name + optional qualifier).
    ///
    /// Always starts by clearing all `resolved_symbol_id` values, then fully relinks.
    /// This avoids dangling ids after DELETE+INSERT of a file whose symbols were targets.
    pub fn resolve_symbol_ids(&mut self) -> Result<usize> {
        // Drop every previous link so incremental reindex cannot leave dangling sids.
        self.conn
            .execute("UPDATE refs SET resolved_symbol_id = NULL", [])?;

        // Two-pass: qualified match first, then bare name. Uses only portable SQLite.
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, name, qualifier, path FROM refs WHERE resolved_symbol_id IS NULL",
            )?;
            let pending: Vec<(i64, String, Option<String>, String)> = {
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
            drop(stmt);

            let mut lookup_q = self.conn.prepare_cached(
                "SELECT id FROM symbols
                 WHERE (qualified_name = ?1 OR qualified_name = ?2)
                 ORDER BY CASE WHEN path = ?3 THEN 0 ELSE 1 END, path
                 LIMIT 1",
            )?;
            let mut lookup_n = self.conn.prepare_cached(
                "SELECT id FROM symbols WHERE name = ?1
                 ORDER BY CASE WHEN path = ?2 THEN 0 ELSE 1 END, path
                 LIMIT 1",
            )?;
            let mut update = self
                .conn
                .prepare_cached("UPDATE refs SET resolved_symbol_id = ?1 WHERE id = ?2")?;

            for (id, name, qual, rpath) in pending {
                let mut sid: Option<i64> = None;
                if let Some(q) = qual.as_deref() {
                    if !q.is_empty() {
                        sid = lookup_q
                            .query_row(
                                params![format!("{q}.{name}"), format!("{q}::{name}"), rpath],
                                |r| r.get(0),
                            )
                            .optional()?;
                    }
                }
                if sid.is_none() {
                    sid = lookup_n
                        .query_row(params![name, rpath], |r| r.get(0))
                        .optional()?;
                }
                if let Some(sid) = sid {
                    update.execute(params![sid, id])?;
                }
            }
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
        // (path, var) -> type from define join function return_type
        let mut map: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT r.path, r.name, s.return_type
                 FROM refs r
                 JOIN symbols s ON s.name = r.module
                 WHERE r.kind = 'define' AND r.module IS NOT NULL
                   AND s.return_type IS NOT NULL AND s.return_type != ''",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (path, var, ty) = row?;
                map.insert((path, var), ty);
            }
        }

        // Also: New* constructors already set qualifier to type in extract.
        // Upgrade call refs: qualifier is var name matching define map.
        let mut updated = 0usize;
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, path, qualifier FROM refs
                 WHERE kind = 'call' AND qualifier IS NOT NULL",
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
            let mut upd = self
                .conn
                .prepare_cached("UPDATE refs SET qualifier = ?1 WHERE id = ?2")?;
            for (id, path, qual) in pending {
                // If qualifier already looks like a known type (class/struct), skip.
                let is_type: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM symbols
                     WHERE name = ?1 AND kind IN ('class','struct','interface','enum','trait')",
                    params![qual],
                    |r| r.get(0),
                )?;
                if is_type > 0 {
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
        Ok(updated)
    }

    pub fn importers_of_file(&self, file_path: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, kind, path, line, enclosing, module, resolved, qualifier
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

    pub fn all_refs_for_export(&self) -> Result<Vec<ReferenceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, kind, path, line, enclosing, module, resolved, qualifier FROM refs ORDER BY path, line",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ReferenceRecord {
                name: r.get(0)?,
                kind: EdgeKind::parse(&r.get::<_, String>(1)?),
                path: r.get(2)?,
                line: r.get::<_, i64>(3)? as usize,
                enclosing: r.get(4)?,
                module: r.get(5)?,
                resolved: r.get(6)?,
                qualifier: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// True BFS impact. `limit` caps total output; per-layer fetch uses a larger slice.
    ///
    /// Frontier expansion via `enclosing` only happens when a symbol with that leaf name
    /// actually exists in the index (and preferably in the same language as the citing
    /// file). This prevents cross-language last_segment(name) collisions from linking
    /// e.g. Python `validate_email` impact into an unrelated TypeScript `authenticate`.
    pub fn impact(&self, name: &str, depth: usize, limit: usize) -> Result<Vec<ImpactNode>> {
        {
            let mut c = self.cache.borrow_mut();
            if let Some(v) = c.impact.get(&(name.to_string(), depth, limit)).cloned() {
                c.note_hit();
                return Ok(v);
            }
            c.note_miss();
        }
        let result = self.impact_uncached(name, depth, limit)?;
        {
            let mut c = self.cache.borrow_mut();
            if QueryCache::evict_if_needed(c.impact.len(), c.max_entries) {
                c.impact.clear();
            }
            c.impact
                .insert((name.to_string(), depth, limit), result.clone());
        }
        Ok(result)
    }

    fn impact_uncached(&self, name: &str, depth: usize, limit: usize) -> Result<Vec<ImpactNode>> {
        let mut visited_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut visited_ref: std::collections::HashSet<(String, i64, String)> =
            std::collections::HashSet::new();
        let mut out: Vec<ImpactNode> = Vec::new();
        let fetch_cap = limit.saturating_mul(4).max(200);
        let mut frontier: std::collections::VecDeque<(String, usize)> =
            std::collections::VecDeque::new();
        frontier.push_back((name.to_string(), 0));
        visited_names.insert(name.to_string());

        while let Some((current, d)) = frontier.pop_front() {
            if d >= depth || out.len() >= limit {
                continue;
            }
            let refs = self.callers_uncached(&current, fetch_cap)?;
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
}

fn last_segment(s: &str) -> String {
    s.rsplit(['.', ':']).next().unwrap_or(s).to_string()
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
    Ok(ReferenceRecord {
        name: r.get(0)?,
        kind: EdgeKind::parse(&r.get::<_, String>(1)?),
        path: r.get(2)?,
        line: r.get::<_, i64>(3)? as usize,
        enclosing: r.get(4)?,
        module: r.get(5)?,
        resolved: r.get(6)?,
        qualifier: r.get(7)?,
    })
}
