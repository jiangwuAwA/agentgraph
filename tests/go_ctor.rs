use agentgraph::index::extract::extract_file;
use agentgraph::model::{EdgeKind, Language};
use std::collections::HashSet;

#[test]
fn go_constructor_return_becomes_type() {
    let src = r#"
package main

type Server struct{}

func NewServer() *Server { return &Server{} }

func (s *Server) Start() {}

func main() {
    s := NewServer()
    s.Start()
}
"#;
    let out = extract_file(src, Language::Go, "main.go", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "Start" && matches!(r.kind, EdgeKind::Call))
        .expect("Start call");
    assert_eq!(
        hit.qualifier.as_deref(),
        Some("Server"),
        "qualifier={:?}",
        hit.qualifier
    );
}
