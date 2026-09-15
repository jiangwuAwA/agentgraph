use anyhow::Result;
use tree_sitter::Node;

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
}

#[derive(Debug, Clone)]
pub struct ExtractedFile {
    pub symbols: Vec<ExtractedSymbol>,
    pub references: Vec<ExtractedRef>,
}

pub fn extract_file(source: &str, lang: Language, _path: &str) -> Result<ExtractedFile> {
    let tree = super::parser::parse(source, lang)?;
    let root = tree.root_node();
    let mut symbols = Vec::new();
    let mut references = Vec::new();

    match lang {
        Language::TypeScript => walk_ts(root, source, None, &mut symbols, &mut references),
        Language::Python => walk_py(root, source, None, &mut symbols, &mut references),
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
    // Prefer field "name"
    if let Some(n) = child_by_field(&node, "name") {
        let t = node_text(n, source);
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    // Fallback: first identifier-like child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" | "type_identifier" | "property_identifier" | "shorthand_property_identifier"
            | "string_fragment" => {
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

fn walk_ts(
    node: Node,
    source: &str,
    parent: Option<String>,
    symbols: &mut Vec<ExtractedSymbol>,
    references: &mut Vec<ExtractedRef>,
) {
    let mut cursor = node.walk();
    let kind = node.kind();

    let mut local_parent = parent.clone();

    // --- symbols ---
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
        "lexical_declaration" | "variable_declaration" => {
            // const foo = () => {} / function assigned
            extract_var_function(node, source)
        }
        _ => None,
    };

    if let Some((name, skind)) = symbol_info {
        let qname = match &parent {
            Some(p) if skind == SymbolKind::Method => format!("{p}.{name}"),
            Some(p) if skind != SymbolKind::Class && skind != SymbolKind::Function => format!("{p}.{name}"),
            _ => name.clone(),
        };
        let start_line = super::parser::line_of(node.start_byte(), source);
        let end_line = super::parser::line_of(node.end_byte(), source);
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

    // --- references ---
    match kind {
        "call_expression" | "new_expression" => {
            if let Some(fn_node) = child_by_field(&node, "function") {
                if let Some(n) = call_target_name(fn_node, source) {
                    references.push(ExtractedRef {
                        name: n,
                        kind: EdgeKind::Call,
                        line: super::parser::line_of(node.start_byte(), source),
                        enclosing: local_parent.clone(),
                    });
                }
            }
        }
        "import_statement" => {
            // Collect imported bindings as import refs (resolves via export names later)
            collect_import_names(node, source, &mut |name| {
                references.push(ExtractedRef {
                    name,
                    kind: EdgeKind::Import,
                    line: super::parser::line_of(node.start_byte(), source),
                    enclosing: local_parent.clone(),
                });
            });
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_ts(child, source, local_parent.clone(), symbols, references);
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
            // a.b.c -> take last property (c) AND also record full path as last
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
            // generic fallback: last identifier in subtree
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
            "source" => {}
            _ => {}
        }
    }
}

fn walk_py(
    node: Node,
    source: &str,
    parent: Option<String>,
    symbols: &mut Vec<ExtractedSymbol>,
    references: &mut Vec<ExtractedRef>,
) {
    let mut cursor = node.walk();
    let kind = node.kind();
    let mut local_parent = parent.clone();

    let symbol_info: Option<(String, SymbolKind)> = match kind {
        "function_definition" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Function)),
        "class_definition" => first_identifier_name(node, source).map(|n| (n, SymbolKind::Class)),
        _ => None,
    };

    if let Some((name, skind)) = symbol_info {
        let qname = match &parent {
            Some(p) if skind == SymbolKind::Function || skind == SymbolKind::Method => {
                format!("{p}.{name}")
            }
            Some(p) => format!("{p}.{name}"),
            None => name.clone(),
        };
        // Python methods: function inside a class
        let skind = if skind == SymbolKind::Function && parent.is_some() {
            // parent may be class — treat as method if parent was class
            // We don't have parent kind here easily; keep function, qualified name still works.
            skind
        } else {
            skind
        };
        let start_line = super::parser::line_of(node.start_byte(), source);
        let end_line = super::parser::line_of(node.end_byte(), source);
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
                    references.push(ExtractedRef {
                        name: n,
                        kind: EdgeKind::Call,
                        line: super::parser::line_of(node.start_byte(), source),
                        enclosing: local_parent.clone(),
                    });
                }
            }
        }
        "import_statement" | "import_from_statement" => {
            collect_py_imports(node, source, &mut |name| {
                references.push(ExtractedRef {
                    name,
                    kind: EdgeKind::Import,
                    line: super::parser::line_of(node.start_byte(), source),
                    enclosing: local_parent.clone(),
                });
            });
        }
        _ => {}
    }

    for child in node.children(&mut cursor) {
        walk_py(child, source, local_parent.clone(), symbols, references);
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

fn collect_py_imports(node: Node, source: &str, sink: &mut impl FnMut(String)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                let t = node_text(child, source);
                // last component
                if let Some(last) = t.rsplit('.').next() {
                    sink(last.to_string());
                }
            }
            "aliased_import" => {
                if let Some(n) = first_identifier_name(child, source) {
                    sink(n);
                }
            }
            "relative_import" | "import_prefix" | "module_name" => {
                let t = node_text(child, source);
                if let Some(last) = t.rsplit('.').next() {
                    if !last.is_empty() {
                        sink(last.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}
