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

// --- AST scanner advantages (comments/strings must NOT trigger) ---------------

#[test]
fn py_comment_mentioning_eval_stays_in_s() {
    // Lexical v1 would flag `eval(` inside a comment; AST must not.
    let src = "def f(x):\n    # never do eval(x) here\n    return x\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "comment mentioning eval( must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn py_string_containing_eval_stays_in_s() {
    let src = "def f():\n    return 'call eval(x) to run code'\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "string containing eval( must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn py_docstring_with_exec_stays_in_s() {
    let src = "def f():\n    \"\"\"Do not exec(code) here.\"\"\"\n    return 1\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "docstring mentioning exec must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn py_multiline_getattr_literal_stays_in_s() {
    // Multi-line call with a string-literal second arg is finite-domain (in S).
    let src = "def f(obj):\n    return getattr(\n        obj,\n        'name'\n    )\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "multi-line getattr with literal name stays in S: {:?}",
        r.violations
    );
}

#[test]
fn py_import_module_literal_stays_in_s() {
    let src = "import importlib\ndef f():\n    return importlib.import_module('os.path')\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "import_module with literal stays in S: {:?}",
        r.violations
    );
}

#[test]
fn py_import_module_dynamic_leaves_s() {
    let src = "import importlib\ndef f(name):\n    return importlib.import_module(name)\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "import_module with dynamic name leaves S: {:?}",
        r.violations
    );
}

#[test]
fn py_eval_alias_leaves_s() {
    for src in [
        "e = eval\ndef f(x):\n    return e(x)\n",
        "from builtins import eval as e\ndef f(x):\n    return e(x)\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "eval alias must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn py_parse_error_fails_closed() {
    let src = "def f(:\n    return 1\n"; // invalid syntax
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(!r.in_subset, "parse error must leave S: {:?}", r.violations);
    assert!(
        r.violations.iter().any(|v| v.kind == "parse_error"),
        "expected parse_error violation: {:?}",
        r.violations
    );
}

#[test]
fn py_vars_subscript_leaves_s() {
    let src = "def f(name):\n    return vars()[name]\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "vars() subscript leaves S: {:?}",
        r.violations
    );
}

#[test]
fn py_attrgetter_reference_leaves_s() {
    let src = "from operator import attrgetter\ndef f():\n    return attrgetter\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "attrgetter reference leaves S: {:?}",
        r.violations
    );
}

// --- Go AST advantages --------------------------------------------------------

#[test]
fn go_comment_mentioning_unsafe_stays_in_s() {
    let src = "package main\n\n// never use unsafe.Pointer here\nfunc main() {}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        r.in_subset,
        "comment mentioning unsafe. must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn go_string_mentioning_reflect_stays_in_s() {
    let src = "package main\n\nfunc main() {\n\t_ = \"reflect.ValueOf\"\n}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        r.in_subset,
        "string containing reflect. must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn go_linkname_comment_leaves_s() {
    let src = "package main\n\n//go:linkname f runtime.f\nfunc f()\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        !r.in_subset,
        "//go:linkname must leave S: {:?}",
        r.violations
    );
}

#[test]
fn go_plugin_open_leaves_s() {
    let src = "package main\n\nimport \"plugin\"\n\nfunc main() {\n\tplugin.Open(\"x.so\")\n}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(!r.in_subset, "plugin.Open leaves S: {:?}", r.violations);
}

#[test]
fn go_parse_error_fails_closed() {
    let src = "package main\n\nfunc main( {\n}\n"; // invalid syntax
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(!r.in_subset, "parse error must leave S: {:?}", r.violations);
    assert!(
        r.violations.iter().any(|v| v.kind == "parse_error"),
        "expected parse_error violation: {:?}",
        r.violations
    );
}

#[test]
fn go_unsafe_selector_leaves_s_without_import_line() {
    // Using unsafe.X without a separate import line still leaves S.
    let src = "package main\n\nfunc f(p *int) uintptr {\n\treturn uintptr(unsafe.Pointer(p))\n}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        !r.in_subset,
        "unsafe. selector leaves S: {:?}",
        r.violations
    );
}
