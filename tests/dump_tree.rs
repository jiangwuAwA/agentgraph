use agentgraph::index::parser::parse;
use agentgraph::model::Language;

fn dump(node: tree_sitter::Node, src: &str, depth: usize) {
    let text = src.get(node.byte_range()).unwrap_or("");
    let short: String = text.chars().take(40).collect();
    println!(
        "{}{} [{}] {:?}",
        "  ".repeat(depth),
        node.kind(),
        node.child_count(),
        short
    );
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        dump(ch, src, depth + 1);
    }
}

#[test]
fn dump_import_tree() {
    let src = "import { createUser } from \"./auth\";\n";
    let tree = parse(src, Language::TypeScript).unwrap();
    dump(tree.root_node(), src, 0);
}
