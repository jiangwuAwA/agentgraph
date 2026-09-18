use anyhow::Result;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

use super::parser::LineIndex;
use super::resolve;
use crate::model::{Confidence, EdgeKind, Evidence, Language, SymbolKind};

#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub start_line: usize,
    pub end_line: usize,
    pub parent: Option<String>,
    pub start_col: usize,
    pub end_col: usize,
    pub return_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedRef {
    pub name: String,
    pub kind: EdgeKind,
    pub line: usize,
    pub enclosing: Option<String>,
    pub module: Option<String>,
    pub resolved: Option<String>,
    /// Type/module qualifier for type-aware calls, e.g. `ModelClient` in `ModelClient::connect`.
    pub qualifier: Option<String>,
    /// L0 edges are Exact; L1 rules set Heuristic / DynamicCandidate.
    pub confidence: Confidence,
    /// Rule id + snippet for non-Exact edges.
    pub evidence: Option<Evidence>,
}

#[derive(Debug, Clone)]
pub struct ExtractedFile {
    pub symbols: Vec<ExtractedSymbol>,
    pub references: Vec<ExtractedRef>,
}

pub struct ExtractContext<'a> {
    pub path: &'a str,
    pub known_files: &'a HashSet<String>,
    pub lines: LineIndex,
    /// File-scope variable/param -> type name (best-effort). (best-effort).
    pub var_types: RefCell<HashMap<String, String>>,
}

pub fn extract_file(
    source: &str,
    lang: Language,
    path: &str,
    known_files: &HashSet<String>,
) -> Result<ExtractedFile> {
    let tree = super::parser::parse(source, lang)?;
    let root = tree.root_node();
    let mut symbols = Vec::new();
    let mut references = Vec::new();
    let ctx = ExtractContext {
        path,
        known_files,
        lines: LineIndex::new(source),
        var_types: RefCell::new(HashMap::new()),
    };

    match lang {
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            walk_ts(root, source, None, &mut symbols, &mut references, &ctx)
        }
        Language::Python => walk_py(root, source, None, &mut symbols, &mut references, &ctx),
        Language::Go => walk_go(root, source, None, &mut symbols, &mut references, &ctx),
        Language::Rust => walk_rust(root, source, None, &mut symbols, &mut references, &ctx),
    }

    // L1: append Heuristic / DynamicCandidate candidate edges (DI, reflection, maps).
    super::rules::apply(lang, root, source, &ctx, &mut references);

    Ok(ExtractedFile {
        symbols,
        references,
    })
}

fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

fn child_by_field<'a>(node: &Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

fn first_identifier_name(node: Node, source: &str) -> Option<String> {
    if let Some(n) = child_by_field(&node, "name") {
        let t = node_text(n, source);
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier"
            | "type_identifier"
            | "property_identifier"
            | "shorthand_property_identifier"
            | "field_identifier"
            | "string_fragment"
            | "primitive_type" => {
                let t = node_text(child, source);
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn push_call(
    references: &mut Vec<ExtractedRef>,
    name: String,
    line: usize,
    enclosing: Option<String>,
) {
    push_call_q(references, name, None, line, enclosing);
}

fn push_call_q(
    references: &mut Vec<ExtractedRef>,
    name: String,
    qualifier: Option<String>,
    line: usize,
    enclosing: Option<String>,
) {
    references.push(ExtractedRef {
        name,
        kind: EdgeKind::Call,
        line,
        enclosing,
        module: None,
        resolved: None,
        qualifier,
        confidence: Confidence::Exact,
        evidence: None,
    });
}

fn push_import(
    references: &mut Vec<ExtractedRef>,
    name: String,
    line: usize,
    enclosing: Option<String>,
    module: Option<String>,
    resolved: Option<String>,
) {
    references.push(ExtractedRef {
        name,
        kind: EdgeKind::Import,
        line,
        enclosing,
        module,
        resolved,
        qualifier: None,
        confidence: Confidence::Exact,
        evidence: None,
    });
}

/// `s := NewServer()` / `const s = createStore()` → define edge name=s, module=callee.
fn push_assign(
    references: &mut Vec<ExtractedRef>,
    var: String,
    callee: String,
    line: usize,
    enclosing: Option<String>,
) {
    references.push(ExtractedRef {
        name: var,
        kind: EdgeKind::Define,
        line,
        enclosing,
        module: Some(callee),
        resolved: None,
        qualifier: None,
        confidence: Confidence::Exact,
        evidence: None,
    });
}

fn first_identifier_node(node: Node) -> Option<Node> {
    if let Some(n) = child_by_field(&node, "name") {
        return Some(n);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier"
            | "type_identifier"
            | "property_identifier"
            | "field_identifier"
            | "shorthand_property_identifier" => return Some(child),
            _ => {}
        }
    }
    None
}

fn make_symbol(
    name: String,
    qualified_name: String,
    kind: SymbolKind,
    node: Node,
    parent: Option<String>,
    ctx: &ExtractContext,
    source: &str,
) -> ExtractedSymbol {
    // Prefer the identifier node so SCIP range is the name, not the whole body.
    let range_node = first_identifier_node(node).unwrap_or(node);
    ExtractedSymbol {
        name,
        qualified_name,
        kind,
        start_line: ctx.lines.line_of(range_node.start_byte()),
        end_line: ctx.lines.line_of(range_node.end_byte()),
        parent,
        start_col: ctx.lines.col_utf16(source, range_node.start_byte()),
        end_col: ctx
            .lines
            .col_utf16(source, range_node.end_byte())
            .max(ctx.lines.col_utf16(source, range_node.start_byte()) + 1),
        return_type: extract_return_type(node, source),
    }
}

/// Best-effort return type from the function/method signature.
fn extract_return_type(node: Node, source: &str) -> Option<String> {
    // TS: type_annotation after parameters; Rust: return_type field; Go: result field.
    for field in ["return_type", "result", "type"] {
        if let Some(n) = child_by_field(&node, field) {
            let t = node_text(n, source).trim();
            let t = t.trim_start_matches("->").trim();
            let t = t.trim_start_matches('*').trim();
            let base = t.split('<').next().unwrap_or(t).trim();
            if !base.is_empty() && base != "void" && base != "unit" {
                return Some(base.rsplit('.').next().unwrap_or(base).to_string());
            }
        }
    }
    // TS function_declaration: look for type_annotation sibling of parameters
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_annotation" {
            let t = node_text(child, source)
                .trim()
                .trim_start_matches(':')
                .trim();
            let base = t.split('<').next().unwrap_or(t).trim();
            if !base.is_empty() && base != "void" {
                return Some(base.rsplit('.').next().unwrap_or(base).to_string());
            }
        }
    }
    None
}

// 鈹€鈹€鈹€ TypeScript 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

fn walk_ts(
    node: Node,
    source: &str,
    parent: Option<String>,
    symbols: &mut Vec<ExtractedSymbol>,
    references: &mut Vec<ExtractedRef>,
    ctx: &ExtractContext,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let mut local_parent = parent.clone();

    let is_fn_scope = matches!(
        kind,
        "function_declaration"
            | "method_definition"
            | "generator_function_declaration"
            | "arrow_function"
            | "function_expression"
            | "generator_function"
    );
    let saved_var_types = if is_fn_scope {
        Some(ctx.var_types.borrow().clone())
    } else {
        None
    };

    match kind {
        "function_declaration" | "method_definition" | "generator_function_declaration" => {
            collect_ts_param_types(node, source, ctx);
        }
        "lexical_declaration" | "variable_declaration" => {
            collect_ts_var_types(node, source, ctx);
        }
        _ => {}
    }

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_declaration" | "generator_function_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Function))
        }
        // Named function expression: `app.main = function main() { ... }`
        // Must become a symbol so impact BFS can expand via `enclosing`.
        "function_expression" => node.child_by_field_name("name").and_then(|n| {
            let t = node_text(n, source);
            if t.is_empty() {
                None
            } else {
                Some((t.to_string(), SymbolKind::Function))
            }
        }),
        "class_declaration" | "class" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Class))
        }
        "interface_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Interface))
        }
        "type_alias_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::TypeAlias))
        }
        "method_definition" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Method)),
        "lexical_declaration" | "variable_declaration" => extract_var_function(node, source),
        _ => None,
    };

    if let Some((name, skind)) = symbol_info {
        let qname = match &parent {
            Some(p) if skind == SymbolKind::Method => format!("{p}.{name}"),
            Some(p)
                if skind != SymbolKind::Class
                    && skind != SymbolKind::Function
                    && skind != SymbolKind::Interface
                    && skind != SymbolKind::TypeAlias =>
            {
                format!("{p}.{name}")
            }
            _ => name.clone(),
        };
        symbols.push(make_symbol(
            name.clone(),
            qname.clone(),
            skind,
            node,
            parent.clone(),
            ctx,
            source,
        ));
        local_parent = Some(qname);
    }

    match kind {
        "call_expression" | "new_expression" => {
            // tree-sitter-typescript: new_expression field is `constructor`.
            let fn_node =
                child_by_field(&node, "function").or_else(|| child_by_field(&node, "constructor"));
            if let Some(fn_node) = fn_node {
                if let Some((n, q)) = call_target_q_with_ctx(fn_node, source, ctx) {
                    push_call_q(
                        references,
                        n,
                        q,
                        ctx.lines.line_of(node.start_byte()),
                        local_parent.clone(),
                    );
                }
            }
        }
        "import_statement" => {
            let line = ctx.lines.line_of(node.start_byte());
            let specifier = ts_import_specifier(node, source);
            let resolved = specifier
                .as_ref()
                .and_then(|s| resolve::resolve_typescript_import(ctx.path, s, ctx.known_files));
            collect_import_names(node, source, &mut |name| {
                push_import(
                    references,
                    name,
                    line,
                    local_parent.clone(),
                    specifier.clone(),
                    resolved.clone(),
                );
            });
        }
        // `export { A, B as C } from "./mod"` / `export { A }` — re-export surface.
        // Mint findable symbols + import-like refs so blast/who-calls file sets
        // can include the barrel file (multi-root path-alias / re-export recall).
        "export_statement" => {
            let line = ctx.lines.line_of(node.start_byte());
            let specifier = ts_import_specifier(node, source);
            let resolved = specifier
                .as_ref()
                .and_then(|s| resolve::resolve_typescript_import(ctx.path, s, ctx.known_files));
            for (public_name, src_mod) in collect_ts_export_names(node, source) {
                symbols.push(make_symbol(
                    public_name.clone(),
                    public_name.clone(),
                    SymbolKind::Module,
                    node,
                    local_parent.clone(),
                    ctx,
                    source,
                ));
                let module = src_mod.or_else(|| specifier.clone());
                push_import(
                    references,
                    public_name,
                    line,
                    local_parent.clone(),
                    module,
                    resolved.clone(),
                );
            }
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_ts(
            child,
            source,
            local_parent.clone(),
            symbols,
            references,
            ctx,
        );
    }

    if let Some(s) = saved_var_types {
        *ctx.var_types.borrow_mut() = s;
    }
}

/// Public export names from `export { A, B as C } from "mod"` / `export { A }`.
/// Returns `(public_name, source_module_if_any)`.
fn collect_ts_export_names(node: Node, source: &str) -> Vec<(String, Option<String>)> {
    let mut module: Option<String> = None;
    let mut names = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "string" => {
                module = ts_string_content(child, source);
            }
            "source" => {
                let mut c2 = child.walk();
                for part in child.children(&mut c2) {
                    if part.kind() == "string" {
                        module = ts_string_content(part, source);
                    }
                }
            }
            "export_clause" => {
                let mut c2 = child.walk();
                for spec in child.children(&mut c2) {
                    match spec.kind() {
                        "export_specifier" => {
                            let mut original = None;
                            let mut public = None;
                            let mut c3 = spec.walk();
                            for part in spec.children(&mut c3) {
                                match part.kind() {
                                    "identifier" => {
                                        let t = node_text(part, source);
                                        if !t.is_empty() {
                                            if original.is_none() {
                                                original = Some(t.to_string());
                                            } else {
                                                public = Some(t.to_string());
                                            }
                                        }
                                    }
                                    "alias" => {
                                        let mut c4 = part.walk();
                                        for idn in part.children(&mut c4) {
                                            if idn.kind() == "identifier" {
                                                let t = node_text(idn, source);
                                                if !t.is_empty() {
                                                    public = Some(t.to_string());
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if let Some(pub_name) = public.or(original) {
                                names.push((pub_name, module.clone()));
                            }
                        }
                        "identifier" => {
                            let t = node_text(spec, source);
                            if !t.is_empty() {
                                names.push((t.to_string(), module.clone()));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    names
}

fn ts_string_content(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string_fragment" {
            let t = node_text(child, source);
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    let raw = node_text(node, source);
    let t = raw.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn ts_import_specifier(node: Node, source: &str) -> Option<String> {
    // tree-sitter-typescript: string may be direct child, or under "source"
    let mut string_node: Option<Node> = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string" {
            string_node = Some(child);
            break;
        }
        if child.kind() == "source" {
            let mut c2 = child.walk();
            for part in child.children(&mut c2) {
                if part.kind() == "string" {
                    string_node = Some(part);
                    break;
                }
            }
            if string_node.is_some() {
                break;
            }
        }
    }
    let target = string_node?;
    let mut c3 = target.walk();
    for part in target.children(&mut c3) {
        if part.kind() == "string_fragment" {
            let t = node_text(part, source);
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    let raw = node_text(target, source);
    let t = raw.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn extract_var_function(node: Node, source: &str) -> Option<(String, SymbolKind)> {
    let mut cursor = node.walk();
    for declarator in node.children(&mut cursor) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        let name = child_by_field(&declarator, "name")?;
        let name_text = node_text(name, source).to_string();
        if name_text.is_empty() {
            continue;
        }
        if let Some(value) = child_by_field(&declarator, "value") {
            match value.kind() {
                "arrow_function" | "function_expression" | "function" | "generator_function" => {
                    return Some((name_text, SymbolKind::Function));
                }
                _ => {}
            }
        }
    }
    None
}

/// Type-aware TS/JS call target. `Foo.bar()` �?(bar, Foo) when object is a simple identifier.
fn call_target_q(node: Node, source: &str) -> Option<(String, Option<String>)> {
    match node.kind() {
        "identifier" | "property_identifier" => Some((node_text(node, source).to_string(), None)),
        "member_expression" => {
            let prop = child_by_field(&node, "property")?;
            let name = node_text(prop, source).to_string();
            let obj = child_by_field(&node, "object")?;
            let q = match obj.kind() {
                "identifier" => {
                    let t = node_text(obj, source).to_string();
                    if t == "this" || t == "self" {
                        None
                    } else {
                        Some(t)
                    }
                }
                "new_expression" => child_by_field(&obj, "constructor")
                    .map(|ctor| node_text(ctor, source).to_string()),
                _ => None,
            };
            Some((name, q))
        }
        "parenthesized_expression" => {
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if let Some(n) = call_target_q(ch, source) {
                    return Some(n);
                }
            }
            None
        }
        _ => {
            let mut last = None;
            fn scan(n: Node, src: &str, last: &mut Option<String>) {
                if matches!(n.kind(), "identifier" | "property_identifier") {
                    *last = Some(node_text(n, src).to_string());
                }
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    scan(ch, src, last);
                }
            }
            scan(node, source, &mut last);
            last.map(|n| (n, None))
        }
    }
}

fn call_target_q_with_ctx(
    node: Node,
    source: &str,
    ctx: &ExtractContext,
) -> Option<(String, Option<String>)> {
    if node.kind() == "member_expression" {
        if let Some(prop) = child_by_field(&node, "property") {
            let name = node_text(prop, source).to_string();
            if let Some(obj) = child_by_field(&node, "object") {
                if obj.kind() == "identifier" {
                    let t = node_text(obj, source).to_string();
                    if t == "this" || t == "self" {
                        return Some((name, None));
                    }
                    if let Some(ty) = ctx.var_types.borrow().get(&t).cloned() {
                        return Some((name, Some(ty)));
                    }
                    return Some((name, Some(t)));
                }
            }
        }
    }
    call_target_q(node, source)
}

fn collect_ts_var_types(node: Node, source: &str, ctx: &ExtractContext) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        let Some(name_n) = child_by_field(&child, "name") else {
            continue;
        };
        let name = node_text(name_n, source).to_string();
        if name.is_empty() {
            continue;
        }
        let mut c2 = name_n.walk();
        for part in name_n.children(&mut c2) {
            if part.kind() == "type_annotation" {
                let mut c3 = part.walk();
                for t in part.children(&mut c3) {
                    let tt = node_text(t, source).trim().trim_start_matches('&');
                    if !tt.is_empty() && t.kind() != ":" {
                        ctx.var_types
                            .borrow_mut()
                            .insert(name.clone(), tt.to_string());
                        break;
                    }
                }
            }
        }
        if let Some(value) = child_by_field(&child, "value") {
            if value.kind() == "new_expression" {
                if let Some(ctor) = child_by_field(&value, "constructor") {
                    let t = node_text(ctor, source).to_string();
                    ctx.var_types.borrow_mut().insert(name.clone(), t);
                }
            }
        }
    }
}

fn collect_ts_param_types(node: Node, source: &str, ctx: &ExtractContext) {
    let Some(params) = child_by_field(&node, "parameters") else {
        return;
    };
    let mut cursor = params.walk();
    for p in params.children(&mut cursor) {
        let kind = p.kind();
        if kind != "required_parameter" && kind != "optional_parameter" {
            continue;
        }
        let mut name = String::new();
        let mut ty = String::new();
        let mut c2 = p.walk();
        for part in p.children(&mut c2) {
            match part.kind() {
                "identifier" => {
                    if name.is_empty() {
                        name = node_text(part, source).to_string();
                    }
                }
                "type_annotation" => {
                    let mut c3 = part.walk();
                    for t in part.children(&mut c3) {
                        let tt = node_text(t, source).trim().trim_start_matches('&');
                        if !tt.is_empty() && t.kind() != ":" {
                            ty = tt.to_string();
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        if !name.is_empty() && !ty.is_empty() {
            ctx.var_types.borrow_mut().insert(name, ty);
        }
    }
}
fn collect_import_names(node: Node, source: &str, sink: &mut impl FnMut(String)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "import_clause" {
            continue;
        }
        let mut c2 = child.walk();
        for part in child.children(&mut c2) {
            match part.kind() {
                "identifier" => sink(node_text(part, source).to_string()),
                "named_imports" => {
                    let mut c3 = part.walk();
                    for imp in part.children(&mut c3) {
                        if imp.kind() == "import_specifier" {
                            if let Some(n) = first_identifier_name(imp, source) {
                                sink(n);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// 鈹€鈹€鈹€ Python 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

fn walk_py(
    node: Node,
    source: &str,
    parent: Option<String>,
    symbols: &mut Vec<ExtractedSymbol>,
    references: &mut Vec<ExtractedRef>,
    ctx: &ExtractContext,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let mut local_parent = parent.clone();

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_definition" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Function))
        }
        "class_definition" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Class)),
        _ => None,
    };

    if let Some((name, skind)) = symbol_info {
        let qname = match &parent {
            Some(p) => format!("{p}.{name}"),
            None => name.clone(),
        };
        symbols.push(make_symbol(
            name.clone(),
            qname.clone(),
            skind,
            node,
            parent.clone(),
            ctx,
            source,
        ));
        local_parent = Some(qname);
    }

    match kind {
        "call" => {
            if let Some(func) = child_by_field(&node, "function") {
                if let Some(n) = py_call_name(func, source) {
                    push_call(
                        references,
                        n,
                        ctx.lines.line_of(node.start_byte()),
                        local_parent.clone(),
                    );
                }
            }
        }
        "import_statement" => {
            let line = ctx.lines.line_of(node.start_byte());
            collect_py_import_statement(
                node,
                source,
                0,
                ctx,
                local_parent.clone(),
                references,
                line,
            );
        }
        "import_from_statement" => {
            let line = ctx.lines.line_of(node.start_byte());
            let level = py_import_level(node, source);
            let module = py_import_module(node, source);
            let resolved = module
                .as_ref()
                .and_then(|m| resolve::resolve_python_import(ctx.path, m, level, ctx.known_files));
            collect_py_from_imports(node, source, &mut |name| {
                push_import(
                    references,
                    name,
                    line,
                    local_parent.clone(),
                    module.clone(),
                    resolved.clone(),
                );
            });
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_py(
            child,
            source,
            local_parent.clone(),
            symbols,
            references,
            ctx,
        );
    }
}

fn py_import_level(node: Node, source: &str) -> usize {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import_from"
            || child.kind() == "relative_import"
            || child.kind() == "import_prefix"
        {
            // count dots in prefix
            let mut c2 = child.walk();
            for p in child.children(&mut c2) {
                if p.kind() == "import_prefix" {
                    let t = node_text(p, source);
                    return t.chars().filter(|c| *c == '.').count();
                }
            }
            let t = node_text(child, source);
            if t.starts_with('.') {
                return t.chars().take_while(|c| *c == '.').count();
            }
        }
    }
    // tree-sitter-python: field may be "module_name" and relative dots as "relative_import"
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "relative_import" {
            let t = node_text(child, source);
            return t.chars().take_while(|c| *c == '.').count();
        }
    }
    0
}

fn py_import_module(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => return Some(node_text(child, source).to_string()),
            "relative_import" => {
                let t = node_text(child, source);
                let modpart = t.trim_start_matches('.');
                if modpart.is_empty() {
                    return Some(String::new());
                }
                return Some(modpart.to_string());
            }
            "module_name" => {
                let t = node_text(child, source);
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_py_from_imports(node: Node, source: &str, sink: &mut impl FnMut(String)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                let t = node_text(child, source);
                if let Some(last) = t.rsplit('.').next() {
                    sink(last.to_string());
                }
            }
            "aliased_import" => {
                if let Some(n) = first_identifier_name(child, source) {
                    sink(n);
                }
            }
            "wildcard_import" => {}
            "module_name" | "relative_import" => {}
            _ => {
                // imported names live under "import_from_statement" as identifiers / aliased
                if matches!(child.kind(), "identifier") {
                    sink(node_text(child, source).to_string());
                } else if child.kind() == "import_list" || child.kind() == "import_from" {
                    let mut c2 = child.walk();
                    for p in child.children(&mut c2) {
                        if p.kind() == "dotted_name" || p.kind() == "identifier" {
                            let t = node_text(p, source);
                            if let Some(last) = t.rsplit('.').next() {
                                sink(last.to_string());
                            }
                        } else if p.kind() == "aliased_import" {
                            if let Some(n) = first_identifier_name(p, source) {
                                sink(n);
                            }
                        }
                    }
                }
            }
        }
    }
    // Also walk all descendants for import_specifiers style
    fn deep(node: Node, source: &str, sink: &mut impl FnMut(String)) {
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            match ch.kind() {
                "identifier" => {
                    // skip module path identifiers �?only collect last-level names in wildcards handled above
                }
                "aliased_import" => {
                    if let Some(n) = first_identifier_name(ch, source) {
                        sink(n);
                    }
                }
                "dotted_name" => {}
                _ => deep(ch, source, sink),
            }
        }
    }
    deep(node, source, sink);
}

fn collect_py_import_statement(
    node: Node,
    source: &str,
    _level: usize,
    ctx: &ExtractContext,
    enclosing: Option<String>,
    references: &mut Vec<ExtractedRef>,
    line: usize,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "dotted_name" {
            let t = node_text(child, source);
            let last = t.rsplit('.').next().unwrap_or(t).to_string();
            let resolved = resolve::resolve_python_import(ctx.path, t, 0, ctx.known_files);
            push_import(
                references,
                last,
                line,
                enclosing.clone(),
                Some(t.to_string()),
                resolved,
            );
        } else if child.kind() == "aliased_import" {
            let mut c2 = child.walk();
            let mut module = None;
            let mut alias = None;
            for p in child.children(&mut c2) {
                if p.kind() == "dotted_name" {
                    module = Some(node_text(p, source).to_string());
                } else if p.kind() == "identifier" {
                    alias = Some(node_text(p, source).to_string());
                }
            }
            let name = alias.clone().or_else(|| {
                module
                    .as_ref()
                    .map(|m| m.rsplit('.').next().unwrap_or(m).to_string())
            });
            if let Some(name) = name {
                let resolved = module
                    .as_ref()
                    .and_then(|m| resolve::resolve_python_import(ctx.path, m, 0, ctx.known_files));
                push_import(references, name, line, enclosing.clone(), module, resolved);
            }
        }
    }
}

fn py_call_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(node_text(node, source).to_string()),
        "attribute" => {
            let attr = child_by_field(&node, "attribute")?;
            Some(node_text(attr, source).to_string())
        }
        _ => None,
    }
}

// 鈹€鈹€鈹€ Go 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

fn walk_go(
    node: Node,
    source: &str,
    parent: Option<String>,
    symbols: &mut Vec<ExtractedSymbol>,
    references: &mut Vec<ExtractedRef>,
    ctx: &ExtractContext,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let mut local_parent = parent.clone();

    // Scoped var_types: entering a function/method/literal snapshots the map so
    // inner `s: User` cannot permanently overwrite outer `s: Store`.
    let is_fn_scope = matches!(
        kind,
        "function_declaration" | "method_declaration" | "func_literal"
    );
    let saved_var_types = if is_fn_scope {
        Some(ctx.var_types.borrow().clone())
    } else {
        None
    };
    if is_fn_scope {
        collect_go_param_types(node, source, ctx);
    }
    if kind == "short_var_declaration" {
        collect_go_short_var(node, source, ctx);
        collect_go_assign_refs(node, source, references, local_parent.clone(), ctx);
    }

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Function))
        }
        "method_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Method))
        }
        "type_declaration" => go_type_decl_name(node, source),
        _ => None,
    };

    if let Some((name, skind)) = symbol_info {
        // Go methods: qualify by receiver type so `Start` doesn't collide across packages.
        let parent_for_qname = if skind == SymbolKind::Method {
            go_receiver_type(node, source).or_else(|| parent.clone())
        } else {
            parent.clone()
        };
        let qname = match &parent_for_qname {
            Some(p) if skind == SymbolKind::Method => format!("{p}.{name}"),
            _ => name.clone(),
        };
        symbols.push(make_symbol(
            name.clone(),
            qname.clone(),
            skind,
            node,
            parent_for_qname.clone(),
            ctx,
            source,
        ));
        local_parent = Some(qname);
    }

    match kind {
        "call_expression" => {
            if let Some(fn_node) = child_by_field(&node, "function") {
                if let Some((n, q)) = go_call_target(fn_node, source, Some(ctx)) {
                    push_call_q(
                        references,
                        n,
                        q,
                        ctx.lines.line_of(node.start_byte()),
                        local_parent.clone(),
                    );
                }
            }
        }
        "type_case" => {
            // `case Cat:` / `case *Cat:` / `case pkg.Cat:` — type names used by a
            // type switch are real impact sites for those types.
            go_type_case_refs(node, source, local_parent.clone(), references, ctx);
        }
        "import_declaration" => {
            let line = ctx.lines.line_of(node.start_byte());
            go_collect_imports(node, source, ctx, local_parent.clone(), references, line);
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_go(
            child,
            source,
            local_parent.clone(),
            symbols,
            references,
            ctx,
        );
    }

    if let Some(s) = saved_var_types {
        *ctx.var_types.borrow_mut() = s;
    }
}

/// Extract Go method receiver type name, e.g. `func (s *Server) Start()` �?`Server`.
fn go_receiver_type(node: Node, source: &str) -> Option<String> {
    let params = child_by_field(&node, "receiver")?;
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        let ty = child_by_field(&child, "type")?;
        let text = node_text(ty, source);
        // *Server / Server / pkg.Server
        let cleaned = text.trim().trim_start_matches('*').trim();
        let base = cleaned.rsplit('.').next().unwrap_or(cleaned);
        if !base.is_empty() {
            return Some(base.to_string());
        }
    }
    None
}

/// Go builtins — type-switch cases like `case string:` must not mint noise refs.
fn is_go_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "string"
            | "bool"
            | "byte"
            | "rune"
            | "error"
            | "any"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            | "float32"
            | "float64"
            | "complex64"
            | "complex128"
            | "comparable"
    )
}

/// Recurse a Go type expression (`*T`, `[]T`, `pkg.T`, `*pkg.T`, `map[K]V`)
/// and mint refs for every non-builtin named type.
fn go_type_name_refs(
    node: Node,
    source: &str,
    line: usize,
    parent: Option<String>,
    references: &mut Vec<ExtractedRef>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_identifier" => {
                let name = node_text(child, source).to_string();
                if !name.is_empty() && !is_go_builtin_type(&name) {
                    push_call_q(references, name, None, line, parent.clone());
                }
            }
            "qualified_type" => {
                let mut q = child.walk();
                let mut name = None;
                let mut pkg = None;
                for c in child.children(&mut q) {
                    match c.kind() {
                        "type_identifier" => name = Some(node_text(c, source).to_string()),
                        "package_identifier" => pkg = Some(node_text(c, source).to_string()),
                        _ => {}
                    }
                }
                if let Some(n) = name {
                    if !n.is_empty() && !is_go_builtin_type(&n) {
                        push_call_q(references, n, pkg, line, parent.clone());
                    }
                }
            }
            k if k.ends_with("_type") => {
                go_type_name_refs(child, source, line, parent.clone(), references);
            }
            _ => {}
        }
    }
}

/// Collect type names from a `type_case` (before the `:`).
fn go_type_case_refs(
    node: Node,
    source: &str,
    parent: Option<String>,
    references: &mut Vec<ExtractedRef>,
    ctx: &ExtractContext,
) {
    let line = ctx.lines.line_of(node.start_byte());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let k = child.kind();
        if k == ":" {
            break;
        }
        if k == "case" {
            continue;
        }
        match k {
            "type_identifier" => {
                let name = node_text(child, source).to_string();
                if !name.is_empty() && !is_go_builtin_type(&name) {
                    push_call_q(references, name, None, line, parent.clone());
                }
            }
            "qualified_type" => {
                let mut q = child.walk();
                let mut name = None;
                let mut pkg = None;
                for c in child.children(&mut q) {
                    match c.kind() {
                        "type_identifier" => name = Some(node_text(c, source).to_string()),
                        "package_identifier" => pkg = Some(node_text(c, source).to_string()),
                        _ => {}
                    }
                }
                if let Some(n) = name {
                    if !n.is_empty() && !is_go_builtin_type(&n) {
                        push_call_q(references, n, pkg, line, parent.clone());
                    }
                }
            }
            _ if k.ends_with("_type") => {
                // pointer_type / slice_type / array_type / map_type.
                // Inner may be type_identifier OR qualified_type (`*pkg.Cat`,
                // `[]pkg.Cat`) — recurse so qualified names are not dropped.
                go_type_name_refs(child, source, line, parent.clone(), references);
            }
            _ => {}
        }
    }
}

fn go_type_decl_name(node: Node, source: &str) -> Option<(String, SymbolKind)> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_spec" {
            let name = first_identifier_name(child, source)?;
            let mut c2 = child.walk();
            let mut skind = SymbolKind::Struct;
            for p in child.children(&mut c2) {
                match p.kind() {
                    "struct_type" => skind = SymbolKind::Struct,
                    "interface_type" => skind = SymbolKind::Interface,
                    _ => {}
                }
            }
            return Some((name, skind));
        }
    }
    None
}

/// `pkg.Func` / `x.Method` �?(Func/Method, Some(pkg/x)).
fn go_call_target(
    node: Node,
    source: &str,
    ctx: Option<&ExtractContext>,
) -> Option<(String, Option<String>)> {
    match node.kind() {
        "identifier" => Some((node_text(node, source).to_string(), None)),
        "selector_expression" => {
            let field = child_by_field(&node, "field").unwrap_or_else(|| {
                let mut last = node;
                let mut c = node.walk();
                for ch in node.children(&mut c) {
                    if ch.kind() == "field_identifier" {
                        last = ch;
                    }
                }
                last
            });
            let name = node_text(field, source).to_string();
            // Object is the first identifier child (tree-sitter-go may not set "operand").
            let mut obj: Option<Node> =
                child_by_field(&node, "operand").or_else(|| child_by_field(&node, "value"));
            if obj.is_none() {
                let mut c = node.walk();
                for ch in node.children(&mut c) {
                    if ch.kind() == "identifier" {
                        obj = Some(ch);
                        break;
                    }
                }
            }
            let q = obj.filter(|o| o.kind() == "identifier").map(|o| {
                let t = node_text(o, source).to_string();
                if let Some(ctx) = ctx {
                    if let Some(ty) = ctx.var_types.borrow().get(&t).cloned() {
                        return ty;
                    }
                }
                t
            });
            Some((name, q))
        }
        _ => {
            let mut last = None;
            fn scan(n: Node, src: &str, last: &mut Option<String>) {
                if matches!(n.kind(), "identifier" | "field_identifier") {
                    *last = Some(node_text(n, src).to_string());
                }
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    scan(ch, src, last);
                }
            }
            scan(node, source, &mut last);
            last.map(|n| (n, None))
        }
    }
}

/// Collect Go parameter/receiver types into var_types so `x.Method()` can use
/// the type as qualifier. Handles `func f(s *Server)`, `func (s *Server) M()`.
fn collect_go_param_types(node: Node, source: &str, ctx: &ExtractContext) {
    for field in ["parameters", "receiver"] {
        let Some(params) = child_by_field(&node, field) else {
            continue;
        };
        let mut cursor = params.walk();
        for p in params.children(&mut cursor) {
            if p.kind() != "parameter_declaration" {
                continue;
            }
            let Some(ty_n) = child_by_field(&p, "type") else {
                continue;
            };
            let ty_text = node_text(ty_n, source);
            // *Server / Server / pkg.Server → base type name
            let cleaned = ty_text.trim().trim_start_matches('*').trim();
            let base = cleaned.rsplit('.').next().unwrap_or(cleaned);
            if base.is_empty() {
                continue;
            }
            let ty = base.to_string();
            if let Some(name_n) = child_by_field(&p, "name") {
                let name = node_text(name_n, source).trim().to_string();
                if !name.is_empty() && name != "_" {
                    ctx.var_types.borrow_mut().insert(name, ty.clone());
                }
            }
            // Multi-name form `a, b string` — collect bare identifiers.
            let mut c2 = p.walk();
            for part in p.children(&mut c2) {
                if part.kind() == "identifier" {
                    let name = node_text(part, source).trim().to_string();
                    if !name.is_empty() && name != "_" {
                        ctx.var_types.borrow_mut().insert(name, ty.clone());
                    }
                }
            }
        }
    }
}

/// `s := NewServer()` → var_types[s] = Server (strip New/new prefix).
/// AST: short_var_declaration → expression_list(names) := expression_list(call_expression(...))
fn collect_go_short_var(node: Node, source: &str, ctx: &ExtractContext) {
    fn find_ctor_type(n: Node, source: &str) -> Option<String> {
        if n.kind() == "call_expression" {
            let mut fn_name: Option<String> =
                child_by_field(&n, "function").map(|f| node_text(f, source).to_string());
            if fn_name.is_none() {
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    if ch.kind() == "identifier" {
                        fn_name = Some(node_text(ch, source).to_string());
                        break;
                    }
                }
            }
            let ctor = fn_name?;
            let leaf = ctor.rsplit('.').next().unwrap_or(ctor.as_str());
            let ty = leaf
                .strip_prefix("New")
                .or_else(|| leaf.strip_prefix("new"))?;
            if ty.is_empty() {
                return None;
            }
            return Some(ty.to_string());
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            if let Some(t) = find_ctor_type(ch, source) {
                return Some(t);
            }
        }
        None
    }

    fn collect_names(n: Node, source: &str, ctx: &ExtractContext, ty: &str) {
        if n.kind() == "identifier" {
            let name = node_text(n, source).trim().to_string();
            if !name.is_empty() && name != "_" {
                ctx.var_types.borrow_mut().insert(name, ty.to_string());
            }
        }
        // only walk the first expression_list for names
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            if ch.kind() == "expression_list" {
                let mut c2 = ch.walk();
                for id in ch.children(&mut c2) {
                    if id.kind() == "identifier" {
                        let name = node_text(id, source).trim().to_string();
                        if !name.is_empty() && name != "_" {
                            ctx.var_types.borrow_mut().insert(name, ty.to_string());
                        }
                    }
                }
                return;
            }
        }
    }

    let Some(ty) = find_ctor_type(node, source) else {
        return;
    };
    collect_names(node, source, ctx, &ty);
}

/// Record `s := f()` define edges for later return-type propagation.
fn collect_go_assign_refs(
    node: Node,
    source: &str,
    references: &mut Vec<ExtractedRef>,
    enclosing: Option<String>,
    ctx: &ExtractContext,
) {
    fn first_ident(n: Node, source: &str) -> Option<String> {
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            if ch.kind() == "identifier" {
                return Some(node_text(ch, source).to_string());
            }
        }
        None
    }
    let mut names = Vec::new();
    let mut callee: Option<String> = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "expression_list" && names.is_empty() {
            let mut c2 = child.walk();
            for id in child.children(&mut c2) {
                if id.kind() == "identifier" {
                    names.push(node_text(id, source).to_string());
                }
            }
        }
        if child.kind() == "expression_list" && callee.is_none() && !names.is_empty() {
            // second list may wrap call
            let mut c2 = child.walk();
            for ch in child.children(&mut c2) {
                if ch.kind() == "call_expression" {
                    callee = first_ident(ch, source);
                }
            }
        }
    }
    let line = ctx.lines.line_of(node.start_byte());
    if let Some(callee) = callee {
        for n in names {
            if !n.is_empty() && n != "_" {
                push_assign(references, n, callee.clone(), line, enclosing.clone());
            }
        }
    }
}

fn go_collect_imports(
    node: Node,
    source: &str,
    ctx: &ExtractContext,
    enclosing: Option<String>,
    references: &mut Vec<ExtractedRef>,
    line: usize,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "import_spec" {
            // also bare import_spec at top? usually under import_declaration
            if child.kind() == "import_declaration" {
                go_collect_imports(child, source, ctx, enclosing.clone(), references, line);
            }
            continue;
        }
        let mut path = String::new();
        let mut alias = None;
        let mut c2 = child.walk();
        for p in child.children(&mut c2) {
            match p.kind() {
                "import_path" | "interpreted_string_literal" | "raw_string_literal" | "string" => {
                    let t = node_text(p, source);
                    path = t
                        .trim_matches(|c| c == '"' || c == '`' || c == '\'')
                        .to_string();
                }
                "package_identifier" | "dot" | "blank_identifier" => {
                    alias = Some(node_text(p, source).to_string());
                }
                _ => {}
            }
        }
        if path.is_empty() {
            continue;
        }
        let name = alias
            .filter(|a| a != "." && a != "_")
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(&path).to_string());
        let resolved = resolve::resolve_go_import(ctx.path, &path, ctx.known_files);
        push_import(
            references,
            name,
            line,
            enclosing.clone(),
            Some(path),
            resolved,
        );
    }
}

// 鈹€鈹€鈹€ Rust 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

fn walk_rust(
    node: Node,
    source: &str,
    parent: Option<String>,
    symbols: &mut Vec<ExtractedSymbol>,
    references: &mut Vec<ExtractedRef>,
    ctx: &ExtractContext,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let mut local_parent = parent.clone();

    let is_fn_scope = matches!(
        kind,
        "function_item" | "function_signature_item" | "closure_expression"
    );
    let saved_var_types = if is_fn_scope {
        Some(ctx.var_types.borrow().clone())
    } else {
        None
    };

    if kind == "function_item" || kind == "function_signature_item" || kind == "closure_expression"
    {
        collect_rust_param_types(node, source, ctx);
    }

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_item" => first_identifier_name(node, source).map(|name| {
            let k = if parent.is_some() {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            (name, k)
        }),
        "struct_item" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Struct)),
        "enum_item" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Enum)),
        "trait_item" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Trait)),
        "type_item" => first_identifier_name(node, source).map(|n| (n, SymbolKind::TypeAlias)),
        "mod_item" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Module)),
        "function_signature_item" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Function))
        }
        _ => None,
    };

    if let Some((name, skind)) = symbol_info {
        let qname = match &parent {
            Some(p) => format!("{p}::{name}"),
            None => name.clone(),
        };
        symbols.push(make_symbol(
            name.clone(),
            qname.clone(),
            skind,
            node,
            parent.clone(),
            ctx,
            source,
        ));
        local_parent = Some(qname);
    }

    // method inside impl
    if kind == "impl_item" {
        if let Some(ty) = rust_impl_type_name(node, source) {
            local_parent = Some(ty);
        }
    }

    match kind {
        "call_expression" => {
            if let Some(fn_node) = child_by_field(&node, "function") {
                if let Some((n, q)) =
                    rust_call_target(fn_node, source, local_parent.as_deref(), Some(ctx))
                {
                    push_call_q(
                        references,
                        n,
                        q,
                        ctx.lines.line_of(node.start_byte()),
                        local_parent.clone(),
                    );
                }
            }
        }
        "use_declaration" => {
            let line = ctx.lines.line_of(node.start_byte());
            rust_collect_use(node, source, ctx, local_parent.clone(), references, line);
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_rust(
            child,
            source,
            local_parent.clone(),
            symbols,
            references,
            ctx,
        );
    }

    if let Some(s) = saved_var_types {
        *ctx.var_types.borrow_mut() = s;
    }
}

fn rust_impl_type_name(node: Node, source: &str) -> Option<String> {
    let ty = child_by_field(&node, "type")?;
    let t = node_text(ty, source);
    let base = t.split('<').next().unwrap_or(t).trim();
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

/// Type-aware Rust call target: `Foo::bar` �?(bar, Foo); `x.bar` �?(bar, inferred type or impl parent).
fn rust_call_target(
    node: Node,
    source: &str,
    enclosing: Option<&str>,
    ctx: Option<&ExtractContext>,
) -> Option<(String, Option<String>)> {
    match node.kind() {
        "identifier" => Some((node_text(node, source).to_string(), None)),
        "scoped_identifier" => {
            let name = child_by_field(&node, "name")?;
            let name_t = node_text(name, source).to_string();
            let path = child_by_field(&node, "path")?;
            let q = node_text(path, source).to_string();
            let q = q.split('<').next().unwrap_or(&q).trim().to_string();
            if q.is_empty() {
                Some((name_t, None))
            } else {
                Some((name_t, Some(q)))
            }
        }
        "field_expression" => {
            let f = child_by_field(&node, "field")?;
            let name = node_text(f, source).to_string();
            if let Some(val) = child_by_field(&node, "value") {
                let vt = node_text(val, source);
                if vt == "self" || vt == "Self" {
                    // enclosing is typically `Type::method` — take Type (parent of last segment).
                    let q = enclosing.and_then(|e| {
                        let parent = e.rsplit_once("::").map(|(p, _)| p).unwrap_or(e);
                        let base = parent.rsplit("::").next().unwrap_or(parent);
                        if base.is_empty() {
                            None
                        } else {
                            Some(base.to_string())
                        }
                    });
                    return Some((name, q));
                }
                if vt.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    if let Some(ctx) = ctx {
                        if let Some(ty) = ctx.var_types.borrow().get(vt).cloned() {
                            return Some((name, Some(ty)));
                        }
                    }
                    return Some((name, Some(vt.to_string())));
                }
            }
            Some((name, None))
        }
        _ => {
            let mut last = None;
            fn scan(n: Node, src: &str, last: &mut Option<String>) {
                if matches!(n.kind(), "identifier" | "field_identifier") {
                    *last = Some(node_text(n, src).to_string());
                }
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    scan(ch, src, last);
                }
            }
            scan(node, source, &mut last);
            last.map(|n| (n, None))
        }
    }
}

/// `fn f(c: Client)` �?var_types[c]=Client
fn collect_rust_param_types(node: Node, source: &str, ctx: &ExtractContext) {
    let Some(params) = child_by_field(&node, "parameters") else {
        return;
    };
    let mut cursor = params.walk();
    for p in params.children(&mut cursor) {
        if p.kind() != "parameter" {
            continue;
        }
        let mut name = String::new();
        let mut ty = String::new();
        let mut c2 = p.walk();
        for part in p.children(&mut c2) {
            match part.kind() {
                "identifier" => {
                    if name.is_empty() {
                        name = node_text(part, source).to_string();
                    }
                }
                "type_identifier" | "primitive_type" => {
                    ty = node_text(part, source).to_string();
                }
                "reference_type" => {
                    if let Some(inner) = child_by_field(&part, "type") {
                        let t = node_text(inner, source);
                        let t = t.split('<').next().unwrap_or(t).trim();
                        if !t.is_empty() {
                            ty = t.to_string();
                        }
                    }
                }
                _ => {}
            }
        }
        if !name.is_empty() && !ty.is_empty() && name != "self" {
            ctx.var_types.borrow_mut().insert(name, ty);
        }
    }
}

fn rust_collect_use(
    node: Node,
    source: &str,
    ctx: &ExtractContext,
    enclosing: Option<String>,
    references: &mut Vec<ExtractedRef>,
    line: usize,
) {
    // Collect all path segments and leaf names under use_declaration
    fn walk(n: Node, source: &str, paths: &mut Vec<(String, String)>) {
        // (full_path, leaf_name)
        match n.kind() {
            "scoped_identifier" => {
                let text = node_text(n, source).to_string();
                let leaf = child_by_field(&n, "name")
                    .map(|x| node_text(x, source).to_string())
                    .unwrap_or_else(|| text.rsplit("::").next().unwrap_or(&text).to_string());
                paths.push((text, leaf));
            }
            "identifier" => {
                // only if parent isn't already collecting as scoped
            }
            _ => {}
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            walk(ch, source, paths);
        }
    }
    let mut found: Vec<(String, String)> = Vec::new();
    walk(node, source, &mut found);

    // Also handle use crate::foo::{Bar, Baz}
    fn collect_leaves(n: Node, source: &str, prefix: &str, out: &mut Vec<(String, String)>) {
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            match ch.kind() {
                "identifier" => {
                    let name = node_text(ch, source).to_string();
                    let full = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}::{name}")
                    };
                    out.push((full, name));
                }
                "scoped_identifier" => {
                    let text = node_text(ch, source).to_string();
                    let leaf = child_by_field(&ch, "name")
                        .map(|x| node_text(x, source).to_string())
                        .unwrap_or_else(|| text.rsplit("::").next().unwrap_or(&text).to_string());
                    out.push((text, leaf));
                }
                "use_list" | "scoped_use_list" => {
                    let p = if ch.kind() == "scoped_use_list" {
                        child_by_field(&ch, "path")
                            .map(|x| node_text(x, source).to_string())
                            .unwrap_or_else(|| prefix.to_string())
                    } else {
                        prefix.to_string()
                    };
                    collect_leaves(ch, source, &p, out);
                }
                "use_wildcard" => {}
                _ => collect_leaves(ch, source, prefix, out),
            }
        }
    }
    collect_leaves(node, source, "", &mut found);

    // dedup by leaf name
    let mut seen = HashSet::new();
    for (full, leaf) in found {
        if !seen.insert(leaf.clone()) {
            continue;
        }
        let resolved = resolve::resolve_rust_use(ctx.path, &full, ctx.known_files);
        push_import(
            references,
            leaf,
            line,
            enclosing.clone(),
            Some(full),
            resolved,
        );
    }
}
