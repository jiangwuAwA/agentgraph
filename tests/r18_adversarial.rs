//! R18 adversarial probes: watch dir-delete, forRootAsync nested in Module,
//! SCIP after incremental delete, path-qualified Rust impl Trait, Go type
//! switch, Python walrus.

use agentgraph::index::extract::{extract_file, ExtractedFile};
use agentgraph::index::store::Store;
use agentgraph::index::subset::scan_subset;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

fn extract(src: &str, path: &str) -> ExtractedFile {
    let known = HashSet::new();
    extract_file(src, Language::TypeScript, path, &known).expect("extract")
}

fn extract_lang(src: &str, path: &str, lang: Language) -> ExtractedFile {
    let known = HashSet::new();
    extract_file(src, lang, path, &known).expect("extract")
}

fn nest_hits<'a>(
    out: &'a ExtractedFile,
    name: &str,
    rule_id: &str,
) -> Vec<&'a agentgraph::index::extract::ExtractedRef> {
    out.references
        .iter()
        .filter(|r| {
            r.name == name
                && r.confidence == Confidence::Heuristic
                && r.evidence
                    .as_ref()
                    .map(|e| e.rule_id == rule_id)
                    .unwrap_or(false)
        })
        .collect()
}

fn dump_refs(out: &ExtractedFile) -> String {
    format!(
        "{:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.confidence.as_str(),
                r.enclosing.clone(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    )
}

fn temp_db(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-r18-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("index.db")
}

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agentgraph-r18-watch-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/mod_a")).unwrap();
    std::fs::write(
        dir.join("src/mod_a/helper.ts"),
        "export function helper() { return 1; }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/caller.ts"),
        "import { helper } from './mod_a/helper';\nexport function run() { return helper(); }\n",
    )
    .unwrap();
    dir
}

// ── Surface 2: watch + directory delete ──────────────────────────────

/// Deleting a whole source directory must prune its files from the index.
/// Windows/notify typically emits a single Remove on the directory path
/// (no per-file events, no source extension) — must not be filtered out.
#[test]
fn watch_directory_delete_prunes_stale_symbols() {
    let root = temp_root("dir-del");
    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let before = {
        let store = indexer.open_store().unwrap();
        store.find_symbol("helper", 10).unwrap()
    };
    assert!(
        before.iter().any(|s| s.name == "helper"),
        "seed must index helper"
    );

    let (rx, _handle) = indexer
        .watch_events(Duration::from_millis(50))
        .expect("start watcher");
    std::thread::sleep(Duration::from_millis(80));

    std::fs::remove_dir_all(root.join("src/mod_a")).unwrap();

    // Expect a reindex notification.
    let ev = rx.recv_timeout(Duration::from_millis(1500));
    assert!(
        ev.is_ok(),
        "directory delete must trigger watch reindex; got {:?}",
        ev
    );

    let store = indexer.open_store().unwrap();
    let after = store.find_symbol("helper", 10).unwrap();
    assert!(
        !after.iter().any(|s| s.name == "helper"),
        "stale helper symbol remains after directory delete: {after:?}"
    );
    let callers = store.callers("helper", 20).unwrap();
    assert!(
        !callers.iter().any(|r| r.path.contains("mod_a")),
        "stale refs from deleted directory remain: {callers:?}"
    );
}

/// index_paths on a deleted directory path must prune every file under it.
#[test]
fn index_paths_deleted_directory_prunes_all_children() {
    let root = temp_root("ip-dir");
    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let dir = root.join("src/mod_a");
    std::fs::remove_dir_all(&dir).unwrap();

    let stats = indexer
        .index_paths(std::slice::from_ref(&dir))
        .expect("index_paths on deleted dir");
    assert!(
        stats.symbols < 3,
        "expected fewer symbols after prune: {stats:?}"
    );

    let store = indexer.open_store().unwrap();
    let after = store.find_symbol("helper", 10).unwrap();
    assert!(
        !after.iter().any(|s| s.name == "helper"),
        "helper must be pruned after directory delete via index_paths: {after:?}"
    );
}

/// Renaming a source directory must not leave the old path in the index.
/// notify may emit only a directory Rename/Modify(Name) event (no per-file
/// source extensions) — the old path's files must be pruned.
#[test]
fn watch_directory_rename_prunes_old_path() {
    let root = temp_root("dir-ren");
    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let (rx, _handle) = indexer
        .watch_events(Duration::from_millis(50))
        .expect("start watcher");
    std::thread::sleep(Duration::from_millis(80));

    std::fs::rename(root.join("src/mod_a"), root.join("src/mod_b")).unwrap();

    let ev = rx.recv_timeout(Duration::from_millis(1500));
    assert!(
        ev.is_ok(),
        "directory rename must trigger watch reindex; got {:?}",
        ev
    );

    let store = indexer.open_store().unwrap();
    let helpers = store.find_symbol("helper", 10).unwrap();
    let stale: Vec<_> = helpers
        .iter()
        .filter(|s| s.path.contains("mod_a"))
        .collect();
    assert!(
        stale.is_empty(),
        "old directory path remains after rename: {helpers:?}"
    );
}

// ── Surface 3: Nest forRootAsync nested inside @Module imports ───────

/// `imports: [ConfigModule.forRootAsync({ inject, useFactory })]` — the
/// forRootAsync call is one level deeper than a top-level export. DI edges
/// from inject/useFactory must still fire.
#[test]
fn for_root_async_nested_in_module_imports_fires_di_edges() {
    let src = r#"
import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { DatabaseService } from './database.service';

@Module({
  imports: [
    ConfigModule.forRootAsync({
      inject: [DatabaseService],
      useFactory: (db: DatabaseService) => ({
        url: db.url(),
      }),
    }),
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "DatabaseService", "ts.nest.module_providers").is_empty(),
        "nested forRootAsync inject/useFactory must emit provider edges; refs={}",
        dump_refs(&out)
    );
}

/// Nested forRootAsync imports array must also fire module_imports.
#[test]
fn for_root_async_nested_imports_array_fires() {
    let src = r#"
import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { ThrottlerModule } from '@nestjs/throttler';

@Module({
  imports: [
    ConfigModule.forRootAsync({
      imports: [ThrottlerModule],
      useFactory: () => ({}),
    }),
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "ThrottlerModule", "ts.nest.module_imports").is_empty(),
        "nested forRootAsync imports must emit import edges; refs={}",
        dump_refs(&out)
    );
}

// ── Surface 5: SCIP export after incremental delete ──────────────────

/// Index, export SCIP, delete one file via prune + partial resolve, export
/// again without full reindex. Deleted file must not appear as a document;
/// remaining refs must not panic or point at missing defs.
#[test]
fn scip_export_after_incremental_delete_omits_deleted_doc() {
    use agentgraph::index::export::export_scip_json;

    let db = temp_db("scip-del");
    let dir = db.parent().unwrap().to_path_buf();
    let mut store = Store::open(&db).unwrap();

    let a_src = "export function helper() { return 1; }\n";
    let b_src = "import { helper } from './a';\nexport function run() { return helper(); }\n";
    let a = extract(a_src, "src/a.ts");
    let b = extract(b_src, "src/b.ts");
    store.begin_batch().unwrap();
    store
        .replace_file("src/a.ts", "ha", "typescript", &a)
        .unwrap();
    store
        .replace_file("src/b.ts", "hb", "typescript", &b)
        .unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();

    let out1 = dir.join("before.scip.json");
    // root for file_uri — use a fake project root under temp
    let proj =
        std::env::temp_dir().join(format!("agentgraph-r18-scip-proj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&proj);
    let _ = std::fs::create_dir_all(proj.join("src"));
    let _ = std::fs::write(proj.join("src/a.ts"), a_src);
    let _ = std::fs::write(proj.join("src/b.ts"), b_src);
    export_scip_json(&store, &proj, &out1).unwrap();
    let before = std::fs::read_to_string(&out1).unwrap();
    assert!(
        before.contains("src/a.ts"),
        "before export must include a.ts"
    );
    assert!(
        before.contains("src/b.ts"),
        "before export must include b.ts"
    );

    // Incremental delete a.ts.
    store.prune_missing(&["src/b.ts".to_string()]).unwrap();
    store
        .resolve_symbol_ids_for_paths(&["src/a.ts".to_string()])
        .unwrap();
    let _ = std::fs::remove_file(proj.join("src/a.ts"));

    let out2 = dir.join("after.scip.json");
    // CLI path: ensure_sids_for_export then export — no full reindex.
    store.ensure_sids_for_export().unwrap();
    export_scip_json(&store, &proj, &out2).unwrap();
    let after = std::fs::read_to_string(&out2).unwrap();
    assert!(
        !after.contains("src/a.ts"),
        "deleted file must not appear as SCIP document after incremental delete"
    );
    assert!(
        after.contains("src/b.ts"),
        "remaining file must still export"
    );
}

// ── Surface 10: Rust impl Trait for path-qualified Type ──────────────

/// `impl Trait for crate::foo::Bar` — type is a scoped_type_identifier /
/// path, not bare type_identifier. Methods must still get qualifier edges.
#[test]
fn rust_impl_trait_for_path_qualified_type_emits_methods() {
    let src = r#"
pub trait Greeter {
    fn greet(&self) -> String;
}

impl Greeter for crate::models::User {
    fn greet(&self) -> String {
        format!("hi {}", self.name)
    }
}
"#;
    let out = extract_lang(src, "src/lib.rs", Language::Rust);
    let greet_refs: Vec<_> = out
        .references
        .iter()
        .filter(|r| r.name == "greet")
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == "rs.di.impl_trait")
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !greet_refs.is_empty(),
        "path-qualified impl Trait for Type must emit method edges; refs={}",
        dump_refs(&out)
    );
}

// ── Surface 8: Go type switch ────────────────────────────────────────

/// Type switch `switch v := x.(type)` must extract case type names as refs
/// and not crash; cases should be queryable.
#[test]
fn go_type_switch_extracts_case_type_refs() {
    let src = r#"
package main

type Cat struct{ name string }
type Dog struct{ name string }

func speak(a interface{}) string {
	switch v := a.(type) {
	case Cat:
		return v.name
	case Dog:
		return v.name
	case *Cat:
		return v.name
	default:
		return "?"
	}
}

func main() {
	speak(Cat{name: "k"})
}
"#;
    let out = extract_lang(src, "main.go", Language::Go);
    let names: Vec<&str> = out.references.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"Cat") && names.contains(&"Dog"),
        "type switch case types must appear as refs; got {}",
        dump_refs(&out)
    );
}

/// Embedded struct receiver: `type Derived struct { Base }` +
/// `func (d Derived) Hello()` and method call on Derived must resolve.
/// Also `func (b Base) Hello()` used via embedding.
#[test]
fn go_embedded_struct_receiver_extracts_method() {
    let src = r#"
package main

type Base struct{}

func (b Base) Hello() string { return "hi" }

type Derived struct {
	Base
	extra string
}

func (d Derived) Greet() string { return d.Hello() }

func main() {
	d := Derived{}
	_ = d.Greet()
	_ = d.Hello()
}
"#;
    let out = extract_lang(src, "main.go", Language::Go);
    let names: Vec<&str> = out.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Hello") && names.contains(&"Greet"),
        "embedded struct methods must extract; symbols={names:?}"
    );
    let refs: Vec<&str> = out.references.iter().map(|r| r.name.as_str()).collect();
    assert!(
        refs.contains(&"Greet"),
        "method call must emit ref; refs={}",
        dump_refs(&out)
    );
}

// ── Surface 9: Python walrus in getattr ──────────────────────────────

/// getattr(obj, (n := 'eval')) — walrus inside the string-literal slot.
/// Must leave S (non-literal second arg from scanner's view) or flag.
#[test]
fn py_walrus_in_getattr_leaves_s() {
    let src = r#"
def run(obj):
    f = getattr(obj, (name := "eval"))
    return f("1+1")
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "walrus inside getattr string slot must leave S: {:?}",
        r.violations
    );
}

/// Nested f-string getattr: getattr(obj, f"{'eval'}") — dynamic, leaves S.
#[test]
fn py_nested_fstring_getattr_leaves_s() {
    let src = r#"
def run(obj):
    key = f"{'eval'}"
    return getattr(obj, key)("1")
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    // key is a variable — getattr without string-literal second arg → leave S.
    assert!(
        !r.in_subset,
        "getattr with non-literal (variable) second arg must leave S: {:?}",
        r.violations
    );
}

// ── Surface 1: full vs partial resolve consistency after rename race ──

/// Two dirty files both defining the same bare name `helper` (different
/// paths). After partial resolve of only one dirty path, inbound refs must
/// not dangle and must pick a still-existing symbol.
#[test]
fn partial_resolve_two_same_name_symbols_no_dangle() {
    let db = temp_db("same-name");
    let mut store = Store::open(&db).unwrap();

    let a1 = extract_lang("pub fn helper() -> i32 { 1 }", "src/a.rs", Language::Rust);
    let b1 = extract_lang("pub fn helper() -> i32 { 2 }", "src/b.rs", Language::Rust);
    let c = extract_lang("pub fn run() { helper(); }", "src/c.rs", Language::Rust);
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha1", "rust", &a1).unwrap();
    store.replace_file("src/b.rs", "hb1", "rust", &b1).unwrap();
    store.replace_file("src/c.rs", "hc", "rust", &c).unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    store.assert_no_dangling_sids().unwrap();

    // Both a and b dirty: a renames away helper; b keeps helper.
    let a2 = extract_lang("pub fn helper2() -> i32 { 1 }", "src/a.rs", Language::Rust);
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha2", "rust", &a2).unwrap();
    store.commit_batch().unwrap();
    store
        .resolve_symbol_ids_for_paths(&["src/a.rs".to_string(), "src/b.rs".to_string()])
        .unwrap();
    store.assert_no_dangling_sids().unwrap();

    let helpers: Vec<_> = store
        .find_symbol("helper", 10)
        .unwrap()
        .into_iter()
        .filter(|s| s.name == "helper")
        .collect();
    assert_eq!(
        helpers.len(),
        1,
        "only b.rs helper should remain; got {helpers:?}"
    );
    let callers = store.callers("helper", 20).unwrap();
    assert!(
        !callers.is_empty(),
        "c.rs must still resolve to remaining helper"
    );
}
