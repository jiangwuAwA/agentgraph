use agentgraph::index::resolve::resolve_typescript_import;
use std::collections::HashSet;

#[test]
fn resolve_ts_relative() {
    let mut known = HashSet::new();
    known.insert("src/auth.ts".to_string());
    known.insert("src/api.ts".to_string());
    known.insert("src/index.ts".to_string());
    let hit = resolve_typescript_import("src/api.ts", "./auth", &known);
    assert_eq!(hit.as_deref(), Some("src/auth.ts"));
}
