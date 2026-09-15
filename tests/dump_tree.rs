use agentgraph::index::parser::parse;
use agentgraph::model::Language;

/// Smoke: TS import parses and has import_statement under program.
#[test]
fn ts_import_tree_has_import_statement() {
    let src = "import { createUser } from \"./auth\";\n";
    let tree = parse(src, Language::TypeScript).unwrap();
    let root = tree.root_node();
    assert_eq!(root.kind(), "program");
    let mut c = root.walk();
    let kinds: Vec<&str> = root.children(&mut c).map(|n| n.kind()).collect();
    assert!(
        kinds.contains(&"import_statement"),
        "expected import_statement, got {kinds:?}"
    );
}
