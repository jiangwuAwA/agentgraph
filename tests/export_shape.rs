//! Shape checks for experimental SCIP / LSIF export.
use agentgraph::index::export::{export_lsif, export_scip, file_uri};
use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::Language;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-export-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn seed_store(db: &Path) -> Store {
    let mut store = Store::open(db).unwrap();
    let known: HashSet<String> = HashSet::new();
    let src_a = r#"
class A {
  save() {}
}
export function useA(a: A) {
  a.save();
}
"#;
    let src_b = r#"
class B {
  save() {}
}
export function useB(b: B) {
  b.save();
}
"#;
    let pa = extract_file(src_a, Language::TypeScript, "src/a.ts", &known).unwrap();
    let pb = extract_file(src_b, Language::TypeScript, "src/b.ts", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/a.ts", "hash-a", "typescript", &pa)
        .unwrap();
    store
        .replace_file("src/b.ts", "hash-b", "typescript", &pb)
        .unwrap();
    store.commit_batch().unwrap();
    store
}

#[test]
fn file_uri_windows_drive_has_three_slashes() {
    let root = Path::new("C:/proj");
    assert_eq!(file_uri(root, "src/a.ts"), "file:///C:/proj/src/a.ts");
    assert_eq!(file_uri(root, ""), "file:///C:/proj");

    let back = Path::new("C:\\proj");
    assert_eq!(file_uri(back, "src\\a.ts"), "file:///C:/proj/src/a.ts");

    // POSIX absolute still three slashes via file:// + /abs
    let posix = Path::new("/home/user/proj");
    assert_eq!(
        file_uri(posix, "src/a.rs"),
        "file:///home/user/proj/src/a.rs"
    );
}

#[test]
fn scip_export_parses_and_uses_protocol_v3() {
    let dir = temp_dir("scip");
    let store = seed_store(&dir.join("index.db"));
    let out = dir.join("index.scip");
    export_scip(&store, Path::new("C:/fake/root"), &out).unwrap();

    let text = std::fs::read_to_string(&out).unwrap();
    let v: Value = serde_json::from_str(&text).expect("SCIP JSON must parse");
    // Official scip.Index has no schemaVersion field (protobuf).
    assert!(v.get("schemaVersion").is_none());
    assert_eq!(v["metadata"]["toolInfo"]["name"], "agentgraph");
    assert!(v["documents"]
        .as_array()
        .map(|d| !d.is_empty())
        .unwrap_or(false));
    // relativePath camelCase, symbol scheme scip-typescript npm agentgraph ...
    let docs = v["documents"].as_array().unwrap();
    let mut saw_symbol = false;
    for d in docs {
        assert!(
            d.get("relativePath").is_some(),
            "documents need relativePath"
        );
        if let Some(occs) = d["occurrences"].as_array() {
            for o in occs {
                let sym = o["symbol"].as_str().unwrap_or("");
                if sym.starts_with("scip-typescript npm agentgraph 0.0.0 ") {
                    saw_symbol = true;
                    assert!(o.get("symbolRoles").is_some());
                }
            }
        }
    }
    assert!(
        saw_symbol,
        "expected scip-typescript npm agentgraph symbols"
    );
}

#[test]
fn lsif_first_line_is_metadata() {
    let dir = temp_dir("lsif-meta");
    let store = seed_store(&dir.join("index.db"));
    let out = dir.join("index.lsif");
    export_lsif(&store, Path::new("/tmp/proj"), &out).unwrap();

    let text = std::fs::read_to_string(&out).unwrap();
    let first = text.lines().next().expect("LSIF non-empty");
    let meta: Value = serde_json::from_str(first).expect("first line JSON");
    assert_eq!(meta["label"], "metaData");
    assert_eq!(meta["type"], "vertex");
}

#[test]
fn lsif_file_uris_use_windows_three_slash_form() {
    let dir = temp_dir("lsif-uri");
    let store = seed_store(&dir.join("index.db"));
    let out = dir.join("index.lsif");
    export_lsif(&store, Path::new("C:/fake/root"), &out).unwrap();

    let text = std::fs::read_to_string(&out).unwrap();
    let mut saw_project_root = false;
    let mut saw_doc = false;
    for line in text.lines() {
        let v: Value = serde_json::from_str(line).expect("LSIF line JSON");
        if v["label"] == "metaData" {
            let pr = v["projectRoot"].as_str().unwrap();
            assert!(
                pr.starts_with("file:///C:/"),
                "projectRoot should be file:///C:/..., got {pr}"
            );
            saw_project_root = true;
        }
        if v["label"] == "document" {
            let uri = v["uri"].as_str().unwrap();
            assert!(
                uri.starts_with("file:///C:/fake/root/"),
                "document uri should be file:///C:/..., got {uri}"
            );
            saw_doc = true;
        }
    }
    assert!(saw_project_root && saw_doc);
}

#[test]
fn lsif_result_sets_not_shared_across_same_bare_name() {
    let dir = temp_dir("lsif-rs");
    let store = seed_store(&dir.join("index.db"));
    let out = dir.join("index.lsif");
    // root with spaces-free windows style path
    export_lsif(&store, Path::new("C:/fake/root"), &out).unwrap();

    let lines: Vec<Value> = std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    // Collect range→resultSet via next edges, and resultSet ids.
    let mut range_to_rs: HashMap<u64, u64> = HashMap::new();
    let mut next_edges: Vec<(u64, u64)> = Vec::new();
    let mut result_sets: HashSet<u64> = HashSet::new();
    let mut ranges: HashSet<u64> = HashSet::new();
    for v in &lines {
        if v["type"] == "vertex" && v["label"] == "resultSet" {
            result_sets.insert(v["id"].as_u64().unwrap());
        }
        if v["type"] == "vertex" && v["label"] == "range" {
            ranges.insert(v["id"].as_u64().unwrap());
        }
        if v["type"] == "edge" && v["label"] == "next" {
            next_edges.push((v["outV"].as_u64().unwrap(), v["inV"].as_u64().unwrap()));
        }
    }
    for (out_v, in_v) in &next_edges {
        if ranges.contains(out_v) && result_sets.contains(in_v) {
            range_to_rs.insert(*out_v, *in_v);
        }
    }

    // Two `save` definitions (A.save, B.save) must map to distinct resultSets.
    // Identify definition ranges that have next→resultSet: count distinct rs among them.
    // We expect at least: A, A.save, useA, B, B.save, useB → 6 resultSets.
    // Bug (shared by bare name) would collapse A.save and B.save → 5.
    assert!(
        result_sets.len() >= 6,
        "expected >=6 resultSets (A, A.save, useA, B, B.save, useB), got {}: {:?}",
        result_sets.len(),
        result_sets
    );

    // refersTo targets: two refersTo edges to `save` must not share one resultSet
    // when both A.save and B.save exist. Collect refersTo in-V values that are
    // resultSets and ensure we have two distinct ones for the two call sites
    // (a.save / b.save). Total refersTo should be 2 and they should differ.
    let refers_to: Vec<u64> = lines
        .iter()
        .filter(|v| v["type"] == "edge" && v["label"] == "refersTo")
        .map(|v| v["inV"].as_u64().unwrap())
        .collect();
    assert_eq!(
        refers_to.len(),
        2,
        "expected 2 refersTo edges (a.save, b.save), got {refers_to:?}"
    );
    assert_ne!(
        refers_to[0], refers_to[1],
        "A.save and B.save must not share a resultSet"
    );
}
