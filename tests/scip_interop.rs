//! Interop: parse agentgraph SCIP JSON with the official `scip` crate (protobuf).
use agentgraph::index::export::export_scip;
use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::Path;

mod common;

fn seed(db: &Path) -> Store {
    let mut store = Store::open(db).unwrap();
    let known: HashSet<String> = HashSet::new();
    let src = r#"
class Store {
  save() {}
}
export function run(s: Store) {
  s.save();
}
export function loginHandler() {
  run(new Store());
}
"#;
    let parsed = extract_file(src, Language::TypeScript, "src/app.ts", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/app.ts", "h", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    store
}

#[test]
fn scip_json_parses_with_official_crate() {
    let dir = common::temp_root("agentgraph-scip-interop");
    std::fs::create_dir_all(&dir).unwrap();
    let store = seed(&dir.join("index.db"));
    let out = dir.join("index.scip.json");
    agentgraph::index::export::export_scip_json(&store, Path::new("C:/proj"), &out).unwrap();

    let json = std::fs::read_to_string(&out).unwrap();
    let mut index = scip::types::Index::default();
    protobuf_json_mapping::merge_from_str(&mut index, &json)
        .expect("official scip::types::Index must accept our JSON");

    let meta = index.metadata.as_ref().expect("metadata");
    assert_eq!(meta.tool_info.as_ref().unwrap().name, "agentgraph");
    assert!(
        meta.project_root.starts_with("file:///"),
        "project_root={}",
        meta.project_root
    );
    assert!(!index.documents.is_empty());
    let doc = &index.documents[0];
    assert_eq!(doc.relative_path, "src/app.ts");
    assert!(!doc.occurrences.is_empty());
    let has_def = doc.occurrences.iter().any(|o| o.symbol_roles & 1 == 1);
    assert!(has_def, "expected at least one definition occurrence");
    let sym = &doc
        .occurrences
        .iter()
        .find(|o| o.symbol_roles & 1 == 1)
        .unwrap()
        .symbol;
    assert!(
        sym.starts_with("scip-typescript npm agentgraph 0.0.0 "),
        "symbol={sym}"
    );
}

#[test]
fn scip_binary_export_parses_with_official_crate() {
    let dir = common::temp_root("agentgraph-scip-bin");
    std::fs::create_dir_all(&dir).unwrap();
    let store = seed(&dir.join("index.db"));
    let out = dir.join("index.scip");
    export_scip(&store, Path::new("/proj"), &out).unwrap();
    use protobuf::Message;
    let bytes = std::fs::read(&out).unwrap();
    let index = scip::types::Index::parse_from_bytes(&bytes).expect("decode protobuf binary");
    assert!(!index.documents.is_empty());
    assert_eq!(index.documents[0].relative_path, "src/app.ts");
}
