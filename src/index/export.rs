//! SCIP / LSIF export.
//!
//! SCIP follows the official `scip.Index` protobuf JSON mapping (protocol v3):
//! symbols are `scip-<lang> <manager> <package> <version> <descriptor>` with
//! standard descriptors (`Type.`, `Type#method()`, `func()`, `ns/`).
//! LSIF remains a simplified JSONL dump.

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use super::store::Store;
use crate::model::{ReferenceRecord, SymbolKind, SymbolRecord};

/// Build a `file://` URI from a project root and a (possibly empty) relative path.
pub fn file_uri(root: &Path, rel: &str) -> String {
    let mut path = root.to_string_lossy().replace('\\', "/");
    if !rel.is_empty() {
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(&rel.replace('\\', "/"));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return format!("file:///{path}");
    }
    if path.starts_with('/') {
        return format!("file://{path}");
    }
    format!("file:///{path}")
}

/// SCIP protocol version used for docs (`scip.Metadata.version` enum is
/// UnspecifiedProtocolVersion in current scip.proto; this is informational).
#[allow(dead_code)]
pub const SCIP_PROTOCOL_VERSION: i32 = 3;

fn package_manager(lang: &str) -> &'static str {
    match lang {
        "typescript" | "tsx" | "javascript" | "jsx" => "npm",
        "python" => "pypi",
        "go" => "go",
        "rust" => "cargo",
        _ => "generic",
    }
}

fn scip_language_id(lang: &str) -> &'static str {
    match lang {
        "typescript" | "tsx" => "typescript",
        "javascript" | "jsx" => "javascript",
        "python" => "python",
        "go" => "go",
        "rust" => "rust",
        _ => "plaintext",
    }
}

/// Build a SCIP descriptor from kind + qualified name.
///
/// Examples:
/// - class `Store` → `Store.`
/// - method `Store.save` → `Store#save()`
/// - function `loginHandler` → `loginHandler()`
/// - rust `ModelClient::connect` → `ModelClient#connect()`
fn scip_descriptor(kind: SymbolKind, qualified: &str) -> String {
    let sep = if qualified.contains("::") { "::" } else { "." };
    let parts: Vec<&str> = qualified.split(sep).collect();
    let (last, head) = parts.split_last().unwrap_or((&"", &[]));
    let ns = head
        .iter()
        .map(|s| format!("{s}/"))
        .collect::<Vec<_>>()
        .join("");

    match kind {
        SymbolKind::Class
        | SymbolKind::Struct
        | SymbolKind::Interface
        | SymbolKind::Enum
        | SymbolKind::Trait
        | SymbolKind::TypeAlias => format!("{ns}{last}."),
        SymbolKind::Method => format!("{ns}{last}#()"),
        SymbolKind::Function => format!("{ns}{last}()"),
        SymbolKind::Module => format!("{ns}{last}/"),
        SymbolKind::Variable => format!("{ns}{last}"),
    }
}

/// `Store.save` / `Store::save` → `Store#save()`
fn scip_method_descriptor(qualified: &str) -> String {
    let sep = if qualified.contains("::") { "::" } else { "." };
    if let Some((ty, method)) = qualified.rsplit_once(sep) {
        let ty_path = ty.split(sep).collect::<Vec<_>>().join("/");
        return format!("{ty_path}#{method}()");
    }
    format!("{qualified}#()")
}

fn scip_symbol_name(s: &SymbolRecord) -> String {
    let lang = scip_language_id(&s.language);
    let mgr = package_manager(&s.language);
    let descriptor = if s.kind == SymbolKind::Method {
        scip_method_descriptor(&s.qualified_name)
    } else {
        scip_descriptor(s.kind, &s.qualified_name)
    };
    // scheme <space> mgr <space> package <space> version <space> descriptor
    format!("scip-{lang} {mgr} agentgraph 0.0.0 {descriptor}")
}

/// Pick the definition symbol for a reference.
///
/// Bare-name match only when unambiguous; otherwise require `qualifier` to
/// disambiguate (`A.save` vs `B.save`). Returns `None` when still ambiguous —
/// safer to drop the link than to wire it to the wrong definition.
fn pick_symbol<'a>(
    r: &ReferenceRecord,
    symbols: &'a [SymbolRecord],
    by_bare: &BTreeMap<String, Vec<usize>>,
) -> Option<&'a SymbolRecord> {
    let idxs = by_bare.get(&r.name)?;
    if idxs.is_empty() {
        return None;
    }
    if idxs.len() == 1 {
        return symbols.get(idxs[0]);
    }
    let q = r.qualifier.as_deref()?;
    let want_dot = format!("{q}.{}", r.name);
    let want_path = format!("{q}::{}", r.name);
    let mut matched: Option<&SymbolRecord> = None;
    for &i in idxs {
        let s = symbols.get(i)?;
        let qn = &s.qualified_name;
        let ok = qn == &want_dot
            || qn == &want_path
            || qn.ends_with(&format!(".{want_dot}"))
            || qn.ends_with(&format!("::{want_path}"))
            || s.parent.as_deref() == Some(q);
        if ok {
            if matched.is_some() {
                return None; // still ambiguous
            }
            matched = Some(s);
        }
    }
    matched
}

fn index_by_bare(symbols: &[SymbolRecord]) -> BTreeMap<String, Vec<usize>> {
    let mut by_bare: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, s) in symbols.iter().enumerate() {
        by_bare.entry(s.name.clone()).or_default().push(i);
    }
    by_bare
}

/// Locate `name` on a 1-based line and return UTF-16 [start_col, end_col].
fn name_cols_on_line(root: &Path, rel: &str, line: usize, name: &str) -> (usize, usize) {
    let Ok(src) = std::fs::read_to_string(root.join(rel)) else {
        return (0, name.chars().map(|c| c.len_utf16()).sum::<usize>().max(1));
    };
    let Some(text) = src.lines().nth(line.saturating_sub(1)) else {
        return (0, 1);
    };
    let Some(byte_idx) = text.find(name) else {
        return (0, name.chars().map(|c| c.len_utf16()).sum::<usize>().max(1));
    };
    let start: usize = text[..byte_idx].chars().map(|c| c.len_utf16()).sum();
    let end = start + name.chars().map(|c| c.len_utf16()).sum::<usize>();
    (start, end.max(start + 1))
}

/// Export SCIP JSON following official `scip.Index` protobuf JSON mapping.
pub fn export_scip(store: &Store, root: &Path, out: &Path) -> Result<()> {
    let symbols = store.all_symbols_for_export()?;
    let refs = store.all_refs_for_export()?;
    let mut docs: BTreeMap<String, (String, Vec<Value>)> = BTreeMap::new();
    let by_bare = index_by_bare(&symbols);

    for s in &symbols {
        let line = s.start_line.saturating_sub(1);
        let symbol = scip_symbol_name(s);
        let mut occ = json!({
            "range": [line, s.start_col, s.end_col.max(s.start_col + 1)],
            "symbol": symbol,
            "symbolRoles": 1,
        });
        if let Some(d) = &s.description {
            occ["documentation"] = json!([d]);
        }
        docs.entry(s.path.clone())
            .or_insert_with(|| (s.language.clone(), Vec::new()))
            .1
            .push(occ);
    }

    for r in &refs {
        let Some(def) = pick_symbol(r, &symbols, &by_bare) else {
            continue;
        };
        let symbol = scip_symbol_name(def);
        let line = r.line.saturating_sub(1);
        let (c0, c1) = name_cols_on_line(root, &r.path, r.line, &r.name);
        let occ = json!({
            "range": [line, c0, c1],
            "symbol": symbol,
            "symbolRoles": 0,
        });
        docs.entry(r.path.clone())
            .or_insert_with(|| ("plaintext".into(), Vec::new()))
            .1
            .push(occ);
    }

    let documents: Vec<Value> = docs
        .into_iter()
        .map(|(path, (lang, occurrences))| {
            json!({
                "language": scip_language_id(&lang),
                "relativePath": path,
                "occurrences": occurrences,
            })
        })
        .collect();

    // Official scip.Index protobuf JSON (proto3): no schemaVersion field;
    // Metadata.version is the ProtocolVersion enum (default Unspecified).
    let scip = json!({
        "metadata": {
            "toolInfo": {
                "name": "agentgraph",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "projectRoot": file_uri(root, ""),
        },
        "documents": documents,
    });

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, serde_json::to_vec(&scip)?)?;
    Ok(())
}

fn alloc_id(next_id: &mut u64) -> u64 {
    *next_id += 1;
    *next_id - 1
}

fn ensure_doc(
    lines: &mut Vec<String>,
    next_id: &mut u64,
    doc_by_path: &mut BTreeMap<String, u64>,
    root: &Path,
    path: &str,
    language: Option<&str>,
) -> Result<u64> {
    if let Some(&id) = doc_by_path.get(path) {
        return Ok(id);
    }
    let did = alloc_id(next_id);
    let mut v = json!({
        "id": did,
        "type": "vertex",
        "label": "document",
        "uri": file_uri(root, path),
    });
    if let Some(lang) = language {
        v["languageId"] = json!(lang);
    }
    lines.push(serde_json::to_string(&v)?);
    doc_by_path.insert(path.to_string(), did);
    Ok(did)
}

/// Export LSIF JSONL (experimental).
/// ranges → resultSet → item; refersTo → resultSet.
/// First line is always `metaData`. resultSets are keyed by qualified symbol
/// (path + qualified name), so `A.save` and `B.save` do not share a resultSet.
pub fn export_lsif(store: &Store, root: &Path, out: &Path) -> Result<()> {
    let symbols = store.all_symbols_for_export()?;
    let refs = store.all_refs_for_export()?;
    let mut lines: Vec<String> = Vec::new();
    let mut next_id = 1u64;
    let by_bare = index_by_bare(&symbols);

    // metaData must be the first line (LSIF).
    let meta_id = alloc_id(&mut next_id);
    lines.push(serde_json::to_string(&json!({
        "id": meta_id,
        "type": "vertex",
        "label": "metaData",
        "version": "0.5.0",
        "projectRoot": file_uri(root, ""),
        "positionEncoding": "utf-16",
    }))?);

    let project_id = alloc_id(&mut next_id);
    lines.push(serde_json::to_string(&json!({
        "id": project_id,
        "type": "vertex",
        "label": "project",
        "resource": file_uri(root, ""),
    }))?);

    lines.push(serde_json::to_string(&json!({
        "id": alloc_id(&mut next_id),
        "type": "edge",
        "label": "next",
        "outV": meta_id,
        "inV": project_id,
    }))?);

    let mut doc_by_path: BTreeMap<String, u64> = BTreeMap::new();
    // Key by SCIP symbol (includes path + qualified name) — not bare name.
    let mut result_set_by_symbol: BTreeMap<String, u64> = BTreeMap::new();

    for s in &symbols {
        let doc_id = ensure_doc(
            &mut lines,
            &mut next_id,
            &mut doc_by_path,
            root,
            &s.path,
            Some(&s.language),
        )?;

        let rid = alloc_id(&mut next_id);
        let line = s.start_line.saturating_sub(1);
        lines.push(serde_json::to_string(&json!({
            "id": rid,
            "type": "vertex",
            "label": "range",
            "start": {"line": line, "character": s.start_col},
            "end": {"line": line, "character": s.end_col.max(s.start_col + 1)},
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": alloc_id(&mut next_id),
            "type": "edge",
            "label": "contains",
            "outV": doc_id,
            "inV": rid,
        }))?);

        let scip = scip_symbol_name(s);
        let rs_id = *result_set_by_symbol.entry(scip).or_insert_with(|| {
            let id = alloc_id(&mut next_id);
            lines.push(
                serde_json::to_string(&json!({
                    "id": id,
                    "type": "vertex",
                    "label": "resultSet",
                }))
                .unwrap(),
            );
            id
        });
        // LSIF: definition range --next--> resultSet
        lines.push(serde_json::to_string(&json!({
            "id": alloc_id(&mut next_id),
            "type": "edge",
            "label": "next",
            "outV": rid,
            "inV": rs_id,
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": alloc_id(&mut next_id),
            "type": "edge",
            "label": "item",
            "outV": rs_id,
            "inV": rid,
            "document": doc_id,
        }))?);
    }

    for r in &refs {
        let Some(def) = pick_symbol(r, &symbols, &by_bare) else {
            continue;
        };
        let scip = scip_symbol_name(def);
        let Some(rs) = result_set_by_symbol.get(&scip).copied() else {
            continue;
        };
        let doc_id = ensure_doc(
            &mut lines,
            &mut next_id,
            &mut doc_by_path,
            root,
            &r.path,
            None,
        )?;
        let rid = alloc_id(&mut next_id);
        let line = r.line.saturating_sub(1);
        let (c0, c1) = name_cols_on_line(root, &r.path, r.line, &r.name);
        lines.push(serde_json::to_string(&json!({
            "id": rid,
            "type": "vertex",
            "label": "range",
            "start": {"line": line, "character": c0},
            "end": {"line": line, "character": c1},
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": alloc_id(&mut next_id),
            "type": "edge",
            "label": "contains",
            "outV": doc_id,
            "inV": rid,
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": alloc_id(&mut next_id),
            "type": "edge",
            "label": "refersTo",
            "outV": rid,
            "inV": rs,
        }))?);
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, lines.join("\n") + "\n")?;
    Ok(())
}
