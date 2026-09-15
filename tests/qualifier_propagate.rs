//! Cross-file / return-type qualifier propagation.
use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{EdgeKind, Language};
use std::collections::HashSet;

#[test]
fn return_type_propagates_via_define_edges() {
    let dir = std::env::temp_dir().join("agentgraph-test-qual");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = Store::open(&dir.join("index.db")).unwrap();
    let known: HashSet<String> = HashSet::new();

    // lib defines NewServer and Server methods; main uses short_var in Go
    // Simulated via two files after extract: store NewServer return type
    let lib = r#"
package lib
type Server struct{}
func NewServer() *Server { return &Server{} }
func (s *Server) Start() {}
"#;
    let main = r#"
package lib
func main() {
  s := NewServer()
  s.Start()
}
"#;
    let p_lib = extract_file(lib, Language::Go, "lib.go", &known).unwrap();
    let p_main = extract_file(main, Language::Go, "main.go", &known).unwrap();
    store.begin_batch().unwrap();
    store.replace_file("lib.go", "h1", "go", &p_lib).unwrap();
    store.replace_file("main.go", "h2", "go", &p_main).unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    let n = store.resolve_qualifiers().unwrap();

    let callers = store.callers("Server.Start", 20).unwrap();
    assert!(
        !callers.is_empty(),
        "Server.Start should find upgraded qualifier (upgraded={n})"
    );
    assert!(callers
        .iter()
        .all(|c| c.qualifier.as_deref() == Some("Server")));
}

#[test]
fn ts_annotated_param_already_typed() {
    let src = r#"
class Store { save() {} }
function run(s: Store) { s.save(); }
"#;
    let out = extract_file(src, Language::TypeScript, "a.ts", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "save" && matches!(r.kind, EdgeKind::Call))
        .unwrap();
    assert_eq!(hit.qualifier.as_deref(), Some("Store"));
}
