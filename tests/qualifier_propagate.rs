//! Cross-file / return-type qualifier propagation.
use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::model::{ConfidenceFilter, EdgeKind, Language};
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

#[test]
fn go_ambiguous_factory_name_is_not_upgraded() {
    let dir = std::env::temp_dir().join("agentgraph-test-qual-ambig");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = Store::open(&dir.join("index.db")).unwrap();
    let a = r#"
package main
func createThing() *Server { return nil }
type Server struct{}
func (s *Server) Start() {}
"#;
    let b = r#"
package main
func createThing() *Client { return nil }
type Client struct{}
func (c *Client) Start() {}
func main() {
  x := createThing()
  x.Start()
}
"#;
    let pa = extract_file(a, Language::Go, "a.go", &HashSet::new()).unwrap();
    let pb = extract_file(b, Language::Go, "b.go", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store.replace_file("a.go", "ha", "go", &pa).unwrap();
    store.replace_file("b.go", "hb", "go", &pb).unwrap();
    store.commit_batch().unwrap();
    store.resolve_qualifiers().unwrap();
    let server = store
        .callers_filtered("Server.Start", 10, ConfidenceFilter::ExactOnly)
        .unwrap()
        .into_iter()
        .filter(|r| r.path == "b.go")
        .count();
    let client = store
        .callers_filtered("Client.Start", 10, ConfidenceFilter::ExactOnly)
        .unwrap()
        .into_iter()
        .filter(|r| r.path == "b.go")
        .count();
    assert!(
        server == 0 || client == 0,
        "ambiguous createThing must not Exact-upgrade main.x.Start to both types; server={server} client={client}"
    );
}

#[test]
fn go_single_typed_factory_name_collision_not_exact_upgrade() {
    let dir = std::env::temp_dir().join("agentgraph-test-qual-single-typed");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = Store::open(&dir.join("index.db")).unwrap();
    // Only Server factory has a return type; Client factory does not.
    let a = r#"
package main
func createThing() *Server { return nil }
type Server struct{}
func (s *Server) Start() {}
"#;
    let b = r#"
package main
func createThing() { }
type Client struct{}
func (c *Client) Start() {}
func main() {
  x := createThing()
  x.Start()
}
"#;
    let pa = extract_file(a, Language::Go, "a.go", &HashSet::new()).unwrap();
    let pb = extract_file(b, Language::Go, "b.go", &HashSet::new()).unwrap();
    store.begin_batch().unwrap();
    store.replace_file("a.go", "ha", "go", &pa).unwrap();
    store.replace_file("b.go", "hb", "go", &pb).unwrap();
    store.commit_batch().unwrap();
    store.resolve_qualifiers().unwrap();
    let server = store
        .callers_filtered("Server.Start", 10, ConfidenceFilter::ExactOnly)
        .unwrap()
        .into_iter()
        .filter(|r| r.path == "b.go")
        .count();
    assert_eq!(
        server, 0,
        "createThing with no result in b.go must not Exact-upgrade x.Start to Server"
    );
}
