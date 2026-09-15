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
    "py.di.depends",
    "py.di.inject",
    "go.di.handler_map",
    "rs.di.impl_trait",
];

/// DynamicCandidate rules that only fire on **string-literal** keys (finite domain).
const SOUND_FINITE_DYNAMIC_RULES: &[&str] = &[
    "ts.dynamic.computed",
    "py.dynamic.getattr",
    "py.dynamic.import_module",
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
            scan_js(source, path, &mut violations)
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

fn scan_js(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    let Ok(tree) = parser::parse(source, Language::TypeScript) else {
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
                let text = snippet_at(source, c);
                let last = text.rsplit('.').next().unwrap_or(&text);
                // eval / (0,eval) — last segment must be exactly eval
                if last == "eval" {
                    push_v(
                        violations,
                        path,
                        line,
                        "eval",
                        &snippet_at(source, node).replace('\n', " "),
                    );
                }
                // Function('...') with or without new (C2)
                if last == "Function" {
                    push_v(
                        violations,
                        path,
                        line,
                        "Function",
                        &snippet_at(source, node).replace('\n', " "),
                    );
                }
                if last == "Proxy" && kind == "new_expression" {
                    push_v(
                        violations,
                        path,
                        line,
                        "Proxy",
                        &snippet_at(source, node).replace('\n', " "),
                    );
                }
                // Reflect.* — left S.
                if text == "Reflect" || text.starts_with("Reflect.") {
                    push_v(
                        violations,
                        path,
                        line,
                        "Reflect",
                        &snippet_at(source, node).replace('\n', " "),
                    );
                }
                // Computed call keys: only string literals stay in S (C2).
                let mut n = c;
                while n.kind() == "parenthesized_expression" {
                    let mut pc = n.walk();
                    let Some(inner) = n.children(&mut pc).find(|x| !matches!(x.kind(), "(" | ")"))
                    else {
                        break;
                    };
                    n = inner;
                }
                if n.kind() == "subscript_expression" {
                    if let Some(key) = n.child_by_field_name("index") {
                        let kt = snippet_at(source, key);
                        // Only string / template nodes can be finite-domain keys.
                        let key_kind = key.kind();
                        let is_stringish = key_kind == "string" || key_kind == "template_string";
                        let is_plain_string_lit = is_stringish && {
                            let inner = kt.trim_matches(|ch| ch == '\'' || ch == '"' || ch == '`');
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

fn scan_py(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    // Lightweight lexical scan: eval( / exec( / __import__ with dynamic name.
    // Also: monkey-patching callables via setattr(obj, ...) leaves S_py v1.
    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let t = raw_line.trim();
        if t.starts_with('#') {
            continue;
        }
        if t.contains("eval(") || t.contains("exec(") {
            push_v(violations, path, line_no, "py_eval_exec", t);
        }
        if t.contains("__import__(") {
            push_v(violations, path, line_no, "py___import__", t);
        }
        if t.contains("setattr(") {
            push_v(violations, path, line_no, "py_setattr", t);
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
