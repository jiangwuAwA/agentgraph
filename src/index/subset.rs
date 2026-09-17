//! L2 language-subset S: violation scan + sound-edge eligibility.
//!
//! Promise (PLAN §4): for programs inside S, every runtime call edge that can
//! occur is contained in the static over-approximation (`--sound` walk).
//! Outside S: **no** completeness claim. Violations are reported, not hidden.
//!
//! **Language-aware honesty:** AST-modeled S (js/ts/tsx/jsx, python, go, rust)
//! carries a stronger claim than a lexical scanner would. CLI/MCP select the
//! promise string by tier. LexicalV1 remains in the enum for API stability;
//! no currently shipped language selects it.

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use super::parser;
use crate::model::{Confidence, Language};

// ---------------------------------------------------------------------------
// Language-aware sound promise (shared by CLI + MCP — single source of truth)
// ---------------------------------------------------------------------------

/// AST-modeled S (js/ts/tsx/jsx, python, go, rust): full S-qualified OK text.
/// Still an engineering S gate — **not** ecosystem sound / not a proven
/// runtime call-graph over-approx.
pub const SOUND_PROMISE_OK_AST: &str = "S satisfied (AST-modeled subset). Sound walk over-approximates modeled reference edges (direct, literal-key, emit↔on dispatch, DI/route registration). This is NOT a proven runtime call-graph over-approx; registration≠HTTP ServeHTTP. AST scanner is an engineering S gate, not ecosystem sound.";

/// Lexical/scanner v1 (reserved): weaker text for a future non-AST scanner.
/// **No currently shipped language selects this tier** (Rust is AST-modeled).
/// Kept for API stability of `SoundPromiseTier::LexicalV1`.
pub const SOUND_PROMISE_OK_LEXICAL_V1: &str = "S satisfied (scanner tier: lexical v1). Scanner is conservative lexical v1 (not frozen, weaker than AST-modeled S) — this OK is NOT the same assurance as an AST-modeled subset. Sound walk over-approximates modeled reference edges only; this is NOT a proven runtime call-graph over-approx.";

/// Mixed AST + lexical-v1 corpus: weakest tier governs; name both tiers.
/// Reserved — no currently shipped language is lexical-v1.
pub const SOUND_PROMISE_OK_MIXED_LEXICAL_V1: &str = "S satisfied, but the corpus mixes AST-modeled languages with a lexical-v1 scanner. The weakest tier governs: lexical v1 (not frozen) — NOT the same assurance as AST-modeled S. Sound walk over-approximates modeled reference edges only.";

/// Violations present → eligibility claim disabled (unchanged behavior).
pub const SOUND_PROMISE_DISABLED: &str =
    "S violated — eligibility claim disabled; results are best-effort sound-eligible edges only.";

/// Which assurance tier applies to a sound query on this corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundPromiseTier {
    /// AST-modeled S (js/ts/tsx/jsx, python, go, rust): full S-qualified OK text.
    AstModeled,
    /// Lexical/scanner v1 only: weaker text. Reserved — no shipped language
    /// currently selects this arm.
    LexicalV1,
    /// Mixed AST + lexical-v1: weakest tier governs. Reserved for future
    /// non-AST scanners.
    MixedLexicalV1,
    /// Violations present → disabled.
    Disabled,
}

impl SoundPromiseTier {
    pub fn as_str(self) -> &'static str {
        match self {
            SoundPromiseTier::AstModeled => "ast_modeled",
            SoundPromiseTier::LexicalV1 => "lexical_v1",
            SoundPromiseTier::MixedLexicalV1 => "mixed_lexical_v1",
            SoundPromiseTier::Disabled => "disabled",
        }
    }
}

/// Language strings use `Language::as_str()` (from the index `files` table).
///
/// **Reserved:** no currently shipped language is lexical-v1. Rust was
/// upgraded to a tree-sitter AST scanner (`scan_rust`) and joins the AST tier.
/// This function remains so a future non-AST language can re-enter the
/// LexicalV1 / MixedLexicalV1 arms without an API break.
pub fn is_lexical_v1_language(_lang: &str) -> bool {
    false
}

pub fn is_ast_modeled_language(lang: &str) -> bool {
    matches!(
        lang,
        "typescript" | "tsx" | "javascript" | "jsx" | "python" | "go" | "rust"
    )
}

/// Select the promise tier from `subset_ok` + languages in the indexed corpus.
///
/// Rules (fail-honest, weakest tier wins):
/// 1. Any S violation → `Disabled`.
/// 2. Corpus contains a lexical-v1 language **only** → `LexicalV1`
///    (reserved — no shipped language is currently lexical-v1).
/// 3. Corpus mixes AST languages with a lexical-v1 language → `MixedLexicalV1`.
/// 4. Corpus is AST-only (or empty) → `AstModeled` (js/ts/tsx/jsx, python, go, rust).
pub fn sound_promise_tier(subset_ok: bool, languages: &[String]) -> SoundPromiseTier {
    if !subset_ok {
        return SoundPromiseTier::Disabled;
    }
    let has_lexical = languages.iter().any(|l| is_lexical_v1_language(l));
    let has_ast = languages.iter().any(|l| is_ast_modeled_language(l));
    if has_lexical && has_ast {
        SoundPromiseTier::MixedLexicalV1
    } else if has_lexical {
        SoundPromiseTier::LexicalV1
    } else {
        SoundPromiseTier::AstModeled
    }
}

/// Promise text for a tier (shared constants — CLI/MCP must not drift).
pub fn sound_promise_text(tier: SoundPromiseTier) -> &'static str {
    match tier {
        SoundPromiseTier::AstModeled => SOUND_PROMISE_OK_AST,
        SoundPromiseTier::LexicalV1 => SOUND_PROMISE_OK_LEXICAL_V1,
        SoundPromiseTier::MixedLexicalV1 => SOUND_PROMISE_OK_MIXED_LEXICAL_V1,
        SoundPromiseTier::Disabled => SOUND_PROMISE_DISABLED,
    }
}

/// One-shot selection used by CLI/MCP sound payloads.
pub fn select_sound_promise(
    subset_ok: bool,
    languages: &[String],
) -> (SoundPromiseTier, &'static str) {
    let tier = sound_promise_tier(subset_ok, languages);
    (tier, sound_promise_text(tier))
}

/// Heuristic / dynamic rule ids treated as finite-domain over-approx (in S).
/// Keep in sync with `src/index/rules.rs`.
const SOUND_HEURISTIC_RULES: &[&str] = &[
    "ts.di.register",
    "ts.di.bind",
    "ts.di.to",
    "ts.di.decorator",
    // Nest finite-domain registration: identifiers written in @Module metadata
    // / constructor type annotations (registration ≠ HTTP ServeHTTP).
    "ts.nest.module_providers",
    "ts.nest.module_controllers",
    "ts.nest.module_imports",
    "ts.nest.module_exports",
    "ts.nest.ctor_inject",
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
        let Some(rel) = parser::rel_path_under_root(&path, root) else {
            continue;
        };
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
    // R13 M2: ERROR recovery still yields a tree — fail-closed on has_error.
    if tree.root_node().has_error() {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter ERROR nodes — cannot certify S",
        );
        return;
    }
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
/// data:/blob: URLs are **not** static — body is not indexed (R10 C3).
fn static_module_specifier(args: Option<Node>, source: &str) -> bool {
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
    if first.kind() != "string" {
        return false;
    }
    let text = source.get(first.byte_range()).unwrap_or("");
    let low = text.to_ascii_lowercase();
    !(low.contains("data:") || low.contains("blob:"))
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
                // R13 C1: eval.call / Function.bind on the callee text.
                if (last == "call" || last == "apply" || last == "bind")
                    && (text.contains("eval") || text.contains("Function"))
                {
                    push_v(violations, path, line, "eval", &snippet);
                }
                if last == "Proxy" && kind == "new_expression" {
                    push_v(violations, path, line, "Proxy", &snippet);
                }
                // Proxy.revocable / bare revocable aliases (R11 M2).
                if last == "revocable" || text.contains("revocable") {
                    push_v(violations, path, line, "Proxy", &snippet);
                }
                // constructor ×2 or ['constructor'] === Function (R10 C2 + R11).
                if js_text_is_constructor_chain(&text) {
                    push_v(violations, path, line, "Function", &snippet);
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
                    if !static_module_specifier(args, source) {
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
        // Bare eval / Function used as a *value* (callback arg, array element,
        // returned binding). Property access is `property_identifier` and is
        // intentionally not matched — `obj.eval` stays in S.
        "identifier" => {
            let t = snippet_at(source, node);
            if t == "eval" {
                push_v(violations, path, line, "eval", &t);
            } else if t == "Function" {
                push_v(violations, path, line, "Function", &t);
            }
        }
        // Destructuring rename: `const { eval: e } = globalThis` — the local
        // name is `e`, so call-site detection never fires. Catch the pattern key.
        "pair_pattern" => {
            if let Some(key) = node.child_by_field_name("key") {
                let kt = snippet_at(source, key);
                let snip = snippet_at(source, node).replace('\n', " ");
                if kt == "eval" {
                    push_v(violations, path, line, "eval", &snip);
                } else if kt == "Function" {
                    push_v(violations, path, line, "Function", &snip);
                }
            }
        }
        "assignment_expression"
        | "augmented_assignment_expression"
        | "variable_declarator"
        | "return_statement"
        | "subscript_expression" => {
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
            // R10 C2: constructor.constructor === Function
            if js_text_is_constructor_chain(&t) {
                push_v(violations, path, line, "Function", &t.replace('\n', " "));
            }
            // R11 M2: Proxy.revocable alias
            if t.contains("revocable") {
                push_v(violations, path, line, "Proxy", &t.replace('\n', " "));
            }
            // R12 C2/C3: const P = Proxy; const R = Reflect;
            if looks_like_proxy_or_reflect_alias(&t) {
                push_v(violations, path, line, "Proxy", &t.replace('\n', " "));
            }
            // R13 C4: const f = obj[k] — non-literal load of a call target.
            if t.contains('[') && !t.contains("[\"") && !t.contains("['") {
                let c: String = t.chars().filter(|c| !c.is_whitespace()).collect();
                if c.contains("=obj[") || c.contains("=this[") || c.contains("=globalThis[") {
                    push_v(
                        violations,
                        path,
                        line,
                        "nonliteral_computed_key",
                        &t.replace('\n', " "),
                    );
                }
            }
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_js(child, source, path, violations);
    }
}

/// True when text encodes a Function-via-constructor access (any hop — R12 C1).
/// Over-flags `this.constructor` on purpose: S purity > recall.
fn js_text_is_constructor_chain(text: &str) -> bool {
    let compact: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    compact.contains(".constructor")
        || compact.contains("['constructor']")
        || compact.contains("[\"constructor\"]")
        || compact.contains("constructor.constructor")
}

/// Proxy / Reflect identity aliases leave S (R12 C2/C3).
fn looks_like_proxy_or_reflect_alias(t: &str) -> bool {
    let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    let Some((_, rhs)) = compact.split_once('=') else {
        return false;
    };
    let mut s = rhs;
    loop {
        if s.len() >= 2 && s.starts_with('(') && s.ends_with(')') {
            s = &s[1..s.len() - 1];
        } else {
            break;
        }
    }
    let last = s.rsplit(',').next().unwrap_or(s);
    let tail = last.rsplit('.').next().unwrap_or(last);
    matches!(last, "Proxy" | "Reflect")
        || matches!(tail, "Proxy" | "Reflect")
        || tail.ends_with("['Proxy']")
        || tail.ends_with("[\"Proxy\"]")
        || tail.ends_with("['Reflect']")
        || tail.ends_with("[\"Reflect\"]")
}

/// Conservative: any binding that aliases eval/Function leaves S (R5 M6).
/// R13: also .call/.apply/.bind wrappers and TS `as`/`!`/ternary passthrough.
fn looks_like_eval_alias(t: &str) -> bool {
    let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    // Call-family: eval.call / Function.bind anywhere on the line.
    if compact.contains("eval.call")
        || compact.contains("eval.apply")
        || compact.contains("eval.bind")
        || compact.contains("Function.call")
        || compact.contains("Function.apply")
        || compact.contains("Function.bind")
    {
        return true;
    }
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
    // R13 C2: TS wrappers / ternary / await — any eval|Function token in RHS.
    if js_rhs_mentions_eval_or_function(rhs) {
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

/// True when RHS text contains eval/Function as an identifier token (wrappers OK).
fn js_rhs_mentions_eval_or_function(rhs: &str) -> bool {
    // Re-insert spaces around TS wrappers so tokens split (`evalasany` → `eval as any`).
    let expanded = rhs
        .replace("asany", " as any ")
        .replace("satisfies", " satisfies ")
        .replace("asunknown", " as unknown ")
        .replace("asnever", " as never ");
    let mut in_tok = false;
    let mut tok = String::new();
    for ch in expanded.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '$' {
            tok.push(ch);
            in_tok = true;
        } else {
            if in_tok && (tok == "eval" || tok == "Function") {
                return true;
            }
            tok.clear();
            in_tok = false;
        }
    }
    in_tok && (tok == "eval" || tok == "Function")
}

fn scan_py(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    // AST scanner (tree-sitter-python). Fail-closed on parse errors.
    // Comments/strings are not AST call targets — they no longer false-positive.
    let Ok(tree) = parser::parse(source, Language::Python) else {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter failed to parse — cannot certify S",
        );
        return;
    };
    if tree.root_node().has_error() {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter ERROR nodes — cannot certify S",
        );
        return;
    }
    // Cheap alias pass: `from builtins import eval as e` / `import builtins as b`.
    let mut import_aliases: Vec<(String, String)> = Vec::new();
    collect_py_dangerous_imports(tree.root_node(), source, &mut import_aliases);
    walk_py_s(tree.root_node(), source, path, &import_aliases, violations);
}

/// Dangerous Python names that leave S when referenced (not only when called).
const PY_DANGEROUS_NAMES: &[&str] = &[
    "eval",
    "exec",
    "__import__",
    "__getattribute__",
    "attrgetter",
    "methodcaller",
    "FunctionType",
    "compile",
];

/// Import aliases that shadow dangerous builtins: local_name → original.
fn collect_py_dangerous_imports(node: Node, source: &str, out: &mut Vec<(String, String)>) {
    let mut cursor = node.walk();
    match node.kind() {
        "aliased_import" => {
            // `eval as e` — original then alias.
            let mut names = Vec::new();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "dotted_name" || child.kind() == "identifier" {
                    names.push(snippet_at(source, child));
                }
            }
            if names.len() >= 2 {
                out.push((names[1].clone(), names[0].clone()));
            }
        }
        "import_from_statement" => {
            // `from builtins import eval` (no alias) — local name is the original.
            let mut module = String::new();
            let mut cursor2 = node.walk();
            for child in node.children(&mut cursor2) {
                if child.kind() == "dotted_name" {
                    module = snippet_at(source, child);
                    break;
                }
            }
            let dangerous_module = matches!(
                module.as_str(),
                "builtins" | "importlib" | "operator" | "types"
            );
            if dangerous_module {
                let mut cursor3 = node.walk();
                for child in node.children(&mut cursor3) {
                    if child.kind() == "dotted_name" || child.kind() == "aliased_import" {
                        // handled below via recursion; bare names:
                    }
                    if child.kind() == "dotted_name"
                        && child.parent().map(|p| p.id()) == Some(node.id())
                    {
                        let name = snippet_at(source, child);
                        if PY_DANGEROUS_NAMES.contains(&name.as_str()) || name == "import_module" {
                            out.push((name.clone(), name));
                        }
                    }
                }
                // Wildcard / parenthesized import list: walk named children for identifiers.
                let mut cursor4 = node.walk();
                for child in node.named_children(&mut cursor4) {
                    if child.kind() == "dotted_name" {
                        let name = snippet_at(source, child);
                        if name != module
                            && (PY_DANGEROUS_NAMES.contains(&name.as_str())
                                || name == "import_module")
                            && !out.iter().any(|(l, _)| *l == name)
                        {
                            out.push((name.clone(), name));
                        }
                    } else if child.kind() == "aliased_import" {
                        let mut names = Vec::new();
                        let mut c5 = child.walk();
                        for g in child.named_children(&mut c5) {
                            if g.kind() == "dotted_name" || g.kind() == "identifier" {
                                names.push(snippet_at(source, g));
                            }
                        }
                        if names.len() >= 2
                            && (PY_DANGEROUS_NAMES.contains(&names[0].as_str())
                                || names[0] == "import_module")
                        {
                            out.push((names[1].clone(), names[0].clone()));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_py_dangerous_imports(child, source, out);
    }
}

/// Resolve a Python callee node to a flat name for S checks.
/// `eval` → `eval`; `importlib.import_module` → `import_module`;
/// `__builtins__.eval` → `eval`.
fn py_callee_flat_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(snippet_at(source, node)),
        "attribute" => {
            let attr = node.child_by_field_name("attribute")?;
            Some(snippet_at(source, attr))
        }
        _ => None,
    }
}

/// Full dotted text of a callee (`importlib.import_module`, `builtins.eval`).
fn py_callee_dotted(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(snippet_at(source, node)),
        "attribute" => {
            let obj = node.child_by_field_name("object")?;
            let attr = node.child_by_field_name("attribute")?;
            let obj_s = py_callee_dotted(obj, source)?;
            Some(format!("{obj_s}.{}", snippet_at(source, attr)))
        }
        _ => None,
    }
}

/// True when this Python expression is a plain string literal node.
fn py_is_string_literal(node: Node) -> bool {
    matches!(node.kind(), "string" | "concatenated_string")
}

/// Second positional arg of a call, if present.
fn py_call_second_arg(call: Node) -> Option<Node> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let named: Vec<Node> = args.named_children(&mut cursor).collect();
    named.get(1).copied()
}

/// First positional arg of a call, if present.
fn py_call_first_arg(call: Node) -> Option<Node> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let named: Vec<Node> = args.named_children(&mut cursor).collect();
    named.first().copied()
}

fn walk_py_s(
    node: Node,
    source: &str,
    path: &str,
    import_aliases: &[(String, String)],
    violations: &mut Vec<SubsetViolation>,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let line = line_of_offset(source, node.start_byte());
    let snippet = snippet_at(source, node).replace('\n', " ");

    match kind {
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                let flat = py_callee_flat_name(func, source);
                let dotted = py_callee_dotted(func, source);
                let flat_s = flat.as_deref().unwrap_or("");
                // Import alias: `e = eval` via `from builtins import eval as e`.
                let flat_resolved = import_aliases
                    .iter()
                    .find(|(local, _)| *local == flat_s)
                    .map(|(_, orig)| orig.as_str())
                    .unwrap_or(flat_s);

                if matches!(flat_resolved, "eval" | "exec") {
                    push_v(violations, path, line, "py_eval_exec", &snippet);
                }
                if flat_resolved == "__import__" {
                    push_v(violations, path, line, "py___import__", &snippet);
                }
                if flat_resolved == "setattr" {
                    push_v(violations, path, line, "py_setattr", &snippet);
                }
                if flat_resolved == "compile" {
                    push_v(violations, path, line, "py_dynamic_attr", &snippet);
                }
                // getattr: second arg must be a string literal (finite domain).
                // A string naming eval/exec/__import__ invents a call target.
                if flat_resolved == "getattr" {
                    match py_call_second_arg(node) {
                        Some(arg) if py_is_string_literal(arg) => {
                            let inner = snippet_at(source, arg);
                            let inner_trim = inner.trim_matches(|c| c == '\'' || c == '"');
                            if matches!(inner_trim, "eval" | "exec" | "__import__") {
                                push_v(violations, path, line, "py_eval_alias", &snippet);
                            }
                        }
                        Some(_) => {
                            push_v(violations, path, line, "py_getattr_dynamic", &snippet);
                        }
                        None => {
                            // Single-arg getattr is not a name lookup — stay in S.
                        }
                    }
                }
                // importlib.import_module / from-import import_module:
                // first arg must be a string literal.
                if flat_resolved == "import_module"
                    || dotted.as_deref() == Some("importlib.import_module")
                {
                    match py_call_first_arg(node) {
                        Some(arg) if py_is_string_literal(arg) => {}
                        _ => {
                            push_v(violations, path, line, "py_import_module_dynamic", &snippet);
                        }
                    }
                }
                // __getattribute__ / attrgetter / methodcaller / FunctionType calls.
                if matches!(
                    flat_resolved,
                    "__getattribute__" | "attrgetter" | "methodcaller" | "FunctionType"
                ) {
                    push_v(violations, path, line, "py_dynamic_attr", &snippet);
                }
                // vars/globals/locals used as subscript base: vars()['x'].
                if matches!(flat_resolved, "vars" | "globals" | "locals") {
                    if let Some(parent) = node.parent() {
                        if parent.kind() == "subscript" {
                            push_v(violations, path, line, "py_vars_subscript", &snippet);
                        }
                    }
                }
            }
        }
        "attribute" => {
            // __builtins__.eval / __builtins__['eval'] handled via subscript/attribute.
            let text = snippet_at(source, node);
            if text.contains("__builtins__") {
                push_v(violations, path, line, "py_builtins", &snippet);
            }
            if text.ends_with(".__dict__") || text == "__dict__" {
                push_v(violations, path, line, "py_dynamic_attr", &snippet);
            }
            // .eval / .exec member access (builtins.eval, obj.eval, …).
            if let Some(attr) = node.child_by_field_name("attribute") {
                let an = snippet_at(source, attr);
                if an == "eval" || an == "exec" {
                    push_v(violations, path, line, "py_eval_alias", &snippet);
                }
            }
        }
        "subscript" => {
            let text = snippet_at(source, node);
            if text.contains("__builtins__") {
                push_v(violations, path, line, "py_builtins", &snippet);
            }
            if text.contains("__dict__") {
                push_v(violations, path, line, "py_dynamic_attr", &snippet);
            }
            // __builtins__['eval'] / d['eval'] via string key.
            if let Some(sub) = node.child_by_field_name("subscript") {
                if py_is_string_literal(sub) {
                    let inner = snippet_at(source, sub);
                    let inner_trim = inner.trim_matches(|c| c == '\'' || c == '"');
                    if matches!(inner_trim, "eval" | "exec" | "__import__") {
                        push_v(violations, path, line, "py_eval_alias", &snippet);
                    }
                }
            }
        }
        "assignment" => {
            // e = eval / e = exec / e = __import__
            if let Some(right) = node.child_by_field_name("right") {
                let rt = snippet_at(source, right);
                if matches!(rt.as_str(), "eval" | "exec" | "__import__") {
                    push_v(violations, path, line, "py_eval_alias", &snippet);
                }
                // e = builtins.eval
                if right.kind() == "attribute" {
                    if let Some(attr) = right.child_by_field_name("attribute") {
                        let an = snippet_at(source, attr);
                        if an == "eval" || an == "exec" {
                            push_v(violations, path, line, "py_eval_alias", &snippet);
                        }
                    }
                }
            }
        }
        "identifier" => {
            // Bare reference to a dangerous name (not the callee of a call —
            // those are already flagged). Catches `e = eval` and `return attrgetter`.
            let name = snippet_at(source, node);
            let resolved = import_aliases
                .iter()
                .find(|(local, _)| *local == name)
                .map(|(_, orig)| orig.clone())
                .unwrap_or_else(|| name.clone());
            let is_callee = node
                .parent()
                .map(|p| {
                    p.kind() == "call"
                        && p.child_by_field_name("function")
                            .map(|f| f.id() == node.id())
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if !is_callee {
                if matches!(resolved.as_str(), "eval" | "exec" | "__import__") {
                    push_v(violations, path, line, "py_eval_alias", &snippet);
                }
                if matches!(
                    resolved.as_str(),
                    "__getattribute__" | "attrgetter" | "methodcaller" | "FunctionType" | "compile"
                ) {
                    push_v(violations, path, line, "py_dynamic_attr", &snippet);
                }
                if resolved == "__builtins__" {
                    push_v(violations, path, line, "py_builtins", &snippet);
                }
                if resolved == "__dict__" {
                    push_v(violations, path, line, "py_dynamic_attr", &snippet);
                }
            }
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_py_s(child, source, path, import_aliases, violations);
    }
}

/// `//go:linkname` and `//export` are significant cgo/compiler directives.
/// `//export Foo` exports a Go symbol to C — same escape class as linkname.
fn go_comment_is_linkname(text: &str) -> bool {
    let t = text.trim_start();
    t.contains("go:linkname")
        || t.starts_with("//export ")
        || t.starts_with("//export\t")
        || t == "//export"
}

fn scan_go(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    // AST scanner (tree-sitter-go). Fail-closed on parse errors.
    // Comments/strings are not AST selectors — they no longer false-positive
    // (except //go:linkname, which is a significant compiler directive).
    let Ok(tree) = parser::parse(source, Language::Go) else {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter failed to parse — cannot certify S",
        );
        return;
    };
    if tree.root_node().has_error() {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter ERROR nodes — cannot certify S",
        );
        return;
    }
    walk_go_s(tree.root_node(), source, path, violations);
}

fn walk_go_s(node: Node, source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let line = line_of_offset(source, node.start_byte());
    let snippet = snippet_at(source, node).replace('\n', " ");

    match kind {
        "comment" => {
            if go_comment_is_linkname(&snippet_at(source, node)) {
                push_v(violations, path, line, "go_cgo_linkname", &snippet);
            }
        }
        "import_spec" => {
            // import "C" / import "unsafe" / import "reflect"
            if let Some(path_node) = node.child_by_field_name("path") {
                let raw = snippet_at(source, path_node);
                let import_path = raw.trim_matches(|c| c == '"' || c == '`');
                if matches!(import_path, "C" | "unsafe" | "reflect") {
                    push_v(violations, path, line, "go_cgo_linkname", &snippet);
                }
            }
        }
        "selector_expression" => {
            // unsafe.X / reflect.X / plugin.Open / syscall.NewCallback
            if let Some(operand) = node.child_by_field_name("operand") {
                if operand.kind() == "identifier" {
                    let op = snippet_at(source, operand);
                    if op == "unsafe" {
                        push_v(violations, path, line, "go_unsafe", &snippet);
                    }
                    if op == "reflect" {
                        push_v(violations, path, line, "go_reflect", &snippet);
                    }
                    if op == "plugin" || op == "syscall" {
                        if let Some(field) = node.child_by_field_name("field") {
                            let fname = snippet_at(source, field);
                            if (op == "plugin" && fname == "Open")
                                || (op == "syscall" && fname == "NewCallback")
                            {
                                push_v(violations, path, line, "go_dynamic_symbol", &snippet);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_go_s(child, source, path, violations);
    }
}

/// True when this node (or a direct / `function_modifiers` child) is `unsafe`.
fn rs_has_unsafe_token(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "unsafe" {
            return true;
        }
        // `unsafe fn`: token lives under `function_modifiers`.
        if child.kind() == "function_modifiers" {
            let mut mc = child.walk();
            let mut found = false;
            for g in child.children(&mut mc) {
                if g.kind() == "unsafe" {
                    found = true;
                    break;
                }
            }
            if found {
                return true;
            }
        }
    }
    false
}

/// Flatten a scoped identifier / type path to text (`std::mem::transmute`).
fn rs_path_text(node: Node, source: &str) -> String {
    snippet_at(source, node).replace([' ', '\n', '\t'], "")
}

fn scan_rust(source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    // AST scanner (tree-sitter-rust). Fail-closed on parse errors.
    // Comments/strings are not AST items — they no longer false-positive.
    let Ok(tree) = parser::parse(source, Language::Rust) else {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter failed to parse — cannot certify S",
        );
        return;
    };
    if tree.root_node().has_error() {
        push_v(
            violations,
            path,
            1,
            "parse_error",
            "tree-sitter ERROR nodes — cannot certify S",
        );
        return;
    }
    walk_rs_s(tree.root_node(), source, path, violations);
}

fn walk_rs_s(node: Node, source: &str, path: &str, violations: &mut Vec<SubsetViolation>) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let line = line_of_offset(source, node.start_byte());
    let snippet = snippet_at(source, node).replace('\n', " ");

    match kind {
        // unsafe { ... }
        "unsafe_block" => {
            push_v(violations, path, line, "unsafe", &snippet);
        }
        // unsafe fn / unsafe extern fn (modifiers child carries the token).
        "function_item" | "function_signature_item" => {
            if rs_has_unsafe_token(node) {
                push_v(violations, path, line, "unsafe", &snippet);
            }
        }
        // unsafe impl / unsafe trait
        "impl_item" | "trait_item" => {
            if rs_has_unsafe_token(node) {
                push_v(violations, path, line, "unsafe", &snippet);
            }
        }
        // asm! / global_asm! (over-flag: any asm macro leaves S).
        "macro_invocation" => {
            if let Some(mac) = node.child_by_field_name("macro") {
                let t = snippet_at(source, mac);
                let last = t.rsplit("::").next().unwrap_or(t.as_str());
                if last == "asm" || last == "global_asm" {
                    push_v(violations, path, line, "asm", &snippet);
                }
            }
        }
        // transmute calls / paths; std::ptr::* / core::ptr::* (over-flag OK).
        "identifier" => {
            let t = snippet_at(source, node);
            if t == "transmute" {
                push_v(violations, path, line, "transmute", &snippet);
            }
        }
        "scoped_identifier" => {
            let text = rs_path_text(node, source);
            let last = text.rsplit("::").next().unwrap_or(text.as_str());
            if last == "transmute" || text.ends_with("::mem::transmute") {
                push_v(violations, path, line, "transmute", &snippet);
            }
            if text.starts_with("std::ptr::")
                || text.starts_with("core::ptr::")
                || text == "std::ptr"
                || text == "core::ptr"
            {
                push_v(violations, path, line, "std_ptr", &snippet);
            }
        }
        "scoped_use_list" | "use_declaration" => {
            // use std::ptr / use core::mem::transmute
            let text = rs_path_text(node, source);
            if text.contains("std::ptr") || text.contains("core::ptr") {
                push_v(violations, path, line, "std_ptr", &snippet);
            }
            if text.contains("::transmute") || text.ends_with("transmute") {
                push_v(violations, path, line, "transmute", &snippet);
            }
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_rs_s(child, source, path, violations);
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
