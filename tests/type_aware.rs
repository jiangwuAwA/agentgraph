use agentgraph::index::extract::extract_file;
use agentgraph::model::{EdgeKind, Language};
use std::collections::HashSet;

#[test]
fn rust_scoped_call_has_qualifier() {
    let src = r#"
struct ModelClient;
impl ModelClient {
    fn connect_websocket(&self) {}
}
fn use_it(c: &ModelClient) {
    ModelClient::connect_websocket(c);
}
"#;
    let out = extract_file(src, Language::Rust, "src/lib.rs", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "connect_websocket" && matches!(r.kind, EdgeKind::Call))
        .expect("call ref");
    assert_eq!(hit.qualifier.as_deref(), Some("ModelClient"));
}

#[test]
fn ts_member_call_has_qualifier() {
    let src = r#"
class Store {
  save() {}
}
function run(s: Store) {
  s.save();
}
"#;
    let out = extract_file(src, Language::TypeScript, "src/a.ts", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "save" && matches!(r.kind, EdgeKind::Call))
        .expect("call ref");
    assert_eq!(hit.qualifier.as_deref(), Some("s"));
}
