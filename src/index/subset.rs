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
    let base = parser::normalize_root(root);
    for path in files {
        let p_n = parser::normalize_root(&path);
        let rel = p_n
            .strip_prefix(&base)
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

/// True when `s` contains `word` as a standalone identifier (not `__getattr__` / `foogetattr`).
fn contains_ident(s: &str, word: &str) -> bool {
    let bytes = s.as_bytes();
    let w = word.as_bytes();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i + w.len() <= bytes.len() {
        if &bytes[i..i + w.len()] == w {
            let before_ok = i == 0 || !is_ident(bytes[i - 1]);
            let after = i + w.len();
            let after_ok = after >= bytes.len() || !is_ident(bytes[after]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// True when the remainder after `getattr(` proves the second arg is a *plain*
/// string literal on this line (same-line only). Fail-closed: multi-line args,
/// concat, ternary, or a second call of `getattr` with a dynamic name are dynamic.
fn getattr_call_second_is_plain_literal(rest: &str) -> bool {
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    let mut comma_at: Option<usize> = None;
    let mut closed = false;
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
                    closed = true;
                    break;
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
    if comma_at.is_none() {
        // Single-arg getattr is not a name lookup — but only if we actually saw `)`.
        // Open-ended (multi-line args) must leave S.
        return closed;
    }
    let ci = comma_at.unwrap();
    let after = rest[ci + 1..].trim_start();
    let Some(q) = after.chars().next() else {
        return false; // comma then EOL — multi-line second arg
    };
    if q != '\'' && q != '"' {
        return false;
    }
    let body = &after[1..];
    let Some(end) = body.find(q) else {
        return false; // unterminated — multi-line
    };
    let tail = body[end + 1..].trim_start();
    // Only closing of the getattr call (or another arg / comment) may follow.
    tail.is_empty() || tail.starts_with(')') || tail.starts_with(',') || tail.starts_with('#')
}

/// True when this line's `getattr` uses are all safe for S_py.
/// Over-flags: a missed escape is worse than a false S violation.
fn line_getattr_is_s_safe(compact_line: &str) -> bool {
    // `getattr` / `getattr (` split across lines: identifier without `(` on this line.
    if contains_ident(compact_line, "getattr") && !compact_line.contains("getattr(") {
        return false;
    }
    let mut search = 0usize;
    while let Some(rel) = compact_line[search..].find("getattr(") {
        let start = search + rel;
        let rest = &compact_line[start + "getattr(".len()..];
        if !getattr_call_second_is_plain_literal(rest) {
            return false;
        }
        search = start + "getattr(".len();
    }
    true
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

                // Non-literal event keys (emit/on/once/…) leave S — we cannot
                // pair dispatch without a finite event name.
                let is_event_api = matches!(
                    last,
                    "emit"
                        | "trigger"
                        | "publish"
                        | "fire"
                        | "on"
                        | "once"
                        | "subscribe"
                        | "addListener"
                        | "addEventListener"
                );
                // bus['emit'](...) / bus["on"](...)
                let computed_event_key = if unwrapped.kind() == "subscript_expression" {
                    unwrapped
                        .child_by_field_name("index")
                        .map(|k| {
                            let kt = snippet_at(source, k);
                            let inner = string_lit_inner(&kt);
                            matches!(
                                inner,
                                "emit"
                                    | "trigger"
                                    | "publish"
                                    | "fire"
                                    | "on"
                                    | "once"
                                    | "subscribe"
                                    | "addListener"
                                    | "addEventListener"
                            )
                        })
                        .unwrap_or(false)
                } else {
                    false
                };
                if is_event_api || computed_event_key {
                    if let Some(args) = node.child_by_field_name("arguments") {
                        let mut ac = args.walk();
                        let first = args
                            .children(&mut ac)
                            .find(|x| !matches!(x.kind(), "," | "(" | ")"));
                        if let Some(key) = first {
                            let kt = snippet_at(source, key);
                            let is_plain = key.kind() == "string" && {
                                let inner = string_lit_inner(&kt);
                                !inner.is_empty()
                                    && !kt.contains("${")
                                    && inner
                                        .chars()
                                        .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
                            };
                            if !is_plain {
                                push_v(violations, path, line, "nonliteral_event_key", &kt);
                            }
                        }
                    }
                    let is_sub = is_event_api
                        && matches!(
                            last,
                            "on" | "once" | "subscribe" | "addListener" | "addEventListener"
                        );
                    if is_sub {
                        if let Some(args) = node.child_by_field_name("arguments") {
                            let mut ac = args.walk();
                            let named: Vec<_> = args
                                .children(&mut ac)
                                .filter(|x| !matches!(x.kind(), "," | "(" | ")"))
                                .collect();
                            if let Some(handler) = named.get(1) {
                                let mut h = *handler;
                                while h.kind() == "parenthesized_expression" {
                                    let mut pc = h.walk();
                                    let Some(inner) = h
                                        .children(&mut pc)
                                        .find(|x| !matches!(x.kind(), "(" | ")"))
                                    else {
                                        break;
                                    };
                                    h = inner;
                                }
                                let ok_handler = matches!(
                                    h.kind(),
                                    "identifier"
                                        | "member_expression"
                                        | "arrow_function"
                                        | "function_expression"
                                        | "generator_function"
                                        | "func_literal"
                                ) || (h.kind() == "subscript_expression"
                                    && h.child_by_field_name("index")
                                        .map(|k| k.kind() == "string")
                                        .unwrap_or(false));
                                if !ok_handler {
                                    push_v(
                                        violations,
                                        path,
                                        line,
                                        "unmodeled_event_handler",
                                        &snippet_at(source, h),
                                    );
                                }
                            }
                        }
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
        "assignment_expression" | "augmented_assignment_expression" | "variable_declarator" => {
            // Monkey-patching / eval-Function aliasing (C1 R4).
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
            // const e = eval; const F = Function; const e = globalThis.eval;
            if looks_like_eval_alias(&t) {
                push_v(violations, path, line, "eval_alias", &t.replace('\n', " "));
            }
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_js(child, source, path, violations);
    }
}

/// Conservative: any binding that aliases eval/Function leaves S (R5 M6).
fn looks_like_eval_alias(t: &str) -> bool {
    let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    let Some((_, rhs)) = compact.split_once('=') else {
        return false;
    };
    let mut s = rhs;
    // Unwrap (…), including (0, eval)
    loop {
        if s.len() >= 2 && s.starts_with('(') && s.ends_with(')') {
            s = &s[1..s.len() - 1];
        } else {
            break;
        }
    }
    let last = s.rsplit(',').next().unwrap_or(s);
    if last == "eval" || last == "Function" {
        return true;
    }
    // window.eval / globalThis["Function"] / window['eval']
    let tail = last.rsplit('.').next().unwrap_or(last);
    matches!(
        tail,
        "eval" | "Function" | "['eval']" | "[\"eval\"]" | "['Function']" | "[\"Function\"]"
    ) || tail.ends_with("['eval']")
        || tail.ends_with("[\"eval\"]")
        || tail.ends_with("['Function']")
        || tail.ends_with("[\"Function\"]")
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
        // Non-literal / multi-line getattr invents call targets (R6 space + R7 fail-closed).
        if !line_getattr_is_s_safe(&compact) {
            push_v(violations, path, line_no, "py_getattr_dynamic", t);
        }
        // M3: `__builtins__['eval']` / `__builtins__.eval` escapes S_py.
        if t.contains("__builtins__") {
            push_v(violations, path, line_no, "py_builtins", t);
        }
        // R8: sibling dynamic-attr / import APIs leave S_py (fail-closed).
        // R9: also bare identifier without `(` (alias g = attrgetter).
        if compact.contains("__getattribute__(")
            || compact.contains("attrgetter(")
            || compact.contains("attrgetter")
            || compact.contains("__getattribute__")
        {
            push_v(violations, path, line_no, "py_dynamic_attr", t);
        }
        if (compact.contains("vars(")
            || compact.contains("globals(")
            || compact.contains("locals("))
            && t.contains('[')
        {
            push_v(violations, path, line_no, "py_vars_subscript", t);
        }
        // Non-literal importlib.import_module — **every** call on the line (R9).
        if compact.contains("import_module") && !import_module_all_literal(&compact) {
            push_v(violations, path, line_no, "py_import_module_dynamic", t);
        }
    }
}

/// Fail-closed: every `import_module(` on the line must take a plain string first arg.
fn import_module_all_literal(compact: &str) -> bool {
    let mut rest = compact;
    let mut seen = 0usize;
    while let Some(idx) = rest.find("import_module(") {
        seen += 1;
        let after = &rest[idx + "import_module(".len()..];
        let Some(end) = after.find(')') else {
            return false; // multi-line
        };
        let args = after[..end].trim();
        let first = args.split(',').next().unwrap_or("").trim();
        let ok = first.len() >= 2
            && (first.starts_with('\'') || first.starts_with('"'))
            && first.ends_with(first.chars().next().unwrap())
            && !first.contains('+');
        if !ok {
            return false;
        }
        rest = &after[end..];
    }
    seen > 0
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
    fn eval_alias_forms() {
        for t in [
            "e = eval",
            "e = (eval)",
            "e = (0, eval)",
            "e = window['eval']",
            "F = globalThis[\"Function\"]",
        ] {
            assert!(looks_like_eval_alias(t), "should flag {t}");
        }
        assert!(!looks_like_eval_alias("e = 1"));
        assert!(!looks_like_eval_alias("eval = e"));
    }

    #[test]
    fn unknown_heuristic_is_unsound() {
        assert!(!is_sound_eligible(Confidence::Heuristic, Some("nope")));
    }
}
