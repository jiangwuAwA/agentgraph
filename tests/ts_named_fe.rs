//! Named function expressions must set enclosing so impact BFS can expand.

use agentgraph::index::extract::extract_file;
use agentgraph::model::Language;
use std::collections::HashSet;

#[test]
fn named_function_expression_sets_enclosing() {
    let src = r#"
const app = {};
app.validateEmail = function validateEmail(email) { return true; };
app.loginHandler = function loginHandler(email) {
  return app.validateEmail(email);
};
app.main = function main() {
  return app.loginHandler("a@b.com");
};
"#;
    let out = extract_file(src, Language::JavaScript, "src/a.js", &HashSet::new()).unwrap();
    let call = out
        .references
        .iter()
        .find(|r| r.name == "loginHandler")
        .expect("call to loginHandler");
    assert_eq!(
        call.enclosing.as_deref(),
        Some("main"),
        "call inside named FE `main` must have enclosing=main; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.enclosing.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        out.symbols.iter().any(|s| s.name == "main"),
        "named FE must be a symbol for BFS expansion"
    );
}
