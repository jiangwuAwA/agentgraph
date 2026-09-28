//! H1: package.json `exports` subset — minimal condition resolution.
//!
//! Contract (partial package map, **not** full npm/TS resolution):
//! - Keys: `"."` and `"./sub"` only (no `*`, no `imports`).
//! - Conditions: first string among `import` / `default` / `require` / `types`.
//! - Priority: exports key (full specifier) → package root → wildcard.
//! - No match → no invented file path.

use agentgraph::index::resolve::resolve_package_import;
use agentgraph::index::workspace::{discover_package_aliases, WorkspaceRoot};
use std::path::{Path, PathBuf};

mod common;

fn temp_dir(tag: &str) -> PathBuf {
    common::temp_root(&format!("agentgraph-exports-{tag}"))
}

fn write_pkg(root: &Path, package_json: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("dist")).unwrap();
    std::fs::write(root.join("package.json"), package_json).unwrap();
    std::fs::write(root.join("src/index.ts"), "export const root = 1;\n").unwrap();
    std::fs::write(root.join("src/client.ts"), "export const client = 1;\n").unwrap();
    std::fs::write(root.join("dist/index.js"), "exports.root = 1;\n").unwrap();
    std::fs::write(root.join("dist/client.js"), "exports.client = 1;\n").unwrap();
}

#[test]
fn exports_dot_and_subpath_map_to_concrete_files() {
    let base = temp_dir("dot-sub");
    let pkg = base.join("packages/lib");
    write_pkg(
        &pkg,
        r#"{
          "name": "@demo/lib",
          "exports": {
            ".": "./src/index.ts",
            "./client": "./src/client.ts"
          }
        }"#,
    );
    let roots = vec![WorkspaceRoot {
        id: "lib".into(),
        path: pkg.clone(),
    }];
    let aliases = discover_package_aliases(&roots, &[]);

    let root_hit = resolve_package_import("@demo/lib", &aliases).expect("root");
    assert_eq!(root_hit.root_id, "lib");
    let joined = format!(
        "{} {}",
        root_hit.workspace_file.clone().unwrap_or_default(),
        root_hit.entry.clone().unwrap_or_default()
    );
    assert!(
        joined.contains("src/index.ts"),
        "exports '.' must map to src/index.ts: {joined}"
    );

    let sub_hit = resolve_package_import("@demo/lib/client", &aliases).expect("sub");
    let joined = format!(
        "{} {}",
        sub_hit.workspace_file.clone().unwrap_or_default(),
        sub_hit.entry.clone().unwrap_or_default()
    );
    assert!(
        joined.contains("src/client.ts"),
        "exports './client' must win over root barrel: {joined}"
    );
    assert!(
        !joined.contains("src/index.ts"),
        "subpath must not fall back to root entry: {joined}"
    );
}

#[test]
fn exports_condition_object_picks_import_then_default() {
    let base = temp_dir("cond");
    let pkg = base.join("packages/lib");
    write_pkg(
        &pkg,
        r#"{
          "name": "@demo/lib",
          "exports": {
            ".": {
              "types": "./src/index.ts",
              "import": "./src/client.ts",
              "default": "./dist/index.js"
            }
          }
        }"#,
    );
    let roots = vec![WorkspaceRoot {
        id: "lib".into(),
        path: pkg.clone(),
    }];
    let aliases = discover_package_aliases(&roots, &[]);
    let hit = resolve_package_import("@demo/lib", &aliases).expect("hit");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    // Prefer import over types/default per H1 order (import/default/require/types).
    // Spec: take first of import/default/require/types — import wins here.
    assert!(
        joined.contains("src/client.ts"),
        "condition import must win: {joined}"
    );
}

#[test]
fn exports_string_form_is_accepted() {
    let base = temp_dir("string");
    let pkg = base.join("packages/lib");
    write_pkg(
        &pkg,
        r#"{ "name": "@demo/lib", "exports": "./dist/index.js" }"#,
    );
    let roots = vec![WorkspaceRoot {
        id: "lib".into(),
        path: pkg.clone(),
    }];
    let aliases = discover_package_aliases(&roots, &[]);
    let hit = resolve_package_import("@demo/lib", &aliases).expect("hit");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(
        joined.contains("dist/index.js"),
        "string exports must map: {joined}"
    );
}

#[test]
fn exports_missing_key_does_not_invent_file() {
    let base = temp_dir("missing");
    let pkg = base.join("packages/lib");
    write_pkg(
        &pkg,
        r#"{
          "name": "@demo/lib",
          "exports": { ".": "./src/index.ts", "./client": "./src/client.ts" }
        }"#,
    );
    let roots = vec![WorkspaceRoot {
        id: "lib".into(),
        path: pkg.clone(),
    }];
    let aliases = discover_package_aliases(&roots, &[]);
    let hit = resolve_package_import("@demo/lib/nope", &aliases).expect("root fallback");
    // No exports key for ./nope → fall back to package root entry (not invent nope.ts).
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(
        !joined.contains("nope"),
        "must not invent exports leaf: {joined}"
    );
}

#[test]
fn exports_wildcard_keys_are_ignored() {
    let base = temp_dir("star");
    let pkg = base.join("packages/lib");
    write_pkg(
        &pkg,
        r#"{
          "name": "@demo/lib",
          "exports": { "./*": "./src/*.ts", ".": "./src/index.ts" }
        }"#,
    );
    let roots = vec![WorkspaceRoot {
        id: "lib".into(),
        path: pkg.clone(),
    }];
    let aliases = discover_package_aliases(&roots, &[]);
    assert!(
        !aliases
            .keys()
            .any(|k| k.contains('*') && k.starts_with("@demo/lib")),
        "exports star keys must not enter alias map: {:?}",
        aliases.keys().collect::<Vec<_>>()
    );
    let hit = resolve_package_import("@demo/lib", &aliases).expect("dot");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(joined.contains("src/index.ts"), "{joined}");
}

/// Non-regression: no exports → barrel discover still works.
#[test]
fn no_exports_falls_back_to_barrel() {
    let base = temp_dir("barrel");
    let pkg = base.join("packages/lib");
    write_pkg(&pkg, r#"{ "name": "@demo/lib" }"#);
    let roots = vec![WorkspaceRoot {
        id: "lib".into(),
        path: pkg.clone(),
    }];
    let aliases = discover_package_aliases(&roots, &[]);
    let hit = resolve_package_import("@demo/lib", &aliases).expect("hit");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(joined.contains("index"), "barrel fallback: {joined}");
}
