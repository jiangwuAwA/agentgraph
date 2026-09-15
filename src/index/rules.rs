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
        Language::Go => walk_go(root, source, ctx, references),
        Language::Rust => walk_rust(root, source, ctx, references),
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
    references.push(ExtractedRef {
        name: edge.name,
        kind: EdgeKind::Call,
        line: edge.line,
        enclosing: edge.enclosing,
        module: None,
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
    ) {
        if let Some(n) = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source).to_string())
            .filter(|s| !s.is_empty())
        {
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
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_ts(child, source, ctx, references, local_enclosing.clone());
    }
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
    if matches!(
        method.as_str(),
        "on" | "subscribe" | "addListener" | "addEventListener"
    ) || is_route
    {
        if let Some(handler) = nth_arg_identifier(node, source, 1) {
            push_l1(
                references,
                L1Edge {
                    name: handler,
                    qualifier: None,
                    line,
                    enclosing: enclosing.clone(),
                    confidence: Confidence::Heuristic,
                    rule_id: if is_route {
                        "go.di.route_register"
                    } else {
                        "ts.event.subscribe"
                    },
                    snippet: format!("{}(...)", fn_text),
                },
            );
        }
    }
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

fn ts_decorator_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
) {
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
        let method = last_segment(node_text(fn_node, source));
        if !matches!(method, "Inject" | "Injectable" | "Optional" | "forwardRef") {
            continue;
        }
        if let Some(arg) = first_interesting_arg(child, source) {
            if let Some(name) = ident_name(arg, source) {
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

    // Depends(get_user_service) as a call argument anywhere
    if last == "Depends" {
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
                        snippet: format!("Depends({t})"),
                    },
                );
            }
        }
    }
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

fn walk_go(node: Node, source: &str, ctx: &ExtractContext<'_>, references: &mut Vec<ExtractedRef>) {
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
        go_method_impl_rule(node, source, ctx, references);
    }
    if matches!(
        node.kind(),
        "var_declaration" | "short_var_declaration" | "var_spec"
    ) {
        go_interface_assertion_rule(node, source, ctx, references);
    }

    for child in node.children(&mut cursor) {
        walk_go(child, source, ctx, references);
    }
}

/// `func (s *Server) ServeHTTP(...)` — concrete method often implements an
/// interface with the same name (PLAN: 接口方法 + 显式实现集).
fn go_method_impl_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    let Some(name_node) = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "field_identifier")
    else {
        return;
    };
    let method = node_text(name_node, source).to_string();
    if method.is_empty() {
        return;
    }
    // Receiver type from parameter_list: (s *Server) / (s Server)
    let mut recv_ty = None;
    if let Some(pl) = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "parameter_list")
    {
        let t = node_text(pl, source);
        // last identifier-ish token in receiver list
        let cleaned = t.trim_start_matches('(').trim_end_matches(')');
        if let Some(last) = cleaned.split([' ', '*', '(', ')']).rfind(|s| {
            !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_') && *s != "func"
        }) {
            recv_ty = Some(last.to_string());
        }
    }
    let snippet = match &recv_ty {
        Some(ty) => format!("func ({ty}) {method}"),
        None => format!("func {method}"),
    };
    push_l1(
        references,
        L1Edge {
            name: method,
            qualifier: recv_ty,
            line: line_of(ctx, node),
            enclosing: None,
            confidence: Confidence::Heuristic,
            rule_id: "go.di.interface_impl",
            snippet,
        },
    );
}

/// `var _ Store = (*MemStore)(nil)` — compile-time interface implementation proof.
fn go_interface_assertion_rule(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    let t = node_text(node, source);
    if !t.contains("= (*") && !t.contains("=(*") {
        // also allow var _ I = T{}
        if !(t.contains("var _") && t.contains(" = ")) {
            return;
        }
    }
    // Extract type after (* or =: (*MemStore) or MemStore{}
    let ty = if let Some(idx) = t.find("(*") {
        let rest = &t[idx + 2..];
        rest.split(')').next().unwrap_or("").trim().to_string()
    } else if let Some(idx) = t.find(" = ") {
        t[idx + 3..]
            .trim()
            .trim_end_matches("{}")
            .trim()
            .to_string()
    } else {
        return;
    };
    if ty.is_empty()
        || !ty
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        return;
    }
    let line = line_of(ctx, node);
    // Edge to the type (registration/impl proof site).
    push_l1(
        references,
        L1Edge {
            name: ty.clone(),
            qualifier: None,
            line,
            enclosing: None,
            confidence: Confidence::Heuristic,
            rule_id: "go.di.interface_assert",
            snippet: format!("var _ Iface = (*{ty})(nil)"),
        },
    );
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

fn walk_rust(
    node: Node,
    source: &str,
    ctx: &ExtractContext<'_>,
    references: &mut Vec<ExtractedRef>,
) {
    let mut cursor = node.walk();

    if node.kind() == "impl_item" {
        rust_impl_trait_rule(node, source, ctx, references);
    }

    for child in node.children(&mut cursor) {
        walk_rust(child, source, ctx, references);
    }
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
