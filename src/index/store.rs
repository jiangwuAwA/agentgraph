use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use super::extract::ExtractedFile;
use crate::model::{
    EdgeKind, EnrichReport, ImpactNode, IndexStats, ReferenceRecord, SymbolKind, SymbolRecord,
};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("open db {}", path.display()))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

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
                FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_qname ON symbols(qualified_name);
            CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
            CREATE INDEX IF NOT EXISTS idx_refs_name ON refs(name);
            CREATE INDEX IF NOT EXISTS idx_refs_path ON refs(path);
            "#,
        )?;
        // Migrate older DBs first (columns may not exist yet).
        let _ = conn.execute("ALTER TABLE symbols ADD COLUMN description TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN module TEXT", []);
        let _ = conn.execute("ALTER TABLE refs ADD COLUMN resolved TEXT", []);
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_refs_resolved ON refs(resolved);",
        )?;
        Ok(Self { conn })
    }

    pub fn file_hash(&self, path: &str) -> Result<Option<String>> {
        let row = self
            .conn
            .query_row("SELECT hash FROM files WHERE path = ?1", params![path], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(row)
    }

    pub fn known_files(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM files")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for row in rows {
            set.insert(row?);
        }
        Ok(set)
    }

    pub fn replace_file(
        &mut self,
        path: &str,
        hash: &str,
        language: &str,
        extracted: &ExtractedFile,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        // Preserve description across reindex for same qualified_name
        // (simple approach: descriptions are re-fetched via enrich; keep them if name+path match)
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM refs WHERE path = ?1", params![path])?;
        tx.execute(
            "INSERT INTO files(path, hash, language) VALUES(?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET hash = excluded.hash, language = excluded.language",
            params![path, hash, language],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO symbols(path, name, qualified_name, kind, start_line, end_line, parent, description)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
            )?;
            for s in &extracted.symbols {
                stmt.execute(params![
                    path,
                    s.name,
                    s.qualified_name,
                    s.kind.as_str(),
                    s.start_line as i64,
                    s.end_line as i64,
                    s.parent,
                ])?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT INTO refs(name, kind, path, line, enclosing, module, resolved)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn prune_missing(&mut self, keep_paths: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        let existing: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM files")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let keep: std::collections::HashSet<&str> =
            keep_paths.iter().map(|s| s.as_str()).collect();
        for path in existing {
            if !keep.contains(path.as_str()) {
                tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
            }
        }
        tx.commit()?;
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
        let described: i64 = self
            .conn
            .query_row(
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
        })
    }

    pub fn find_symbol(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, kind, path, start_line, end_line, parent, description
             FROM symbols
             WHERE name = ?1 OR qualified_name = ?1 OR name LIKE ?2
             ORDER BY
               CASE WHEN name = ?1 THEN 0 WHEN qualified_name = ?1 THEN 1 ELSE 2 END,
               path, start_line
             LIMIT ?3",
        )?;
        let pattern = format!("%{name}%");
        let rows = stmt.query_map(params![name, pattern, limit as i64], |r| {
            Ok(SymbolRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                qualified_name: r.get(2)?,
                kind: SymbolKind::parse(&r.get::<_, String>(3)?),
                path: r.get(4)?,
                language: String::new(),
                start_line: r.get::<_, i64>(5)? as usize,
                end_line: r.get::<_, i64>(6)? as usize,
                parent: r.get(7)?,
                description: r.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            let mut s = row?;
            let lang: Option<String> = self
                .conn
                .query_row(
                    "SELECT language FROM files WHERE path = ?1",
                    params![s.path],
                    |r| r.get(0),
                )
                .optional()?;
            s.language = lang.unwrap_or_default();
            out.push(s);
        }
        Ok(out)
    }

    pub fn callers(&self, name: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, kind, path, line, enclosing, module, resolved
             FROM refs
             WHERE name = ?1
             ORDER BY path, line
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![name, limit as i64], |r| {
            Ok(ReferenceRecord {
                name: r.get(0)?,
                kind: EdgeKind::parse(&r.get::<_, String>(1)?),
                path: r.get(2)?,
                line: r.get::<_, i64>(3)? as usize,
                enclosing: r.get(4)?,
                module: r.get(5)?,
                resolved: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// References that resolve to a given file (import edges + call sites in that file).
    pub fn importers_of_file(&self, file_path: &str, limit: usize) -> Result<Vec<ReferenceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, kind, path, line, enclosing, module, resolved
             FROM refs
             WHERE resolved = ?1 AND kind = 'import'
             ORDER BY path, line
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![file_path, limit as i64], |r| {
            Ok(ReferenceRecord {
                name: r.get(0)?,
                kind: EdgeKind::parse(&r.get::<_, String>(1)?),
                path: r.get(2)?,
                line: r.get::<_, i64>(3)? as usize,
                enclosing: r.get(4)?,
                module: r.get(5)?,
                resolved: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn impact(&self, name: &str, depth: usize, limit: usize) -> Result<Vec<ImpactNode>> {
        let mut visited_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut visited_ref: std::collections::HashSet<(String, i64, String)> =
            std::collections::HashSet::new();
        let mut out: Vec<ImpactNode> = Vec::new();
        let mut frontier: Vec<(String, usize)> = vec![(name.to_string(), 0)];
        visited_names.insert(name.to_string());

        while let Some((current, d)) = frontier.pop() {
            if d >= depth {
                continue;
            }
            let refs = self.callers(&current, limit)?;
            for r in refs {
                let key = (r.name.clone(), r.line as i64, r.path.clone());
                if !visited_ref.insert(key) {
                    continue;
                }
                if out.len() >= limit {
                    return Ok(out);
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
                if let Some(enc) = r.enclosing.clone() {
                    let candidates = [enc.clone(), last_segment(&enc)];
                    for c in candidates {
                        if !c.is_empty() && visited_names.insert(c.clone()) {
                            frontier.push((c, d + 1));
                        }
                    }
                }
                // Also expand via resolved import: if someone imports the defining file,
                // symbols defined there are impacted — already covered by name refs.
            }
        }
        Ok(out)
    }

    pub fn related_files(&self, name: &str, limit: usize) -> Result<Vec<(String, usize, String)>> {
        let mut scores: std::collections::HashMap<String, (usize, String)> =
            std::collections::HashMap::new();

        let mut stmt = self
            .conn
            .prepare("SELECT path, kind, description FROM symbols WHERE name = ?1 OR qualified_name = ?1")?;
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

        // importers of defining files (module-level edges)
        for dp in &def_paths {
            let imps = self.importers_of_file(dp, limit * 10)?;
            for r in imps {
                let e = scores.entry(r.path.clone()).or_insert((0, String::new()));
                e.0 += 3;
                if e.1.is_empty() {
                    e.1 = format!("imports:{}", dp);
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

    // ── LLM enrichment ────────────────────────────────────────────────────────

    pub fn symbols_needing_description(&self, limit: usize) -> Result<Vec<(i64, String, String, String, String)>> {
        // id, name, kind, path, snippet context we'll fill later
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
        self.conn
            .execute("UPDATE symbols SET description = ?1 WHERE id = ?2", params![description, id])?;
        Ok(())
    }

    pub fn count_described(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE description IS NOT NULL AND description != ''",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }
}

fn last_segment(s: &str) -> String {
    s.rsplit(['.', ':', ':']).next().unwrap_or(s).to_string()
}

// re-export for enrich
pub fn make_enrich_report(attempted: usize, described: usize, skipped: usize, failed: usize, model: String) -> EnrichReport {
    EnrichReport {
        attempted,
        described,
        skipped,
        failed,
        model,
    }
}
