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
