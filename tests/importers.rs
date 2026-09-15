use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::Language;
use std::collections::HashSet;

#[test]
fn importers_of_file_returns_resolved_imports() {
    let dir = std::env::temp_dir().join("agentgraph-test-importers");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("index.db");
    let mut store = Store::open(&db).unwrap();

    let mut known = HashSet::new();
    known.insert("src/api.ts".to_string());
    known.insert("src/auth.ts".to_string());

    let api = r#"import { createUser } from "./auth";
export function loginHandler() { createUser("a","b"); }
"#;
    let auth = r#"export function createUser(e: string, p: string) { return {e,p}; }
"#;
    let p_api = extract_file(api, Language::TypeScript, "src/api.ts", &known).unwrap();
    let p_auth = extract_file(auth, Language::TypeScript, "src/auth.ts", &known).unwrap();

    store.begin_batch().unwrap();
    store
        .replace_file("src/api.ts", "h1", "typescript", &p_api)
        .unwrap();
    store
        .replace_file("src/auth.ts", "h2", "typescript", &p_auth)
        .unwrap();
    store.commit_batch().unwrap();

    let hits = store.importers_of_file("src/auth.ts", 20).unwrap();
    assert!(!hits.is_empty(), "expected importers of auth.ts");
    assert!(hits.iter().any(|h| h.path == "src/api.ts" && h.name == "createUser"));
    assert!(hits.iter().all(|h| h.kind.as_str() == "import"));
}
