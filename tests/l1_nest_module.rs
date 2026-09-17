//! TDD: real NestJS `@Module` / constructor-DI L1 rules.
//!
//! Mirrors production Nest (nestjs-starter), not inversify `bind`:
//! `@Injectable()` bare + `@Module({ controllers, providers, imports })`
//! + `constructor(private readonly x: X)`.

use agentgraph::index::extract::extract_file;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

fn extract(src: &str, path: &str) -> agentgraph::index::extract::ExtractedFile {
    let known = HashSet::new();
    extract_file(src, Language::TypeScript, path, &known).expect("extract")
}

fn nest_hits<'a>(
    out: &'a agentgraph::index::extract::ExtractedFile,
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

fn dump_refs(out: &agentgraph::index::extract::ExtractedFile) -> String {
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

#[test]
fn nest_module_providers_bare_ident() {
    let src = r#"
import { Module } from '@nestjs/common';
import { AppService } from './app.service';

@Module({
  providers: [AppService],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let hits = nest_hits(&out, "AppService", "ts.nest.module_providers");
    assert!(
        !hits.is_empty(),
        "providers: [AppService] must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert_eq!(
        hits[0].enclosing.as_deref(),
        Some("AppModule"),
        "enclosing must be the module class"
    );
    assert!(hits[0]
        .evidence
        .as_ref()
        .unwrap()
        .snippet
        .contains("AppService"));
}

#[test]
fn nest_module_providers_use_class() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = Symbol('CONFIG');
export class ConfigService {}
export class AppService {}

@Module({
  providers: [AppService, { provide: CONFIG, useClass: ConfigService }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let use_class = nest_hits(&out, "ConfigService", "ts.nest.module_providers");
    assert!(
        !use_class.is_empty(),
        "useClass: ConfigService must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert_eq!(use_class[0].enclosing.as_deref(), Some("AppModule"));
    // bare ident in the same array still fires
    assert!(
        !nest_hits(&out, "AppService", "ts.nest.module_providers").is_empty(),
        "bare AppService in providers still required"
    );
}

#[test]
fn nest_module_controllers() {
    let src = r#"
import { Module } from '@nestjs/common';
import { AppController } from './app.controller';

@Module({
  controllers: [AppController],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let hits = nest_hits(&out, "AppController", "ts.nest.module_controllers");
    assert!(
        !hits.is_empty(),
        "controllers: [AppController] must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert_eq!(hits[0].enclosing.as_deref(), Some("AppModule"));
}

#[test]
fn nest_module_imports_ident_and_for_root() {
    let src = r#"
import { Module } from '@nestjs/common';
import { OtherModule } from './other.module';

@Module({
  imports: [OtherModule, ObserveModule.forRoot({ appKey: 'x' })],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let other = nest_hits(&out, "OtherModule", "ts.nest.module_imports");
    assert!(
        !other.is_empty(),
        "imports: [OtherModule] must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert_eq!(other[0].enclosing.as_deref(), Some("AppModule"));
    let observe = nest_hits(&out, "ObserveModule", "ts.nest.module_imports");
    assert!(
        !observe.is_empty(),
        "imports: [ObserveModule.forRoot()] must yield Heuristic to ObserveModule; refs={}",
        dump_refs(&out)
    );
    assert_eq!(observe[0].enclosing.as_deref(), Some("AppModule"));
}

#[test]
fn nest_ctor_inject_typed_param() {
    let src = r#"
import { Controller } from '@nestjs/common';
import { AppService } from './app.service';

@Controller()
export class AppController {
  constructor(private readonly appService: AppService) {}
}
"#;
    let out = extract(src, "src/app.controller.ts");
    let hits = nest_hits(&out, "AppService", "ts.nest.ctor_inject");
    assert!(
        !hits.is_empty(),
        "constructor(private appService: AppService) must yield Heuristic; refs={}",
        dump_refs(&out)
    );
    assert_eq!(
        hits[0].enclosing.as_deref(),
        Some("AppController"),
        "ctor inject enclosing must be the class, not 'constructor'"
    );
}

#[test]
fn nest_ctor_inject_not_for_plain_methods() {
    // Typed params on non-constructor methods must NOT fire ctor_inject.
    let src = r#"
export class AppController {
  helper(svc: AppService) { return svc; }
}
"#;
    let out = extract(src, "src/app.controller.ts");
    assert!(
        nest_hits(&out, "AppService", "ts.nest.ctor_inject").is_empty(),
        "non-constructor typed param must not yield ctor_inject; refs={}",
        dump_refs(&out)
    );
}

#[test]
fn nest_module_non_export_class() {
    // Decorator hangs on class_declaration when there is no `export`.
    let src = r#"
import { Module } from '@nestjs/common';
import { AppService } from './app.service';

@Module({ providers: [AppService] })
class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let hits = nest_hits(&out, "AppService", "ts.nest.module_providers");
    assert!(
        !hits.is_empty(),
        "non-export @Module must still fire; refs={}",
        dump_refs(&out)
    );
    assert_eq!(hits[0].enclosing.as_deref(), Some("AppModule"));
}

#[test]
fn nest_module_trailing_comma_and_multi_ident() {
    let src = r#"
import { Module } from '@nestjs/common';

@Module({
  controllers: [AppController, ],
  providers: [
    AppService,
    OtherService,
  ],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "AppController", "ts.nest.module_controllers").is_empty(),
        "trailing comma controllers; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "AppService", "ts.nest.module_providers").is_empty(),
        "multi providers AppService; refs={}",
        dump_refs(&out)
    );
    assert!(
        !nest_hits(&out, "OtherService", "ts.nest.module_providers").is_empty(),
        "multi providers OtherService; refs={}",
        dump_refs(&out)
    );
}

#[test]
fn nest_module_sibling_decorators_still_attribute_class() {
    // @Global() + @Module() — enclosing must still be AppModule.
    let src = r#"
import { Global, Module } from '@nestjs/common';
import { AppService } from './app.service';

@Global()
@Module({
  providers: [AppService],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let hits = nest_hits(&out, "AppService", "ts.nest.module_providers");
    assert!(
        !hits.is_empty(),
        "multi-decorator @Module must fire; refs={}",
        dump_refs(&out)
    );
    assert_eq!(hits[0].enclosing.as_deref(), Some("AppModule"));
}

#[test]
fn nest_no_fake_bare_decorator_edges() {
    // Bare @Injectable()/@Controller() must not invent callee-name edges.
    let src = r#"
import { Controller, Injectable } from '@nestjs/common';

@Injectable()
export class AppService {}

@Controller()
export class AppController {}
"#;
    let out = extract(src, "src/empty.ts");
    let invents = out.references.iter().any(|r| {
        r.confidence == Confidence::Heuristic
            && matches!(r.name.as_str(), "Injectable" | "Controller")
    });
    assert!(
        !invents,
        "bare decorators must not invent Injectable/Controller edges; refs={}",
        dump_refs(&out)
    );
}

#[test]
fn nest_real_starter_shape_end_to_end() {
    // Compact mirror of eval-corpus/nestjs-starter sources.
    let module_src = r#"
import { Module } from '@nestjs/common';
import { AppController } from './app.controller';
import { AppService } from './app.service';

export const { ObserveModule } = { ObserveModule: { forRoot: (_o: any) => class {} } };

@Module({
  imports: [
    ObserveModule.forRoot({
      appKey: 'x',
    }),
  ],
  controllers: [AppController],
  providers: [AppService],
})
export class AppModule {}
"#;
    let out = extract(module_src, "src/app.module.ts");
    for (name, rule) in [
        ("AppService", "ts.nest.module_providers"),
        ("AppController", "ts.nest.module_controllers"),
        ("ObserveModule", "ts.nest.module_imports"),
    ] {
        let hits = nest_hits(&out, name, rule);
        assert!(
            !hits.is_empty(),
            "{name} via {rule} missing; refs={}",
            dump_refs(&out)
        );
        assert_eq!(hits[0].enclosing.as_deref(), Some("AppModule"));
    }

    let controller_src = r#"
import { Controller, Get } from '@nestjs/common';
import { AppService } from './app.service';

@Controller()
export class AppController {
  constructor(private readonly appService: AppService) {}

  @Get()
  getHello(): string {
    return this.appService.getHello();
  }
}
"#;
    let cout = extract(controller_src, "src/app.controller.ts");
    let hits = nest_hits(&cout, "AppService", "ts.nest.ctor_inject");
    assert!(
        !hits.is_empty(),
        "starter ctor inject missing; refs={}",
        dump_refs(&cout)
    );
    assert_eq!(hits[0].enclosing.as_deref(), Some("AppController"));
}

/// R14: Nest circular DI — `forwardRef` must unwrap to the real module,
/// never invent an edge to the `forwardRef` helper itself.
#[test]
fn nest_imports_forward_ref_resolves_inner_module() {
    let src = r#"
import { Module, forwardRef } from '@nestjs/common';
import { AuthModule } from './auth.module';

@Module({
  imports: [forwardRef(() => AuthModule)],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    let inner = nest_hits(&out, "AuthModule", "ts.nest.module_imports");
    assert!(
        !inner.is_empty(),
        "forwardRef(() => AuthModule) must yield AuthModule; refs={}",
        dump_refs(&out)
    );
    assert_eq!(inner[0].enclosing.as_deref(), Some("AppModule"));
    let bogus = nest_hits(&out, "forwardRef", "ts.nest.module_imports");
    assert!(
        bogus.is_empty(),
        "must NOT invent edge to forwardRef itself; refs={}",
        dump_refs(&out)
    );
}

#[test]
fn nest_imports_forward_ref_bare_ident() {
    let src = r#"
import { Module, forwardRef } from '@nestjs/common';
import { AuthModule } from './auth.module';

@Module({
  imports: [forwardRef(AuthModule)],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "AuthModule", "ts.nest.module_imports").is_empty(),
        "forwardRef(AuthModule) must yield AuthModule; refs={}",
        dump_refs(&out)
    );
    assert!(
        nest_hits(&out, "forwardRef", "ts.nest.module_imports").is_empty(),
        "must NOT invent forwardRef edge; refs={}",
        dump_refs(&out)
    );
}

#[test]
fn nest_providers_use_factory_still_emits_token() {
    let src = r#"
import { Module } from '@nestjs/common';
export const CONFIG = 'CONFIG';

@Module({
  providers: [{ provide: CONFIG, useFactory: () => ({}) }],
})
export class AppModule {}
"#;
    let out = extract(src, "src/app.module.ts");
    assert!(
        !nest_hits(&out, "CONFIG", "ts.nest.module_providers").is_empty(),
        "useFactory provider must still emit provide token; refs={}",
        dump_refs(&out)
    );
}
