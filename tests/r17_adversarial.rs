//! R17 adversarial probes: incremental delete, forRootAsync deeper, S Function
//! type-position, impact edges, computed member, Py/Go/Rust edges.

use agentgraph::index::extract::{extract_file, ExtractedFile};
use agentgraph::index::store::Store;
use agentgraph::index::subset::scan_subset;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;
use std::path::PathBuf;

mod common;

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
    common::temp_db(&format!("agentgraph-r17-{name}"))
}

// ── Surface 9: incremental delete — nest heuristic edges must not remain ──

/// Delete app.module.ts → its ts.nest.* heuristic refs must cascade away.
/// Stale nest edges after delete = Major stale incremental graph.
#[test]
fn incremental_delete_nest_module_clears_heuristic_edges() {
    let db = temp_db("del-nest");
    let mut store = Store::open(&db).unwrap();

    let mod_src = r#"
import { Module } from '@nestjs/common';
import { AppService } from './app.service';

@Module({
  providers: [AppService],
  exports: [AppService],
})
export class AppModule {}
"#;
    let svc_src = r#"
import { Injectable } from '@nestjs/common';

@Injectable()
export class AppService {
  getHello(): string { return 'hi'; }
}
"#;
    let m = extract(mod_src, "src/app.module.ts");
    let s = extract(svc_src, "src/app.service.ts");
    store.begin_batch().unwrap();
    store
        .replace_file("src/app.module.ts", "hm", "typescript", &m)
        .unwrap();
    store
        .replace_file("src/app.service.ts", "hs", "typescript", &s)
        .unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();

    let before = store.callers("AppService", 50).unwrap();
    assert!(
        before.iter().any(|r| r.path == "src/app.module.ts"),
        "seed must have nest edge from app.module.ts; got {before:?}"
    );

    // Incremental delete of app.module.ts (keep only service).
    let keep = vec!["src/app.service.ts".to_string()];
    store.prune_missing(&keep).unwrap();
    store
        .resolve_symbol_ids_for_paths(&["src/app.module.ts".to_string()])
        .unwrap();

    let after = store.callers("AppService", 50).unwrap();
    assert!(
        !after.iter().any(|r| r.path == "src/app.module.ts"),
        "stale nest heuristic edge remains after prune_missing: {after:?}"
    );
}

/// Cross-file dangling sid: A defines helper, B calls helper. Delete A via
/// prune_missing + partial resolve (the real incremental path). B's ref must
/// not keep a dangling resolved_symbol_id.
#[test]
fn incremental_delete_file_clears_inbound_dangling_sids() {
    let db = temp_db("del-sid");
    let mut store = Store::open(&db).unwrap();

    let a_src = r#"pub fn helper() -> i32 { 1 }"#;
    let b_src = r#"pub fn run() { helper(); }"#;
    let a = extract_lang(a_src, "src/a.rs", Language::Rust);
    let b = extract_lang(b_src, "src/b.rs", Language::Rust);
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha", "rust", &a).unwrap();
    store.replace_file("src/b.rs", "hb", "rust", &b).unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    store.assert_no_dangling_sids().unwrap();

    // Delete a.rs — only keep b.rs. Mirrors index() incremental delete path:
    // prune_missing then resolve_symbol_ids_for_paths(deleted).
    store.prune_missing(&["src/b.rs".to_string()]).unwrap();
    store
        .resolve_symbol_ids_for_paths(&["src/a.rs".to_string()])
        .unwrap();

    store.assert_no_dangling_sids().unwrap_or_else(|e| {
        panic!("dangling sid after incremental delete: {e}");
    });
}

/// Rename-away via replace: A defines helper; replace A with helper2.
/// B's inbound sid to old helper must not dangle after partial resolve.
#[test]
fn incremental_rename_away_clears_inbound_dangling_sids() {
    let db = temp_db("ren-sid");
    let mut store = Store::open(&db).unwrap();

    let a_src = r#"pub fn helper() -> i32 { 1 }"#;
    let a2_src = r#"pub fn helper2() -> i32 { 2 }"#;
    let b_src = r#"pub fn run() { helper(); }"#;
    let a = extract_lang(a_src, "src/a.rs", Language::Rust);
    let a2 = extract_lang(a2_src, "src/a.rs", Language::Rust);
    let b = extract_lang(b_src, "src/b.rs", Language::Rust);
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha1", "rust", &a).unwrap();
    store.replace_file("src/b.rs", "hb", "rust", &b).unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();
    store.assert_no_dangling_sids().unwrap();

    // Rename helper → helper2 in a.rs; partial resolve dirty=[a.rs].
    store.begin_batch().unwrap();
    store.replace_file("src/a.rs", "ha2", "rust", &a2).unwrap();
    store.commit_batch().unwrap();
    store
        .resolve_symbol_ids_for_paths(&["src/a.rs".to_string()])
        .unwrap();

    store.assert_no_dangling_sids().unwrap_or_else(|e| {
        panic!("dangling sid after incremental rename-away: {e}");
    });
}

// ── Surface 1: forRootAsync deeper ───────────────────────────────────

/// useFactory returning an object with nested `imports` array must NOT invent
/// import edges (returned config object ≠ Nest module metadata).
#[test]
fn for_root_async_factory_returned_object_imports_not_edges() {
    let src = r#"
import { ConfigModule } from '@nestjs/config';
import { DbModule } from './db.module';
import { CacheModule } from './cache.module';

export const configModule = ConfigModule.forRootAsync({
  useFactory: () => ({
    imports: [CacheModule],
    url: process.env.DB_URL,
  }),
});
"#;
    let out = extract(src, "src/config.ts");
    assert!(
        nest_hits(&out, "CacheModule", "ts.nest.module_imports").is_empty(),
        "useFactory returned-object imports must not invent import edges; refs={}",
        dump_refs(&out)
    );
}

/// externalDependencies is a package-name list — must not invent module edges.
#[test]
fn for_root_async_external_dependencies_no_module_edges() {
    let src = r#"
import { ConfigModule } from '@nestjs/config';

export const configModule = ConfigModule.forRootAsync({
  useFactory: () => ({}),
  externalDependencies: ['dotenv'],
});
"#;
    let out = extract(src, "src/config.ts");
    assert!(
        nest_hits(&out, "dotenv", "ts.nest.module_imports").is_empty()
            && nest_hits(&out, "dotenv", "ts.nest.module_providers").is_empty(),
        "externalDependencies must not invent DI edges; refs={}",
        dump_refs(&out)
    );
}

/// TypeORM entities/migrations arrays — must not invent call/registration edges.
#[test]
fn typeorm_for_root_async_entities_no_edges() {
    let src = r#"
import { TypeOrmModule } from '@nestjs/typeorm';
import { User } from './user.entity';
import { Init123 } from './migrations/123-init';

export const db = TypeOrmModule.forRootAsync({
  useFactory: () => ({
    entities: [User],
    migrations: [Init123],
  }),
});
"#;
    let out = extract(src, "src/db.ts");
    assert!(
        nest_hits(&out, "User", "ts.nest.module_imports").is_empty()
            && nest_hits(&out, "User", "ts.nest.module_providers").is_empty()
            && nest_hits(&out, "Init123", "ts.nest.module_imports").is_empty()
            && nest_hits(&out, "Init123", "ts.nest.module_providers").is_empty(),
        "TypeORM entities/migrations must not invent DI edges; refs={}",
        dump_refs(&out)
    );
}

// ── Surface 3: Function type-position on Nest-like code ──────────────

/// Normal Nest shape with `Function` only in type position (method handler
/// type, not a call). If this leaves S, that is a Major false-positive that
/// disables sound on clean Nest.
#[test]
fn nest_like_function_type_annotation_must_stay_in_s() {
    let src = r#"
import { Injectable } from '@nestjs/common';

@Injectable()
export class HandlerRegistry {
  private handlers = new Map<string, Function>();

  register(name: string, handler: Function): void {
    this.handlers.set(name, handler);
  }

  invoke(name: string): void {
    const h = this.handlers.get(name);
    if (h) h();
  }
}
"#;
    let r = scan_subset(src, Language::TypeScript, "src/handlers.ts");
    assert!(
        r.in_subset,
        "type-position Function on clean Nest-like code must stay in S: {:?}",
        r.violations
    );
}

/// interface field typed as Function — type only, no runtime escape.
#[test]
fn interface_function_type_field_stays_in_s() {
    let src = r#"
export interface Plugin {
  name: string;
  run: Function;
}

export function callPlugin(p: Plugin): void {
  p.run();
}
"#;
    let r = scan_subset(src, Language::TypeScript, "src/plugin.ts");
    assert!(
        r.in_subset,
        "interface {{ f: Function }} type-only must stay in S: {:?}",
        r.violations
    );
}

// ── Surface 4: computed member as value ──────────────────────────────

/// globalThis['eval'] as a value (not call) — still leaves S.
#[test]
fn computed_member_eval_as_value_leaves_s() {
    let src = r#"
const e = globalThis['eval'];
export function run(code: string) { return e(code); }
"#;
    let r = scan_subset(src, Language::JavaScript, "a.js");
    assert!(
        !r.in_subset,
        "globalThis['eval'] as value must leave S: {:?}",
        r.violations
    );
}

/// window['Function'] as a value.
#[test]
fn computed_member_function_as_value_leaves_s() {
    let src = r#"
const F = window['Function'];
export const x = F;
"#;
    let r = scan_subset(src, Language::JavaScript, "a.js");
    assert!(
        !r.in_subset,
        "window['Function'] as value must leave S: {:?}",
        r.violations
    );
}

// ── Surface 5: Python getattr / __builtins__ / match ─────────────────

/// getattr(obj, 'eval') as call-of-getattr — dynamic candidate or violation?
#[test]
fn py_getattr_eval_leaves_s_or_finite_domain() {
    let src = r#"
def run(obj):
    f = getattr(obj, 'eval')
    return f('1+1')
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    // getattr with string 'eval' is a known escape hatch pattern.
    assert!(
        !r.in_subset
            || r.violations
                .iter()
                .any(|v| v.kind.contains("getattr") || v.kind.contains("dynamic")),
        "getattr(obj,'eval') must leave S or flag dynamic: {:?}",
        r.violations
    );
}

/// __builtins__ access.
#[test]
fn py_builtins_import_leaves_s() {
    let src = r#"
import builtins
def run():
    return builtins.eval('1')
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "import builtins + builtins.eval must leave S: {:?}",
        r.violations
    );
}

/// match statement — not an S violation by itself.
#[test]
fn py_match_statement_stays_in_s() {
    let src = r#"
def classify(x):
    match x:
        case 1:
            return 'one'
        case _:
            return 'other'
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        r.in_subset,
        "match statement alone must stay in S: {:?}",
        r.violations
    );
}

// ── Surface 6: Go generic methods ────────────────────────────────────

/// Generic method receiver `func (s *S[T]) M()` must extract M as a symbol.
#[test]
fn go_generic_method_receiver_extracts_symbol() {
    let src = r#"
package main

type Stack[T any] struct {
	items []T
}

func (s *Stack[T]) Push(v T) {
	s.items = append(s.items, v)
}

func (s *Stack[T]) Pop() (T, bool) {
	var zero T
	if len(s.items) == 0 {
		return zero, false
	}
	v := s.items[len(s.items)-1]
	s.items = s.items[:len(s.items)-1]
	return v, true
}

func main() {
	s := &Stack[int]{}
	s.Push(1)
	s.Pop()
}
"#;
    let out = extract_lang(src, "main.go", Language::Go);
    let names: Vec<&str> = out.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Push") && names.contains(&"Pop"),
        "generic method receiver must extract Push/Pop; symbols={names:?}"
    );
    let refs: Vec<&str> = out.references.iter().map(|r| r.name.as_str()).collect();
    assert!(
        refs.contains(&"Push") && refs.contains(&"Pop"),
        "generic method calls must emit refs; refs={}",
        dump_refs(&out)
    );
}

// ── Surface 7: Rust full-path transmute / asm ────────────────────────

/// core::mem::transmute full path call must leave S.
#[test]
fn rust_core_mem_transmute_full_path_leaves_s() {
    let src = r#"
fn cast(x: u32) -> i32 {
    unsafe { core::mem::transmute(x) }
}
"#;
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(
        !r.in_subset,
        "core::mem::transmute must leave S: {:?}",
        r.violations
    );
}

/// std::arch::asm! — inline asm leaves S if modeled; if unmodeled, stay in S
/// only when no unsafe is present. Pin: asm! with unsafe block should leave S
/// if any rule fires, else document.
#[test]
fn rust_std_arch_asm_leaves_s_or_is_documented() {
    let src = r#"
fn pause() {
    unsafe {
        std::arch::asm!("pause");
    }
}
"#;
    let r = scan_subset(src, Language::Rust, "a.rs");
    // Prefer leave S; if not modeled, at least no panic.
    if r.in_subset {
        eprintln!(
            "NOTE: std::arch::asm! currently stays in S (unmodeled): {:?}",
            r.violations
        );
    } else {
        assert!(!r.violations.is_empty());
    }
}

// ── Surface 8: impact depth 0 / limit 1 / module_exports ─────────────

/// depth=0 must return empty (no expansion).
#[test]
fn impact_depth_zero_is_empty() {
    let db = temp_db("impact0");
    let mut store = Store::open(&db).unwrap();
    let src = "export function a() { return 1; }\nexport function b() { return a(); }\n";
    let parsed = extract(src, "src/m.ts");
    store.begin_batch().unwrap();
    store
        .replace_file("src/m.ts", "h1", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    let hits = store.impact("a", 0, 100).unwrap();
    assert!(
        hits.is_empty(),
        "depth=0 must yield empty impact; got {hits:?}"
    );
}

/// limit=1 must cap output.
#[test]
fn impact_limit_one_caps_output() {
    let db = temp_db("impact1");
    let mut store = Store::open(&db).unwrap();
    let src = r#"
export function a() { return 1; }
export function b() { return a(); }
export function c() { return a(); }
export function d() { return a(); }
"#;
    let parsed = extract(src, "src/m.ts");
    store.begin_batch().unwrap();
    store
        .replace_file("src/m.ts", "h1", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    let hits = store.impact("a", 5, 1).unwrap();
    assert!(
        hits.len() <= 1,
        "limit=1 must cap impact; got {} hits",
        hits.len()
    );
}

/// module_exports is registration, not a call. impact_sound walking exports
/// must not invent a false *transitive call* from unrelated callers through
/// the module class when depth is large. Pin: impact of exported provider
/// includes the module (registration) but not random functions of the module
/// as "callers of the provider".
#[test]
fn nest_exports_impact_includes_module_not_false_transitive_calls() {
    let db = temp_db("exports-impact");
    let mut store = Store::open(&db).unwrap();

    let mod_src = r#"
import { Module } from '@nestjs/common';
import { AppService } from './app.service';

@Module({
  providers: [AppService],
  exports: [AppService],
})
export class AppModule {
  boot() { /* unrelated method */ }
}
"#;
    let svc_src = r#"
import { Injectable } from '@nestjs/common';

@Injectable()
export class AppService {
  getHello(): string { return 'hi'; }
}
"#;
    let ctrl_src = r#"
import { Controller } from '@nestjs/common';
import { AppService } from './app.service';

@Controller()
export class AppController {
  constructor(private readonly appService: AppService) {}
  getHello(): string { return this.appService.getHello(); }
}
"#;
    let m = extract(mod_src, "src/app.module.ts");
    let s = extract(svc_src, "src/app.service.ts");
    let c = extract(ctrl_src, "src/app.controller.ts");
    store.begin_batch().unwrap();
    store
        .replace_file("src/app.module.ts", "hm", "typescript", &m)
        .unwrap();
    store
        .replace_file("src/app.service.ts", "hs", "typescript", &s)
        .unwrap();
    store
        .replace_file("src/app.controller.ts", "hc", "typescript", &c)
        .unwrap();
    store.commit_batch().unwrap();
    store.resolve_symbol_ids().unwrap();

    let (hits, _viol) = store.impact_sound("AppService", 3, 50).unwrap();
    // Must include AppModule (exports) and AppController (ctor inject).
    assert!(
        hits.iter().any(|h| h.path == "src/app.module.ts"),
        "exports registration should appear in impact; hits={hits:?}"
    );
    assert!(
        hits.iter().any(|h| h.path == "src/app.controller.ts"),
        "ctor inject should appear in impact; hits={hits:?}"
    );
    // Must NOT invent a call from AppModule.boot as a caller of AppService
    // via enclosing expansion of unrelated method — boot is not a ref.
    assert!(
        !hits.iter().any(|h| h.enclosing.as_deref() == Some("boot")),
        "unrelated method must not appear as transitive caller; hits={hits:?}"
    );
}

// ── Surface 12: nestjs-module fixture subset_ok ──────────────────────

/// Index nestjs-module fixture sources; subset scan must be clean.
#[test]
fn nestjs_module_fixture_stays_in_s() {
    let files = [
        (
            "src/app.module.ts",
            include_str!("../fixtures/eval-l1-real/nestjs-module/src/app.module.ts"),
        ),
        (
            "src/app.service.ts",
            include_str!("../fixtures/eval-l1-real/nestjs-module/src/app.service.ts"),
        ),
        (
            "src/app.controller.ts",
            include_str!("../fixtures/eval-l1-real/nestjs-module/src/app.controller.ts"),
        ),
    ];
    for (path, src) in files {
        let r = scan_subset(src, Language::TypeScript, path);
        assert!(
            r.in_subset,
            "nestjs-module fixture {path} must stay in S: {:?}",
            r.violations
        );
    }
}
