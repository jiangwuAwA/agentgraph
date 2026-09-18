//! R16 adversarial probes: Nest after R15, S over-flag, store/export edges.

use agentgraph::index::extract::{extract_file, ExtractedFile};
use agentgraph::index::subset::scan_subset;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

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

fn any_rule(out: &ExtractedFile, name: &str) -> Vec<String> {
    out.references
        .iter()
        .filter(|r| r.name == name)
        .filter_map(|r| r.evidence.as_ref().map(|e| e.rule_id.clone()))
        .collect()
}

// ── Nest exports ────────────────────────────────────────────────────

/// `exports: [AppService]` is a real Nest re-export registration.
/// Other modules can import this provider; L1 must emit Heuristic.
#[test]
fn nest_module_exports_bare_ident() {
    let src = r#"
import { Module } from '@nestjs/common';

@Module({
  providers: [AppService],
  exports: [AppService],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "AppService", "ts.nest.module_exports").is_empty(),
        "exports: [AppService] must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert_eq!(
        nest_hits(&out, "AppService", "ts.nest.module_exports")[0]
            .enclosing
            .as_deref(),
        Some("AppModule")
    );
}

/// `exports: [forwardRef(() => OtherModule)]` and string tokens.
#[test]
fn nest_module_exports_forward_ref_and_string() {
    let src = r#"
import { Module, forwardRef } from '@nestjs/common';

@Module({
  exports: [forwardRef(() => AuthModule), 'CONFIG_TOKEN'],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "AuthModule", "ts.nest.module_exports").is_empty(),
        "exports forwardRef must unwrap; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "CONFIG_TOKEN", "ts.nest.module_exports").is_empty(),
        "exports string token must emit; refs={}",
        dump_refs(&out)
    );
    assert!(
        nest_hits(&out, "forwardRef", "ts.nest.module_exports").is_empty(),
        "must not invent forwardRef edge; refs={}",
        dump_refs(&out)
    );
}

// ── Nest forRootAsync config deps ───────────────────────────────────

/// `ConfigModule.forRootAsync({ imports: [DbModule], inject: [ConfigService], useFactory })`
/// nested inside `@Module({ imports: [...] })` — config-object DI deps must fire.
#[test]
fn nest_for_root_async_config_deps() {
    let src = r#"
import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { DbModule } from './db.module';
import { ConfigService } from './config.service';

@Module({
  imports: [
    ConfigModule.forRootAsync({
      imports: [DbModule],
      inject: [ConfigService],
      useFactory: async (cfg: ConfigService) => ({ url: cfg.get('DB') }),
    }),
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    // Module import edge for ConfigModule itself still works via member call.
    assert!(
        !nest_hits(&out, "ConfigModule", "ts.nest.module_imports").is_empty(),
        "ConfigModule import edge; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "DbModule", "ts.nest.module_imports").is_empty(),
        "forRootAsync imports: [DbModule] must fire; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "ConfigService", "ts.nest.module_imports").is_empty()
            || !nest_hits(&out, "ConfigService", "ts.nest.module_providers").is_empty(),
        "forRootAsync inject/useFactory ConfigService must fire; refs={}",
        dump_refs(&out)
    );
}

/// Same shape as a non-decorator static method call at top level
/// (dynamic module factory exported function).
#[test]
fn nest_for_root_async_top_level_call() {
    let src = r#"
import { ConfigModule } from '@nestjs/config';
import { DatabaseModule } from './database.module';
import { AppConfigService } from './app-config.service';

export const configModule = ConfigModule.forRootAsync({
  imports: [DatabaseModule],
  inject: [AppConfigService],
  useFactory: () => ({}),
});
"#;
    let out = extract(src, "src/config.ts");
    assert!(
        !nest_hits(&out, "DatabaseModule", "ts.nest.module_imports").is_empty(),
        "top-level forRootAsync imports must fire; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "AppConfigService", "ts.nest.module_providers").is_empty(),
        "top-level forRootAsync inject must fire; refs={}",
        dump_refs(&out)
    );
    // Unused helper kept for debug.
    let _ = any_rule(&out, "AppConfigService");
}

// ── Nest useValue ───────────────────────────────────────────────────

/// Bare string token in providers array: `providers: ['CONFIG']`.
#[test]
fn nest_providers_bare_string_token() {
    let src = r#"
import { Module } from '@nestjs/common';

@Module({
  providers: ['CONFIG', AppService],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "CONFIG", "ts.nest.module_providers").is_empty(),
        "providers: ['CONFIG'] must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "AppService", "ts.nest.module_providers").is_empty(),
        "providers: [AppService] still fires; refs={}",
        dump_refs(&out)
    );
}

/// `useValue: someObj` must not invent property-name edges; provide token still fires.
#[test]
fn nest_use_value_no_property_noise() {
    let src = r#"
import { Module } from '@nestjs/common';

@Module({
  providers: [
    { provide: 'CONFIG', useValue: { host: 'localhost', port: 5432 } },
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "CONFIG", "ts.nest.module_providers").is_empty(),
        "provide token still required; refs={}",
        dump_refs(&out)
    );
    assert!(
        nest_hits(&out, "host", "ts.nest.module_providers").is_empty()
            && nest_hits(&out, "port", "ts.nest.module_providers").is_empty(),
        "useValue object properties must not invent edges; refs={}",
        dump_refs(&out)
    );
}

/// `useValue: Config.DEFAULT` — emit Config (object) not DEFAULT (property)?
/// Conservative: emit Config via ident_name on member? Currently none.
/// Require at least the provide token and no property noise.
#[test]
fn nest_use_value_member_no_property_name() {
    let src = r#"
import { Module } from '@nestjs/common';

@Module({
  providers: [{ provide: 'CONFIG', useValue: Config.DEFAULT }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "CONFIG", "ts.nest.module_providers").is_empty(),
        "provide token; refs={}",
        dump_refs(&out)
    );
    assert!(
        nest_hits(&out, "DEFAULT", "ts.nest.module_providers").is_empty(),
        "must not emit property DEFAULT; refs={}",
        dump_refs(&out)
    );
}

// ── Python S: importlib.__import__ / builtins.eval alias ────────────

/// `importlib.__import__` is the dynamic import escape hatch.
#[test]
fn py_importlib_dunder_import_leaves_s() {
    let src = r#"
import importlib
importlib.__import__("os")
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "importlib.__import__ must leave S: {:?}",
        r.violations
    );
}

/// `from importlib import __import__` then bare `__import__("os")`.
#[test]
fn py_from_importlib_import_dunder_leaves_s() {
    let src = r#"
from importlib import __import__
__import__("os")
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "from importlib import __import__ must leave S: {:?}",
        r.violations
    );
}

/// `import builtins` then `e = builtins.eval` then `e("1+1")`.
#[test]
fn py_builtins_eval_alias_after_import_leaves_s() {
    let src = r#"
import builtins
e = builtins.eval
e("1+1")
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "builtins.eval alias must leave S: {:?}",
        r.violations
    );
}

/// `eval = lambda s: s` local shadow — fail-closed OK (over-flag).
/// Document only: must still leave S (fail-closed).
#[test]
fn py_local_eval_shadow_fail_closed() {
    let src = r#"
def f(s):
    eval = lambda x: x
    return eval(s)
"#;
    let r = scan_subset(src, Language::Python, "a.py");
    // Fail-closed is acceptable; this pins the intentional over-flag.
    assert!(
        !r.in_subset,
        "local eval shadow must fail-closed leave S (over-flag OK): {:?}",
        r.violations
    );
}

// ── JS S: local const eval shadow ───────────────────────────────────

/// `const eval = () => {}` shadow — fail-closed leave S is OK.
#[test]
fn js_local_eval_shadow_fail_closed() {
    let src = r#"
function f() {
  const eval = (s) => s;
  return eval("1+1");
}
"#;
    let r = scan_subset(src, Language::JavaScript, "a.js");
    assert!(
        !r.in_subset,
        "const eval shadow must fail-closed leave S: {:?}",
        r.violations
    );
}

/// `const Function = () => {}` shadow — fail-closed leave S is OK.
#[test]
fn js_local_function_shadow_fail_closed() {
    let src = r#"
function f() {
  const Function = () => 0;
  return Function();
}
"#;
    let r = scan_subset(src, Language::JavaScript, "a.js");
    assert!(
        !r.in_subset,
        "const Function shadow must fail-closed leave S: {:?}",
        r.violations
    );
}

/// Type-only `typeof Function` / type position — should NOT leave S
/// (no runtime call). If we over-flag, that's fail-closed OK; prefer stay in S.
#[test]
fn ts_typeof_function_type_position_stays_in_s() {
    let src = r#"
export type Fn = typeof Function;
export function f(): Fn {
  return Function;
}
"#;
    // Returning Function as a value IS an escape hatch — leave S.
    let r = scan_subset(src, Language::TypeScript, "a.ts");
    assert!(
        !r.in_subset,
        "returning Function value must leave S: {:?}",
        r.violations
    );
}

/// Type-only `typeof Function` in a type alias must NOT leave S
/// (M2 over-flag fix: type positions are not runtime Function uses).
#[test]
fn ts_typeof_function_only_type_annotation_stays_in_s() {
    let src = r#"
type F = typeof Function;
export function id(x: number): number { return x; }
"#;
    let r = scan_subset(src, Language::TypeScript, "a.ts");
    assert!(
        r.in_subset,
        "typeof Function in type alias must stay in S (type-only): {:?}",
        r.violations
    );
}

/// Interface/type-annotation `Function` stays in S; value use still leaves S.
#[test]
fn ts_interface_function_type_stays_in_s_value_use_leaves() {
    let type_only = r#"
export interface CtorLike {
  new (...args: unknown[]): unknown;
  prototype: object;
}
export type AnyFn = Function;
export function make(h: Function): Function {
  return h;
}
"#;
    let r = scan_subset(type_only, Language::TypeScript, "types.ts");
    assert!(
        r.in_subset,
        "interface + type-annotation Function must stay in S: {:?}",
        r.violations
    );

    let value_use = r#"
export function make(code: string) {
  return Function(code);
}
"#;
    let r2 = scan_subset(value_use, Language::TypeScript, "value.ts");
    assert!(
        !r2.in_subset,
        "Function(value) call must leave S: {:?}",
        r2.violations
    );
}

// ── Go extract: method values as callee ─────────────────────────────

/// `h := s.Handle; h(req)` — method value used as callee.
/// L0/L1 should emit Handle (or s.Handle) as a call edge candidate.
#[test]
fn go_method_value_as_callee() {
    let src = r#"
package main

type Server struct{}

func (s *Server) Handle(req string) string { return req }

func main() {
	s := &Server{}
	h := s.Handle
	h("x")
}
"#;
    let out = extract_lang(src, "main.go", Language::Go);
    let names: Vec<String> = out.references.iter().map(|r| r.name.clone()).collect();
    assert!(
        names.iter().any(|n| n == "Handle" || n == "s.Handle"),
        "method value s.Handle as callee must emit Handle; refs={}",
        dump_refs(&out)
    );
}

// ── Rust L1 impl_trait: generic / trait-object false edges ──────────

/// `impl<T> Trait for T` must NOT invent a concrete implementor edge.
#[test]
fn rust_generic_impl_for_all_t_no_false_implementor() {
    let src = r#"
trait Marker {}
impl<T> Marker for T {}
struct Foo;
fn main() {}
"#;
    let out = extract_lang(src, "a.rs", Language::Rust);
    let impl_hits: Vec<_> = out
        .references
        .iter()
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == "rs.di.impl_trait")
                .unwrap_or(false)
        })
        .collect();
    assert!(
        impl_hits.iter().all(|r| r.name != "T" && r.name != "t"),
        "generic impl<T> must not emit T as implementor; refs={}",
        dump_refs(&out)
    );
}

/// `Box<dyn Trait>` as a type must not invent implementor of Trait.
#[test]
fn rust_box_dyn_trait_type_no_implementor() {
    let src = r#"
trait Handler {}
struct App;
fn takes(h: Box<dyn Handler>) {}
fn main() {}
"#;
    let out = extract_lang(src, "a.rs", Language::Rust);
    let impl_hits: Vec<_> = out
        .references
        .iter()
        .filter(|r| {
            r.evidence
                .as_ref()
                .map(|e| e.rule_id == "rs.di.impl_trait")
                .unwrap_or(false)
        })
        .collect();
    assert!(
        impl_hits.iter().all(|r| r.name != "App"),
        "Box<dyn Handler> must not invent App as Handler implementor; refs={}",
        dump_refs(&out)
    );
}

// ── Store callers: unicode / case ───────────────────────────────────

/// Unicode symbol name must round-trip through callers SQL equality.
#[test]
fn store_unicode_symbol_name_callers() {
    use agentgraph::index::store::Store;
    use agentgraph::model::Confidence;
    use std::path::PathBuf;

    let dir = common::temp_root("ag_r16_unicode");
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("t.db");
    let mut store = Store::open(&db).expect("open");
    let path = "src/服务.ts";
    let extracted = agentgraph::index::extract::ExtractedFile {
        symbols: vec![],
        references: vec![agentgraph::index::extract::ExtractedRef {
            name: "处理请求".into(),
            kind: agentgraph::model::EdgeKind::Call,
            line: 3,
            enclosing: Some("App".into()),
            module: None,
            resolved: None,
            qualifier: None,
            confidence: Confidence::Exact,
            evidence: None,
        }],
    };
    store
        .replace_file(path, "h1", "typescript", &extracted)
        .expect("replace");
    let hits = store.callers("处理请求", 10).expect("callers");
    assert_eq!(hits.len(), 1, "unicode name must match exactly");
    assert_eq!(hits[0].name, "处理请求");
    let _ = std::fs::remove_dir_all(&dir);
    let _: PathBuf = db;
}
