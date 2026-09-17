//! R15 adversarial probes: remaining Nest L1 + Go/Rust S + store edges.

use agentgraph::index::extract::{extract_file, ExtractedFile};
use agentgraph::index::subset::scan_subset;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

fn extract(src: &str, path: &str) -> ExtractedFile {
    let known = HashSet::new();
    extract_file(src, Language::TypeScript, path, &known).expect("extract")
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

// ── Nest L1 remaining ───────────────────────────────────────────────

/// `inject: [Dep]` is a real Nest dependency declaration; must emit Heuristic.
#[test]
fn nest_providers_inject_array_dep() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = 'CONFIG';
export class ConfigService {}

@Module({
  providers: [
    {
      provide: CONFIG,
      useFactory: () => ({}),
      inject: [ConfigService],
    },
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let hits = nest_hits(&out, "ConfigService", "ts.nest.module_providers");
    assert!(
        !hits.is_empty(),
        "inject: [ConfigService] must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert_eq!(hits[0].enclosing.as_deref(), Some("AppModule"));
}

/// `useFactory: () => new ConfigService()` must register ConfigService.
#[test]
fn nest_providers_use_factory_new_expr() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = 'CONFIG';
export class ConfigService {}

@Module({
  providers: [{ provide: CONFIG, useFactory: () => new ConfigService() }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let hits = nest_hits(&out, "ConfigService", "ts.nest.module_providers");
    assert!(
        !hits.is_empty(),
        "useFactory: () => new ConfigService() must yield ConfigService; refs={}",
        dump_refs(&out)
    );
    assert_eq!(hits[0].enclosing.as_deref(), Some("AppModule"));
}

/// `useFactory: () => ConfigService` (bare ident return).
#[test]
fn nest_providers_use_factory_bare_return() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = 'CONFIG';
export class ConfigService {}

@Module({
  providers: [{ provide: CONFIG, useFactory: () => ConfigService }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "ConfigService", "ts.nest.module_providers").is_empty(),
        "useFactory: () => ConfigService must yield ConfigService; refs={}",
        dump_refs(&out)
    );
}

/// `useFactory: () => { return createConfig(); }` block body.
#[test]
fn nest_providers_use_factory_block_body_call() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = 'CONFIG';
export function createConfig() { return {}; }

@Module({
  providers: [{ provide: CONFIG, useFactory: () => { return createConfig(); } }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "createConfig", "ts.nest.module_providers").is_empty(),
        "useFactory block-body createConfig() must yield edge; refs={}",
        dump_refs(&out)
    );
}

/// String injection tokens are first-class Nest: `@Inject('AppService')` +
/// `provide: 'AppService'`. Both must yield the token string.
#[test]
fn nest_inject_string_token_and_provide_string() {
    let src = r#"
import { Injectable, Inject, Module } from '@nestjs/common';
export class AppService {}

@Injectable()
export class Consumer {
  constructor(@Inject('AppService') private svc: AppService) {}
}

@Module({
  providers: [
    { provide: 'AppService', useClass: AppService },
    Consumer,
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let provide_tok = nest_hits(&out, "AppService", "ts.nest.module_providers");
    assert!(
        !provide_tok.is_empty(),
        "provide: 'AppService' string token must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    let inject_tok = nest_hits(&out, "AppService", "ts.di.decorator");
    assert!(
        !inject_tok.is_empty(),
        "@Inject('AppService') string token must yield Heuristic; refs={}",
        dump_refs(&out)
    );
}

/// `useExisting: 'OTHER_TOKEN'` string form.
#[test]
fn nest_providers_use_existing_string() {
    let src = r#"
import { Module } from '@nestjs/common';

@Module({
  providers: [
    { provide: 'Alias', useExisting: 'AppService' },
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "AppService", "ts.nest.module_providers").is_empty(),
        "useExisting: 'AppService' must yield Heuristic; refs={}",
        dump_refs(&out)
    );
}

/// `new TYPES.ConfigService()` must emit ConfigService, not the namespace `TYPES`.
#[test]
fn nest_use_factory_new_member_uses_type_not_namespace() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = 'CONFIG';
export namespace TYPES { export class ConfigService {} }

@Module({
  providers: [{ provide: CONFIG, useFactory: () => new TYPES.ConfigService() }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "ConfigService", "ts.nest.module_providers").is_empty(),
        "new TYPES.ConfigService() must yield ConfigService; refs={}",
        dump_refs(&out)
    );
    assert!(
        nest_hits(&out, "TYPES", "ts.nest.module_providers").is_empty(),
        "must NOT emit namespace TYPES; refs={}",
        dump_refs(&out)
    );
}

/// Factory body member property is not a registration target.
#[test]
fn nest_use_factory_member_property_not_emitted() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = 'CONFIG';
const config = { default: {} };

@Module({
  providers: [{ provide: CONFIG, useFactory: () => config.default }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        nest_hits(&out, "default", "ts.nest.module_providers").is_empty(),
        "config.default must NOT emit 'default'; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "CONFIG", "ts.nest.module_providers").is_empty(),
        "provide token still required; refs={}",
        dump_refs(&out)
    );
}

// ── Go S: alias / dot-import of unsafe ─────────────────────────────

/// `import u "unsafe"` then `u.Pointer` — import path still unsafe.
#[test]
fn go_aliased_unsafe_import_leaves_s() {
    let src = r#"
package main

import u "unsafe"

func evil(p *int) uintptr {
	return uintptr(u.Pointer(p))
}

func main() {}
"#;
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        !r.in_subset,
        "aliased unsafe import must leave S: {:?}",
        r.violations
    );
}

/// `import . "unsafe"` then bare `Pointer`.
#[test]
fn go_dot_import_unsafe_leaves_s() {
    let src = r#"
package main

import . "unsafe"

func evil(p *int) uintptr {
	return uintptr(Pointer(p))
}

func main() {}
"#;
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        !r.in_subset,
        "dot-import unsafe must leave S: {:?}",
        r.violations
    );
}

// ── Rust S: transmute via path alias ───────────────────────────────

/// `use core::mem::transmute as t; t(x)` — use leaves S already, but
/// also ensure the call itself is not a false claim if use is stripped.
#[test]
fn rust_transmute_alias_use_leaves_s() {
    let src = "use core::mem::transmute as t;\nfn f(x: u32) -> i32 {\n    unsafe { t(x) }\n}\n";
    let r = scan_subset(src, Language::Rust, "a.rs");
    assert!(
        !r.in_subset,
        "transmute-as-t use must leave S: {:?}",
        r.violations
    );
}
