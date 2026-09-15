use anyhow::Result;
use std::collections::HashSet;
use tree_sitter::Node;

use super::parser::LineIndex;
use super::resolve;
use crate::model::{EdgeKind, Language, SymbolKind};

#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub start_line: usize,
    pub end_line: usize,
    pub parent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedRef {
    pub name: String,
    pub kind: EdgeKind,
    pub line: usize,
    pub enclosing: Option<String>,
    pub module: Option<String>,
    pub resolved: Option<String>,
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
    };

    match lang {
        Language::TypeScript
        | Language::Tsx
        | Language::JavaScript
        | Language::Jsx => walk_ts(root, source, None, &mut symbols, &mut references, &ctx),
        Language::Python => walk_py(root, source, None, &mut symbols, &mut references, &ctx),
        Language::Go => walk_go(root, source, None, &mut symbols, &mut references, &ctx),
        Language::Rust => walk_rust(root, source, None, &mut symbols, &mut references, &ctx),
    }

    Ok(ExtractedFile { symbols, references })
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
    references.push(ExtractedRef {
        name,
        kind: EdgeKind::Call,
        line,
        enclosing,
        module: None,
        resolved: None,
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
    });
}

// ─── TypeScript ───────────────────────────────────────────────────────────────

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

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_declaration" | "generator_function_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Function))
        }
        "class_declaration" | "class" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Class))
        }
        "interface_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Interface))
        }
        "type_alias_declaration" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::TypeAlias))
        }
        "method_definition" => {
            first_identifier_name(node, source).map(|n| (n, SymbolKind::Method))
        }
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
        let start_line = ctx.lines.line_of(node.start_byte());
        let end_line = ctx.lines.line_of(node.end_byte());
        symbols.push(ExtractedSymbol {
            name: name.clone(),
            qualified_name: qname.clone(),
            kind: skind,
            start_line,
            end_line,
            parent: parent.clone(),
        });
        local_parent = Some(qname);
    }

    match kind {
        "call_expression" | "new_expression" => {
            if let Some(fn_node) = child_by_field(&node, "function") {
                if let Some(n) = call_target_name(fn_node, source) {
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
            let specifier = ts_import_specifier(node, source);
            let resolved = specifier.as_ref().and_then(|s| {
                resolve::resolve_typescript_import(ctx.path, s, ctx.known_files)
            });
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
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_ts(child, source, local_parent.clone(), symbols, references, ctx);
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

fn call_target_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "property_identifier" => Some(node_text(node, source).to_string()),
        "member_expression" => {
            let prop = child_by_field(&node, "property")?;
            Some(node_text(prop, source).to_string())
        }
        "parenthesized_expression" => {
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if let Some(n) = call_target_name(ch, source) {
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
            last
        }
    }
}

fn collect_import_names(node: Node, source: &str, sink: &mut impl FnMut(String)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_clause" => {
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
            _ => {}
        }
    }
}

// ─── Python ───────────────────────────────────────────────────────────────────

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
        let start_line = ctx.lines.line_of(node.start_byte());
        let end_line = ctx.lines.line_of(node.end_byte());
        symbols.push(ExtractedSymbol {
            name: name.clone(),
            qualified_name: qname.clone(),
            kind: skind,
            start_line,
            end_line,
            parent: parent.clone(),
        });
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
            collect_py_import_statement(node, source, 0, ctx, local_parent.clone(), references, line);
        }
        "import_from_statement" => {
            let line = ctx.lines.line_of(node.start_byte());
            let level = py_import_level(node, source);
            let module = py_import_module(node, source);
            let resolved = module.as_ref().and_then(|m| {
                resolve::resolve_python_import(ctx.path, m, level, ctx.known_files)
            });
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
        walk_py(child, source, local_parent.clone(), symbols, references, ctx);
    }
}

fn py_import_level(node: Node, source: &str) -> usize {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import_from" || child.kind() == "relative_import" || child.kind() == "import_prefix" {
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
            push_import(references, last, line, enclosing.clone(), Some(t.to_string()), resolved);
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
                module.as_ref().map(|m| m.rsplit('.').next().unwrap_or(m).to_string())
            });
            if let Some(name) = name {
                let resolved = module.as_ref().and_then(|m| {
                    resolve::resolve_python_import(ctx.path, m, 0, ctx.known_files)
                });
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

// ─── Go ───────────────────────────────────────────────────────────────────────

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

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_declaration" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Function)),
        "method_declaration" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Method)),
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
        let start_line = ctx.lines.line_of(node.start_byte());
        let end_line = ctx.lines.line_of(node.end_byte());
        symbols.push(ExtractedSymbol {
            name: name.clone(),
            qualified_name: qname.clone(),
            kind: skind,
            start_line,
            end_line,
            parent: parent_for_qname.clone(),
        });
        local_parent = Some(qname);
    }

    match kind {
        "call_expression" => {
            if let Some(fn_node) = child_by_field(&node, "function") {
                if let Some(n) = go_call_name(fn_node, source) {
                    push_call(
                        references,
                        n,
                        ctx.lines.line_of(node.start_byte()),
                        local_parent.clone(),
                    );
                }
            }
        }
        "import_declaration" => {
            let line = ctx.lines.line_of(node.start_byte());
            go_collect_imports(node, source, ctx, local_parent.clone(), references, line);
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_go(child, source, local_parent.clone(), symbols, references, ctx);
    }
}

/// Extract Go method receiver type name, e.g. `func (s *Server) Start()` → `Server`.
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

fn go_call_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(node_text(node, source).to_string()),
        "selector_expression" | "field_identifier" => {
            let f = child_by_field(&node, "field")
                .or_else(|| child_by_field(&node, "name"))
                .unwrap_or(node);
            // last identifier
            let mut last = node_text(f, source).to_string();
            if f.kind() != "field_identifier" && f.kind() != "identifier" {
                let mut cursor = f.walk();
                for ch in f.children(&mut cursor) {
                    if ch.kind() == "field_identifier" || ch.kind() == "identifier" {
                        last = node_text(ch, source).to_string();
                    }
                }
            }
            Some(last)
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
            last
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
                    path = t.trim_matches(|c| c == '"' || c == '`' || c == '\'').to_string();
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
            .unwrap_or_else(|| {
                path.rsplit('/').next().unwrap_or(&path).to_string()
            });
        let resolved = resolve::resolve_go_import(ctx.path, &path, ctx.known_files);
        push_import(references, name, line, enclosing.clone(), Some(path), resolved);
    }
}

// ─── Rust ─────────────────────────────────────────────────────────────────────

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

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_item" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Function)),
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
        let start_line = ctx.lines.line_of(node.start_byte());
        let end_line = ctx.lines.line_of(node.end_byte());
        symbols.push(ExtractedSymbol {
            name: name.clone(),
            qualified_name: qname.clone(),
            kind: skind,
            start_line,
            end_line,
            parent: parent.clone(),
        });
        local_parent = Some(qname);
    }

    // method inside impl
    if kind == "impl_item" {
        // don't change parent for symbols inside �?methods will nest under previous parent
        // better: use impl type name as parent
        if let Some(ty) = rust_impl_type_name(node, source) {
            local_parent = Some(ty);
        }
    }

    match kind {
        "call_expression" => {
            if let Some(fn_node) = child_by_field(&node, "function") {
                if let Some(n) = rust_call_name(fn_node, source) {
                    push_call(
                        references,
                        n,
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
        walk_rust(child, source, local_parent.clone(), symbols, references, ctx);
    }
}

fn rust_impl_type_name(node: Node, source: &str) -> Option<String> {
    let ty = child_by_field(&node, "type")?;
    let t = node_text(ty, source);
    // strip generics
    let base = t.split('<').next().unwrap_or(t).trim();
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

fn rust_call_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" => Some(node_text(node, source).to_string()),
        "scoped_identifier" => {
            let name = child_by_field(&node, "name")?;
            Some(node_text(name, source).to_string())
        }
        "field_expression" => {
            let f = child_by_field(&node, "field")?;
            Some(node_text(f, source).to_string())
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
            last
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
                    .unwrap_or_else(|| {
                        text.rsplit("::").next().unwrap_or(&text).to_string()
                    });
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
                        .unwrap_or_else(|| {
                            text.rsplit("::").next().unwrap_or(&text).to_string()
                        });
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
