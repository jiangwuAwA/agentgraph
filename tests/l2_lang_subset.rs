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

// --- M2.4 table: must-detect Python escapes ---------------------------------

#[test]
fn py_compile_call_leaves_s() {
    for src in [
        "def f(src):\n    return compile(src, '<s>', 'exec')\n",
        "c = compile\n",
        "def f(src):\n    return compile(src, name, mode)\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "compile must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn py_dunder_import_call_leaves_s() {
    for src in [
        "def f(name):\n    return __import__(name)\n",
        "__import__('os')\n",
        "import importlib\nimportlib.__import__('os')\n",
        "from importlib import __import__\n__import__('os')\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "__import__ must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn py_ctypes_import_leaves_s() {
    for src in [
        "import ctypes\n",
        "from ctypes import CDLL\n",
        "import ctypes as ct\n",
        "from ctypes import *\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "ctypes import must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn py_ctypes_usage_leaves_s() {
    for src in [
        "import ctypes\nlib = ctypes.CDLL('libc.so.6')\n",
        "import ctypes as ct\nx = ct.cdll.LoadLibrary('x')\n",
        "def f():\n    return ctypes.memmove\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "ctypes usage must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn py_comment_mentioning_ctypes_stays_in_s() {
    let src = "def f():\n    # do not import ctypes here\n    return 1\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "comment mentioning ctypes must stay in S: {:?}",
        r.violations
    );
}

// --- M2.4 table: must-detect Go escapes -------------------------------------

#[test]
fn go_c_import_cgo_leaves_s() {
    let src = "package main\n\n/*\n#include <stdlib.h>\n*/\nimport \"C\"\n\nfunc main() {}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        !r.in_subset,
        "import \"C\" must leave S: {:?}",
        r.violations
    );
}

#[test]
fn go_export_directive_leaves_s() {
    for src in [
        "package main\n//export Foo\nfunc Foo() {}\n",
        "package main\n//export\tBar\nfunc Bar() {}\n",
    ] {
        let r = scan_subset(src, Language::Go, "main.go");
        assert!(
            !r.in_subset,
            "//export must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn go_string_mentioning_export_stays_in_s() {
    // Non-directive string content must not leave S (AST advantage).
    let src = "package main\n\nfunc main() {\n\t_ = \"//export Foo\"\n}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        r.in_subset,
        "string containing //export must stay in S: {:?}",
        r.violations
    );
}

// --- M2 over-flag: type-only typeof Function / interface Function ------------

#[test]
fn ts_type_only_typeof_function_stays_in_s() {
    // M2.2.4: type-only `typeof Function` must NOT be an S violation.
    let fixtures: &[(&str, &str)] = &[
        (
            "type_alias",
            "type F = typeof Function;\nexport function id(x: number): number { return x; }\n",
        ),
        (
            "export_type",
            "export type Fn = typeof Function;\nexport function id(x: number): number { return x; }\n",
        ),
        (
            "type_annotation",
            "export function f(cb: typeof Function): void { cb; }\n",
        ),
        (
            "interface_member",
            "interface Registry {\n  factory: typeof Function;\n}\nexport const r: Registry = { factory: () => 1 };\n",
        ),
        (
            "type_annotation_function",
            "export function apply(cb: Function): void { cb(); }\n",
        ),
        (
            "interface_function_name",
            "export interface FunctionLike {\n  call(x: number): number;\n}\nexport function use(f: FunctionLike): number {\n  return f.call(1);\n}\n",
        ),
        (
            "nest_like_clean",
            "import { Injectable } from '@nestjs/common';\n@Injectable()\nexport class AppService {\n  getHello(): string {\n    return 'Hello World!';\n  }\n}\n",
        ),
    ];
    for (label, src) in fixtures {
        let path = format!("{label}.ts");
        let r = scan_subset(src, Language::TypeScript, &path);
        assert!(
            r.in_subset,
            "type-only Function position `{label}` must stay in S: {:?}",
            r.violations
        );
    }
}

#[test]
fn ts_value_use_function_still_leaves_s() {
    // Value uses of Function remain S violations (escape hatch).
    let fixtures: &[(&str, &str)] = &[
        (
            "return_value",
            "export function f(): Function {\n  return Function;\n}\n",
        ),
        (
            "call",
            "export function f(code: string) {\n  return Function(code);\n}\n",
        ),
        (
            "new_expr",
            "export function f(code: string) {\n  return new Function(code);\n}\n",
        ),
        ("alias", "const F = Function;\nexport default F;\n"),
        (
            "callback_arg",
            "export function f() {\n  return [Function];\n}\n",
        ),
    ];
    for (label, src) in fixtures {
        let path = format!("{label}.ts");
        let r = scan_subset(src, Language::TypeScript, &path);
        assert!(
            !r.in_subset,
            "value-use Function `{label}` must leave S: {:?}",
            r.violations
        );
    }
}

#[test]
fn ts_clean_fixture_without_dynamic_stays_in_s() {
    // M2.6: clean Nest-like TS with only type positions → subset_ok.
    let src = r#"
import { Module, Controller, Get, Injectable } from '@nestjs/common';

type Handler = (...args: unknown[]) => unknown;

interface ProviderToken {
  provide: string;
  useClass: Function;
}

@Injectable()
export class AppService {
  getHello(): string {
    return 'Hello World!';
  }
}

@Controller()
export class AppController {
  constructor(private readonly appService: AppService) {}
  @Get()
  getHello(): string {
    return this.appService.getHello();
  }
}

@Module({
  controllers: [AppController],
  providers: [AppService],
})
export class AppModule {}
"#;
    let r = scan_subset(src, Language::TypeScript, "app.module.ts");
    assert!(
        r.in_subset,
        "clean Nest-like TS (type-only Function in interface) must stay in S: {:?}",
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

// --- Rust AST advantages (comments/strings must NOT trigger) -----------------

#[test]
fn rust_clean_fn_stays_in_s() {
    let src = r#"
pub fn authenticate(email: &str, password: &str) -> bool {
    !email.is_empty() && !password.is_empty()
}

pub fn login_handler(email: &str, password: &str) -> bool {
    authenticate(email, password)
}
"#;
    let r = scan_subset(src, Language::Rust, "src/auth.rs");
    assert!(r.in_subset, "clean Rust stays in S: {:?}", r.violations);
}

#[test]
fn rust_comment_mentioning_unsafe_stays_in_s() {
    // Lexical v1 would flag mid-line / block-comment `unsafe`; AST must not.
    let src = "fn f() {\n    // never do unsafe { *p } here\n    let _ = 1;\n}\n/* unsafe block comment */\nfn g() {}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(
        r.in_subset,
        "comment mentioning unsafe must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn rust_string_with_transmute_stays_in_s() {
    let src = "fn f() -> &'static str {\n    \"call std::mem::transmute to cast\"\n}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(
        r.in_subset,
        "string containing transmute must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn rust_unsafe_block_leaves_s() {
    let src = "fn f(p: *const u8) -> u8 {\n    unsafe { *p }\n}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(
        !r.in_subset,
        "unsafe block must leave S: {:?}",
        r.violations
    );
    assert!(r.violations.iter().any(|v| v.kind == "unsafe"));
}

#[test]
fn rust_unsafe_fn_leaves_s() {
    let src = "unsafe fn evil(p: *const u8) -> u8 {\n    *p\n}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(!r.in_subset, "unsafe fn must leave S: {:?}", r.violations);
    assert!(r.violations.iter().any(|v| v.kind == "unsafe"));
}

#[test]
fn rust_unsafe_impl_leaves_s() {
    let src = "struct S;\nunsafe impl Send for S {}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(!r.in_subset, "unsafe impl must leave S: {:?}", r.violations);
    assert!(r.violations.iter().any(|v| v.kind == "unsafe"));
}

#[test]
fn rust_unsafe_trait_leaves_s() {
    let src = "unsafe trait Marker {}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(
        !r.in_subset,
        "unsafe trait must leave S: {:?}",
        r.violations
    );
    assert!(r.violations.iter().any(|v| v.kind == "unsafe"));
}

#[test]
fn rust_transmute_call_leaves_s() {
    for src in [
        "fn f(x: u32) -> i32 {\n    unsafe { std::mem::transmute(x) }\n}\n",
        "use std::mem;\nfn f(x: u32) -> i32 {\n    unsafe { mem::transmute(x) }\n}\n",
        "fn f(x: u32) -> i32 {\n    unsafe { core::mem::transmute(x) }\n}\n",
    ] {
        let r = scan_subset(src, Language::Rust, "a.rs");
        assert!(
            !r.in_subset,
            "transmute must leave S: {src} -> {:?}",
            r.violations
        );
        assert!(
            r.violations
                .iter()
                .any(|v| v.kind == "transmute" || v.kind == "unsafe"),
            "expected transmute or unsafe: {:?}",
            r.violations
        );
    }
}

#[test]
fn rust_asm_macro_leaves_s() {
    let src = "fn f() {\n    unsafe {\n        std::arch::asm!(\"nop\");\n    }\n}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(!r.in_subset, "asm! must leave S: {:?}", r.violations);
}

#[test]
fn rust_std_ptr_leaves_s() {
    let src = "fn f(p: *const u8) -> *const u8 {\n    std::ptr::read_volatile(&p)\n}\n";
    // Over-flag is OK: either std::ptr path or nearby unsafe is enough.
    // `read_volatile` itself is typically inside unsafe; the path is the signal.
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(
        !r.in_subset,
        "std::ptr path should leave S (over-flag OK): {:?}",
        r.violations
    );
}

#[test]
fn rust_parse_error_fails_closed() {
    let src = "fn f( {\n}\n"; // invalid syntax
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(!r.in_subset, "parse error must leave S: {:?}", r.violations);
    assert!(
        r.violations.iter().any(|v| v.kind == "parse_error"),
        "expected parse_error violation: {:?}",
        r.violations
    );
}
