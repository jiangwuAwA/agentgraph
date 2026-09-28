//! Next-cut A: cross-root package-name import 鈫?symbol/file connection.
//!
//! TDD contract (docs/workspace.md):
//! - Workspace package alias map discovered at index time
//!   (CLI `--workspace-alias`, package.json `name`, optional tsconfig paths).
//! - `import { X } from "@demo/registry"` links to the registry root barrel/symbols.
//! - Honesty: **partial package map, not full TypeScript resolution**.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentgraph::index::resolve::{resolve_package_import, split_package_specifier};
use agentgraph::index::store::Store as AgStore;
use agentgraph::index::workspace::{
    discover_package_aliases, package_aliases_from_meta, package_aliases_to_meta, parse_alias_flag,
    PackageAliasEntry, PackageAliasMap, WorkspaceRoot,
};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(name: &str) -> PathBuf {
    common::temp_root(&format!("agentgraph-ws-alias-{name}"))
}

fn run_raw(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn parse_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).unwrap_or_else(|e| {
        panic!(
            "invalid JSON: {e}\nstdout={}\nstderr={}",
            stdout(out),
            stderr(out)
        )
    })
}

fn write_registry(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "@demo/registry", "version": "0.0.1" }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/client.ts"),
        r#"
export class RegistryClient {
  fetch(id: string): string { return id; }
}
export function createClient(): RegistryClient {
  return new RegistryClient();
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        r#"export { RegistryClient, createClient } from "./client";
"#,
    )
    .unwrap();
}

fn write_service(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "@demo/service", "version": "0.0.1" }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/order.service.ts"),
        r#"
import { RegistryClient } from "@demo/registry";

export class OrderService {
  constructor(private client: RegistryClient) {}
  load(id: string) { return this.client.fetch(id); }
}
"#,
    )
    .unwrap();
}

/// Two-root fixture: packages/registry + packages/service under one workspace.
fn write_demo_workspace(base: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let registry = base.join("packages/registry");
    let service = base.join("packages/service");
    std::fs::create_dir_all(&registry).unwrap();
    std::fs::create_dir_all(&service).unwrap();
    write_registry(&registry);
    write_service(&service);
    let db = base.join("ws.db");
    (registry, service, db)
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

// ---------------------------------------------------------------------------
// Unit: pure resolve (package specifier + workspace aliases 鈫?root / file)
// ---------------------------------------------------------------------------

#[test]
fn resolve_package_import_maps_to_workspace_file_and_root_id() {
    let mut aliases = PackageAliasMap::new();
    aliases.insert(
        "@demo/registry".to_string(),
        alias_entry(
            "registry",
            Some("src/index.ts"),
            Some("packages/registry"),
            "tsconfig_paths",
        ),
    );
    let hit = resolve_package_import("@demo/registry", &aliases)
        .expect("@demo/registry must resolve via workspace aliases");
    assert_eq!(hit.root_id, "registry");
    assert_eq!(hit.entry.as_deref(), Some("src/index.ts"));
    assert_eq!(
        hit.workspace_file.as_deref(),
        Some("packages/registry/src/index.ts"),
        "workspace-relative barrel path required for cross-root file connection"
    );
}

#[test]
fn resolve_package_import_subpath_strips_to_package() {
    let mut aliases = PackageAliasMap::new();
    aliases.insert(
        "@demo/registry".to_string(),
        alias_entry(
            "registry",
            Some("src/index.ts"),
            Some("packages/registry"),
            "package.json",
        ),
    );
    let (pkg, sub) = split_package_specifier("@demo/registry/client");
    assert_eq!(pkg, "@demo/registry");
    assert_eq!(sub, Some("client"));
    let hit = resolve_package_import("@demo/registry/client", &aliases).expect("subpath");
    assert_eq!(hit.package, "@demo/registry");
    assert_eq!(hit.root_id, "registry");
}

#[test]
fn resolve_package_import_unknown_package_is_none() {
    let aliases = PackageAliasMap::new();
    assert!(resolve_package_import("@other/pkg", &aliases).is_none());
    // Relative imports are not package aliases.
    let mut al = PackageAliasMap::new();
    al.insert(
        "@demo/registry".to_string(),
        alias_entry("registry", None, None, "cli"),
    );
    assert!(resolve_package_import("./client", &al).is_none());
}

#[test]
fn parse_alias_flag_accepts_pkg_eq_target() {
    let (pkg, target) = parse_alias_flag("@demo/registry=registry").expect("parse");
    assert_eq!(pkg, "@demo/registry");
    assert_eq!(target, "registry");
    let (pkg2, target2) = parse_alias_flag("@demo/registry=./packages/registry").expect("parse2");
    assert_eq!(pkg2, "@demo/registry");
    assert_eq!(target2, "./packages/registry");
    assert!(parse_alias_flag("no-equals").is_err());
}

#[test]
fn discover_package_aliases_from_package_json_and_cli_override() {
    let base = temp_dir("discover");
    let (registry, service, _db) = write_demo_workspace(&base);
    let roots = vec![
        WorkspaceRoot {
            id: "registry".into(),
            path: registry.clone(),
        },
        WorkspaceRoot {
            id: "service".into(),
            path: service.clone(),
        },
    ];
    // package.json names only
    let discovered = discover_package_aliases(&roots, &[]);
    assert!(
        discovered.contains_key("@demo/registry"),
        "package.json name must discover alias: {discovered:?}"
    );
    assert_eq!(discovered["@demo/registry"].root_id, "registry");
    assert_eq!(discovered["@demo/registry"].source, "package.json");
    assert_eq!(
        discovered["@demo/registry"].entry.as_deref(),
        Some("src/index.ts"),
        "barrel entry discovered from disk when package.json has no exports"
    );

    // CLI wins over package.json
    let cli = vec![("@demo/registry".to_string(), "service".to_string())];
    let overridden = discover_package_aliases(&roots, &cli);
    assert_eq!(overridden["@demo/registry"].root_id, "service");
    assert_eq!(overridden["@demo/registry"].source, "cli");
}

#[test]
fn package_alias_meta_roundtrip() {
    let mut aliases = PackageAliasMap::new();
    aliases.insert(
        "@demo/registry".to_string(),
        alias_entry(
            "registry",
            Some("src/index.ts"),
            Some("packages/registry"),
            "cli",
        ),
    );
    let json = package_aliases_to_meta(&aliases);
    let back = package_aliases_from_meta(Some(&json));
    assert_eq!(back, aliases);
    assert!(package_aliases_from_meta(None).is_empty());
}

// ---------------------------------------------------------------------------
// Integration: multi-root index + CLI alias + cross-root query connection
// ---------------------------------------------------------------------------

#[test]
fn workspace_package_alias_links_cross_root_import() {
    let base = temp_dir("cross-root");
    let (registry, service, db) = write_demo_workspace(&base);

    let idx = run_raw(&[
        "index",
        "--workspace-root",
        registry.to_str().unwrap(),
        "--workspace-root",
        service.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--workspace-alias",
        "@demo/registry=registry",
        "--force",
    ]);
    assert!(idx.status.success(), "index stderr={}", stderr(&idx));

    // 1) Alias map persisted in workspace meta.
    {
        let store = AgStore::open(&db).unwrap_or_else(|e| panic!("open store: {e}"));
        let aliases = store.package_aliases_meta().expect("package_aliases_meta");
        assert!(
            aliases.contains_key("@demo/registry"),
            "meta must record @demo/registry alias: {aliases:?}"
        );
        assert_eq!(aliases["@demo/registry"].root_id, "registry");

        // 2) Import ref on the service side is linked (module + resolved).
        let importers = store
            .importers_of_package("@demo/registry", 50)
            .expect("importers_of_package");
        assert!(
            importers
                .iter()
                .any(|r| r.path.contains("order.service") && r.name == "RegistryClient"),
            "package import must be queryable cross-root; got {importers:?}"
        );
        let linked = importers
            .iter()
            .find(|r| r.path.contains("order.service"))
            .expect("service importer row");
        assert!(
            linked
                .resolved
                .as_deref()
                .map(|s| s.contains("index.ts") || s.contains("registry"))
                .unwrap_or(false),
            "import ref resolved should point at registry barrel/file/root, got {:?}",
            linked.resolved
        );
        assert_eq!(linked.module.as_deref(), Some("@demo/registry"));

        // 3) Pure resolve against stored aliases 鈫?root_id / workspace file.
        let hit = resolve_package_import("@demo/registry", &aliases).expect("resolve");
        assert_eq!(hit.root_id, "registry");
        assert!(
            hit.entry.as_deref() == Some("src/index.ts")
                || hit
                    .workspace_file
                    .as_deref()
                    .map(|s| s.ends_with("src/index.ts") || s.ends_with("registry/src/index.ts"))
                    .unwrap_or(false),
            "resolve hit must expose registry barrel: {hit:?}"
        );
    }

    // 4) find: RegistryClient definition/re-export lives under registry root.
    let find = run_raw(&[
        "find",
        "RegistryClient",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(find.status.success(), "{}", stderr(&find));
    let hits = parse_json(&find);
    let arr = hits.as_array().expect("find array");
    assert!(
        arr.iter().any(|h| {
            h["name"] == "RegistryClient"
                && h["root_id"] == "registry"
                && h["path"]
                    .as_str()
                    .map(|p| p.contains("client") || p.contains("index"))
                    .unwrap_or(false)
        }),
        "find must show registry definition/re-export: {hits}"
    );

    // 5) who-calls / callers: service root connected via package import + ctor type.
    let callers = run_raw(&[
        "callers",
        "RegistryClient",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(callers.status.success(), "{}", stderr(&callers));
    let cp = parse_json(&callers);
    let rows = if cp.is_array() {
        cp.as_array().cloned().unwrap_or_default()
    } else {
        cp.get("callers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    assert!(
        rows.iter().any(|r| {
            r["path"]
                .as_str()
                .map(|p| p.contains("order.service"))
                .unwrap_or(false)
                && r["root_id"].as_str() == Some("service")
        }),
        "callers must connect service 鈫?registry package import: {cp}"
    );

    // 6) related file set spans both roots (registry barrel/def + service importer).
    let related = run_raw(&[
        "related",
        "RegistryClient",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(related.status.success(), "{}", stderr(&related));
    let rel = parse_json(&related);
    let empty = vec![];
    let rel_arr = rel.as_array().unwrap_or(&empty);
    let paths: Vec<String> = rel_arr
        .iter()
        .filter_map(|h| h["path"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        paths
            .iter()
            .any(|p| p.contains("client") || p.contains("index")),
        "related must include registry files: {paths:?} / {rel}"
    );
    assert!(
        paths.iter().any(|p| p.contains("order.service")),
        "related must include service importer via package alias: {paths:?} / {rel}"
    );

    // 7) CLI importers accepts the package specifier.
    let imp = run_raw(&[
        "importers",
        "@demo/registry",
        "--workspace-db",
        db.to_str().unwrap(),
    ]);
    assert!(imp.status.success(), "{}", stderr(&imp));
    let imp_rows = parse_json(&imp);
    let imp_arr = imp_rows.as_array().cloned().unwrap_or_default();
    assert!(
        imp_arr.iter().any(|r| r["path"]
            .as_str()
            .map(|p| p.contains("order.service"))
            .unwrap_or(false)),
        "importers @demo/registry must list service files: {imp_rows}"
    );
}

/// package.json `name` alone is enough 鈥?no CLI alias required.
#[test]
fn workspace_package_alias_from_package_json_name() {
    let base = temp_dir("pkgjson");
    let (registry, service, db) = write_demo_workspace(&base);
    let idx = run_raw(&[
        "index",
        "--workspace-root",
        registry.to_str().unwrap(),
        "--workspace-root",
        service.to_str().unwrap(),
        "--workspace-db",
        db.to_str().unwrap(),
        "--force",
    ]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let store = AgStore::open(&db).expect("open");
    let aliases = store.package_aliases_meta().expect("aliases");
    assert!(
        aliases.contains_key("@demo/registry"),
        "package.json name discovery: {aliases:?}"
    );
    let importers = store
        .importers_of_package("@demo/registry", 20)
        .expect("importers");
    assert!(
        !importers.is_empty(),
        "package.json-discovered alias must still link imports"
    );
}

/// tsconfig paths alias source (explicit, preferred over basename guesses).
#[test]
fn workspace_package_alias_from_tsconfig_paths() {
    let base = temp_dir("tsconfig");
    let (registry, service, db) = write_demo_workspace(&base);
    // Workspace-level tsconfig paths (manifest sibling form).
    std::fs::write(
        base.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@demo/registry": ["packages/registry/src/index.ts"]
    }
  }
}"#,
    )
    .unwrap();

    let roots = vec![
        WorkspaceRoot {
            id: "registry".into(),
            path: registry.clone(),
        },
        WorkspaceRoot {
            id: "service".into(),
            path: service.clone(),
        },
    ];
    let aliases = discover_package_aliases(&roots, &[]);
    let entry = aliases
        .get("@demo/registry")
        .expect("tsconfig paths must create alias");
    // Prefer explicit tsconfig over package.json when both exist 鈥?either is
    // acceptable as long as root_id and entry point at the registry barrel.
    assert_eq!(entry.root_id, "registry");
    assert!(
        entry.entry.as_deref() == Some("src/index.ts")
            || entry
                .entry
                .as_deref()
                .map(|e| e.ends_with("src/index.ts"))
                .unwrap_or(false),
        "tsconfig path must map to registry barrel entry: {entry:?}"
    );

    let _ = db;
}

/// Single-root classic index: default behavior unchanged (no invented aliases).
#[test]
fn single_root_default_has_no_package_aliases() {
    let root = temp_dir("single");
    write_service(&root);
    let out = Command::new(bin())
        .arg("--root")
        .arg(&root)
        .args(["index", "--force"])
        .stdin(Stdio::null())
        .output()
        .expect("run");
    assert!(out.status.success(), "{}", stderr(&out));
    let db = root.join(".agentgraph").join("index.db");
    let store = AgStore::open(&db).expect("open");
    let aliases = store.package_aliases_meta().unwrap_or_default();
    assert!(
        aliases.is_empty(),
        "classic single-root must not invent workspace package aliases: {aliases:?}"
    );
}

/// Hard fixture lock: package.json names auto-map `@demo/registry` 鈫?registry
/// root so multi-root package-name imports link (P0-5d follow-up A).
#[test]
fn hard_fixture_package_json_enables_cross_root_link() {
    use std::path::PathBuf;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/eval-agent-tasks-hard/ts-multi-root-client");
    if !fixture.is_dir() {
        eprintln!("skip: hard fixture missing");
        return;
    }
    let base = temp_dir("hard-pkg-alias");
    copy_dir(&fixture, &base);
    let ws = base.join("workspace.json");
    let out = run_raw(&["index", "--workspace", ws.to_str().unwrap(), "--force"]);
    assert!(
        out.status.success(),
        "workspace index failed: {}",
        stderr(&out)
    );
    let st = run_raw(&["workspace", "status", "--workspace", ws.to_str().unwrap()]);
    let status = parse_json(&st);
    let raw = serde_json::to_string(&status).unwrap();
    assert!(
        raw.contains("@demo/registry") || raw.contains("package_aliases"),
        "status should mention package aliases when package.json present: {raw}"
    );
    let imp = run_raw(&[
        "importers",
        "@demo/registry",
        "--workspace",
        ws.to_str().unwrap(),
    ]);
    let importers = parse_json(&imp);
    let text = serde_json::to_string(&importers).unwrap();
    assert!(
        text.contains("order.service.ts") && text.contains("RegistryClient"),
        "package importers must list service consumer: {text}"
    );
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let t = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            let _ = std::fs::copy(e.path(), t);
        }
    }
}

/// Two roots claiming the same package.json name must not silently pick the first.
/// Either discovery fails-loud, or the alias is marked ambiguous and requires
/// CLI `--workspace-alias` override (no silent cross-root).
#[test]
fn duplicate_package_name_requires_explicit_override() {
    let base = temp_dir("dup-pkg-name");
    let r1 = base.join("packages/a");
    let r2 = base.join("packages/b");
    std::fs::create_dir_all(r1.join("src")).unwrap();
    std::fs::create_dir_all(r2.join("src")).unwrap();
    std::fs::write(
        r1.join("package.json"),
        r#"{"name":"@demo/registry","version":"0.0.1"}"#,
    )
    .unwrap();
    std::fs::write(
        r2.join("package.json"),
        r#"{"name":"@demo/registry","version":"0.0.1"}"#,
    )
    .unwrap();
    std::fs::write(r1.join("src/index.ts"), "export class A {}\n").unwrap();
    std::fs::write(r2.join("src/index.ts"), "export class B {}\n").unwrap();
    std::fs::write(
        base.join("workspace.json"),
        format!(
            "{{\"roots\":[{{\"id\":\"aa\",\"path\":\"{}\"}},{{\"id\":\"bb\",\"path\":\"{}\"}}]}}",
            r1.display().to_string().replace('\\', "/"),
            r2.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    let ws = base.join("workspace.json");
    // Without CLI override: must not silently succeed as if unambiguous.
    let out = run_raw(&["index", "--workspace", ws.to_str().unwrap(), "--force"]);
    let text = format!("{}{}", stdout(&out), stderr(&out));
    let status_ok = out.status.success();
    let ambiguous = text.to_lowercase().contains("duplicate")
        || text.to_lowercase().contains("ambiguous")
        || text.contains("@demo/registry");
    assert!(
        !status_ok || ambiguous,
        "duplicate package.json name must fail-loud or warn, got status={status_ok} text={text}"
    );

    // With CLI override: index must succeed and map to chosen root.
    let out2 = run_raw(&[
        "index",
        "--workspace",
        ws.to_str().unwrap(),
        "--workspace-alias",
        "@demo/registry=aa",
        "--force",
    ]);
    assert!(
        out2.status.success(),
        "CLI override must resolve duplicate: {}",
        stderr(&out2)
    );
}

/// Subpath package imports map to the same package root.
#[test]
fn package_alias_subpath_resolves_to_same_root() {
    use agentgraph::index::resolve::resolve_package_import;
    use agentgraph::index::workspace::{
        discover_package_aliases, PackageAliasEntry, WorkspaceRoot,
    };
    let base = temp_dir("pkg-subpath");
    let r = base.join("packages/registry");
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(
        r.join("package.json"),
        r#"{"name":"@demo/registry","version":"0.0.1"}"#,
    )
    .unwrap();
    std::fs::write(r.join("src/index.ts"), "export * from \"./client\";\n").unwrap();
    std::fs::write(r.join("src/client.ts"), "export class RegistryClient {}\n").unwrap();
    let roots = vec![WorkspaceRoot {
        id: "registry".into(),
        path: r.clone(),
    }];
    let aliases = discover_package_aliases(&roots, &[]);
    assert!(
        aliases.contains_key("@demo/registry"),
        "package.json name must produce alias: {:?}",
        aliases
    );
    let hit = resolve_package_import("@demo/registry/client", &aliases)
        .expect("subpath must resolve to package root");
    assert_eq!(hit.package, "@demo/registry");
    assert_eq!(hit.root_id, "registry");
    let _ = PackageAliasEntry {
        root_id: String::new(),
        entry: None,
        root_path: None,
        source: String::new(),
    };
}
