//! H3: synthetic dirty monorepo fixture — package-map coverage matrix.
//!
//! Public fixture only (`fixtures/eval-package-map-dirty`); no private corpus.
//! Locks: exports subset, wildcard, external module-only, duplicate fail-loud.

use agentgraph::index::store::Store as AgStore;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/eval-package-map-dirty")
}

fn temp_dir(tag: &str) -> PathBuf {
    common::temp_root(&format!("agentgraph-dirty-pkg-{tag}"))
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

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for ent in std::fs::read_dir(src).unwrap() {
        let ent = ent.unwrap();
        let to = dst.join(ent.file_name());
        if ent.file_type().unwrap().is_dir() {
            copy_tree(&ent.path(), &to);
        } else {
            std::fs::copy(ent.path(), &to).unwrap();
        }
    }
}

fn roots_for(base: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("app", base.join("packages/app")),
        ("ui", base.join("packages/ui")),
        ("lib-a", base.join("packages/lib-a")),
        ("lib-b", base.join("packages/lib-b")),
        ("api", base.join("services/api")),
    ]
}

fn index_args(base: &Path, db: &Path, extra: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = vec!["index".into()];
    for (_, p) in roots_for(base) {
        args.push("--workspace-root".into());
        args.push(p.to_str().unwrap().to_string());
    }
    args.push("--workspace-db".into());
    args.push(db.to_str().unwrap().to_string());
    args.push("--force".into());
    for e in extra {
        args.push((*e).into());
    }
    args
}

#[test]
fn dirty_fixture_duplicate_names_fail_loud() {
    let work = temp_dir("dup");
    let base = work.join("tree");
    copy_tree(&fixture_root(), &base);
    let db = work.join("ws.db");
    let args = index_args(&base, &db, &[]);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_raw(&arg_refs);
    assert!(
        !out.status.success(),
        "duplicate @demo/dup must fail-loud: {}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("duplicate") && err.contains("@demo/dup"),
        "error must name duplicate package: {err}"
    );
}

#[test]
fn dirty_fixture_indexes_with_override_and_covers_matrix() {
    let work = temp_dir("cover");
    let base = work.join("tree");
    copy_tree(&fixture_root(), &base);
    let db = work.join("ws.db");
    let args = index_args(&base, &db, &["--workspace-alias", "@demo/dup=lib-a"]);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_raw(&arg_refs);
    assert!(out.status.success(), "index: {}", stderr(&out));

    let store = AgStore::open(&db).expect("open");
    let aliases = store.package_aliases_meta().expect("aliases");

    // exports subset on @demo/ui
    assert!(
        aliases.contains_key("@demo/ui"),
        "exports root key: {:?}",
        aliases.keys().collect::<Vec<_>>()
    );
    assert!(
        aliases.contains_key("@demo/ui/button"),
        "exports subpath key: {:?}",
        aliases.keys().collect::<Vec<_>>()
    );

    let ui_root = store
        .importers_of_package("@demo/ui", 20)
        .expect("ui importers");
    // app imports @ui/button via tsconfig alias, not @demo/ui — check resolve instead
    use agentgraph::index::resolve::resolve_package_import;
    let hit = resolve_package_import("@demo/ui/button", &aliases).expect("ui/button");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(
        joined.contains("src/button.ts"),
        "exports ./button: {joined}"
    );

    let hit = resolve_package_import("@demo/ui", &aliases).expect("ui root");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(joined.contains("src/index.ts"), "exports '.': {joined}");

    // dirty-app exports ./config
    let hit = resolve_package_import("dirty-app/config", &aliases).expect("app/config");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(
        joined.contains("chartTheme"),
        "dirty-app/config exports: {joined}"
    );

    // wildcard still works
    let hit = resolve_package_import("@/config/chartTheme", &aliases).expect("wild");
    let joined = format!(
        "{} {}",
        hit.workspace_file.clone().unwrap_or_default(),
        hit.entry.clone().unwrap_or_default()
    );
    assert!(joined.contains("chartTheme.ts"), "wildcard: {joined}");

    // external package stays module-only
    let ext = store
        .importers_of_package("@ant-design/icons", 20)
        .expect("ext");
    assert!(
        ext.iter()
            .any(|r| r.external_dependency && r.resolved.is_none()),
        "external @ant-design/icons module-only: {ext:?}"
    );

    // override target lib-a owns @demo/dup
    assert_eq!(
        aliases.get("@demo/dup").map(|e| e.root_id.as_str()),
        Some("lib-a")
    );
    let _ = ui_root;
}
