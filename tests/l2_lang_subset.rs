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
fn py_eval_with_space_leaves_s() {
    let src = "def f(x):\n    return eval (x)\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(!r.in_subset, "eval (x) must leave S: {:?}", r.violations);
}

#[test]
fn py_exec_with_space_leaves_s() {
    let src = "def f(x):\n    exec (x)\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(!r.in_subset, "exec (x) must leave S: {:?}", r.violations);
}

#[test]
fn py_getattr_dynamic_name_leaves_s() {
    for src in [
        "def f(obj, name):\n    return getattr(obj, name)\n",
        "def f(obj):\n    return getattr(obj, name + 'x')\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "getattr with non-literal 2nd arg must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn py_getattr_literal_stays_in_s() {
    let src = "def f(obj):\n    return getattr(obj, 'foo')\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "getattr(obj, 'literal') stays in S: {:?}",
        r.violations
    );
}

#[test]
fn py_builtins_eval_leaves_s() {
    for src in [
        "def f(x):\n    return __builtins__['eval'](x)\n",
        "def f(x):\n    return __builtins__.eval(x)\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "__builtins__ eval access must leave S: {src} -> {:?}",
            r.violations
        );
    }
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
