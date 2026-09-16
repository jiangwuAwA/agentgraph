//! L2 language-subset S: violation scan + sound-edge eligibility.
//!
//! Promise (PLAN §4): for programs inside S, every runtime call edge that can
//! occur is contained in the static over-approximation (`--sound` walk).
//! Outside S: **no** completeness claim. Violations are reported, not hidden.

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use super::parser;
use crate::model::{Confidence, Language};

/// Heuristic / dynamic rule ids treated as finite-domain over-approx (in S).
/// Keep in sync with `src/index/rules.rs`.
const SOUND_HEURISTIC_RULES: &[&str] = &[
    "ts.di.register",
    "ts.di.bind",
    "ts.di.to",
    "ts.di.decorator",
    "ts.event.subscribe",
    "ts.event.dispatch",
    "py.di.depends",
    "py.di.inject",
    "py.framework.init_subclass",
    "go.di.handler_map",
    "go.di.interface_impl",
    "go.di.interface_assert",
    "go.di.route_register",
    "rs.di.impl_trait",
];

/// DynamicCandidate rules that only fire on **string-literal** keys (finite domain).
const SOUND_FINITE_DYNAMIC_RULES: &[&str] = &[
    "ts.dynamic.computed",
    "py.dynamic.getattr",
    "py.dynamic.import_module",
    "ts.event.emit",
];

/// How an edge participates in the sound over-approx.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundClass {
    /// Exact syntactic edge, or allowlisted DI/event heuristic.
    Sound,
    /// String-literal / finite-domain dynamic target (still in S over-approx).
    SoundFiniteDomain,
    /// Not trusted for soundness (unknown rule or non-allowlisted dynamic).
    Unsound,
}

impl SoundClass {
    pub fn of(confidence: Confidence, rule_id: Option<&str>) -> Self {
        match confidence {
            Confidence::Exact => SoundClass::Sound,
            Confidence::Heuristic => {
                if rule_id.is_some_and(|r| SOUND_HEURISTIC_RULES.contains(&r)) {
                    SoundClass::Sound
                } else {
                    SoundClass::Unsound
                }
            }
            Confidence::DynamicCandidate => {
                if rule_id.is_some_and(|r| SOUND_FINITE_DYNAMIC_RULES.contains(&r)) {
                    SoundClass::SoundFiniteDomain
                } else {
                    SoundClass::Unsound
                }
            }
        }
    }

    pub fn in_sound_walk(self) -> bool {
        matches!(self, SoundClass::Sound | SoundClass::SoundFiniteDomain)
    }
}

/// True when this (confidence, rule_id) pair may be traversed by `--sound`.
pub fn is_sound_eligible(confidence: Confidence, rule_id: Option<&str>) -> bool {
    SoundClass::of(confidence, rule_id).in_sound_walk()
}

/// One construct that takes the program outside subset S.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubsetViolation {
    pub kind: String,
    pub path: String,
    pub line: usize,
    pub snippet: String,
}

/// Result of scanning one file against S_js / S_rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsetReport {
    pub path: String,
    pub language: String,
    pub in_subset: bool,
    pub violations: Vec<SubsetViolation>,
}

/// Scan a source file for constructs forbidden by subset S (PLAN §4.2).
pub fn scan_subset(source: &str, lang: Language, path: &str) -> SubsetReport {
    let mut violations = Vec::new();
    match lang {
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            scan_js(source, lang, path, &mut violations)
        }
        Language::Rust => scan_rust(source, path, &mut violations),
        // Python / Go S v1: conservative lexical/AST escapes leave S.
        Language::Python => scan_py(source, path, &mut violations),
        Language::Go => scan_go(source, path, &mut violations),
    }
    SubsetReport {
        path: path.to_string(),
        language: lang.as_str().to_string(),
        in_subset: violations.is_empty(),
        violations,
    }
}

/// Scan an entire index root (all supported source files).
pub fn scan_tree(root: &std::path::Path) -> Vec<SubsetReport> {
    let mut out = Vec::new();
    let Ok(files) = super::walker::collect_source_files(root) else {
        return out;
    };
    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let Some(lang) = Language::from_path(&rel) else {
            continue;
        };
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        out.push(scan_subset(&src, lang, &rel));
    }
    out
}

fn push_v(
    violations: &mut Vec<SubsetViolation>,
    path: &str,
    line: usize,
    kind: &str,
    snippet: &str,
) {
    violations.push(SubsetViolation {
        kind: kind.to_string(),
        path: path.to_string(),
        line,
        snippet: snippet.chars().take(120).collect(),
    });
}

fn line_of_offset(source: &str, byte: usize) -> usize {
    source[..byte.min(source.len())].matches('\n').count() + 1
}

fn snippet_at(source: &str, node: Node) -> String {
    source.get(node.byte_range()).unwrap_or("").to_string()
}

fn scan_js(source: &str, lang: Language, path: &str, violations: &mut Vec<SubsetViolation>) {
    // m9: use the file's real Language so .tsx/.jsx parse as TSX, not TS.
    let Ok(tree) = parser::parse(source, lang) else {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter failed to parse — cannot certify S",
        );
        return;
    };
    walk_js(tree.root_node(), source, path, violations);
}

/// Unwrap `( ... )` and comma/sequence expressions so `(0, eval)(x)` /
/// `(eval)(x)` resolve to the actual callee.
fn unwrap_js_callee<'a>(mut n: Node<'a>) -> Node<'a> {
    loop {
        match n.kind() {
            "parenthesized_expression" => {
                let mut pc = n.walk();
                let mut inner = None;
                for child in n.children(&mut pc) {
                    if !matches!(child.kind(), "(" | ")") {
                        inner = Some(child);
                        break;
                    }
                }
                match inner {
                    Some(i) => n = i,
                    None => break,
                }
            }
            "sequence_expression" => {
                // Last operand of a comma expression is the effective callee.
                let mut pc = n.walk();
                let mut last = None;
                for child in n.named_children(&mut pc) {
                    last = Some(child);
                }
                match last {
                    Some(i) => n = i,
                    None => break,
                }
            }
            _ => break,
        }
    }
    n
}

fn string_lit_inner(text: &str) -> &str {
    text.trim_matches(|ch| ch == '\'' || ch == '"' || ch == '`')
}

/// True when the call's single argument is a plain `string` node (static module specifier).
fn static_module_specifier(args: Option<Node>) -> bool {
    let Some(args) = args else {
        return false;
    };
    let mut cursor = args.walk();
    let mut named = args.named_children(&mut cursor);
    let Some(first) = named.next() else {
        return false;
    };
    if named.next().is_some() {
        return false;
    }
    first.kind() == "string"
}

fn walk_js(node: Node, source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let line = line_of_offset(source, node.start_byte());

    match kind {
        "with_statement" => {
            push_v(
                violations,
                path,
                line,
                "with",
                &snippet_at(source, node).replace('\n', " "),
            );
        }
        "call_expression" | "new_expression" => {
            let callee = node
                .child_by_field_name("function")
                .or_else(|| node.child_by_field_name("constructor"));
            if let Some(c) = callee {
                let snippet = snippet_at(source, node).replace('\n', " ");
                // Unwrap `(0, eval)` / `(eval)` BEFORE eval/Function detection.
                let unwrapped = unwrap_js_callee(c);
                let text = snippet_at(source, unwrapped);
                let last = text.rsplit('.').next().unwrap_or(text.as_str());
                let is_ident = unwrapped.kind() == "identifier";

                // Bare identifier named eval/Function (incl. after unwrap).
                // Member access `foo.eval` / `window.Function` via last segment.
                if last == "eval" || (is_ident && text == "eval") {
                    push_v(violations, path, line, "eval", &snippet);
                }
                if last == "Function" || (is_ident && text == "Function") {
                    push_v(violations, path, line, "Function", &snippet);
                }
                if last == "Proxy" && kind == "new_expression" {
                    push_v(violations, path, line, "Proxy", &snippet);
                }
                // Reflect.* — left S.
                if text == "Reflect" || text.starts_with("Reflect.") || last == "Reflect" {
                    push_v(violations, path, line, "Reflect", &snippet);
                }

                // require(non-literal) / dynamic import(non-literal): S_js requires
                // a static module graph.
                let is_require = is_ident && text == "require";
                let is_dyn_import = unwrapped.kind() == "import";
                if is_require || is_dyn_import {
                    let args = node.child_by_field_name("arguments");
                    if !static_module_specifier(args) {
                        let kind_s = if is_require {
                            "require_dynamic"
                        } else {
                            "import_dynamic"
                        };
                        push_v(violations, path, line, kind_s, &snippet);
                    }
                }

                // Subscript callee: obj['eval'] / obj[k]
                if unwrapped.kind() == "subscript_expression" {
                    if let Some(key) = unwrapped.child_by_field_name("index") {
                        let kt = snippet_at(source, key);
                        let key_kind = key.kind();
                        let is_stringish = key_kind == "string" || key_kind == "template_string";
                        let inner = string_lit_inner(&kt);
                        // String key eval/Function is still an escape hatch.
                        if is_stringish && (inner == "eval" || inner == "Function") {
                            push_v(
                                violations,
                                path,
                                line,
                                if inner == "eval" { "eval" } else { "Function" },
                                &snippet,
                            );
                        }
                        let is_plain_string_lit = is_stringish && {
                            !inner.is_empty()
                                && !kt.contains("${")
                                && inner.chars().all(|ch| {
                                    ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == '-'
                                })
                        };
                        if !is_plain_string_lit {
                            let kind_s = if kt.contains("${") {
                                "template_computed_key"
                            } else {
                                "nonliteral_computed_key"
                            };
                            push_v(violations, path, line, kind_s, &kt);
                        }
                    }
                }
            }
        }
        "assignment_expression" | "augmented_assignment_expression" => {
            // Monkey-patching: obj.fn = ..., Function.prototype.x = ..., global.eval = ...
            let t = snippet_at(source, node);
            if t.contains("prototype")
                || t.starts_with("globalThis")
                || t.starts_with("global.")
                || t.starts_with("window.")
            {
                push_v(
                    violations,
                    path,
                    line,
                    "monkey_patch",
                    &t.replace('\n', " "),
                );
            }
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_js(child, source, path, violations);
    }
}

/// Drop whitespace that sits immediately before `(` so `eval (` matches `eval(`.
fn strip_space_before_paren(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_ws = String::new();
    for ch in s.chars() {
        if ch == ' ' || ch == '\t' {
            pending_ws.push(ch);
            continue;
        }
        if ch == '(' {
            pending_ws.clear();
        } else {
            out.push_str(&pending_ws);
            pending_ws.clear();
        }
        out.push(ch);
    }
    out.push_str(&pending_ws);
    out
}

/// True when `getattr(`'s second argument is a plain string literal.
/// Conservative: anything else (identifiers, concatenations, calls) is dynamic.
fn getattr_second_is_string_literal(line: &str) -> bool {
    let Some(start) = line.find("getattr(") else {
        return true;
    };
    let rest = &line[start + "getattr(".len()..];
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    let mut comma_at: Option<usize> = None;
    for (i, ch) in rest.char_indices() {
        if let Some(q) = in_str {
            if ch == q {
                in_str = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => in_str = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                if depth == 0 {
                    return true; // no second arg
                }
                depth -= 1;
            }
            ',' if depth == 0 => {
                comma_at = Some(i);
                break;
            }
            _ => {}
        }
    }
    let Some(ci) = comma_at else {
        return true; // single-arg getattr — not a name lookup
    };
    let after = rest[ci + 1..].trim_start();
    after.starts_with('\'') || after.starts_with('"')
}

fn scan_py(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    // v1 conservative lexical scanner (not a full freeze / AST analysis).
    // Prefer over-flag: a false violation is cheaper than a missed escape.
    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let t = raw_line.trim();
        if t.starts_with('#') {
            continue;
        }
        let compact = strip_space_before_paren(t);
        if compact.contains("eval(") || compact.contains("exec(") {
            push_v(violations, path, line_no, "py_eval_exec", t);
        }
        if compact.contains("__import__(") {
            push_v(violations, path, line_no, "py___import__", t);
        }
        if compact.contains("setattr(") {
            push_v(violations, path, line_no, "py_setattr", t);
        }
        // M3: non-literal getattr second arg invents call targets.
        if t.contains("getattr(") && !getattr_second_is_string_literal(t) {
            push_v(violations, path, line_no, "py_getattr_dynamic", t);
        }
        // M3: `__builtins__['eval']` / `__builtins__.eval` escapes S_py.
        if t.contains("__builtins__") {
            push_v(violations, path, line_no, "py_builtins", t);
        }
    }
}

fn scan_go(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let t = raw_line.trim();
        if t.starts_with("//") {
            continue;
        }
        // unsafe.Pointer / unsafe.Sizeof / unsafe.Add — leave S_go v1.
        if t.contains("unsafe.") || t.starts_with("unsafe ") {
            push_v(violations, path, line_no, "go_unsafe", t);
        }
        // reflect.Value.Call / MethodByName invents call edges.
        if t.contains("reflect.") {
            push_v(violations, path, line_no, "go_reflect", t);
        }
        // plugin.Open / syscall.NewCallback-style dynamic symbols.
        if t.contains("plugin.Open") || t.contains("syscall.NewCallback") {
            push_v(violations, path, line_no, "go_dynamic_symbol", t);
        }
    }
}

fn scan_rust(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let t = raw_line.trim();
        if t.starts_with("//") {
            continue;
        }
        // `unsafe {` blocks / fn are outside S_rs v1 (fn-pointer black magic).
        if t.starts_with("unsafe ")
            || t.contains(" unsafe ")
            || t == "unsafe {"
            || t.starts_with("unsafe {")
        {
            // allow `unsafe impl` still flagged — conservative
            push_v(violations, path, line_no, "unsafe", t);
        }
        if t.contains("transmute") {
            push_v(violations, path, line_no, "transmute", t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_is_always_sound() {
        assert!(is_sound_eligible(Confidence::Exact, None));
        assert_eq!(SoundClass::of(Confidence::Exact, None), SoundClass::Sound);
    }

    #[test]
    fn unknown_heuristic_is_unsound() {
        assert!(!is_sound_eligible(Confidence::Heuristic, Some("nope")));
    }
}
