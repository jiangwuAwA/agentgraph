//! SCIP / LSIF export — **experimental**.
//!
//! Output is a simplified JSON/JSONL shape inspired by SCIP/LSIF, not a full
//! schema mapping. Ranges use UTF-16 columns on the symbol *name* node
//! (single-line). Not consumable by Sourcegraph or the official `scip` CLI yet.
//!
//! Ranges use UTF-16 columns on the symbol *name* node (single-line).

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use super::store::Store;
use crate::model::{ReferenceRecord, SymbolRecord};

/// Build a `file://` URI from a project root and a (possibly empty) relative path.
///
/// Windows drive paths get three slashes: `C:/proj` + `src/a.rs` →
/// `file:///C:/proj/src/a.rs`. POSIX absolute paths become `file:///abs/...`.
pub fn file_uri(root: &Path, rel: &str) -> String {
    let mut path = root.to_string_lossy().replace('\\', "/");
    if !rel.is_empty() {
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(&rel.replace('\\', "/"));
    }
    let bytes = path.as_bytes();
    // Windows drive letter → file:///C:/...
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return format!("file:///{path}");
    }
    // POSIX absolute → file:// + /abs = file:///abs
    if path.starts_with('/') {
        return format!("file://{path}");
    }
    format!("file:///{path}")
}

/// SCIP global symbol (5 space-separated segments, experimental form):
/// `scip-agentgraph . agentgraph . {lang} . {path} . {descriptor}`
/// descriptor uses `#` methods / types style without making everything local.
fn scip_symbol_name(lang: &str, qualified: &str, path: &str) -> String {
    let descriptor = if qualified.contains("::") {
        // rust Module::Type::fn → Module/Type#fn
        let parts: Vec<&str> = qualified.split("::").collect();
        let (last, head) = parts.split_last().unwrap();
        format!("{}#{}", head.join("/"), last)
    } else if qualified.contains('.') {
        let parts: Vec<&str> = qualified.split('.').collect();
        let (last, head) = parts.split_last().unwrap();
        format!("{}#{}", head.join("/"), last)
    } else {
        qualified.to_string()
    };
    format!(
        "scip-agentgraph . agentgraph . {} . {} . {}",
        lang.replace('.', "_"),
        path.replace('\\', "/"),
        descriptor
    )
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

/// Export SCIP JSON. Experimental — simplified schema, not a full protobuf mapping.
pub fn export_scip(store: &Store, root: &Path, out: &Path) -> Result<()> {
    let symbols = store.all_symbols_for_export()?;
    let refs = store.all_refs_for_export()?;
    let mut docs: BTreeMap<String, (String, Vec<Value>)> = BTreeMap::new();
    let by_bare = index_by_bare(&symbols);

    for s in &symbols {
        // Single-line name range only (SCIP 3-tuple is same-line).
        let line = s.start_line.saturating_sub(1);
        let occ = json!({
            "range": [line, s.start_col, s.end_col.max(s.start_col + 1)],
            "symbol": scip_symbol_name(&s.language, &s.qualified_name, &s.path),
            "symbol_roles": 1,
            "documentation": s.description.clone().map(|d| vec![d]).unwrap_or_default(),
        });
        docs.entry(s.path.clone())
            .or_insert_with(|| (s.language.clone(), Vec::new()))
            .1
            .push(occ);
    }

    // Reference occurrences (role 0) — best-effort; skip when ambiguous.
    for r in &refs {
        let Some(def) = pick_symbol(r, &symbols, &by_bare) else {
            continue;
        };
        let sym = scip_symbol_name(&def.language, &def.qualified_name, &def.path);
        let line = r.line.saturating_sub(1);
        let occ = json!({
            "range": [line, 0, 8],
            "symbol": sym,
            "symbol_roles": 0,
        });
        docs.entry(r.path.clone())
            .or_insert_with(|| ("unknown".into(), Vec::new()))
            .1
            .push(occ);
    }

    let documents: Vec<Value> = docs
        .into_iter()
        .map(|(path, (lang, occurrences))| {
            json!({
                "language": lang,
                "relative_path": path,
                "occurrences": occurrences,
            })
        })
        .collect();

    // schemaVersion "2.1.0" is the SCIP JSON convention; symbol strings below
    // are experimental and NOT consumable by Sourcegraph / scip CLI yet.
    let scip = json!({
        "schemaVersion": "2.1.0",
        "toolInfo": {
            "name": "agentgraph",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "project_root": root.to_string_lossy(),
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

        let scip = scip_symbol_name(&s.language, &s.qualified_name, &s.path);
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
        let scip = scip_symbol_name(&def.language, &def.qualified_name, &def.path);
        let Some(rs) = result_set_by_symbol.get(&scip).copied() else {
            continue;
        };
        let doc_id = ensure_doc(&mut lines, &mut next_id, &mut doc_by_path, root, &r.path, None)?;
        let rid = alloc_id(&mut next_id);
        let line = r.line.saturating_sub(1);
        lines.push(serde_json::to_string(&json!({
            "id": rid,
            "type": "vertex",
            "label": "range",
            "start": {"line": line, "character": 0},
            "end": {"line": line, "character": 8},
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
