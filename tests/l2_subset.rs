//! TDD: L2 sound subset — S-violation scan + sound-eligible edges + --sound queries.

use agentgraph::index::extract::extract_file;
use agentgraph::index::subset::{scan_subset, SoundClass};
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

fn extract(src: &str, lang: Language, path: &str) -> agentgraph::index::extract::ExtractedFile {
    let known = HashSet::new();
    extract_file(src, lang, path, &known).unwrap()
}

#[test]
fn s_js_clean_program_has_no_violations() {
    let src = r#"
export function authenticate(email, password) {
  return { email, password };
}
export function loginHandler(email, password) {
  return authenticate(email, password);
}
"#;
    let report = scan_subset(src, Language::TypeScript, "src/auth.ts");
    assert!(
        report.violations.is_empty(),
        "clean S_js program must have zero violations, got {:?}",
        report.violations
    );
    assert!(report.in_subset, "clean program must be in S");
}

// m9: scan_subset must parse with the file's real Language (TSX/JSX).
#[test]
fn s_tsx_jsx_parses_with_tsx_grammar() {
    let src = r#"
export function Panel() {
  return <div className="x">ok</div>;
}
export function usePanel() {
  return Panel();
}
"#;
    let r = scan_subset(src, Language::Tsx, "src/panel.tsx");
    assert!(
        r.in_subset,
        "TSX source must parse with TSX grammar (no parse_error): {:?}",
        r.violations
    );
    assert!(
        !r.violations.iter().any(|v| v.kind == "parse_error"),
        "must not report parse_error for valid TSX: {:?}",
        r.violations
    );
}

#[test]
fn ts_emit_literal_is_finite_domain_dynamic() {
    use agentgraph::index::extract::extract_file;
    use agentgraph::model::Language;
    use std::collections::HashSet;
    let src = r#"
export function wire(bus: any) {
  bus.emit('trade');
}
"#;
    let out = extract_file(src, Language::JavaScript, "src/emit.js", &HashSet::new()).unwrap();
    let hit = out.references.iter().any(|r| {
        r.name == "trade" && r.confidence == agentgraph::model::Confidence::DynamicCandidate
    });
    assert!(
        hit,
        "emit('trade') must yield DynamicCandidate trade; refs={:?}",
        out.references
    );
}

#[test]
fn s_js_nonliteral_event_key_is_violation() {
    let src = r#"
const k = getEvt();
bus.emit(k);
bus.on(k, handler);
"#;
    let r = scan_subset(src, Language::JavaScript, "src/evtk.js");
    assert!(
        !r.in_subset,
        "non-literal emit/on key must leave S: {:?}",
        r.violations
    );
}

#[test]
fn s_js_unmodeled_event_handler_is_violation() {
    let src = "bus.on('e', a || b);\n";
    let r = scan_subset(src, Language::JavaScript, "src/or.js");
    assert!(
        !r.in_subset,
        "a||b handler must leave S: {:?}",
        r.violations
    );
}

#[test]
fn s_js_eval_alias_leaves_s() {
    let src = "const e = eval;\nexport function main() { e('x'); }\n";
    let r = scan_subset(src, Language::JavaScript, "src/alias.js");
    assert!(!r.in_subset, "eval alias must leave S: {:?}", r.violations);
}

#[test]
fn s_js_eval_alias_paren_and_subscript_leave_s() {
    for src in [
        "const e = (eval);\n",
        "const e = (0, eval);\n",
        "const e = window['eval'];\n",
        "const F = globalThis[\"Function\"];\n",
    ] {
        let r = scan_subset(src, Language::JavaScript, "src/a.js");
        assert!(!r.in_subset, "must leave S: {src:?} → {:?}", r.violations);
    }
}

#[test]
fn py_getattr_with_space_leaves_s() {
    let src = "def f(obj, name):\n    return getattr (obj, name)\n";
    let r = scan_subset(src, Language::Python, "src/g.py");
    assert!(
        !r.in_subset,
        "getattr (obj, name) must leave S: {:?}",
        r.violations
    );
}

#[test]
fn py_getattr_multiline_open_leaves_s() {
    let src = "def f(obj, name):\n    return getattr(\n        obj, name\n    )\n";
    let r = scan_subset(src, Language::Python, "src/g_ml.py");
    assert!(
        !r.in_subset,
        "multi-line getattr( must leave S: {:?}",
        r.violations
    );
}

#[test]
fn py_getattr_split_before_comma_leaves_s() {
    let src = "def f(obj, name):\n    x = getattr(obj\n    , name)\n    return x\n";
    let r = scan_subset(src, Language::Python, "src/g_sp.py");
    assert!(
        !r.in_subset,
        "getattr split before comma must leave S: {:?}",
        r.violations
    );
}

#[test]
fn py_second_getattr_dynamic_on_same_line_leaves_s() {
    let src = "def f(a, b, name):\n    return (getattr(a, \"ok\"), getattr(b, name))\n";
    let r = scan_subset(src, Language::Python, "src/g_2nd.py");
    assert!(
        !r.in_subset,
        "second getattr dynamic must leave S: {:?}",
        r.violations
    );
}

#[test]
fn py_getattr_ternary_and_concat_leave_s() {
    for src in [
        "def f(obj, c):\n    return getattr(obj, 'a' if c else 'b')\n",
        "def f(obj, name):\n    return getattr(obj, 'pre' + name)\n",
    ] {
        let r = scan_subset(src, Language::Python, "src/g_dyn.py");
        assert!(
            !r.in_subset,
            "non-plain getattr second arg must leave S: {src:?} {:?}",
            r.violations
        );
    }
}

#[test]
fn py_getattr_plain_literal_stays_in_s() {
    let src = "def f(obj):\n    return getattr(obj, \"name\")\n";
    let r = scan_subset(src, Language::Python, "src/g_ok.py");
    assert!(
        r.in_subset,
        "getattr(obj, \"name\") must stay in S: {:?}",
        r.violations
    );
}

#[test]
fn py_dynamic_attr_family_leaves_s() {
    for src in [
        "x = obj.__getattribute__(name)\n",
        "from operator import attrgetter\nf = attrgetter(name)\n",
        "d = vars(obj)[name]\n",
        "import importlib\nm = importlib.import_module(name)\n",
    ] {
        let r = scan_subset(src, Language::Python, "src/d.py");
        assert!(!r.in_subset, "must leave S: {src:?} → {:?}", r.violations);
    }
}

#[test]
fn py_multi_import_module_same_line_leaves_s() {
    let src = "import importlib\nm = importlib.import_module(\"pkg.mod\"); n = importlib.import_module(name)\n";
    let r = scan_subset(src, Language::Python, "src/mm.py");
    assert!(
        !r.in_subset,
        "second dynamic import_module: {:?}",
        r.violations
    );
}

#[test]
fn py_getattribute_alias_leaves_s() {
    let src = "g = obj.__getattribute__\nreturn g(name)\n";
    let r = scan_subset(src, Language::Python, "src/alias.py");
    assert!(!r.in_subset, "getattribute alias: {:?}", r.violations);
}

#[test]
fn py_literal_import_module_stays_in_s() {
    let src = "import importlib\nm = importlib.import_module(\"pkg.mod\")\n";
    let r = scan_subset(src, Language::Python, "src/ok.py");
    assert!(
        r.in_subset,
        "literal import_module stays in S: {:?}",
        r.violations
    );
}

#[test]
fn js_constructor_constructor_leaves_s() {
    let src = "const F = ({}).constructor.constructor;\nF('return 1')();\n";
    let r = scan_subset(src, Language::JavaScript, "src/cc.js");
    assert!(!r.in_subset, "constructor.constructor: {:?}", r.violations);
}

#[test]
fn js_data_url_import_leaves_s() {
    let src = "await import('data:text/javascript,export const x=1');\n";
    let r = scan_subset(src, Language::JavaScript, "src/data.js");
    assert!(!r.in_subset, "data: import: {:?}", r.violations);
}

#[test]
fn py_eval_alias_bare_leaves_s() {
    let src = "e = eval\ne('pass')\n";
    let r = scan_subset(src, Language::Python, "src/e.py");
    assert!(!r.in_subset, "eval alias: {:?}", r.violations);
}

#[test]
fn go_cgo_linkname_leaves_s() {
    let src = "package main\n//go:linkname f runtime.f\nfunc f()\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(!r.in_subset, "linkname: {:?}", r.violations);
}

#[test]
fn js_constructor_subscript_chain_leaves_s() {
    for src in [
        "const F = {}['constructor']['constructor'];\n",
        "const F = {}.constructor['constructor'];\n",
    ] {
        let r = scan_subset(src, Language::JavaScript, "src/c.js");
        assert!(!r.in_subset, "must leave S: {src:?} {:?}", r.violations);
    }
}

#[test]
fn js_proxy_revocable_alias_leaves_s() {
    let src = "const { revocable } = Proxy;\nrevocable({}, {});\n";
    let r = scan_subset(src, Language::JavaScript, "src/pr.js");
    assert!(!r.in_subset, "revocable alias: {:?}", r.violations);
}

#[test]
fn py_paren_eval_and_builtins_getattr_leave_s() {
    for src in [
        "e = (eval)\n",
        "import builtins\ne = getattr(builtins, 'eval')\n",
    ] {
        let r = scan_subset(src, Language::Python, "src/e.py");
        assert!(!r.in_subset, "must leave S: {src:?} {:?}", r.violations);
    }
}

#[test]
fn go_cgo_import_block_leaves_s() {
    let src = "package main\nimport (\n\t\"C\"\n)\nfunc main() {}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(!r.in_subset, "cgo block: {:?}", r.violations);
}

#[test]
fn js_object_constructor_single_hop_leaves_s() {
    let src = "const F = Object.constructor;\nF('return 1')();\n";
    let r = scan_subset(src, Language::JavaScript, "src/oc.js");
    assert!(!r.in_subset, "Object.constructor: {:?}", r.violations);
}

#[test]
fn js_proxy_reflect_alias_leaves_s() {
    for src in [
        "const P = Proxy;\nnew P({}, {});\n",
        "const R = Reflect;\nR.construct(Function, []);\n",
    ] {
        let r = scan_subset(src, Language::JavaScript, "src/a.js");
        assert!(!r.in_subset, "must leave S: {src:?} {:?}", r.violations);
    }
}

#[test]
fn py_builtins_eval_member_leaves_s() {
    let src = "import builtins\ne = builtins.eval\n";
    let r = scan_subset(src, Language::Python, "src/b.py");
    assert!(!r.in_subset, "builtins.eval: {:?}", r.violations);
}

#[test]
fn go_cgo_comment_alias_forms_leave_s() {
    for src in [
        "package main\nimport (\n\t\"C\" // cgo\n)\n",
        "package main\nimport (\n\t_ \"C\"\n)\n",
        "package main\nimport  \"C\"\n",
    ] {
        let r = scan_subset(src, Language::Go, "main.go");
        assert!(!r.in_subset, "must leave S: {src:?} {:?}", r.violations);
    }
}

#[test]
fn s_js_eval_is_violation() {
    let src = r#"export function evil(x) { return eval(x); }"#;
    let report = scan_subset(src, Language::TypeScript, "src/evil.ts");
    assert!(!report.in_subset);
    assert!(
        report.violations.iter().any(|v| v.kind.contains("eval")),
        "eval must be an S violation: {:?}",
        report.violations
    );
}

#[test]
fn s_js_new_function_proxy_with_are_violations() {
    for (src, needle) in [
        ("export const f = new Function('return 1');", "Function"),
        (
            "export function w(obj) { with (obj) { return x; } }",
            "with",
        ),
        ("export const p = new Proxy({}, {});", "Proxy"),
    ] {
        let report = scan_subset(src, Language::TypeScript, "src/x.ts");
        assert!(!report.in_subset, "must leave S: {src}");
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.kind.contains(needle) || v.snippet.contains(needle)),
            "expected {needle} violation in {:?}",
            report.violations
        );
    }
}

#[test]
fn s_js_template_computed_key_is_violation() {
    let src = r#"
export function run(obj, key) {
  return obj[`m${key}`]();
}
"#;
    let report = scan_subset(src, Language::TypeScript, "src/tpl.ts");
    assert!(
        !report.in_subset,
        "template computed call key leaves S: {:?}",
        report.violations
    );
}

#[test]
fn s_js_function_without_new_is_violation() {
    let src = "export function f() { return Function('return 1')(); }\n";
    let r = scan_subset(src, Language::JavaScript, "src/fn.js");
    assert!(
        !r.in_subset,
        "Function() without new must leave S: {:?}",
        r.violations
    );
}

#[test]
fn s_js_nonliteral_computed_key_is_violation() {
    let src = r#"
export function run(reg, k) {
  return reg[k]();
}
"#;
    let r = scan_subset(src, Language::JavaScript, "src/k.js");
    assert!(
        !r.in_subset,
        "obj[key]() non-literal must leave S: {:?}",
        r.violations
    );
}

#[test]
fn s_js_string_literal_computed_key_stays_in_s() {
    let src = r#"
export function run(reg) {
  return reg['doWork']();
}
"#;
    let r = scan_subset(src, Language::JavaScript, "src/ok.js");
    assert!(
        r.in_subset,
        "obj['doWork']() string literal stays in S: {:?}",
        r.violations
    );
}

#[test]
fn s_js_monkey_patch_prototype_is_violation() {
    let src = "Function.prototype.f = function () { return 1; };\n";
    let r = scan_subset(src, Language::JavaScript, "src/mp.js");
    assert!(
        !r.in_subset,
        "prototype patch must leave S: {:?}",
        r.violations
    );
}

#[test]
fn sound_eligibility_allowlist() {
    use agentgraph::index::subset::is_sound_eligible;
    assert!(is_sound_eligible(Confidence::Exact, None));
    assert!(is_sound_eligible(
        Confidence::Heuristic,
        Some("ts.di.register")
    ));
    assert!(is_sound_eligible(
        Confidence::DynamicCandidate,
        Some("ts.dynamic.computed")
    ));
    assert!(!is_sound_eligible(
        Confidence::DynamicCandidate,
        Some("unknown.rule")
    ));
    assert!(!is_sound_eligible(Confidence::Heuristic, None));
}

#[test]
fn s_js_call_sites_produce_sound_edges() {
    // Invariant I1-ish: every direct call in an S program has an Exact edge.
    let src = r#"
export function a() { return 1; }
export function b() { return a() + a(); }
export function c() { return b(); }
"#;
    let out = extract(src, Language::TypeScript, "src/chain.ts");
    for target in ["a", "b"] {
        let hits: Vec<_> = out
            .references
            .iter()
            .filter(|r| r.name == target && r.confidence == Confidence::Exact)
            .collect();
        assert!(
            !hits.is_empty(),
            "direct call to {target} must yield Exact edge; refs={:?}",
            out.references
                .iter()
                .map(|r| (r.name.clone(), r.confidence.as_str()))
                .collect::<Vec<_>>()
        );
    }
}

// --- C2: S_js scanner false-negatives ---

#[test]
fn s_js_paren_comma_eval_is_violation() {
    // (0, eval)(x) — indirect eval via parenthesized comma expression.
    let src = "export function evil(x) { return (0, eval)(x); }\n";
    let r = scan_subset(src, Language::JavaScript, "src/evil.js");
    assert!(
        !r.in_subset,
        "(0, eval)(x) must leave S: {:?}",
        r.violations
    );
    assert!(
        r.violations.iter().any(|v| v.kind.contains("eval")),
        "expected eval kind: {:?}",
        r.violations
    );
}

#[test]
fn s_js_parenthesized_bare_eval_is_violation() {
    let src = "export function evil(x) { return (eval)(x); }\n";
    let r = scan_subset(src, Language::JavaScript, "src/evil.js");
    assert!(!r.in_subset, "(eval)(x) must leave S: {:?}", r.violations);
}

#[test]
fn s_js_window_eval_subscript_is_violation() {
    for src in [
        "export function evil(x) { return window['eval'](x); }\n",
        "export function evil(x) { return globalThis['eval'](x); }\n",
        "export function evil(x) { return window['Function']('return 1'); }\n",
    ] {
        let r = scan_subset(src, Language::JavaScript, "src/evil.js");
        assert!(
            !r.in_subset,
            "window/globalThis['eval'/'Function'] must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn s_js_require_nonliteral_is_violation() {
    for src in [
        "export function load(n) { return require(n); }\n",
        "export function load(p) { return require('./' + p); }\n",
    ] {
        let r = scan_subset(src, Language::JavaScript, "src/load.js");
        assert!(
            !r.in_subset,
            "require(non-literal) must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn s_js_require_literal_stays_in_s() {
    let src = "export function load() { return require('./util'); }\n";
    let r = scan_subset(src, Language::JavaScript, "src/ok.js");
    assert!(
        r.in_subset,
        "require('literal') stays in S: {:?}",
        r.violations
    );
}

#[test]
fn s_js_dynamic_import_nonliteral_is_violation() {
    let src = "export async function load(n) { return import(n); }\n";
    let r = scan_subset(src, Language::JavaScript, "src/dyn.js");
    assert!(
        !r.in_subset,
        "import(non-literal) must leave S: {:?}",
        r.violations
    );
}

#[test]
fn s_js_dynamic_import_literal_stays_in_s() {
    let src = "export async function load() { return import('./util.js'); }\n";
    let r = scan_subset(src, Language::JavaScript, "src/ok.js");
    assert!(
        r.in_subset,
        "import('literal') stays in S: {:?}",
        r.violations
    );
}

#[test]
fn sound_class_on_refs() {
    let src = r#"
export class UserService {}
export function bootstrap(c) {
  c.register(UserService);
}
export function dyn(obj) {
  obj['UserService']();
}
"#;
    let out = extract(src, Language::TypeScript, "src/mix.ts");
    let classes: Vec<SoundClass> = out
        .references
        .iter()
        .map(|r| {
            let rule = r.evidence.as_ref().map(|e| e.rule_id.as_str());
            SoundClass::of(r.confidence, rule)
        })
        .collect();
    assert!(
        classes.iter().any(|c| matches!(c, SoundClass::Sound)),
        "Exact or allowlisted Heuristic must be Sound: {classes:?}"
    );
    assert!(
        classes
            .iter()
            .any(|c| matches!(c, SoundClass::SoundFiniteDomain)),
        "literal computed key is finite-domain Sound: {classes:?}"
    );
}
