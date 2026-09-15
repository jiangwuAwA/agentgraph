//! TDD: remaining PLAN L1 rules — Python __init_subclass__, Go interface impls.

use agentgraph::index::extract::extract_file;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

fn extract(src: &str, lang: Language, path: &str) -> agentgraph::index::extract::ExtractedFile {
    extract_file(src, lang, path, &HashSet::new()).unwrap()
}

fn has_heuristic(out: &agentgraph::index::extract::ExtractedFile, name: &str) -> bool {
    out.references
        .iter()
        .any(|r| r.name == name && r.confidence == Confidence::Heuristic)
}

#[test]
fn py_init_subclass_registers_subclass() {
    let src = r#"
class Plugin:
    registry = []
    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        Plugin.registry.append(cls)

class AuthPlugin(Plugin):
    def run(self):
        return 1
"#;
    let out = extract(src, Language::Python, "app/plugins.py");
    assert!(
        has_heuristic(&out, "AuthPlugin")
            || out.references.iter().any(|r| {
                r.confidence == Confidence::Heuristic
                    && r.evidence
                        .as_ref()
                        .map(|e| {
                            e.rule_id.contains("init_subclass") || e.snippet.contains("Plugin")
                        })
                        .unwrap_or(false)
            }),
        "__init_subclass__ base must yield Heuristic registration edge for subclass; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn go_interface_impl_method_is_heuristic() {
    // func (s *Server) ServeHTTP matches http.Handler — we emit Heuristic
    // impl edge for the method name on the concrete type.
    let src = r#"
package main

type Handler interface {
	ServeHTTP(w int, r int)
}

type Server struct{}

func (s *Server) ServeHTTP(w int, r int) {}

func use(h Handler) {
	h.ServeHTTP(1, 2)
}

func main() {
	use(&Server{})
}
"#;
    let out = extract(src, Language::Go, "main.go");
    let impl_hit = out.references.iter().any(|r| {
        r.name == "ServeHTTP"
            && r.confidence == Confidence::Heuristic
            && r.evidence
                .as_ref()
                .map(|e| e.rule_id.contains("interface") || e.snippet.contains("Server"))
                .unwrap_or(false)
    });
    assert!(
        impl_hit || has_heuristic(&out, "ServeHTTP"),
        "method with receiver implementing interface name must be Heuristic; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.qualifier.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn go_interface_assertion_var_implies_impl() {
    // var _ Handler = (*Server)(nil) — compile-time proof of implementation.
    let src = r#"
package main

type Store interface {
	Get(id string) string
}

type MemStore struct{}

func (m *MemStore) Get(id string) string { return id }

var _ Store = (*MemStore)(nil)
"#;
    let out = extract(src, Language::Go, "store.go");
    assert!(
        has_heuristic(&out, "Get")
            || out.references.iter().any(|r| {
                r.confidence == Confidence::Heuristic
                    && r.evidence
                        .as_ref()
                        .map(|e| e.snippet.contains("MemStore") || e.rule_id.contains("impl"))
                        .unwrap_or(false)
            }),
        "var _ Iface = (*T)(nil) must yield Heuristic impl edges; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
}
