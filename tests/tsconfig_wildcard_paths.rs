//! F6: tsconfig wildcard path aliases (`@/*` -> `src/*`) resolve to concrete files.
//!
//! Contract:
//! - Limited extension table only (`.ts` / `.tsx` / `.d.ts` / `.js` / `.jsx` + `/index.*`).
//! - On success: `resolved` points at a file that exists.
//! - On failure: `resolved` stays null — never invent a path.

use agentgraph::index::resolve::{package_resolve_display, resolve_package_import};
use agentgraph::index::store::Store as AgStore;
use agentgraph::index::workspace::{
    discover_package_aliases, PackageAliasEntry, PackageAliasMap, WorkspaceRoot,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(tag: &str) -> PathBuf {
    common::temp_root(&format!("agentgraph-ts-wildcard-{tag}"))
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

fn alias_entry(
    root_id: &str,
    entry: Option<&str>,
    root_path: Option<&str>,
    source: &str,
) -> PackageAliasEntry {
    PackageAliasEntry {
        root_id: root_id.to_string(),
        entry: entry.map(|s| s.to_string()),
        root_path: root_path.map(|s| s.to_string()),
        source: source.to_string(),
    }
}

/// Frontend fixture: tsconfig `@/*` -> `src/*` + real files under `src/`.
fn write_frontend(root: &Path) {
    std::fs::create_dir_all(root.join("src/config")).unwrap();
    std::fs::create_dir_all(root.join("src/utils")).unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
          "compilerOptions": {
            "baseUrl": ".",
            "paths": { "@/*": ["src/*"] }
          },
          "include": ["src"]
        }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"stock-trading-app","version":"0.1.0"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/config/chartTheme.ts"),
        "export const chartTheme = 1;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/utils/format.tsx"),
        "export function format(n: number): string { return String(n); }\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src/utils/helpers")).unwrap();
    std::fs::write(
        root.join("src/utils/helpers/index.ts"),
        "export const help = 1;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/app.ts"),
        r#"
import { chartTheme } from "@/config/chartTheme";
import { format } from "@/utils/format";
import { help } from "@/utils/helpers";

export function main(): void {
  void chartTheme;
  void format(1);
  void help;
}
"#,
    )
    .unwrap();
}

fn wildcard_map(fe: &Path) -> PackageAliasMap {
    let roots = vec![WorkspaceRoot {
        id: "frontend".into(),
        path: fe.to_path_buf(),
    }];
    discover_package_aliases(&roots, &[])
}

// ---------------------------------------------------------------------------
// Unit: limited extension table + no invented paths
// ---------------------------------------------------------------------------

#[test]
fn tsconfig_wildcard_resolves_existing_ts_file() {
    let base = temp_dir("ts");
    let fe = base.join("packages/frontend");
    write_frontend(&fe);
    let aliases = wildcard_map(&fe);
    assert!(
        aliases.contains_key("@/*"),
        "tsconfig @/* must appear in alias map: {:?}",
        aliases.keys().collect::<Vec<_>>()
    );

    let hit = resolve_package_import("@/config/chartTheme", &aliases)
        .expect("wildcard alias must match @/config/chartTheme");
    assert_eq!(hit.root_id, "frontend");
    let wf = hit.workspace_file.clone().unwrap_or_default();
    let ent = hit.entry.clone().unwrap_or_default();
    let joined = format!("{wf} {ent}");
    assert!(
        joined.ends_with(".ts") || joined.contains("chartTheme.ts"),
        "resolved must be a concrete .ts file, got workspace_file={wf:?} entry={ent:?}"
    );
    assert!(
        joined.contains("src/config/chartTheme.ts"),
        "must map through src/* to src/config/chartTheme.ts, got {joined}"
    );
    // Display must be the concrete file (not the `src/*` pattern).
    let disp = package_resolve_display(&hit);
    assert!(
        disp.contains("chartTheme.ts") && !disp.contains('*'),
        "display must not keep wildcard pattern: {disp}"
    );
}

#[test]
fn tsconfig_wildcard_resolves_tsx_and_index_dir() {
    let base = temp_dir("tsx-index");
    let fe = base.join("packages/frontend");
    write_frontend(&fe);
    let aliases = wildcard_map(&fe);

    let tsx = resolve_package_import("@/utils/format", &aliases).expect("tsx hit");
    let joined = format!(
        "{} {}",
        tsx.workspace_file.clone().unwrap_or_default(),
        tsx.entry.clone().unwrap_or_default()
    );
    assert!(
        joined.contains("src/utils/format.tsx"),
        "limited table must try .tsx: {joined}"
    );

    let idx = resolve_package_import("@/utils/helpers", &aliases).expect("index hit");
    let joined = format!(
        "{} {}",
        idx.workspace_file.clone().unwrap_or_default(),
        idx.entry.clone().unwrap_or_default()
    );
    assert!(
        joined.contains("src/utils/helpers/index.ts"),
        "directory import must probe /index.ts: {joined}"
    );
}

#[test]
fn tsconfig_wildcard_missing_file_does_not_invent_path() {
    let base = temp_dir("missing");
    let fe = base.join("packages/frontend");
    write_frontend(&fe);
    let aliases = wildcard_map(&fe);

    let hit = resolve_package_import("@/nope/gone", &aliases)
        .expect("prefix alias still matches for root link");
    assert_eq!(hit.root_id, "frontend");
    assert!(
        hit.workspace_file.is_none(),
        "must not invent workspace_file for missing leaf, got {:?}",
        hit.workspace_file
    );
    assert!(
        hit.entry.is_none(),
        "must not invent entry for missing leaf, got {:?}",
        hit.entry
    );
    // Root-link display remains queryable without a fake file.
    let disp = package_resolve_display(&hit);
    assert!(
        disp.contains("frontend") && !disp.contains("src/nope"),
        "display must be root link only, got {disp}"
    );
}

#[test]
fn tsconfig_wildcard_unverified_root_path_does_not_invent_file() {
    // Relative root_path cannot be probed — never invent `src/foo.ts`.
    let mut map = PackageAliasMap::new();
    map.insert(
        "@/*".to_string(),
        alias_entry(
            "frontend",
            Some("src/*"),
            Some("packages/frontend"),
            "tsconfig_paths",
        ),
    );
    let hit = resolve_package_import("@/config/chartTheme", &map).expect("prefix hit");
    assert_eq!(hit.root_id, "frontend");
    assert!(
        hit.workspace_file.is_none(),
        "unverified root must not invent file: {:?}",
        hit.workspace_file
    );
}

// ---------------------------------------------------------------------------
// Integration: index-time link sets refs.resolved for wildcard imports
// ---------------------------------------------------------------------------

#[test]
fn e2e_wildcard_imports_get_resolved_concrete_file() {
    let base = temp_dir("e2e");
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
    assert!(idx.status.success(), "index failed: {}", stderr(&idx));

    let store = AgStore::open(&db).expect("open store");
    let aliases = store.package_aliases_meta().expect("aliases");
    assert!(
        aliases.contains_key("@/*"),
        "meta must keep @/* wildcard: {:?}",
        aliases.keys().collect::<Vec<_>>()
    );

    // importers @/config/chartTheme must list app.ts with a concrete resolved file.
    let rows = store
        .importers_of_package("@/config/chartTheme", 50)
        .expect("importers_of_package");
    assert!(
        rows.iter().any(|r| r.path.contains("app")),
        "importers @/config/chartTheme must list app.ts: {rows:?}"
    );
    let linked = rows
        .iter()
        .find(|r| r.path.contains("app"))
        .expect("app importer row");
    let resolved = linked
        .resolved
        .clone()
        .unwrap_or_else(|| panic!("resolved must be set for existing wildcard file: {linked:?}"));
    assert!(
        resolved.contains("chartTheme.ts"),
        "resolved must be concrete chartTheme.ts, got {resolved}"
    );
    assert!(
        !resolved.contains('*') && !resolved.ends_with("chartTheme"),
        "resolved must include extension (no invented extension-less path): {resolved}"
    );
}

#[test]
fn e2e_wildcard_missing_leaf_leaves_resolved_null() {
    let base = temp_dir("e2e-missing");
    let fe = base.join("packages/frontend");
    write_frontend(&fe);
    // Import a leaf that does not exist on disk.
    std::fs::write(
        fe.join("src/app2.ts"),
        r#"import { x } from "@/ghost/missingLeaf";
export const y = x;
"#,
    )
    .unwrap();
    let db = base.join("ws.db");

    let idx = run_raw(&[
        "index",
        "--workspace-root",
        fe.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(idx.status.success(), "index failed: {}", stderr(&idx));

    let store = AgStore::open(&db).expect("open store");
    let rows = store
        .importers_of_package("@/ghost/missingLeaf", 50)
        .expect("importers");
    assert!(
        rows.iter().any(|r| r.path.contains("app2")),
        "module match must still list the importer: {rows:?}"
    );
    for r in &rows {
        if r.path.contains("app2") {
            assert!(
                r.resolved.is_none(),
                "missing leaf must leave resolved null, got {:?}",
                r.resolved
            );
        }
    }
}

/// Non-regression: exact package alias still resolves barrel without probing.
#[test]
fn exact_package_alias_barrel_unchanged() {
    let mut map = PackageAliasMap::new();
    map.insert(
        "@demo/registry".to_string(),
        alias_entry(
            "registry",
            Some("src/index.ts"),
            Some("packages/registry"),
            "package.json",
        ),
    );
    let hit = resolve_package_import("@demo/registry", &map).expect("exact hit");
    assert_eq!(hit.root_id, "registry");
    assert_eq!(hit.entry.as_deref(), Some("src/index.ts"));
    assert_eq!(
        hit.workspace_file.as_deref(),
        Some("packages/registry/src/index.ts")
    );
}
