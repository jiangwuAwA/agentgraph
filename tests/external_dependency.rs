//! H2: external / `node_modules` packages stay module-only (honest non-goal).
//!
//! - Workspace packages still link via package map.
//! - External package imports (e.g. `@ant-design/icons`) keep `module` only;
//!   `resolved` stays null; `external_dependency=true` in importers payload.
//! - **Never** claim a complete npm graph or invent file edges.

use agentgraph::index::store::Store as AgStore;
use std::path::PathBuf;
use std::process::{Command, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(tag: &str) -> PathBuf {
    common::temp_root(&format!("agentgraph-extdep-{tag}"))
}

fn run_raw(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn write_frontend(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src/config")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"stock-trading-app","version":"0.1.0"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/app.ts"),
        r#"
import { Icon } from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import { chartTheme } from "@/config/chartTheme";

export function main() {
  void Icon;
  void useQuery;
  void chartTheme;
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/config/chartTheme.ts"),
        "export const chartTheme = 1;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"] } } }"#,
    )
    .unwrap();
}

#[test]
fn external_package_imports_stay_module_only() {
    let base = temp_dir("ext");
    let fe = base.join("packages/frontend");
    write_frontend(&fe);
    let db = base.join("ws.db");
    let idx = run_raw(&[
        "index",
        "--workspace-root",
        fe.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(idx.status.success(), "index: {}", stderr(&idx));

    let store = AgStore::open(&db).expect("open");
    let aliases = store.package_aliases_meta().expect("aliases");

    // Workspace wildcard still resolves.
    let ws_rows = store
        .importers_of_package("@/config/chartTheme", 20)
        .expect("ws importers");
    assert!(
        ws_rows.iter().any(|r| r.resolved.is_some()),
        "workspace package must still resolve: {ws_rows:?}"
    );

    // External packages: module listed, no invented file, external_dependency=true.
    for pkg in ["@ant-design/icons", "@tanstack/react-query"] {
        let rows = store.importers_of_package(pkg, 20).expect("ext importers");
        assert!(
            rows.iter().any(|r| r.path.contains("app")),
            "{pkg} must list module importers: {rows:?}"
        );
        for r in &rows {
            if !r.path.contains("app") {
                continue;
            }
            assert_eq!(
                r.module.as_deref(),
                Some(pkg),
                "external import keeps module name"
            );
            assert!(
                r.resolved.is_none(),
                "{pkg} must not invent resolved file: {:?}",
                r.resolved
            );
            assert!(
                r.external_dependency,
                "{pkg} must be marked external_dependency=true: {r:?}"
            );
        }
    }

    // Alias map must not invent keys for external packages.
    assert!(
        !aliases
            .keys()
            .any(|k| k.starts_with("@ant-design") || k.starts_with("@tanstack")),
        "external pkgs must not enter workspace alias map: {:?}",
        aliases.keys().collect::<Vec<_>>()
    );
}

#[test]
fn mark_external_helper_unit() {
    use agentgraph::index::workspace::PackageAliasMap;
    use agentgraph::model::{Confidence, EdgeKind, ReferenceRecord};

    let aliases = PackageAliasMap::new();
    let mut rows = vec![
        ReferenceRecord {
            name: "Icon".into(),
            kind: EdgeKind::Import,
            path: "src/app.ts".into(),
            line: 2,
            enclosing: None,
            module: Some("@ant-design/icons".into()),
            resolved: None,
            qualifier: None,
            confidence: Confidence::Exact,
            evidence: None,
            root_id: String::new(),
            external_dependency: false,
        },
        ReferenceRecord {
            name: "local".into(),
            kind: EdgeKind::Import,
            path: "src/app.ts".into(),
            line: 3,
            enclosing: None,
            module: Some("./config/chartTheme".into()),
            resolved: None,
            qualifier: None,
            confidence: Confidence::Exact,
            evidence: None,
            root_id: String::new(),
            external_dependency: false,
        },
    ];
    agentgraph::index::store::mark_external_package_rows(&mut rows, &aliases);
    assert!(rows[0].external_dependency, "npm package is external");
    assert!(
        !rows[1].external_dependency,
        "relative import is not external npm"
    );
}
