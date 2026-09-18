//! L1 heuristic / dynamic-candidate edge rules.
//!
//! Runs after L0 syntactic extract. Appends refs with
//! `Confidence::Heuristic` or `Confidence::DynamicCandidate` and
//! `Evidence { rule_id, snippet }`. Never claims soundness.

use tree_sitter::Node;

use super::extract::{ExtractContext, ExtractedRef};
use crate::model::{Confidence, EdgeKind, Evidence, Language};

/// Append L1 candidate edges for `source` onto `references`.
pub fn apply(
    lang: Language,
    root: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    match lang {
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            walk_ts(root, source, ctx, references, None);
        }
        Language::Python => walk_py(root, source, ctx, references, None),
        Language::Go => {
            let go_idx = collect_go_iface_index(root, source);
            walk_go(root, source, ctx, references, &go_idx);
        }
        Language::Rust => {
            let inv_submit_aliases = collect_inventory_submit_aliases(root, source);
            let dyn_idx = collect_rust_dyn_index(root, source);
            walk_rust(
                root,
                source,
                ctx,
                references,
                &inv_submit_aliases,
                &dyn_idx,
                None,
            );
        }
    }
}

fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

struct L1Edge {
    name: String,
    qualifier: Option<String>,
    line: usize,
    enclosing: Option<String>,
    confidence: Confidence,
    rule_id: &'static str,
    snippet: String,
}

fn push_l1(references: &mut Vec<ExtractedRef>, edge: L1Edge) {
    push_l1_mod(references, edge, None);
}

fn push_l1_mod(references: &mut Vec<ExtractedRef>, edge: L1Edge, module: Option<String>) {
    references.push(ExtractedRef {
        name: edge.name,
        kind: EdgeKind::Call,
        line: edge.line,
        enclosing: edge.enclosing,
        module,
        resolved: None,
        qualifier: edge.qualifier,
        confidence: edge.confidence,
        evidence: Some(Evidence {
            rule_id: edge.rule_id.to_string(),
            snippet: edge.snippet,
        }),
    });
}

fn line_of(ctx: &ExtractContext<'_>, node: Node) -> usize {
    ctx.lines.line_of(node.start_byte())
}

/// Bare identifier / type identifier text if the node is one.
/// Member expressions resolve to the property name (`TYPES.UserRepository` → `UserRepository`).
fn ident_name(node: Node, source: &str) -> Option<String> {
    let t = node_text(node, source);
    if t.is_empty() {
        return None;
    }
    match node.kind() {
        "identifier" | "type_identifier" | "property_identifier" => Some(t.to_string()),
        "member_expression" => node
            .child_by_field_name("property")
            .map(|p| node_text(p, source).to_string())
            .or_else(|| t.rsplit('.').next().map(|s| s.to_string())),
        _ => {
            // `new Foo()` function field may be identifier or nested expression.
            if node.child_count() == 0
                && t.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            {
                Some(t.to_string())
            } else {
                None
            }
        }
    }
}

/// Last identifier segment of a dotted name: `ioc.container.bind` → `bind`.
fn last_segment(s: &str) -> &str {
    s.rsplit(['.', ':']).next().unwrap_or(s)
}

// ── TypeScript / JavaScript ─────────────────────────────────────────

fn walk_ts(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let mut local_enclosing = enclosing;

    // Track named scopes so L1 edges can expand impact BFS (review M1).
    if matches!(
        kind,
        "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "function_expression"
            | "arrow_function"
            | "class_declaration"
            | "class"
    ) {
        if let Some(n) = ts_scope_name(node, source).filter(|s| !s.is_empty()) {
            local_enclosing = Some(n);
        }
    }

    match kind {
        "call_expression" | "new_expression" => {
            ts_call_rules(node, source, ctx, references, local_enclosing.clone());
        }
        "decorator" => {
            ts_decorator_rule(node, source, ctx, references, local_enclosing.clone());
        }
        "method_definition" => {
            ts_nest_ctor_inject(node, source, ctx, references);
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_ts(child, source, ctx, references, local_enclosing.clone());
    }
}

/// Scope display name: class uses `name` field (type_identifier), functions use
/// `name` (identifier / property_identifier).
fn ts_scope_name(node: Node, source: &str) -> Option<String> {
    if let Some(n) = node.child_by_field_name("name") {
        let t = node_text(n, source).to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    // Fallback for nodes without a name field (shouldn't hit classes).
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        if matches!(
            ch.kind(),
            "identifier" | "type_identifier" | "property_identifier"
        ) {
            let t = node_text(ch, source);
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

/// Nearest enclosing `class_declaration` / `class` name, if any.
fn enclosing_class_name(node: Node, source: &str) -> Option<String> {
    let mut n = node.parent();
    while let Some(p) = n {
        if matches!(p.kind(), "class_declaration" | "class") {
            return ts_scope_name(p, source);
        }
        n = p.parent();
    }
    None
}

/// Class name a decorator applies to (export sibling or parent class).
fn decorated_class_name(decorator: Node, source: &str) -> Option<String> {
    if let Some(parent) = decorator.parent() {
        if matches!(parent.kind(), "class_declaration" | "class") {
            return ts_scope_name(parent, source);
        }
        if parent.kind() == "export_statement" {
            let mut c = parent.walk();
            for ch in parent.children(&mut c) {
                if matches!(ch.kind(), "class_declaration" | "class") {
                    return ts_scope_name(ch, source);
                }
            }
        }
    }
    // Decorator may sit just before a sibling class_declaration.
    let mut sib = decorator.next_sibling();
    while let Some(s) = sib {
        if matches!(s.kind(), "class_declaration" | "class") {
            return ts_scope_name(s, source);
        }
        if s.kind() == "decorator" {
            sib = s.next_sibling();
            continue;
        }
        break;
    }
    None
}

fn unwrap_parens(node: Node) -> Node {
    let mut n = node;
    while n.kind() == "parenthesized_expression" {
        let mut c = n.walk();
        let Some(inner) = n.children(&mut c).find(|x| !matches!(x.kind(), "(" | ")")) else {
            break;
        };
        n = inner;
    }
    n
}

fn ts_call_rules(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    let mut fn_node = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"));
    if fn_node.is_none() {
        // new (expr)(): callee is a bare child, often parenthesized.
        let mut c = node.walk();
        let mut found = None;
        for n in node.children(&mut c) {
            if matches!(
                n.kind(),
                "identifier"
                    | "member_expression"
                    | "subscript_expression"
                    | "parenthesized_expression"
            ) {
                found = Some(unwrap_parens(n));
                break;
            }
        }
        fn_node = found;
    }
    let Some(fn_node) = fn_node.map(unwrap_parens) else {
        return;
    };
    let fn_text = node_text(fn_node, source);
    let method = last_segment(fn_text).to_string();
    let line = line_of(ctx, node);

    // container.register(X) / c.register(X)
    if method == "register" {
        if let Some(arg) = first_interesting_arg(node, source) {
            if let Some(name) = ident_name(arg, source) {
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line,
                        enclosing: enclosing.clone(),
                        confidence: Confidence::Heuristic,
                        rule_id: "ts.di.register",
                        snippet: format!("{}({})", fn_text, node_text(arg, source)),
                    },
                );
            }
        }
    }

    // M3-D: Express/Fastify-style framework registration.
    // router.get/post(..., handler) / app.use(middleware) / app.register(path, handler)
    // Finite handler identifiers written at the registration site.
    ts_framework_register_rule(
        node,
        source,
        ctx,
        references,
        &method,
        fn_text,
        enclosing.clone(),
        line,
    );

    // Nest dynamic module: ConfigModule.forRootAsync({ imports, inject, useFactory })
    // / TypeOrmModule.forRootAsync(...). Config-object DI deps are real registrations.
    if matches!(method.as_str(), "forRootAsync" | "forRoot") {
        ts_nest_for_root_config(
            node,
            source,
            ctx,
            references,
            enclosing.clone(),
            &format!("{fn_text}(...)"),
        );
    }

    // container.bind(X) / ioc.bind(X)
    if method == "bind" {
        if let Some(arg) = first_interesting_arg(node, source) {
            if let Some(name) = ident_name(arg, source) {
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line,
                        enclosing: enclosing.clone(),
                        confidence: Confidence::Heuristic,
                        rule_id: "ts.di.bind",
                        snippet: format!("{}.bind({})", fn_text, node_text(arg, source)),
                    },
                );
            }
        }
    }

    // .to(Y) — often chained after bind; function field is member_expression
    if method == "to" {
        if let Some(arg) = first_interesting_arg(node, source) {
            if let Some(name) = ident_name(arg, source) {
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line,
                        enclosing: enclosing.clone(),
                        confidence: Confidence::Heuristic,
                        rule_id: "ts.di.to",
                        snippet: format!("{}.to({})", fn_text, node_text(arg, source)),
                    },
                );
            }
        }
    }

    // obj['name']() / new (registry['Name'])() — DynamicCandidate
    if fn_node.kind() == "subscript_expression" {
        let key = fn_node.child_by_field_name("index").or_else(|| {
            let mut c = fn_node.walk();
            let mut found = None;
            for n in fn_node.children(&mut c) {
                if matches!(n.kind(), "string" | "template_string" | "string_fragment") {
                    found = Some(n);
                    break;
                }
            }
            found
        });
        if let Some(key) = key {
            if let Some(name) = string_literal_content(key, source) {
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line,
                        enclosing: enclosing.clone(),
                        confidence: Confidence::DynamicCandidate,
                        rule_id: "ts.dynamic.computed",
                        snippet: format!(
                            "{}[...]",
                            node_text(fn_node, source)
                                .chars()
                                .take(80)
                                .collect::<String>()
                        ),
                    },
                );
            }
        }
    }

    // emitter.on('evt', handler) / bus.subscribe('evt', handler)
    // also gin-like: e.GET("/users", GetUsers) / mux.HandleFunc(path, h)
    let is_route = matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "Handle" | "HandleFunc" | "Any"
    );
    // L2: emit('evt') — dispatch site; event name is a finite-domain candidate.
    // Also match obj['emit'] / obj["on"] (computed member with string key).
    let method_from_computed = || -> Option<String> {
        let mut n = fn_node;
        while n.kind() == "parenthesized_expression" {
            let mut c = n.walk();
            let inner = n
                .children(&mut c)
                .find(|x| !matches!(x.kind(), "(" | ")"))?;
            n = inner;
        }
        if n.kind() == "subscript_expression" {
            if let Some(key) = n.child_by_field_name("index") {
                return string_literal_content(key, source);
            }
        }
        None
    };
    let method_eff = if matches!(
        method.as_str(),
        "emit"
            | "trigger"
            | "publish"
            | "fire"
            | "on"
            | "once"
            | "subscribe"
            | "addListener"
            | "addEventListener"
    ) {
        method.clone()
    } else {
        method_from_computed().unwrap_or_default()
    };
    if matches!(method_eff.as_str(), "emit" | "trigger" | "publish" | "fire") {
        if let Some(key) = nth_arg_string_lit(node, source, 0) {
            push_l1_mod(
                references,
                L1Edge {
                    name: key.clone(),
                    qualifier: None,
                    line,
                    enclosing: enclosing.clone(),
                    confidence: Confidence::DynamicCandidate,
                    rule_id: "ts.event.emit",
                    snippet: format!("{method_eff}('{key}')"),
                },
                Some(key),
            );
        }
    }
    let is_subscribe = matches!(
        method_eff.as_str(),
        "on" | "once" | "subscribe" | "addListener" | "addEventListener"
    );
    // HTTP routes: existing uppercase (Go-like) + M3-D lowercase Express verbs.
    let is_route = is_route
        || matches!(
            method.as_str(),
            "get" | "post" | "put" | "delete" | "patch" | "options" | "head" | "all" | "route"
        );
    if is_subscribe || is_route {
        let evt = nth_arg_string_lit(node, source, 0);
        let mut handlers: Vec<String> = Vec::new();
        let arg1 = nth_arg_node(node, source, 1);
        let is_fn_expr = arg1
            .map(|a| {
                matches!(
                    a.kind(),
                    "arrow_function"
                        | "function_expression"
                        | "generator_function"
                        | "func_literal"
                )
            })
            .unwrap_or(false);
        if is_fn_expr {
            if let Some(arg) = arg1 {
                collect_call_names(arg, source, &mut handlers);
            }
        } else if let Some(arg) = arg1 {
            // Subscript handler obj['handleX'] or identifier / member.
            if arg.kind() == "subscript_expression" {
                if let Some(key) = arg.child_by_field_name("index") {
                    if let Some(h) = string_literal_content(key, source) {
                        handlers.push(h);
                    }
                }
            } else if let Some(h) = ident_name(arg, source) {
                handlers.push(h);
            } else if let Some(h) = nth_arg_identifier(node, source, 1) {
                handlers.push(h);
            }
        }
        for handler in handlers {
            let rid = if is_route {
                // Lowercase Express-style verbs → M3-D id; uppercase keeps legacy id.
                if method
                    .chars()
                    .next()
                    .map(|c| c.is_lowercase())
                    .unwrap_or(false)
                {
                    "ts.framework.register"
                } else {
                    "go.di.route_register"
                }
            } else {
                "ts.event.subscribe"
            };
            push_l1_mod(
                references,
                L1Edge {
                    name: handler,
                    qualifier: None,
                    line,
                    enclosing: enclosing.clone(),
                    confidence: Confidence::Heuristic,
                    rule_id: rid,
                    snippet: format!("{}(...)", fn_text),
                },
                evt.clone(),
            );
        }
    }
}

/// M3-D: `router.get(path, h)` / `app.use(middleware)` / `app.register(path, h)`.
/// Finite handler identifiers at the registration call site. Heuristic only.
#[allow(clippy::too_many_arguments)]
fn ts_framework_register_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    method: &str,
    fn_text: &str,
    enclosing: Option<String>,
    line: usize,
) {
    let _ = ctx;
    let is_http_verb = matches!(
        method,
        "get"
            | "post"
            | "put"
            | "delete"
            | "patch"
            | "options"
            | "head"
            | "all"
            | "route"
            | "GET"
            | "POST"
            | "PUT"
            | "DELETE"
            | "PATCH"
    );
    let is_use = method == "use";
    let is_fw_register = method == "register" || method == "mount" || method == "handle";
    if !is_http_verb && !is_use && !is_fw_register {
        return;
    }
    // Handler position: use → arg0; verbs/register → arg1 if arg0 looks like a path.
    let mut candidates: Vec<(usize, String)> = Vec::new();
    let arg0 = nth_arg_node(node, source, 0);
    let arg1 = nth_arg_node(node, source, 1);
    let arg0_is_path = arg0
        .map(|a| {
            matches!(a.kind(), "string" | "string_fragment" | "template_string")
                || string_literal_content(a, source).is_some()
                || node_text(a, source).starts_with('/')
                || node_text(a, source).starts_with('\'')
                || node_text(a, source).starts_with('"')
        })
        .unwrap_or(false);

    if is_use {
        if let Some(a) = arg0 {
            if let Some(n) = ident_name(a, source) {
                if !n.is_empty() && n != method {
                    candidates.push((0, n));
                }
            }
        }
    }
    if is_http_verb || is_fw_register {
        let idx = if arg0_is_path || arg1.is_some() { 1 } else { 0 };
        if let Some(a) = nth_arg_node(node, source, idx) {
            if let Some(n) = ident_name(a, source) {
                if !n.is_empty() && n != method {
                    candidates.push((idx, n));
                }
            }
        }
        // Fastify/koa sometimes register(handler) with no path.
        if candidates.is_empty() && is_fw_register {
            if let Some(a) = arg0 {
                if let Some(n) = ident_name(a, source) {
                    if !n.is_empty() && n != method {
                        candidates.push((0, n));
                    }
                }
            }
        }
    }
    for (_, handler) in candidates {
        push_l1(
            references,
            L1Edge {
                name: handler,
                qualifier: None,
                line,
                enclosing: enclosing.clone(),
                confidence: Confidence::Heuristic,
                rule_id: "ts.framework.register",
                snippet: format!("{fn_text}(...) framework registration"),
            },
        );
    }
}

fn nth_arg_node<'a>(node: Node<'a>, _source: &str, idx: usize) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let args = node
        .children(&mut cursor)
        .find(|c| c.kind() == "arguments")?;
    let mut ac = args.walk();
    let named: Vec<Node> = args
        .children(&mut ac)
        .filter(|c| !matches!(c.kind(), "," | "(" | ")"))
        .collect();
    named.get(idx).copied()
}

/// Collect direct call / member-call base names inside a handler body.
fn collect_call_names(node: Node, source: &str, out: &mut Vec<String>) {
    let mut cursor = node.walk();
    if node.kind() == "call_expression" {
        if let Some(f) = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("constructor"))
        {
            if let Some(n) = ident_name(f, source) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    for c in node.children(&mut cursor) {
        collect_call_names(c, source, out);
    }
}

fn nth_arg_string_lit(node: Node, source: &str, idx: usize) -> Option<String> {
    let mut cursor = node.walk();
    let args = node
        .children(&mut cursor)
        .find(|c| c.kind() == "arguments")?;
    let mut ac = args.walk();
    let named: Vec<Node> = args
        .children(&mut ac)
        .filter(|c| !matches!(c.kind(), "," | "(" | ")"))
        .collect();
    let arg = named.get(idx)?;
    string_literal_content(*arg, source)
}

fn first_interesting_arg<'a>(node: Node<'a>, _source: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let args = node
        .children(&mut cursor)
        .find(|c| c.kind() == "arguments")?;
    let mut ac = args.walk();
    for a in args.children(&mut ac) {
        match a.kind() {
            "identifier"
            | "type_identifier"
            | "member_expression"
            | "subscript_expression"
            | "string"
            | "string_fragment" => return Some(a),
            _ => {}
        }
    }
    None
}

fn nth_arg_identifier(node: Node, source: &str, idx: usize) -> Option<String> {
    let mut cursor = node.walk();
    let args = node
        .children(&mut cursor)
        .find(|c| c.kind() == "arguments")?;
    let mut ac = args.walk();
    let named: Vec<Node> = args
        .children(&mut ac)
        .filter(|c| !matches!(c.kind(), "," | "(" | ")"))
        .collect();
    let arg = named.get(idx)?;
    // handler may be identifier, member (Obj.method), or arrow
    if let Some(n) = ident_name(*arg, source) {
        return Some(n);
    }
    if arg.kind() == "member_expression" {
        if let Some(prop) = arg.child_by_field_name("property") {
            return Some(node_text(prop, source).to_string());
        }
        let t = node_text(*arg, source);
        return t.rsplit('.').next().map(|s| s.to_string());
    }
    if arg.kind() == "arrow_function" || arg.kind() == "function_expression" {
        return first_identifier_descendant(*arg, source);
    }
    None
}

fn first_identifier_descendant(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "call_expression" {
            if let Some(f) = c.child_by_field_name("function") {
                if let Some(n) = ident_name(f, source) {
                    return Some(n);
                }
            }
        }
        if let Some(found) = first_identifier_descendant(c, source) {
            return Some(found);
        }
    }
    // fallback: any identifier
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "identifier" {
            let t = node_text(c, source);
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
        if let Some(n) = first_identifier_descendant(c, source) {
            return Some(n);
        }
    }
    None
}

fn string_literal_content(node: Node, source: &str) -> Option<String> {
    let t = node_text(node, source);
    let inner = t
        .trim_start_matches(['\'', '"', '`'])
        .trim_end_matches(['\'', '"', '`']);
    // Template with ${} is dynamic — only accept pure string literals.
    if inner.contains("${") {
        return None;
    }
    if inner.is_empty()
        || !inner
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        return None;
    }
    Some(inner.to_string())
}

/// Nest DI token: identifier/member OR plain string literal
/// (`provide: 'AppService'`, `@Inject('AppService')`).
fn nest_token_name(node: Node, source: &str) -> Option<String> {
    if matches!(
        node.kind(),
        "string" | "string_fragment" | "template_string"
    ) {
        return string_literal_content(node, source);
    }
    ident_name(node, source)
}

fn ts_decorator_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    // Prefer the decorated class as enclosing for Nest `@Module` metadata.
    let class_enclosing = decorated_class_name(node, source).or_else(|| enclosing.clone());
    // @Inject(UserService) / @Injectable(UserService) / @Component({...})
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "call_expression" {
            // bare @Injectable — no target symbol
            continue;
        }
        let Some(fn_node) = child.child_by_field_name("function") else {
            continue;
        };
        let method = last_segment(node_text(fn_node, source)).to_string();
        // @Module({ providers, controllers, imports }) — real Nest registration.
        if method == "Module" {
            ts_nest_module_metadata(
                child,
                source,
                ctx,
                references,
                class_enclosing.clone().or_else(|| enclosing.clone()),
            );
            continue;
        }
        if !matches!(
            method.as_str(),
            "Inject" | "Injectable" | "Optional" | "forwardRef"
        ) {
            continue;
        }
        if let Some(arg) = first_interesting_arg(child, source) {
            if let Some(name) = nest_token_name(arg, source) {
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line: line_of(ctx, child),
                        enclosing: enclosing.clone(),
                        confidence: Confidence::Heuristic,
                        rule_id: "ts.di.decorator",
                        snippet: format!("@{method}({})", node_text(arg, source)),
                    },
                );
            }
        }
    }
}

/// Nest `@Module({ providers, controllers, imports })` registration edges.
/// Enclosing is the module class name when resolvable.
fn ts_nest_module_metadata(
    call: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    let line = line_of(ctx, call);
    let Some(obj) = first_object_arg(call, source) else {
        return;
    };
    let mut cursor = obj.walk();
    for pair in obj.children(&mut cursor) {
        if pair.kind() != "pair" {
            continue;
        }
        let Some(key) = pair.child_by_field_name("key") else {
            continue;
        };
        let Some(value) = pair.child_by_field_name("value") else {
            continue;
        };
        let key_t = node_text(key, source);
        let (array_kind, rule_id) = match key_t {
            "providers" => ("providers", "ts.nest.module_providers"),
            "controllers" => ("controllers", "ts.nest.module_controllers"),
            "imports" => ("imports", "ts.nest.module_imports"),
            "exports" => ("exports", "ts.nest.module_exports"),
            _ => continue,
        };
        if value.kind() != "array" {
            continue;
        }
        let snippet = format!(
            "{array_kind}: {}",
            node_text(value, source)
                .chars()
                .take(80)
                .collect::<String>()
                .replace('\n', " ")
        );
        ts_nest_array_targets(value, source, rule_id, |name, elem_line| {
            push_l1(
                references,
                L1Edge {
                    name,
                    qualifier: None,
                    line: elem_line.unwrap_or(line),
                    enclosing: enclosing.clone(),
                    confidence: Confidence::Heuristic,
                    rule_id,
                    snippet: snippet.clone(),
                },
            );
        });
    }
}

/// Collect registration target names from a Nest metadata array element list.
/// - bare identifier → the name
/// - `{ provide, useClass, useExisting, useFactory }` → each simple ident
/// - `X.forRoot(...)` → `X` (callee object)
/// - `forwardRef(() => M)` / `forwardRef(M)` → `M` (never the `forwardRef` helper)
fn ts_nest_array_targets(
    array: Node,
    source: &str,
    rule_id: &str,
    mut sink: impl FnMut(String, Option<usize>),
) {
    let mut cursor = array.walk();
    for elem in array.children(&mut cursor) {
        ts_nest_sink_elem(elem, source, rule_id, &mut sink);
    }
}

/// Recursively resolve one Nest metadata array element to registration target name(s).
fn ts_nest_sink_elem(
    elem: Node,
    source: &str,
    rule_id: &str,
    sink: &mut impl FnMut(String, Option<usize>),
) {
    let n = unwrap_parens(elem);
    match n.kind() {
        "identifier" | "type_identifier" | "shorthand_property_identifier" => {
            let t = node_text(n, source);
            if !t.is_empty() {
                sink(t.to_string(), None);
            }
        }
        // Bare string tokens: `exports: ['CONFIG']` / `providers: ['TOKEN']`.
        "string" | "string_fragment" | "template_string" => {
            if let Some(name) = nest_token_name(n, source) {
                sink(name, None);
            }
        }
        "member_expression" => {
            if let Some(name) = ident_name(n, source) {
                sink(name, None);
            }
        }
        "call_expression" | "new_expression" => {
            let fn_node = n
                .child_by_field_name("function")
                .or_else(|| n.child_by_field_name("constructor"));
            let fn_last = fn_node
                .map(|f| last_segment(node_text(f, source)).to_string())
                .unwrap_or_default();
            // Nest circular DI: forwardRef(() => M) — unwrap the real module.
            // Never emit an edge to the `forwardRef` helper itself.
            if fn_last == "forwardRef" {
                if let Some(args) = n.child_by_field_name("arguments") {
                    let mut ac = args.walk();
                    for a in args.children(&mut ac) {
                        if matches!(a.kind(), "(" | ")" | ",") {
                            continue;
                        }
                        ts_nest_sink_elem(a, source, rule_id, sink);
                        break;
                    }
                }
                return;
            }
            if let Some(fn_node) = fn_node {
                if n.kind() == "new_expression" {
                    // `new ConfigService()` / `new TYPES.ConfigService()` → ConfigService
                    // (the constructed type, not a namespace object).
                    if let Some(name) = ident_name(fn_node, source) {
                        sink(name, None);
                    }
                } else if fn_node.kind() == "member_expression" {
                    // imports: [ObserveModule.forRoot(...)] → ObserveModule
                    if let Some(obj) = fn_node.child_by_field_name("object") {
                        if let Some(name) = ident_name(obj, source) {
                            sink(name, None);
                        }
                    }
                } else if let Some(name) = ident_name(fn_node, source) {
                    sink(name, None);
                }
            }
        }
        "arrow_function" | "function_expression" => {
            // forwardRef(() => AuthModule) / forwardRef(() => { return AuthModule; })
            if let Some(body) = n.child_by_field_name("body") {
                ts_nest_sink_body(body, source, rule_id, sink);
            }
        }
        "object" => {
            let mut oc = n.walk();
            for pair in n.children(&mut oc) {
                if pair.kind() != "pair" {
                    continue;
                }
                let Some(key) = pair.child_by_field_name("key") else {
                    continue;
                };
                let Some(value) = pair.child_by_field_name("value") else {
                    continue;
                };
                let key_t = node_text(key, source);
                if !matches!(
                    key_t,
                    "provide" | "useClass" | "useExisting" | "useFactory" | "inject"
                ) {
                    continue;
                }
                // Only fire under the providers rule for provider-object keys.
                if rule_id != "ts.nest.module_providers" && key_t != "provide" {
                    // controllers/imports object forms are rare; still capture provide/useClass cheaply.
                    if !matches!(key_t, "provide" | "useClass") {
                        continue;
                    }
                }
                // `inject: [Dep, 'TOKEN']` — dependency list of a custom provider.
                if key_t == "inject" {
                    if value.kind() == "array" {
                        let mut ic = value.walk();
                        for item in value.children(&mut ic) {
                            ts_nest_sink_elem(item, source, rule_id, sink);
                        }
                    } else {
                        ts_nest_sink_elem(value, source, rule_id, sink);
                    }
                    continue;
                }
                // `useFactory: () => X` / `() => { return create(); }` — unwrap body.
                // Conservative: only identifiers and call/new targets, never bare
                // member property names (`config.default` is not a service).
                if key_t == "useFactory" {
                    let fv = unwrap_parens(value);
                    if matches!(fv.kind(), "arrow_function" | "function_expression") {
                        if let Some(body) = fv.child_by_field_name("body") {
                            ts_nest_factory_body(body, source, sink);
                        }
                        continue;
                    }
                }
                // Tokens may be idents or string literals (`provide: 'APP'`).
                if let Some(name) = nest_token_name(value, source) {
                    sink(name, None);
                }
            }
        }
        _ => {}
    }
}

/// Arrow/function body: expression, or a block whose first `return` holds the target.
fn ts_nest_sink_body(
    body: Node,
    source: &str,
    rule_id: &str,
    sink: &mut impl FnMut(String, Option<usize>),
) {
    let b = unwrap_parens(body);
    if b.kind() != "statement_block" {
        ts_nest_sink_elem(b, source, rule_id, sink);
        return;
    }
    let mut c = b.walk();
    for ch in b.children(&mut c) {
        if ch.kind() != "return_statement" {
            continue;
        }
        let mut rc = ch.walk();
        for rch in ch.children(&mut rc) {
            if matches!(rch.kind(), "return" | ";") {
                continue;
            }
            ts_nest_sink_elem(rch, source, rule_id, sink);
            return;
        }
    }
}

/// Factory body: only emit real type/call targets.
/// `() => ConfigService`, `() => new ConfigService()`, `() => create()`.
/// Skip bare member property names (`config.default` is not a registration).
fn ts_nest_factory_body(body: Node, source: &str, sink: &mut impl FnMut(String, Option<usize>)) {
    let b = unwrap_parens(body);
    if b.kind() == "statement_block" {
        let mut c = b.walk();
        for ch in b.children(&mut c) {
            if ch.kind() != "return_statement" {
                continue;
            }
            let mut rc = ch.walk();
            for rch in ch.children(&mut rc) {
                if matches!(rch.kind(), "return" | ";") {
                    continue;
                }
                ts_nest_factory_body(rch, source, sink);
                return;
            }
        }
        return;
    }
    match b.kind() {
        "identifier" | "type_identifier" => {
            let t = node_text(b, source);
            if !t.is_empty() {
                sink(t.to_string(), None);
            }
        }
        "new_expression" | "call_expression" => {
            let fn_node = b
                .child_by_field_name("function")
                .or_else(|| b.child_by_field_name("constructor"));
            if let Some(f) = fn_node {
                if b.kind() == "new_expression" {
                    // `new TYPES.ConfigService()` → ConfigService
                    if let Some(name) = ident_name(f, source) {
                        sink(name, None);
                    }
                } else if let Some(name) = ident_name(f, source) {
                    sink(name, None);
                }
            }
        }
        "arrow_function" | "function_expression" => {
            if let Some(inner) = b.child_by_field_name("body") {
                ts_nest_factory_body(inner, source, sink);
            }
        }
        _ => {}
    }
}

fn first_object_arg<'a>(node: Node<'a>, _source: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let args = node
        .children(&mut cursor)
        .find(|c| c.kind() == "arguments")?;
    let mut ac = args.walk();
    for a in args.children(&mut ac) {
        if a.kind() == "object" || a.kind() == "parenthesized_expression" {
            let n = unwrap_parens(a);
            if n.kind() == "object" {
                return Some(n);
            }
        }
    }
    None
}

/// Nest dynamic-module config object: `X.forRootAsync({ imports, inject, useFactory })`.
/// Emits Heuristic edges for DI arrays and factory bodies (same finite-domain
/// registration as `@Module` metadata).
fn ts_nest_for_root_config(
    call: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
    snippet_prefix: &str,
) {
    let Some(obj) = first_object_arg(call, source) else {
        return;
    };
    let line = line_of(ctx, call);
    let mut cursor = obj.walk();
    for pair in obj.children(&mut cursor) {
        if pair.kind() != "pair" {
            continue;
        }
        let Some(key) = pair.child_by_field_name("key") else {
            continue;
        };
        let Some(value) = pair.child_by_field_name("value") else {
            continue;
        };
        let key_t = node_text(key, source);
        let rule_id = match key_t {
            "imports" => "ts.nest.module_imports",
            "inject" | "useFactory" => "ts.nest.module_providers",
            _ => continue,
        };
        let snippet = format!(
            "{snippet_prefix} {key_t}: {}",
            node_text(value, source)
                .chars()
                .take(80)
                .collect::<String>()
                .replace('\n', " ")
        );
        if key_t == "useFactory" {
            let fv = unwrap_parens(value);
            if matches!(fv.kind(), "arrow_function" | "function_expression") {
                if let Some(body) = fv.child_by_field_name("body") {
                    let mut local_sink = |name: String, elem_line: Option<usize>| {
                        push_l1(
                            references,
                            L1Edge {
                                name,
                                qualifier: None,
                                line: elem_line.unwrap_or(line),
                                enclosing: enclosing.clone(),
                                confidence: Confidence::Heuristic,
                                rule_id,
                                snippet: snippet.clone(),
                            },
                        );
                    };
                    ts_nest_factory_body(body, source, &mut local_sink);
                }
            }
            continue;
        }
        if value.kind() == "array" {
            ts_nest_array_targets(value, source, rule_id, |name, elem_line| {
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line: elem_line.unwrap_or(line),
                        enclosing: enclosing.clone(),
                        confidence: Confidence::Heuristic,
                        rule_id,
                        snippet: snippet.clone(),
                    },
                );
            });
        } else {
            ts_nest_sink_elem(value, source, rule_id, &mut |name, elem_line| {
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line: elem_line.unwrap_or(line),
                        enclosing: enclosing.clone(),
                        confidence: Confidence::Heuristic,
                        rule_id,
                        snippet: snippet.clone(),
                    },
                );
            });
        }
    }
}

/// `constructor(private readonly svc: AppService)` → Heuristic ref to `AppService`
/// with enclosing = class name (Nest constructor injection).
fn ts_nest_ctor_inject(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    let Some(name_n) = node.child_by_field_name("name") else {
        return;
    };
    if node_text(name_n, source) != "constructor" {
        return;
    }
    let Some(class_name) = enclosing_class_name(node, source) else {
        return;
    };
    let Some(params) = node.child_by_field_name("parameters") else {
        return;
    };
    let line = line_of(ctx, node);
    let mut cursor = params.walk();
    for p in params.children(&mut cursor) {
        if !matches!(p.kind(), "required_parameter" | "optional_parameter") {
            continue;
        }
        let Some(ty) = ts_param_type_name(p, source) else {
            continue;
        };
        push_l1(
            references,
            L1Edge {
                name: ty.clone(),
                qualifier: None,
                line,
                enclosing: Some(class_name.clone()),
                confidence: Confidence::Heuristic,
                rule_id: "ts.nest.ctor_inject",
                snippet: format!("constructor(...: {ty})"),
            },
        );
    }
}

/// Type name of a formal parameter: bare type_identifier or `Foo<...>`.
fn ts_param_type_name(param: Node, source: &str) -> Option<String> {
    let mut cursor = param.walk();
    for part in param.children(&mut cursor) {
        if part.kind() != "type_annotation" {
            continue;
        }
        let mut ac = part.walk();
        for t in part.children(&mut ac) {
            let name = match t.kind() {
                "type_identifier" => node_text(t, source).to_string(),
                "generic_type" => {
                    let mut gc = t.walk();
                    let tid = t
                        .children(&mut gc)
                        .find(|c| c.kind() == "type_identifier")
                        .map(|id| node_text(id, source).to_string());
                    tid?
                }
                _ => continue,
            };
            if is_nest_di_type_name(&name) {
                return Some(name);
            }
        }
    }
    None
}

/// Skip primitives / any so ctor inject stays on DI types.
fn is_nest_di_type_name(name: &str) -> bool {
    !matches!(
        name,
        "string"
            | "number"
            | "boolean"
            | "any"
            | "unknown"
            | "void"
            | "never"
            | "null"
            | "undefined"
            | "object"
            | "symbol"
            | "bigint"
    )
}

// ── Python ──────────────────────────────────────────────────────────

fn walk_py(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    let mut cursor = node.walk();
    let mut local_enclosing = enclosing;

    if matches!(node.kind(), "function_definition" | "decorated_definition") {
        if let Some(n) = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source).to_string())
            .filter(|s| !s.is_empty())
        {
            local_enclosing = Some(n);
        }
    }

    match node.kind() {
        "call" => {
            py_call_rules(node, source, ctx, references, local_enclosing.clone());
        }
        "default_parameter" => {
            py_default_depends(node, source, ctx, references, local_enclosing.clone());
        }
        "decorator" => {
            py_inject_decorator(node, source, ctx, references, local_enclosing.clone());
        }
        "class_definition" => {
            py_init_subclass_rule(node, source, ctx, references);
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_py(child, source, ctx, references, local_enclosing.clone());
    }
}

fn py_call_callee_text(call: Node, source: &str) -> String {
    call.child_by_field_name("function")
        .map(|f| node_text(f, source).to_string())
        .unwrap_or_default()
}

/// Framework registry: base defines `__init_subclass__` → subclass is registered.
/// PLAN L1 Python: `__init_subclass__` / metaclass registry → Heuristic.
fn py_init_subclass_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    if !source.contains("__init_subclass__") {
        return;
    }
    // Only subclasses (class C(Base): ...) — not the base defining the hook.
    let mut has_base = false;
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "argument_list" {
            has_base = true;
        }
    }
    if !has_base {
        return;
    }
    let Some(name) = node
        .child_by_field_name("name")
        .map(|n| node_text(n, source).to_string())
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    // Skip if this class body itself defines __init_subclass__ (the registrar).
    let body = node_text(node, source);
    if body.contains("def __init_subclass__") {
        return;
    }
    push_l1(
        references,
        L1Edge {
            name,
            qualifier: None,
            line: line_of(ctx, node),
            enclosing: None,
            confidence: Confidence::Heuristic,
            rule_id: "py.framework.init_subclass",
            snippet: "class ... (base) with __init_subclass__ registry".to_string(),
        },
    );
}

fn py_call_rules(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    let callee = py_call_callee_text(node, source);
    let last = last_segment(&callee).to_string();
    let line = line_of(ctx, node);

    // getattr(obj, "name")
    if last == "getattr" {
        if let Some(s) = py_nth_arg_string(node, source, 1) {
            let snippet = format!("getattr(..., \"{s}\")");
            push_l1(
                references,
                L1Edge {
                    name: s,
                    qualifier: None,
                    line,
                    enclosing: enclosing.clone(),
                    confidence: Confidence::DynamicCandidate,
                    rule_id: "py.dynamic.getattr",
                    snippet,
                },
            );
        }
    }

    // importlib.import_module("pkg.mod")
    if last == "import_module" || callee.ends_with("import_module") {
        if let Some(s) = py_nth_arg_string(node, source, 0) {
            let snippet = format!("import_module(\"{s}\")");
            push_l1(
                references,
                L1Edge {
                    name: s,
                    qualifier: None,
                    line,
                    enclosing: enclosing.clone(),
                    confidence: Confidence::DynamicCandidate,
                    rule_id: "py.dynamic.import_module",
                    snippet,
                },
            );
        }
    }

    // Depends(get_user_service) / Security(get_current_user) as call arguments.
    if last == "Depends" || last == "Security" {
        if let Some(arg) = py_nth_arg(node, source, 0) {
            let t = node_text(arg, source);
            // Depends(SomeClass) or Depends(get_fn)
            if !t.is_empty()
                && t.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                let name = t.rsplit('.').next().unwrap_or(t).to_string();
                push_l1(
                    references,
                    L1Edge {
                        name,
                        qualifier: None,
                        line,
                        enclosing: enclosing.clone(),
                        confidence: Confidence::Heuristic,
                        rule_id: "py.di.depends",
                        snippet: format!("{last}({t})"),
                    },
                );
            }
        }
    }

    // importlib.metadata.entry_points(group="...") / pkg_resources.iter_entry_points("...")
    // Finite group string at the call site; plugins themselves are NOT enumerated
    // here — Heuristic candidate registration domain, not sound-eligible.
    if last == "entry_points" || last == "iter_entry_points" || last == "load_entry_point" {
        if let Some(group) = py_entry_point_group(node, source) {
            push_l1(
                references,
                L1Edge {
                    name: group.clone(),
                    qualifier: None,
                    line,
                    enclosing: enclosing.clone(),
                    confidence: Confidence::Heuristic,
                    rule_id: "py.di.entry_points",
                    snippet: format!("{last}(... group/domain={group})"),
                },
            );
        }
    }
}

/// Group / entry-point domain string from `entry_points(group="g")` or
/// `iter_entry_points("g")` / `load_entry_point("g", ...)`.
fn py_entry_point_group(call: Node, source: &str) -> Option<String> {
    // keyword argument group=...
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let kids: Vec<Node> = args.children(&mut cursor).collect();
    for (i, k) in kids.iter().enumerate() {
        if k.kind() == "keyword_argument" || node_text(*k, source).starts_with("group") {
            let t = node_text(*k, source);
            if let Some(eq) = t.find('=') {
                let rhs = t[eq + 1..].trim().trim_matches(['\'', '"']);
                if !rhs.is_empty()
                    && rhs
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                {
                    return Some(rhs.to_string());
                }
            }
        }
        // `group = "x"` may be separate tokens
        if node_text(*k, source) == "group" {
            if let Some(next) = kids.get(i + 2) {
                if let Some(s) = string_literal_content(*next, source) {
                    return Some(s);
                }
            }
            if let Some(next) = kids.get(i + 1) {
                let nt = node_text(*next, source);
                let rhs = nt.trim_start_matches('=').trim().trim_matches(['\'', '"']);
                if !rhs.is_empty()
                    && rhs
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                {
                    return Some(rhs.to_string());
                }
            }
        }
    }
    // Positional first string arg (iter_entry_points("g")).
    py_nth_arg_string(call, source, 0)
}

fn py_default_depends(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    // Walk into RHS call if it's Depends(...)
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "call" {
            py_call_rules(c, source, ctx, references, enclosing.clone());
        }
    }
}

fn py_inject_decorator(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "call" {
            let callee = py_call_callee_text(c, source);
            let last = last_segment(&callee);
            if last == "inject" || last == "Inject" {
                if let Some(arg) = py_nth_arg(c, source, 0) {
                    let t = node_text(arg, source);
                    if !t.is_empty() {
                        push_l1(
                            references,
                            L1Edge {
                                name: t.to_string(),
                                qualifier: None,
                                line: line_of(ctx, c),
                                enclosing: enclosing.clone(),
                                confidence: Confidence::Heuristic,
                                rule_id: "py.di.inject",
                                snippet: format!("@inject({t})"),
                            },
                        );
                    }
                }
            }
        }
    }
}

fn py_nth_arg<'a>(call: Node<'a>, _source: &str, idx: usize) -> Option<Node<'a>> {
    let args = call.child_by_field_name("arguments")?;
    let mut ac = args.walk();
    let named: Vec<Node> = args
        .children(&mut ac)
        .filter(|c| !matches!(c.kind(), "," | "(" | ")"))
        .collect();
    named.get(idx).copied()
}

fn py_nth_arg_string(call: Node, source: &str, idx: usize) -> Option<String> {
    let arg = py_nth_arg(call, source, idx)?;
    string_literal_content(arg, source).or_else(|| {
        // python string node contains string_fragment
        let mut cursor = arg.walk();
        for c in arg.children(&mut cursor) {
            if c.kind() == "string_content" || c.kind() == "string_fragment" {
                let t = node_text(c, source);
                if !t.is_empty()
                    && t.chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
                {
                    return Some(t.to_string());
                }
            }
        }
        None
    })
}

// ── Go ──────────────────────────────────────────────────────────────

/// Same-file Go interface / method-set index for M3-B systematization.
/// Only **already-indexed** (this AST) types/methods — never invent implementors.
#[derive(Debug, Default, Clone)]
struct GoIfaceIndex {
    /// interface name → method names
    ifaces: std::collections::HashMap<String, Vec<String>>,
    /// concrete type → method names (from method_declaration)
    method_sets: std::collections::HashMap<String, Vec<String>>,
    /// explicit assertions `var _ I = (*T)(nil)` → (I, T)
    asserts: Vec<(String, String)>,
}

fn collect_go_iface_index(root: Node, source: &str) -> GoIfaceIndex {
    let mut idx = GoIfaceIndex::default();
    walk_go_collect(root, source, &mut idx);
    idx
}

fn walk_go_collect(node: Node, source: &str, idx: &mut GoIfaceIndex) {
    let mut cursor = node.walk();
    match node.kind() {
        "type_declaration" | "type_spec" => {
            go_collect_interface(node, source, idx);
        }
        "method_declaration" => {
            if let Some((recv, method)) = go_method_recv_name(node, source) {
                idx.method_sets.entry(recv).or_default().push(method);
            }
        }
        "var_declaration" | "var_spec" | "short_var_declaration" => {
            if let Some((iface, ty)) = go_parse_interface_assertion(node_text(node, source)) {
                idx.asserts.push((iface, ty));
            }
        }
        _ => {}
    }
    for child in node.children(&mut cursor) {
        walk_go_collect(child, source, idx);
    }
}

fn go_collect_interface(node: Node, source: &str, idx: &mut GoIfaceIndex) {
    // type Spec: name + interface_type
    let mut name = None;
    let mut iface_body = None;
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        if ch.kind() == "type_identifier" && name.is_none() {
            name = Some(node_text(ch, source).to_string());
        }
        if ch.kind() == "interface_type" {
            iface_body = Some(ch);
        }
        if ch.kind() == "type_spec" {
            if let Some(n) = ch.child_by_field_name("name") {
                name = Some(node_text(n, source).to_string());
            }
            if let Some(t) = ch.child_by_field_name("type") {
                if t.kind() == "interface_type" {
                    iface_body = Some(t);
                }
            }
        }
    }
    let (Some(name), Some(body)) = (name, iface_body) else {
        return;
    };
    let mut methods = Vec::new();
    let mut bc = body.walk();
    for m in body.children(&mut bc) {
        // method_elem / field_declaration with field_identifier name
        let mut mc = m.walk();
        for f in m.children(&mut mc) {
            if f.kind() == "field_identifier" || f.kind() == "method_elem" {
                let t = node_text(f, source);
                if !t.is_empty()
                    && t.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !methods.contains(&t.to_string())
                {
                    methods.push(t.to_string());
                }
            }
            if f.kind() == "field_declaration" || f.kind() == "method_elem" {
                if let Some(n) = f.child_by_field_name("name") {
                    let t = node_text(n, source);
                    if !t.is_empty() && !methods.contains(&t.to_string()) {
                        methods.push(t.to_string());
                    }
                }
                let mut fc = f.walk();
                for ff in f.children(&mut fc) {
                    if ff.kind() == "field_identifier" {
                        let t = node_text(ff, source);
                        if !t.is_empty() && !methods.contains(&t.to_string()) {
                            methods.push(t.to_string());
                        }
                    }
                }
            }
        }
    }
    // Fallback: regex-free scan of interface body text for `Name(` patterns.
    if methods.is_empty() {
        let body_t = node_text(body, source);
        for line in body_t.lines() {
            let t = line.trim();
            if let Some(paren) = t.find('(') {
                let cand = t[..paren].trim();
                if !cand.is_empty()
                    && cand.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && cand
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false)
                    && !methods.contains(&cand.to_string())
                {
                    methods.push(cand.to_string());
                }
            }
        }
    }
    if !methods.is_empty() {
        idx.ifaces.insert(name, methods);
    }
}

/// `func (s *Server) ServeHTTP(...)` → (Server, ServeHTTP)
fn go_method_recv_name(node: Node, source: &str) -> Option<(String, String)> {
    let mut name = None;
    let mut recv_ty = None;
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        if ch.kind() == "field_identifier" && name.is_none() {
            name = Some(node_text(ch, source).to_string());
        }
        if ch.kind() == "parameter_list" && recv_ty.is_none() {
            let t = node_text(ch, source);
            let cleaned = t.trim_start_matches('(').trim_end_matches(')');
            if let Some(last) = cleaned.split([' ', '*', '(', ')']).rfind(|s| {
                !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_') && *s != "func"
            }) {
                recv_ty = Some(last.to_string());
            }
        }
    }
    match (recv_ty, name) {
        (Some(r), Some(n)) if !r.is_empty() && !n.is_empty() => Some((r, n)),
        _ => None,
    }
}

/// `var _ Store = (*MemStore)(nil)` / `var _ Store = MemStore{}` → (Store, MemStore)
fn go_parse_interface_assertion(text: &str) -> Option<(String, String)> {
    let t = text.trim();
    if !t.contains("var _") && !t.starts_with("_") {
        // var_spec alone may omit `var`
        if !t.contains("= (*") && !t.contains("=(*") && !t.contains(" = ") {
            return None;
        }
        if !t.contains('_') {
            return None;
        }
    }
    // Interface name: token after `var _` or first identifier before `=`
    let iface = if let Some(rest) = t.split("var _").nth(1) {
        rest.split('=')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        t.split('=')
            .next()
            .unwrap_or("")
            .replace("var", "")
            .replace('_', "")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
    };
    // Concrete type after (* or =
    let ty = if let Some(idx) = t.find("(*") {
        t[idx + 2..]
            .split(')')
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        let idx = t.find(" = ")?;
        t[idx + 3..]
            .trim()
            .trim_end_matches("{}")
            .trim()
            .split(['(', '{'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let ok = |s: &String| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    };
    if ok(&iface) && ok(&ty) && iface != ty {
        Some((iface, ty))
    } else if ok(&ty) {
        Some((String::new(), ty))
    } else {
        None
    }
}

fn walk_go(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    go_idx: &GoIfaceIndex,
) {
    let mut cursor = node.walk();

    // Only consider keyed elements inside a map composite literal whose type
    // looks like a handler/func map — never struct literals or config maps.
    if node.kind() == "composite_literal" {
        go_composite_literal(node, source, ctx, references);
    }
    if node.kind() == "call_expression" {
        go_route_register_rule(node, source, ctx, references);
    }
    if node.kind() == "method_declaration" {
        go_method_impl_rule(node, source, ctx, references, go_idx);
    }
    if matches!(
        node.kind(),
        "var_declaration" | "short_var_declaration" | "var_spec"
    ) {
        go_interface_assertion_rule(node, source, ctx, references, go_idx);
    }

    for child in node.children(&mut cursor) {
        walk_go(child, source, ctx, references, go_idx);
    }
}

/// `func (s *Server) ServeHTTP(...)` — concrete method often implements an
/// interface with the same name (PLAN: 接口方法 + 显式实现集).
/// M3-B also emits `go.di.interface_impl_v2` when the method-set matches a
/// known interface (assertion or complete name-set) in this file.
fn go_method_impl_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    go_idx: &GoIfaceIndex,
) {
    let Some((recv_ty, method)) = go_method_recv_name(node, source) else {
        return;
    };
    if method.is_empty() {
        return;
    }
    let snippet = format!("func ({recv_ty}) {method}");
    let line = line_of(ctx, node);
    push_l1(
        references,
        L1Edge {
            name: method.clone(),
            qualifier: Some(recv_ty.clone()),
            line,
            enclosing: None,
            confidence: Confidence::Heuristic,
            rule_id: "go.di.interface_impl",
            snippet: snippet.clone(),
        },
    );
    go_emit_iface_impl_v2(go_idx, &recv_ty, &method, line, references, &snippet);
}

/// Emit M3-B v2 edges for (type, method) when type implements a known interface
/// via assertion or complete method-set name match (indexed methods only).
fn go_emit_iface_impl_v2(
    go_idx: &GoIfaceIndex,
    recv_ty: &str,
    method: &str,
    line: usize,
    references: &mut Vec<ExtractedRef>,
    base_snippet: &str,
) {
    let type_methods = go_idx.method_sets.get(recv_ty);
    for (iface, imethods) in &go_idx.ifaces {
        if imethods.is_empty() || !imethods.iter().any(|m| m == method) {
            continue;
        }
        let asserted = go_idx
            .asserts
            .iter()
            .any(|(i, t)| i == iface && t == recv_ty);
        let set_match = type_methods
            .map(|tms| !imethods.is_empty() && imethods.iter().all(|m| tms.iter().any(|x| x == m)));
        let method_set_ok = set_match.unwrap_or(false);
        if !asserted && !method_set_ok {
            continue;
        }
        let evidence = if asserted {
            format!("var _ {iface} = (*{recv_ty})(nil) → {method}")
        } else {
            format!("method-set {recv_ty} implements {iface} → {method}")
        };
        push_l1(
            references,
            L1Edge {
                name: method.to_string(),
                qualifier: Some(recv_ty.to_string()),
                line,
                enclosing: Some(iface.clone()),
                confidence: Confidence::Heuristic,
                rule_id: "go.di.interface_impl_v2",
                snippet: if evidence.contains(base_snippet) {
                    evidence
                } else {
                    format!("{evidence} | {base_snippet}")
                },
            },
        );
    }
}

/// `var _ Store = (*MemStore)(nil)` — compile-time interface implementation proof.
fn go_interface_assertion_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    go_idx: &GoIfaceIndex,
) {
    let t = node_text(node, source);
    let Some((iface, ty)) = go_parse_interface_assertion(t) else {
        return;
    };
    let line = line_of(ctx, node);
    // Edge to the type (registration/impl proof site).
    push_l1(
        references,
        L1Edge {
            name: ty.clone(),
            qualifier: if iface.is_empty() {
                None
            } else {
                Some(iface.clone())
            },
            line,
            enclosing: None,
            confidence: Confidence::Heuristic,
            rule_id: "go.di.interface_assert",
            snippet: if iface.is_empty() {
                format!("var _ Iface = (*{ty})(nil)")
            } else {
                format!("var _ {iface} = (*{ty})(nil)")
            },
        },
    );
    // M3-B: link each interface method the type provides (indexed methods only).
    if let Some(imethods) = go_idx.ifaces.get(&iface) {
        let type_methods = go_idx.method_sets.get(&ty);
        for m in imethods {
            let provided = type_methods
                .map(|tms| tms.iter().any(|x| x == m))
                .unwrap_or(false);
            if !provided {
                continue;
            }
            push_l1(
                references,
                L1Edge {
                    name: m.clone(),
                    qualifier: Some(ty.clone()),
                    line,
                    enclosing: Some(iface.clone()),
                    confidence: Confidence::Heuristic,
                    rule_id: "go.di.interface_impl_v2",
                    snippet: format!("var _ {iface} = (*{ty})(nil) → {m}"),
                },
            );
        }
    }
}

/// gin/chi style: e.GET("/users", GetUsers) / mux.HandleFunc(path, h)
fn go_route_register_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    let Some(fn_node) = node.child_by_field_name("function") else {
        return;
    };
    let method = last_segment(node_text(fn_node, source)).to_string();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "Handle" | "HandleFunc" | "Any"
    ) {
        return;
    }
    let mut cursor = node.walk();
    let Some(args) = node
        .children(&mut cursor)
        .find(|c| c.kind() == "argument_list")
    else {
        return;
    };
    let mut ac = args.walk();
    let named: Vec<Node> = args
        .children(&mut ac)
        .filter(|c| !matches!(c.kind(), "," | "(" | ")"))
        .collect();
    let Some(handler_node) = named.get(1) else {
        return;
    };
    let Some(handler) =
        go_handler_name(*handler_node, source).or_else(|| ident_name(*handler_node, source))
    else {
        return;
    };
    push_l1(
        references,
        L1Edge {
            name: handler,
            qualifier: None,
            line: line_of(ctx, node),
            enclosing: None,
            confidence: Confidence::Heuristic,
            rule_id: "go.di.route_register",
            snippet: format!("{method}(..., handler)"),
        },
    );
}

fn go_composite_literal(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    let mut cursor = node.walk();
    let type_node = node.children(&mut cursor).find(|c| c.kind() == "map_type");
    let Some(type_node) = type_node else {
        return;
    };
    let type_text = node_text(type_node, source);
    // Require a function-ish value type: map[string]HandlerFunc, func(...), http.HandlerFunc, etc.
    let looks_like_handler = type_text.contains("Handler")
        || type_text.contains("func(")
        || type_text.contains("HandleFunc")
        || type_text.contains("http.HandlerFunc");
    if !looks_like_handler {
        return;
    }
    let mut lc = node.walk();
    for c in node.children(&mut lc) {
        if c.kind() == "literal_value" {
            let mut vc = c.walk();
            for entry in c.children(&mut vc) {
                if entry.kind() == "keyed_element" {
                    go_map_entry(entry, source, ctx, references);
                }
            }
        }
    }
}

fn go_map_entry(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    // keyed_element: "path": HandlerFunc
    if node.kind() != "keyed_element" {
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let value = children
        .iter()
        .rev()
        .find(|c| {
            matches!(
                c.kind(),
                "identifier" | "literal_element" | "selector_expression" | "func_literal"
            )
        })
        .copied();
    if let Some(val) = value {
        if let Some(name) = go_handler_name(val, source) {
            let snippet = format!("map entry → {name}");
            push_l1(
                references,
                L1Edge {
                    name,
                    qualifier: None,
                    line: line_of(ctx, val),
                    enclosing: None,
                    confidence: Confidence::Heuristic,
                    rule_id: "go.di.handler_map",
                    snippet,
                },
            );
        }
    }
}

fn go_handler_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => {
            let t = node_text(node, source);
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        "func_literal" | "function_literal" => {
            let mut names = Vec::new();
            collect_call_names(node, source, &mut names);
            names.into_iter().next()
        }
        "literal_element" => {
            let mut cursor = node.walk();
            for c in node.children(&mut cursor) {
                if let Some(n) = go_handler_name(c, source) {
                    return Some(n);
                }
            }
            None
        }
        "selector_expression" => {
            // pkg.Handler → Handler
            node.child_by_field_name("field")
                .map(|f| node_text(f, source).to_string())
                .or_else(|| {
                    let t = node_text(node, source);
                    t.rsplit('.').next().map(|s| s.to_string())
                })
        }
        _ => None,
    }
}

// ── Rust ────────────────────────────────────────────────────────────

/// Same-file `dyn Trait` / `impl Trait for Type` index for M3-A.
/// Only **already-indexed** implementors in this AST — never invent types.
#[derive(Debug, Default, Clone)]
struct RustDynIndex {
    /// trait → (impl type → methods)
    impls: std::collections::HashMap<String, Vec<(String, Vec<String>)>>,
    /// traits that appear as `dyn Trait` in this file
    dyn_traits: std::collections::HashSet<String>,
}

fn collect_rust_dyn_index(root: Node, source: &str) -> RustDynIndex {
    let mut idx = RustDynIndex::default();
    walk_rust_dyn_collect(root, source, &mut idx);
    idx
}

fn walk_rust_dyn_collect(node: Node, source: &str, idx: &mut RustDynIndex) {
    let mut cursor = node.walk();
    match node.kind() {
        "impl_item" => {
            if let Some((tr, ty, methods)) = rust_impl_trait_methods(node, source) {
                idx.impls.entry(tr).or_default().push((ty, methods));
            }
        }
        "abstract_type" | "dyn_type" | "bounded_type" => {
            let t = node_text(node, source);
            for tr in extract_dyn_trait_names(t) {
                idx.dyn_traits.insert(tr);
            }
            // Also scan children for type_identifier under dyn
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if ch.kind() == "type_identifier" {
                    let n = node_text(ch, source);
                    let parent_t = node_text(node, source);
                    if parent_t.contains("dyn") || node.kind() == "abstract_type" {
                        idx.dyn_traits.insert(n.to_string());
                    }
                }
            }
        }
        _ => {
            // Any type text containing `dyn Trait`
            if matches!(
                node.kind(),
                "parameter"
                    | "let_declaration"
                    | "reference_type"
                    | "generic_type"
                    | "scoped_type_identifier"
                    | "type_identifier"
            ) {
                let t = node_text(node, source);
                if t.contains("dyn ") {
                    for tr in extract_dyn_trait_names(t) {
                        idx.dyn_traits.insert(tr);
                    }
                }
            }
        }
    }
    for child in node.children(&mut cursor) {
        walk_rust_dyn_collect(child, source, idx);
    }
}

fn extract_dyn_trait_names(type_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = type_text;
    while let Some(pos) = rest.find("dyn ") {
        let after = &rest[pos + 4..];
        let ident: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
            .collect();
        let last = ident.rsplit("::").next().unwrap_or("").to_string();
        if !last.is_empty() {
            out.push(last);
        }
        rest = after;
    }
    out
}

/// `impl Trait for Type { fn m... }` → (Trait, Type, [methods])
fn rust_impl_trait_methods(node: Node, source: &str) -> Option<(String, String, Vec<String>)> {
    let mut trait_name = None;
    let mut type_name = None;
    if let Some(t) = node.child_by_field_name("trait") {
        trait_name = Some(node_text(t, source).to_string());
    }
    if let Some(t) = node.child_by_field_name("type") {
        type_name = Some(node_text(t, source).to_string());
    }
    if trait_name.is_none() || type_name.is_none() {
        let mut cursor = node.walk();
        for c in node.children(&mut cursor) {
            match c.kind() {
                "type_identifier" | "generic_type" => {
                    if trait_name.is_none() {
                        trait_name = Some(node_text(c, source).to_string());
                    } else if type_name.is_none() {
                        type_name = Some(node_text(c, source).to_string());
                    }
                }
                _ => {}
            }
        }
    }
    let (Some(tr), Some(ty)) = (trait_name, type_name) else {
        return None;
    };
    let tr = tr.split('<').next().unwrap_or(&tr).trim().to_string();
    let ty = ty.split('<').next().unwrap_or(&ty).trim().to_string();
    if tr.is_empty() || ty.is_empty() || tr == ty {
        return None;
    }
    let impl_text = node_text(node, source);
    let has_for =
        node.children(&mut node.walk()).any(|c| c.kind() == "for") || impl_text.contains(" for ");
    if !has_for {
        return None;
    }
    let mut methods = Vec::new();
    let mut mc = node.walk();
    for c in node.children(&mut mc) {
        if c.kind() == "declaration_list" {
            let mut dc = c.walk();
            for m in c.children(&mut dc) {
                if m.kind() == "function_item" {
                    if let Some(name) = m
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source).to_string())
                    {
                        methods.push(name);
                    }
                }
            }
        }
    }
    Some((tr, ty, methods))
}

fn walk_rust(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    inv_submit_aliases: &std::collections::HashSet<String>,
    dyn_idx: &RustDynIndex,
    enclosing: Option<String>,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let mut local_enclosing = enclosing;

    if matches!(kind, "function_item" | "function_signature_item") {
        if let Some(n) = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source).to_string())
            .filter(|s| !s.is_empty())
        {
            local_enclosing = Some(n);
        }
    }

    if node.kind() == "impl_item" {
        rust_impl_trait_rule(node, source, ctx, references);
    }
    if node.kind() == "macro_invocation" {
        rust_inventory_submit_rule(node, source, ctx, references, inv_submit_aliases);
    }
    if node.kind() == "attribute_item" || node.kind() == "attribute" {
        rust_linkme_distributed_slice_rule(node, source, ctx, references);
    }
    if node.kind() == "call_expression" {
        rust_dyn_trait_method_rule(
            node,
            source,
            ctx,
            references,
            dyn_idx,
            local_enclosing.clone(),
        );
    }

    for child in node.children(&mut cursor) {
        walk_rust(
            child,
            source,
            ctx,
            references,
            inv_submit_aliases,
            dyn_idx,
            local_enclosing.clone(),
        );
    }
}

/// M3-A: method call on a `dyn Trait` receiver → candidate implementors
/// already present as `impl Trait for Type` in this file. Heuristic only;
/// **not** sound-eligible (open dispatch / not a finite registration site).
fn rust_dyn_trait_method_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    dyn_idx: &RustDynIndex,
    enclosing: Option<String>,
) {
    if dyn_idx.dyn_traits.is_empty() || dyn_idx.impls.is_empty() {
        return;
    }
    let Some(fn_node) = node.child_by_field_name("function") else {
        return;
    };
    // Only method-call shape: recv.method()
    let Some(field) = fn_node.child_by_field_name("field") else {
        return;
    };
    let method = node_text(field, source).to_string();
    if method.is_empty() {
        return;
    }
    let line = line_of(ctx, node);
    let call_snip: String = node_text(node, source)
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();

    for trait_name in &dyn_idx.dyn_traits {
        let Some(impls) = dyn_idx.impls.get(trait_name) else {
            continue;
        };
        for (ty, methods) in impls {
            if !methods.iter().any(|m| m == &method) {
                continue;
            }
            // Never invent implementors — only types from this file's impls.
            push_l1(
                references,
                L1Edge {
                    name: method.clone(),
                    qualifier: Some(ty.clone()),
                    line,
                    enclosing: enclosing.clone(),
                    confidence: Confidence::Heuristic,
                    rule_id: "rs.di.dyn_trait_method",
                    snippet: format!("dyn {trait_name}::{method} → {ty}::{method} | {call_snip}"),
                },
            );
        }
    }
}

/// M3-E: `#[distributed_slice(...)]` / `#[linkme::distributed_slice(...)]`
/// source registration. Finite identifiers written at the attribute/static
/// site (same class as inventory::submit!). Not a sidecar rule.
fn rust_linkme_distributed_slice_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    let t = node_text(node, source);
    if !t.contains("distributed_slice") {
        return;
    }
    // Must be linkme / distributed_slice attribute path, not a random string.
    let is_linkme = t.contains("linkme::distributed_slice")
        || t.contains("#[distributed_slice")
        || t.contains("#[distributed_slice(")
        || (t.contains("distributed_slice") && t.contains('#'));
    if !is_linkme {
        return;
    }
    // Reject unrelated crates that merely mention the word without attribute shape.
    if !t.trim_start().starts_with('#') && !t.contains("linkme::") {
        return;
    }

    let line = line_of(ctx, node);
    let snippet: String = t.lines().next().unwrap_or("").chars().take(80).collect();

    // Registration slice name in arguments: distributed_slice(SLICE_NAME)
    if let Some(slice) = linkme_slice_name(t) {
        push_l1(
            references,
            L1Edge {
                name: slice,
                qualifier: None,
                line,
                enclosing: None,
                confidence: Confidence::Heuristic,
                rule_id: "rs.di.linkme_distributed_slice",
                snippet: snippet.clone(),
            },
        );
    }

    // Look at following sibling static_item for type / initializer identifiers.
    let mut sib = node.next_sibling();
    while let Some(s) = sib {
        if matches!(
            s.kind(),
            "static_item" | "function_item" | "attribute_item" | "attribute"
        ) {
            if s.kind() == "attribute_item" || s.kind() == "attribute" {
                sib = s.next_sibling();
                continue;
            }
            let s_text = node_text(s, source);
            for ty in rust_type_idents_from_static(s_text) {
                push_l1(
                    references,
                    L1Edge {
                        name: ty,
                        qualifier: None,
                        line,
                        enclosing: None,
                        confidence: Confidence::Heuristic,
                        rule_id: "rs.di.linkme_distributed_slice",
                        snippet: format!("{snippet} | {}", s_text.lines().next().unwrap_or("")),
                    },
                );
            }
            break;
        }
        // static may be nested under the attribute_item parent walk — also scan
        // the parent's remaining children when attribute_item wraps things.
        break;
    }

    // Fallback: scan nearby text after the attribute for `static X: Ty = ...`
    if let Some(parent) = node.parent() {
        let p_text = node_text(parent, source);
        if p_text.contains("static") && p_text.contains("distributed_slice") {
            for ty in rust_type_idents_from_static(p_text) {
                if !references.iter().any(|r| {
                    r.name == ty
                        && r.evidence
                            .as_ref()
                            .map(|e| e.rule_id == "rs.di.linkme_distributed_slice")
                            .unwrap_or(false)
                }) {
                    push_l1(
                        references,
                        L1Edge {
                            name: ty,
                            qualifier: None,
                            line,
                            enclosing: None,
                            confidence: Confidence::Heuristic,
                            rule_id: "rs.di.linkme_distributed_slice",
                            snippet: snippet.clone(),
                        },
                    );
                }
            }
        }
    }
}

fn linkme_slice_name(attr_text: &str) -> Option<String> {
    // #[distributed_slice(STRATEGIES)] or #[linkme::distributed_slice(PLUGINS)]
    let start = attr_text.find("distributed_slice(")? + "distributed_slice(".len();
    let rest = &attr_text[start..];
    let end = rest.find(')')?;
    let inner = rest[..end].trim();
    // May be `NAME` or `NAME = ...`
    let name = inner
        .split(['=', ':'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches("::")
        .rsplit("::")
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(name)
}

/// Type / ctor identifiers from a `static NAME: Ty = Ty { ... Type::new ... }` body.
fn rust_type_idents_from_static(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    // After `static Name:`
    if let Some(idx) = text.find("static") {
        let after = &text[idx + 6..];
        if let Some(colon) = after.find(':') {
            let ty_part = after[colon + 1..].split('=').next().unwrap_or("").trim();
            let ty = ty_part
                .split('<')
                .next()
                .unwrap_or(ty_part)
                .trim()
                .trim_start_matches('&')
                .trim();
            if is_type_ident(ty) && ty != "fn" && !out.contains(&ty.to_string()) {
                out.push(ty.to_string());
            }
        }
    }
    // `Type::new` / `Type {` patterns in the body
    let mut chars = text.char_indices().peekable();
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    for i in 0..bytes.len() {
        if bytes[i].1 == ':' && i + 1 < bytes.len() && bytes[i + 1].1 == ':' {
            // walk back for Type
            let mut start = i;
            while start > 0 {
                let c = bytes[start - 1].1;
                if c.is_alphanumeric() || c == '_' {
                    start -= 1;
                } else {
                    break;
                }
            }
            let ty: String = bytes[start..i].iter().map(|(_, c)| *c).collect();
            if is_type_ident(&ty) && ty != "Box" && ty != "Self" && !out.contains(&ty) {
                // forward: ::new or ::default
                let fwd: String = bytes[i + 2..].iter().take(12).map(|(_, c)| *c).collect();
                if fwd.starts_with("new") || fwd.starts_with("default") || fwd.starts_with("from") {
                    out.push(ty);
                }
            }
        }
        if bytes[i].1 == '{' {
            // identifier before `{` may be struct literal type
            let mut start = i;
            while start > 0 && bytes[start - 1].1 == ' ' {
                start -= 1;
            }
            let mut s2 = start;
            while s2 > 0 {
                let c = bytes[s2 - 1].1;
                if c.is_alphanumeric() || c == '_' {
                    s2 -= 1;
                } else {
                    break;
                }
            }
            let ty: String = bytes[s2..start].iter().map(|(_, c)| *c).collect();
            if is_type_ident(&ty) && ty != "Self" && !out.contains(&ty) {
                out.push(ty);
            }
        }
    }
    let _ = chars.next();
    out
}

/// Local identifiers that refer to `inventory::submit` via `use` (including
/// `use inventory::submit as alias` and `use inventory::{submit}`). Bare
/// `submit!` is only treated as inventory when the file imports that name.
fn collect_inventory_submit_aliases(root: Node, source: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    walk_rust_use_decls(root, source, &mut out);
    out
}

fn walk_rust_use_decls(node: Node, source: &str, out: &mut std::collections::HashSet<String>) {
    if node.kind() == "use_declaration" {
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            collect_from_use_item(ch, source, Vec::new(), out);
        }
    }
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        walk_rust_use_decls(ch, source, out);
    }
}

/// Flatten `scoped_identifier` / path segments into `["crate","inventory","submit"]`.
fn path_segments_from_scoped(node: Node, source: &str) -> Option<Vec<String>> {
    match node.kind() {
        "identifier" | "crate" | "self" | "super" => {
            Some(vec![node_text(node, source).to_string()])
        }
        "scoped_identifier" => {
            let mut segs = Vec::new();
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if matches!(
                    ch.kind(),
                    "scoped_identifier" | "identifier" | "crate" | "self" | "super"
                ) {
                    segs.extend(path_segments_from_scoped(ch, source)?);
                }
            }
            if segs.is_empty() {
                None
            } else {
                Some(segs)
            }
        }
        _ => None,
    }
}

/// Join a use-list prefix with a child path segment list.
///
/// Items inside `use PREFIX::{…}` are **suffixes** relative to `PREFIX`.
/// Ignoring the prefix made `use evil::{inventory::submit}` look like the
/// inventory crate's `submit` (sound-allowlist fail-open).
fn join_use_prefix(prefix: &[String], segs: Vec<String>) -> Vec<String> {
    if prefix.is_empty() {
        return segs;
    }
    let mut full = prefix.to_vec();
    full.extend(segs);
    full
}

/// Path segments of a `use …::*` node (the path before `::*`).
fn wildcard_path_segments(node: Node, source: &str) -> Vec<String> {
    let mut segs: Vec<String> = Vec::new();
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        match ch.kind() {
            "scoped_identifier" => {
                if let Some(s) = path_segments_from_scoped(ch, source) {
                    segs = s;
                }
            }
            "identifier" | "crate" | "self" | "super" => {
                // Prefer a full scoped path when present; otherwise accumulate.
                if segs.is_empty() {
                    if let Some(s) = path_segments_from_scoped(ch, source) {
                        segs = s;
                    } else {
                        segs.push(node_text(ch, source).to_string());
                    }
                } else if let Some(s) = path_segments_from_scoped(ch, source) {
                    segs.extend(s);
                } else {
                    segs.push(node_text(ch, source).to_string());
                }
            }
            _ => {}
        }
    }
    segs
}

fn collect_from_use_item(
    node: Node,
    source: &str,
    prefix: Vec<String>,
    out: &mut std::collections::HashSet<String>,
) {
    match node.kind() {
        "scoped_identifier" => {
            // Prefix must stay attached inside use-lists: `use evil::{inventory::submit}`
            // is NOT the inventory crate.
            let segs = match path_segments_from_scoped(node, source) {
                Some(child) => join_use_prefix(&prefix, child),
                None => prefix,
            };
            if is_inventory_submit_full_path(&segs) {
                if let Some(local) = segs.last() {
                    out.insert(local.clone());
                }
            }
        }
        "use_as_clause" => {
            // `use inventory::submit as inv_submit` → scoped_identifier + alias ident.
            let mut segs: Vec<String> = prefix.clone();
            let mut alias: Option<String> = None;
            let mut seen_as = false;
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if ch.kind() == "as" {
                    seen_as = true;
                    continue;
                }
                if seen_as && ch.kind() == "identifier" {
                    alias = Some(node_text(ch, source).to_string());
                    continue;
                }
                if !seen_as {
                    if ch.kind() == "scoped_identifier" {
                        // Child path is relative to the use-list prefix when present.
                        if let Some(child) = path_segments_from_scoped(ch, source) {
                            segs = join_use_prefix(&prefix, child);
                        }
                    } else if matches!(ch.kind(), "identifier" | "crate" | "self" | "super") {
                        segs.push(node_text(ch, source).to_string());
                    }
                }
            }
            if let Some(alias) = alias {
                if is_inventory_submit_full_path(&segs) {
                    out.insert(alias);
                }
            }
        }
        "scoped_use_list" => {
            // `use inventory::{...}` / `use crate::inventory::{...}`
            let mut path_segs = prefix.clone();
            let mut list: Option<Node> = None;
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                match ch.kind() {
                    "scoped_identifier" => {
                        path_segs = path_segments_from_scoped(ch, source).unwrap_or(path_segs);
                    }
                    "identifier" | "crate" | "self" | "super" => {
                        path_segs.push(node_text(ch, source).to_string());
                    }
                    "use_list" => list = Some(ch),
                    _ => {}
                }
            }
            if let Some(list) = list {
                collect_from_use_list(list, source, path_segs, out);
            }
        }
        "use_list" => {
            collect_from_use_list(node, source, prefix, out);
        }
        "use_wildcard" | "scoped_use_list_wildcard" => {
            // `use inventory::*` / `use crate::inventory::*` — path may live on
            // this node (not only in the parent prefix).
            let segs = {
                let from_node = wildcard_path_segments(node, source);
                if from_node.is_empty() {
                    prefix
                } else {
                    join_use_prefix(&prefix, from_node)
                }
            };
            if segs.last().map(|s| s == "inventory").unwrap_or(false)
                && is_inventory_crate_prefix(&segs)
            {
                out.insert("submit".to_string());
            }
        }
        "identifier" => {
            let mut segs = prefix;
            segs.push(node_text(node, source).to_string());
            if is_inventory_submit_full_path(&segs) {
                if let Some(local) = segs.last() {
                    out.insert(local.clone());
                }
            }
        }
        _ => {}
    }
}

fn collect_from_use_list(
    node: Node,
    source: &str,
    prefix: Vec<String>,
    out: &mut std::collections::HashSet<String>,
) {
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        match ch.kind() {
            "identifier" => {
                let mut segs = prefix.clone();
                segs.push(node_text(ch, source).to_string());
                if is_inventory_submit_full_path(&segs) {
                    out.insert(node_text(ch, source).to_string());
                }
            }
            "use_as_clause" => {
                collect_from_use_item(ch, source, prefix.clone(), out);
            }
            "use_wildcard" => {
                if prefix.last().map(|s| s == "inventory").unwrap_or(false)
                    && is_inventory_crate_prefix(&prefix)
                {
                    out.insert("submit".to_string());
                }
            }
            "scoped_identifier" | "use_list" | "scoped_use_list" => {
                collect_from_use_item(ch, source, prefix.clone(), out);
            }
            _ => {}
        }
    }
}

/// True when path segments are exactly `…::inventory::submit` and the prefix
/// before `inventory` is crate-relative (empty / crate / self / super*), not an
/// unrelated crate that happens to contain a module named `inventory`.
fn is_inventory_submit_full_path(path: &[String]) -> bool {
    let n = path.len();
    if n < 2 {
        return false;
    }
    path[n - 1] == "submit"
        && path[n - 2] == "inventory"
        && is_inventory_crate_prefix(&path[..n - 2])
}

fn is_inventory_crate_prefix(prefix: &[String]) -> bool {
    prefix
        .iter()
        .all(|s| matches!(s.as_str(), "crate" | "self" | "super" | "inventory"))
}

/// `inventory::submit! { RegistrationType { factory: || Box::new(Concrete::new(..)), .. } }`
///
/// Finite-domain identifiers written at the call site (registration type +
/// factory constructor type). Registration ≠ runtime call; Heuristic only.
/// Does **not** expand arbitrary macros.
fn rust_inventory_submit_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    inv_submit_aliases: &std::collections::HashSet<String>,
) {
    // tree-sitter-rust leaves the macro path unlabeled; take first path-ish child.
    let mut c = node.walk();
    let path_text = node
        .children(&mut c)
        .find(|ch| {
            matches!(
                ch.kind(),
                "identifier" | "scoped_identifier" | "field_expression"
            )
        })
        .map(|ch| node_text(ch, source).to_string())
        .unwrap_or_default();
    drop(c);

    if !is_inventory_submit_invocation(&path_text, inv_submit_aliases) {
        return;
    }

    // Outer token_tree is the macro body `{ ... }`
    let mut c = node.walk();
    let Some(body) = node.children(&mut c).find(|ch| ch.kind() == "token_tree") else {
        return;
    };

    let line = line_of(ctx, node);
    let snippet_src = node_text(node, source);
    let snippet: String = snippet_src
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();

    // Registration type: first UpperCamel identifier at body top level, typically
    // followed by a nested `{ ... }` struct literal.
    if let Some(reg) = inventory_registration_type(body, source) {
        push_l1(
            references,
            L1Edge {
                name: reg,
                qualifier: None,
                line,
                enclosing: None,
                confidence: Confidence::Heuristic,
                rule_id: "rs.di.inventory_submit",
                snippet: snippet.clone(),
            },
        );
    }

    // Factory constructor types: `Ident::new` anywhere inside the body.
    for ty in inventory_factory_types(body, source) {
        push_l1(
            references,
            L1Edge {
                name: ty,
                qualifier: None,
                line,
                enclosing: None,
                confidence: Confidence::Heuristic,
                rule_id: "rs.di.inventory_submit",
                snippet: snippet.clone(),
            },
        );
    }
}

/// Path is the inventory crate's `submit!` macro, or a file-local alias of it.
///
/// Accept:
/// - `inventory::submit` / `::inventory::submit` / `crate::inventory::submit`
/// - identifiers imported via `use inventory::submit` / `use inventory::submit as X`
///
/// Reject unrelated crate paths that merely contain a module named `inventory`
/// (`evil::inventory::submit`) and bare `submit!` with no inventory import.
fn is_inventory_submit_invocation(
    path: &str,
    inv_submit_aliases: &std::collections::HashSet<String>,
) -> bool {
    let raw = path.trim();
    if raw.is_empty() {
        return false;
    }
    // Local alias / imported name (`submit!`, `inv_submit!`).
    if !raw.contains("::") && inv_submit_aliases.contains(raw) {
        return true;
    }
    is_inventory_submit_path(raw)
}

fn is_inventory_submit_path(path: &str) -> bool {
    let p = path.trim().trim_start_matches("::");
    if p.is_empty() {
        return false;
    }
    let parts: Vec<String> = p
        .split("::")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    is_inventory_submit_full_path(&parts)
}

/// UpperCamel identifier at the top level of the macro token_tree that is
/// followed by a nested struct-literal token_tree.
fn inventory_registration_type(body: Node<'_>, source: &str) -> Option<String> {
    let mut c = body.walk();
    let kids: Vec<Node> = body.children(&mut c).collect();
    for (i, ch) in kids.iter().enumerate() {
        if ch.kind() != "identifier" {
            continue;
        }
        let t = node_text(*ch, source);
        if !is_type_ident(t) {
            continue;
        }
        // Prefer identifier immediately followed by `{` token_tree
        if kids
            .get(i + 1)
            .map(|n| n.kind() == "token_tree")
            .unwrap_or(false)
        {
            return Some(t.to_string());
        }
    }
    // Fallback: first UpperCamel identifier
    kids.iter()
        .find(|ch| ch.kind() == "identifier" && is_type_ident(node_text(**ch, source)))
        .map(|ch| node_text(*ch, source).to_string())
}

/// Collect `Type::new` constructor type names from nested token trees.
fn inventory_factory_types(body: Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    collect_new_ctor_types(body, source, &mut out);
    out
}

fn collect_new_ctor_types(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    // Pattern: identifier :: new  (as sibling sequence inside token_tree)
    let mut c = node.walk();
    let kids: Vec<Node> = node.children(&mut c).collect();
    for (i, ch) in kids.iter().enumerate() {
        if ch.kind() == "identifier" && node_text(*ch, source) == "new" {
            // look back for :: and Type
            if i >= 2 {
                let sep = kids[i - 1];
                let ty = kids[i - 2];
                if sep.kind() == "::" && ty.kind() == "identifier" {
                    let t = node_text(ty, source);
                    if is_type_ident(t) && t != "Box" && t != "Self" && !out.iter().any(|x| x == t)
                    {
                        out.push(t.to_string());
                    }
                }
            }
        }
        if ch.kind() == "token_tree" {
            collect_new_ctor_types(*ch, source, out);
        }
    }
}

fn is_type_ident(t: &str) -> bool {
    t.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
        && t.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn rust_impl_trait_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    // impl Trait for Type { fn method ... }
    let mut cursor = node.walk();
    let mut trait_name = None;
    let mut type_name = None;
    for c in node.children(&mut cursor) {
        match c.kind() {
            "type_identifier" | "generic_type" => {
                if trait_name.is_none() {
                    trait_name = Some(node_text(c, source).to_string());
                } else if type_name.is_none() {
                    type_name = Some(node_text(c, source).to_string());
                }
            }
            _ => {}
        }
    }
    // Also try field-like: tree-sitter-rust uses trait/ type fields in some versions
    if let Some(t) = node.child_by_field_name("trait") {
        trait_name = Some(node_text(t, source).to_string());
    }
    if let Some(t) = node.child_by_field_name("type") {
        type_name = Some(node_text(t, source).to_string());
    }

    let (Some(tr), Some(ty)) = (trait_name, type_name) else {
        return;
    };
    // Strip generics: `Vec<T>` → Vec
    let tr = tr.split('<').next().unwrap_or(&tr).trim().to_string();
    let ty = ty.split('<').next().unwrap_or(&ty).trim().to_string();
    if tr.is_empty() || ty.is_empty() || tr == ty {
        return;
    }

    // Only treat as trait impl when a trait with that name is plausible:
    // `impl Foo for Bar` — the "for" keyword separates them. Confirm via text.
    let impl_text = node_text(node, source);
    if !impl_text.contains(" for ") && !impl_text.starts_with("impl") {
        return;
    }
    // Heuristic: if there's no `for`, it's an inherent impl — skip.
    if !node.children(&mut node.walk()).any(|c| c.kind() == "for") && !impl_text.contains(" for ") {
        return;
    }

    // Emit Heuristic edges for each method in the impl block.
    let mut mc = node.walk();
    for c in node.children(&mut mc) {
        if c.kind() == "declaration_list" {
            let mut dc = c.walk();
            for m in c.children(&mut dc) {
                if m.kind() == "function_item" {
                    if let Some(name) = m
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source).to_string())
                    {
                        push_l1(
                            references,
                            L1Edge {
                                name,
                                qualifier: Some(ty.clone()),
                                line: line_of(ctx, m),
                                enclosing: Some(ty.clone()),
                                confidence: Confidence::Heuristic,
                                rule_id: "rs.di.impl_trait",
                                snippet: format!("impl {tr} for {ty}"),
                            },
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_segment_strips_dotted() {
        assert_eq!(last_segment("a.b.bind"), "bind");
        assert_eq!(last_segment("bind"), "bind");
    }
}
