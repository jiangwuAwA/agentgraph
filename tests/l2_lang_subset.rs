//! TDD: freeze Go/Python subset S scanners (L2 hardening).
//!
//! PLAN §4.2 / docs/sound-subset.md: programs using reflection/unsafe leave S.

use agentgraph::index::subset::scan_subset;
use agentgraph::model::Language;

#[test]
fn go_unsafe_leaves_s() {
    let src = r#"
package main

import "unsafe"

func evil(p *int) uintptr {
	return uintptr(unsafe.Pointer(p))
}

func main() {}
"#;
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(!r.in_subset, "unsafe must leave S: {:?}", r.violations);
    assert!(r.violations.iter().any(|v| v.kind.contains("unsafe")));
}

#[test]
fn go_reflect_call_leaves_s() {
    let src = r#"
package main

import "reflect"

func Call(m interface{}) {
	reflect.ValueOf(m).Call(nil)
}

func main() {}
"#;
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(!r.in_subset, "reflect must leave S: {:?}", r.violations);
}

#[test]
fn go_clean_handler_map_stays_in_s() {
    let src = r#"
package main

import "net/http"

func Health(w http.ResponseWriter, r *http.Request) {}

var routes = map[string]http.HandlerFunc{
	"/health": Health,
}

func main() {}
"#;
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        r.in_subset,
        "clean Go map handler stays in S: {:?}",
        r.violations
    );
}

#[test]
fn py_eval_exec_leave_s() {
    let src = "def f(x):\n    return eval(x)\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(!r.in_subset);
    let src2 = "exec('print(1)')\n";
    let r2 = scan_subset(src2, Language::Python, "b.py");
    assert!(!r2.in_subset);
}

#[test]
fn py_setattr_dunder_leaves_s() {
    // Monkey-patching call targets is outside S_py v1.
    let src = "def f():\n    setattr(f, '__code__', None)\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "setattr on callables leaves S: {:?}",
        r.violations
    );
}

#[test]
fn py_clean_depends_stays_in_s() {
    let src = r#"
def get_service():
    return 1

def route(svc = Depends(get_service)):
    return svc
"#;
    let r = scan_subset(src, Language::Python, "api.py");
    assert!(
        r.in_subset,
        "Depends-only Python stays in S: {:?}",
        r.violations
    );
}
