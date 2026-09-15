use agentgraph::index::extract::extract_file;
use agentgraph::model::{EdgeKind, Language, SymbolKind};
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
fn ts_param_type_becomes_qualifier() {
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
    // qualifier should be the *type* Store, not the variable name s
    assert_eq!(hit.qualifier.as_deref(), Some("Store"));
}

#[test]
fn rust_param_type_becomes_qualifier() {
    let src = r#"
struct Client;
impl Client {
    fn ping(&self) {}
}
fn go(c: &Client) {
    c.ping();
}
"#;
    let out = extract_file(src, Language::Rust, "src/lib.rs", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "ping" && matches!(r.kind, EdgeKind::Call))
        .expect("call ref");
    assert_eq!(hit.qualifier.as_deref(), Some("Client"));
}

#[test]
fn go_method_qname_is_receiver_type() {
    let src = r#"
package main

type Server struct{}

func (s *Server) Start() {}

func run(s *Server) {
    s.Start()
}
"#;
    let out = extract_file(src, Language::Go, "main.go", &HashSet::new()).unwrap();
    // Symbol qname: Server.Start
    let sym = out
        .symbols
        .iter()
        .find(|s| s.name == "Start" && s.kind == SymbolKind::Method)
        .expect("Start method symbol");
    assert_eq!(sym.qualified_name, "Server.Start");
    // Call ref qualifier: Server (type), not s (variable)
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "Start" && matches!(r.kind, EdgeKind::Call))
        .expect("Start call ref");
    assert_eq!(hit.qualifier.as_deref(), Some("Server"));
}

#[test]
fn go_receiver_self_call_uses_type() {
    let src = r#"
package main

type Server struct{}

func (s *Server) helper() {}
func (s *Server) Start() {
    s.helper()
}
"#;
    let out = extract_file(src, Language::Go, "main.go", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "helper" && matches!(r.kind, EdgeKind::Call))
        .expect("helper call ref");
    assert_eq!(hit.qualifier.as_deref(), Some("Server"));
}

#[test]
fn nested_scope_restores_var_types() {
    // Inner function's `s: User` must not leak past the inner scope and
    // overwrite outer `s: Store`.
    let src = r#"
class Store {
  load() {}
}
class User {
  save() {}
}
function outer(s: Store) {
  function inner(s: User) {
    s.save();
  }
  s.load();
}
"#;
    let out = extract_file(src, Language::TypeScript, "src/a.ts", &HashSet::new()).unwrap();
    let save = out
        .references
        .iter()
        .find(|r| r.name == "save" && matches!(r.kind, EdgeKind::Call))
        .expect("save call");
    assert_eq!(save.qualifier.as_deref(), Some("User"));
    let load = out
        .references
        .iter()
        .find(|r| r.name == "load" && matches!(r.kind, EdgeKind::Call))
        .expect("load call");
    assert_eq!(load.qualifier.as_deref(), Some("Store"));
}

#[test]
fn go_nested_scope_restores_var_types() {
    let src = r#"
package main

type Store struct{}
type User struct{}

func (Store) Load() {}
func (User) Save() {}

func outer(s *Store) {
    inner := func(u *User) {
        u.Save()
    }
    _ = inner
    s.Load()
}
"#;
    let out = extract_file(src, Language::Go, "main.go", &HashSet::new()).unwrap();
    let save = out
        .references
        .iter()
        .find(|r| r.name == "Save" && matches!(r.kind, EdgeKind::Call))
        .expect("Save call");
    assert_eq!(save.qualifier.as_deref(), Some("User"));
    let load = out
        .references
        .iter()
        .find(|r| r.name == "Load" && matches!(r.kind, EdgeKind::Call))
        .expect("Load call");
    assert_eq!(load.qualifier.as_deref(), Some("Store"));
}

#[test]
fn rust_nested_scope_restores_var_types() {
    let src = r#"
struct Store;
struct User;
impl Store { fn load(&self) {} }
impl User { fn save(&self) {} }
fn outer(s: &Store) {
    let _f = |u: &User| {
        u.save();
    };
    s.load();
}
"#;
    let out = extract_file(src, Language::Rust, "src/lib.rs", &HashSet::new()).unwrap();
    let save = out
        .references
        .iter()
        .find(|r| r.name == "save" && matches!(r.kind, EdgeKind::Call))
        .expect("save call");
    assert_eq!(save.qualifier.as_deref(), Some("User"));
    let load = out
        .references
        .iter()
        .find(|r| r.name == "load" && matches!(r.kind, EdgeKind::Call))
        .expect("load call");
    assert_eq!(load.qualifier.as_deref(), Some("Store"));
}

#[test]
fn go_param_type_becomes_qualifier() {
    let src = r#"
package main

type Client struct{}
func (c *Client) Ping() {}

func gofn(c *Client) {
    c.Ping()
}
"#;
    let out = extract_file(src, Language::Go, "main.go", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "Ping" && matches!(r.kind, EdgeKind::Call))
        .expect("Ping call");
    assert_eq!(hit.qualifier.as_deref(), Some("Client"));
}

#[test]
fn rust_self_method_uses_impl_type() {
    let src = r#"
struct Server;
impl Server {
    fn helper(&self) {}
    fn start(&self) {
        self.helper();
    }
}
"#;
    let out = extract_file(src, Language::Rust, "src/lib.rs", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "helper" && matches!(r.kind, EdgeKind::Call))
        .expect("helper call");
    assert_eq!(hit.qualifier.as_deref(), Some("Server"));
}

#[test]
fn go_package_qualifier_still_works() {
    let src = r#"
package main

import "strings"

func run(s string) bool {
    return strings.Contains(s, "x")
}
"#;
    let out = extract_file(src, Language::Go, "main.go", &HashSet::new()).unwrap();
    let hit = out
        .references
        .iter()
        .find(|r| r.name == "Contains" && matches!(r.kind, EdgeKind::Call))
        .expect("Contains call");
    // package name, not a local type
    assert_eq!(hit.qualifier.as_deref(), Some("strings"));
}
