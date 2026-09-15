use agentgraph::index::extract::extract_file;
use agentgraph::model::Language;
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
