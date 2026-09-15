//! SCIP / LSIF export from the agentgraph SQLite index.

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use super::store::Store;
use crate::model::SymbolKind;

fn scip_symbol_name(lang: &str, qualified: &str, path: &str) -> String {
    // Simple SCIP-like scheme: `agentgraph {lang} {path} {qualified}`
    format!("agentgraph . {} . {}#", lang.replace('.', "_"), qualified.replace('.', "_"))
    // path included for uniqueness across files with same qname
    .replace(" . ", &format!(" . {path} . "))
}

/// Export SCIP JSON (schema 0.4.0-ish, simplified).
pub fn export_scip(store: &Store, root: &Path, out: &Path) -> Result<()> {
    let symbols = store.all_symbols_for_export()?;
    let mut docs: BTreeMap<String, Vec<Value>> = BTreeMap::new();

    for s in &symbols {
        let occ = json!({
            "range": [
                s.start_line.saturating_sub(1),
                s.start_col,
                s.end_col.max(s.start_col + 1)
            ],
            "symbol": scip_symbol_name(&s.language, &s.qualified_name, &s.path),
            "symbol_roles": 1,
            "documentation": s.description.clone().map(|d| vec![d]).unwrap_or_default(),
        });
        docs.entry(s.path.clone()).or_default().push(occ);
    }

    let documents: Vec<Value> = docs
        .into_iter()
        .map(|(path, occurrences)| {
            let lang = symbols
                .iter()
                .find(|s| s.path == path)
                .map(|s| s.language.clone())
                .unwrap_or_default();
            json!({
                "language": lang,
                "relative_path": path,
                "occurrences": occurrences,
            })
        })
        .collect();

    let scip = json!({
        "schemaVersion": "0.4.0",
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
    std::fs::write(out, serde_json::to_vec_pretty(&scip)?)?;
    Ok(())
}

/// Export LSIF (JSON lines) — simplified vertices for document/range/definition/reference.
pub fn export_lsif(store: &Store, root: &Path, out: &Path) -> Result<()> {
    let symbols = store.all_symbols_for_export()?;
    let refs = store.all_refs_for_export()?;
    let mut lines: Vec<String> = Vec::new();
    let mut next_id = 1u64;
    let mut id = || {
        next_id += 1;
        next_id - 1
    };

    lines.push(serde_json::to_string(&json!({
        "id": id(),
        "type": "vertex",
        "label": "metaData",
        "version": "0.5.0",
        "projectRoot": format!("file://{}", root.to_string_lossy().replace('\\', "/")),
        "positionEncoding": "utf-16",
    }))?);

    let mut range_ids: BTreeMap<(String, usize, usize, usize), u64> = BTreeMap::new();
    let mut doc_ids: BTreeMap<String, u64> = BTreeMap::new();

    for s in &symbols {
        let key = (s.path.clone(), s.start_line, s.start_col, s.end_col);
        if range_ids.contains_key(&key) {
            continue;
        }
        let doc_id = *doc_ids.entry(s.path.clone()).or_insert_with(|| {
            let did = id();
            let uri = format!(
                "file://{}/{}",
                root.to_string_lossy().replace('\\', "/"),
                s.path
            );
            lines.push(
                serde_json::to_string(&json!({
                    "id": did,
                    "type": "vertex",
                    "label": "document",
                    "languageId": s.language,
                    "uri": uri,
                }))
                .unwrap(),
            );
            did
        });
        let rid = id();
        range_ids.insert(key, rid);
        lines.push(serde_json::to_string(&json!({
            "id": rid,
            "type": "vertex",
            "label": "range",
            "start": {"line": s.start_line.saturating_sub(1), "character": s.start_col},
            "end": {"line": s.end_line.saturating_sub(1), "character": s.end_col.max(s.start_col + 1)},
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": id(),
            "type": "edge",
            "label": "contains",
            "outV": doc_id,
            "inVs": [rid],
        }))?);

        let def_id = id();
        lines.push(serde_json::to_string(&json!({
            "id": def_id,
            "type": "vertex",
            "label": "resultSet",
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": id(),
            "type": "edge",
            "label": "contains",
            "outV": rid,
            "inV": def_id,
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": id(),
            "type": "edge",
            "label": "item",
            "outV": def_id,
            "inVs": [rid],
            "document": doc_id,
        }))?);

        let _ = s.kind; // keep kind available for future richer vertices
        let _ = SymbolKind::Function;
    }

    // Reference edges: map name → first range of matching definition
    let mut def_by_name: BTreeMap<String, u64> = BTreeMap::new();
    // rebuild map name → range id of a definition
    for s in &symbols {
        let key = (s.path.clone(), s.start_line, s.start_col, s.end_col);
        if let Some(rid) = range_ids.get(&key) {
            def_by_name.entry(s.name.clone()).or_insert(*rid);
        }
    }
    for r in &refs {
        if let Some(def_rid) = def_by_name.get(&r.name) {
            let key = (r.path.clone(), r.line, 0usize, 0usize);
            // create a coarse range for the call site line if missing
            let call_rid = *range_ids.entry(key).or_insert_with(|| {
                let rid = id();
                lines.push(
                    serde_json::to_string(&json!({
                        "id": rid,
                        "type": "vertex",
                        "label": "range",
                        "start": {"line": r.line.saturating_sub(1), "character": 0},
                        "end": {"line": r.line.saturating_sub(1), "character": 80},
                    }))
                    .unwrap(),
                );
                rid
            });
            lines.push(serde_json::to_string(&json!({
                "id": id(),
                "type": "edge",
                "label": "refersTo",
                "outV": call_rid,
                "inV": def_rid,
            }))?);
        }
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, lines.join("\n") + "\n")?;
    Ok(())
}
