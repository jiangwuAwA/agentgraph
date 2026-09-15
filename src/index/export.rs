//! SCIP / LSIF export (experimental, simplified schema).
//! Ranges use UTF-16 columns on the symbol *name* node (single-line).

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use super::store::Store;

/// SCIP-like global symbol: `scip-agentgraph . {lang} . {path} . {qualified}`
/// No trailing `#` (that marks local-only symbols and breaks cross-file links).
fn scip_symbol_name(lang: &str, qualified: &str, path: &str) -> String {
    format!(
        "scip-agentgraph . {} . {} . {}",
        lang.replace('.', "_"),
        path.replace('\\', "/"),
        qualified.replace('.', "_")
    )
}

/// Export SCIP JSON. Experimental — not a full protobuf mapping.
pub fn export_scip(store: &Store, root: &Path, out: &Path) -> Result<()> {
    let symbols = store.all_symbols_for_export()?;
    let refs = store.all_refs_for_export()?;
    let mut docs: BTreeMap<String, (String, Vec<Value>)> = BTreeMap::new();

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

    // Reference occurrences (role 0) — best-effort by name match.
    let by_name: BTreeMap<String, String> = symbols
        .iter()
        .map(|s| (s.name.clone(), scip_symbol_name(&s.language, &s.qualified_name, &s.path)))
        .collect();
    for r in &refs {
        if let Some(sym) = by_name.get(&r.name) {
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
    std::fs::write(out, serde_json::to_vec(&scip)?)?;
    Ok(())
}

/// Export LSIF JSONL (experimental). ranges → resultSet → item; refersTo → resultSet.
pub fn export_lsif(store: &Store, root: &Path, out: &Path) -> Result<()> {
    let symbols = store.all_symbols_for_export()?;
    let refs = store.all_refs_for_export()?;
    let mut lines: Vec<String> = Vec::new();
    let mut next_id = 1u64;
    let mut next = || {
        next_id += 1;
        next_id - 1
    };

    let project_id = next();
    lines.push(serde_json::to_string(&json!({
        "id": project_id,
        "type": "vertex",
        "label": "project",
        "resource": format!("file://{}", root.to_string_lossy().replace('\\', "/")),
    }))?);

    let meta_id = next();
    lines.push(serde_json::to_string(&json!({
        "id": meta_id,
        "type": "vertex",
        "label": "metaData",
        "version": "0.5.0",
        "projectRoot": format!("file://{}", root.to_string_lossy().replace('\\', "/")),
        "positionEncoding": "utf-16",
    }))?);

    lines.push(serde_json::to_string(&json!({
        "id": next(),
        "type": "edge",
        "label": "next",
        "outV": meta_id,
        "inV": project_id,
    }))?);

    let mut result_set_by_name: BTreeMap<String, u64> = BTreeMap::new();
    let mut last_doc: Option<(String, u64)> = None;

    for s in &symbols {
        if last_doc.as_ref().map(|(p, _)| p.as_str()) != Some(s.path.as_str()) {
            let did = next();
            let uri = format!(
                "file://{}/{}",
                root.to_string_lossy().replace('\\', "/"),
                s.path
            );
            lines.push(serde_json::to_string(&json!({
                "id": did,
                "type": "vertex",
                "label": "document",
                "languageId": s.language,
                "uri": uri,
            }))?);
            last_doc = Some((s.path.clone(), did));
        }
        let doc_id = last_doc.as_ref().map(|(_, id)| *id).unwrap();
        let rid = next();
        let line = s.start_line.saturating_sub(1);
        lines.push(serde_json::to_string(&json!({
            "id": rid,
            "type": "vertex",
            "label": "range",
            "start": {"line": line, "character": s.start_col},
            "end": {"line": line, "character": s.end_col.max(s.start_col + 1)},
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": next(),
            "type": "edge",
            "label": "contains",
            "outV": doc_id,
            "inV": rid,
        }))?);

        let rs_id = *result_set_by_name.entry(s.name.clone()).or_insert_with(|| {
            let id = next();
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
        lines.push(serde_json::to_string(&json!({
            "id": next(),
            "type": "edge",
            "label": "contains",
            "outV": rs_id,
            "inV": rid,
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": next(),
            "type": "edge",
            "label": "item",
            "outV": rs_id,
            "inV": rid,
            "document": doc_id,
        }))?);
    }

    for r in &refs {
        let Some(rs) = result_set_by_name.get(&r.name).copied() else {
            continue;
        };
        if last_doc.as_ref().map(|(p, _)| p.as_str()) != Some(r.path.as_str()) {
            let did = next();
            let uri = format!(
                "file://{}/{}",
                root.to_string_lossy().replace('\\', "/"),
                r.path
            );
            lines.push(serde_json::to_string(&json!({
                "id": did,
                "type": "vertex",
                "label": "document",
                "uri": uri,
            }))?);
            last_doc = Some((r.path.clone(), did));
        }
        let doc_id = last_doc.as_ref().map(|(_, id)| *id).unwrap();
        let rid = next();
        let line = r.line.saturating_sub(1);
        lines.push(serde_json::to_string(&json!({
            "id": rid,
            "type": "vertex",
            "label": "range",
            "start": {"line": line, "character": 0},
            "end": {"line": line, "character": 8},
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": next(),
            "type": "edge",
            "label": "contains",
            "outV": doc_id,
            "inV": rid,
        }))?);
        lines.push(serde_json::to_string(&json!({
            "id": next(),
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
