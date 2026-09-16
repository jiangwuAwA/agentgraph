use agentgraph::index::extract::extract_file;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

#[test]
fn extract_ts_import_resolves() {
    let src = r#"
import { createUser, authenticate } from "./auth";

export function loginHandler(email: string, password: string) {
  return authenticate(email, password);
}
"#;
    let mut known = HashSet::new();
    known.insert("src/api.ts".to_string());
    known.insert("src/auth.ts".to_string());
    let out = extract_file(src, Language::TypeScript, "src/api.ts", &known).unwrap();
    let imports: Vec<_> = out
        .references
        .iter()
        .filter(|r| matches!(r.kind, agentgraph::model::EdgeKind::Import))
        .collect();
    assert!(!imports.is_empty(), "no import refs extracted");
    assert!(
        imports
            .iter()
            .any(|r| r.resolved.as_deref() == Some("src/auth.ts")),
        "imports not resolved: {:?}",
        imports
    );
}

#[test]
fn new_expression_yields_exact_constructor_call_edge() {
    let src = r#"
class Store {
  save() { return 1; }
}
export function run() {
  return new Store();
}
"#;
    let out = extract_file(src, Language::TypeScript, "src/n.ts", &HashSet::new()).unwrap();
    let hits: Vec<_> = out
        .references
        .iter()
        .filter(|r| r.name == "Store" && r.confidence == Confidence::Exact)
        .collect();
    assert!(
        !hits.is_empty(),
        "new Store() must yield Exact call edge to Store; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
}
