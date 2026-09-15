//! TDD: L1 heuristic / dynamic-candidate edges from DI, reflection, maps.
//! Each rule must produce a ref with the right confidence + evidence.

use agentgraph::index::extract::extract_file;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;

fn extract(src: &str, lang: Language, path: &str) -> agentgraph::index::extract::ExtractedFile {
    let known = HashSet::new();
    extract_file(src, lang, path, &known).expect("extract")
}

fn find_refs<'a>(
    out: &'a agentgraph::index::extract::ExtractedFile,
    name: &str,
    conf: Confidence,
) -> Vec<&'a agentgraph::index::extract::ExtractedRef> {
    out.references
        .iter()
        .filter(|r| r.name == name && r.confidence == conf)
        .collect()
}

#[test]
fn ts_container_register_is_heuristic() {
    let src = r#"
import { Container } from 'inversify';
export class UserService {
  load() { return 1; }
}
export function bootstrap(c: Container) {
  c.register(UserService);
}
"#;
    let out = extract(src, Language::TypeScript, "src/di.ts");
    let hits = find_refs(&out, "UserService", Confidence::Heuristic);
    assert!(
        !hits.is_empty(),
        "container.register(UserService) must yield Heuristic edge; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence))
            .collect::<Vec<_>>()
    );
    let ev = hits[0].evidence.as_ref().expect("evidence");
    assert!(ev.rule_id.starts_with("ts.di."), "rule_id={}", ev.rule_id);
    assert!(ev.snippet.contains("UserService"));
}

#[test]
fn ts_bind_to_is_heuristic() {
    let src = r#"
export class UserService {}
export class InMemoryUserService {}
export function wire(c: any) {
  c.bind(UserService).to(InMemoryUserService);
}
"#;
    let out = extract(src, Language::TypeScript, "src/bind.ts");
    assert!(
        !find_refs(&out, "UserService", Confidence::Heuristic).is_empty(),
        "bind(X) must yield Heuristic"
    );
    assert!(
        !find_refs(&out, "InMemoryUserService", Confidence::Heuristic).is_empty(),
        "to(Y) must yield Heuristic"
    );
}

#[test]
fn ts_inject_decorator_is_heuristic() {
    let src = r#"
import { Inject, Injectable } from 'inversify';
@Injectable()
export class UserService {}
export class Controller {
  constructor(@Inject(UserService) private svc: UserService) {}
}
"#;
    let out = extract(src, Language::TypeScript, "src/inject.ts");
    assert!(
        !find_refs(&out, "UserService", Confidence::Heuristic).is_empty(),
        "@Inject(UserService) must yield Heuristic; got {:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn ts_computed_string_call_is_dynamic_candidate() {
    let src = r#"
export function doWork() { return 1; }
export function run(obj: any) {
  obj['doWork']();
}
"#;
    let out = extract(src, Language::TypeScript, "src/dyn.ts");
    let hits = find_refs(&out, "doWork", Confidence::DynamicCandidate);
    assert!(
        !hits.is_empty(),
        "obj['doWork']() must yield DynamicCandidate; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(hits[0]
        .evidence
        .as_ref()
        .unwrap()
        .rule_id
        .contains("dynamic"));
}

#[test]
fn ts_event_handler_subscription_is_heuristic() {
    let src = r#"
export function handleClick() { return 1; }
export function wire(emitter: any) {
  emitter.on('click', handleClick);
}
"#;
    let out = extract(src, Language::TypeScript, "src/evt.ts");
    assert!(
        !find_refs(&out, "handleClick", Confidence::Heuristic).is_empty(),
        "on('click', handleClick) must yield Heuristic"
    );
}

#[test]
fn py_depends_is_heuristic() {
    let src = r#"
from fastapi import Depends

def get_user_service():
    return UserService()

def read_users(svc = Depends(get_user_service)):
    return svc.load()
"#;
    let out = extract(src, Language::Python, "app/api.py");
    assert!(
        !find_refs(&out, "get_user_service", Confidence::Heuristic).is_empty(),
        "Depends(get_user_service) must yield Heuristic; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn py_getattr_string_is_dynamic() {
    let src = r#"
def compute():
    return 1

def run(obj):
    return getattr(obj, "compute")()
"#;
    let out = extract(src, Language::Python, "app/reflect.py");
    assert!(
        !find_refs(&out, "compute", Confidence::DynamicCandidate).is_empty(),
        "getattr(obj, \"compute\") must yield DynamicCandidate"
    );
}

#[test]
fn py_importlib_import_module_is_dynamic() {
    let src = r#"
import importlib

def load():
    return importlib.import_module("myapp.handlers")
"#;
    let out = extract(src, Language::Python, "app/load.py");
    assert!(
        !find_refs(&out, "myapp.handlers", Confidence::DynamicCandidate).is_empty(),
        "import_module(\"...\") must yield DynamicCandidate to the module path"
    );
}

#[test]
fn go_handler_map_is_heuristic() {
    let src = r#"
package main

import "net/http"

func GetUsers(w http.ResponseWriter, r *http.Request) {}
func Health(w http.ResponseWriter, r *http.Request) {}

var routes = map[string]http.HandlerFunc{
	"/users":  GetUsers,
	"/health": Health,
}

func main() {}
"#;
    let out = extract(src, Language::Go, "main.go");
    assert!(
        !find_refs(&out, "GetUsers", Confidence::Heuristic).is_empty(),
        "map[string]Handler registration must yield Heuristic for GetUsers; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(
        !find_refs(&out, "Health", Confidence::Heuristic).is_empty(),
        "map registration must yield Heuristic for Health"
    );
}

#[test]
fn rust_impl_trait_for_type_is_heuristic() {
    let src = r#"
trait Greeter {
    fn greet(&self) -> String;
}

struct EnglishGreeter;

impl Greeter for EnglishGreeter {
    fn greet(&self) -> String {
        String::from("hello")
    }
}

fn use_it(g: &dyn Greeter) -> String {
    g.greet()
}
"#;
    let out = extract(src, Language::Rust, "src/greet.rs");
    // Implementation of trait method is a possible dynamic-dispatch target.
    let hits: Vec<_> = out
        .references
        .iter()
        .filter(|r| r.name == "greet" && r.confidence == Confidence::Heuristic)
        .collect();
    assert!(
        !hits.is_empty(),
        "impl Greeter for EnglishGreeter::greet must yield Heuristic; refs={:?}",
        out.references
            .iter()
            .map(|r| (
                r.name.clone(),
                r.qualifier.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone())
            ))
            .collect::<Vec<_>>()
    );
    assert!(
        hits[0]
            .qualifier
            .as_deref()
            .map(|q| q.contains("EnglishGreeter"))
            .unwrap_or(false)
            || hits[0]
                .evidence
                .as_ref()
                .map(|e| e.snippet.contains("EnglishGreeter"))
                .unwrap_or(false),
        "heuristic edge should identify EnglishGreeter"
    );
}

#[test]
fn ts_new_registry_computed_is_dynamic() {
    let src = r#"
export class ClickHandler { onClick() { return 1; } }
export function createFromRegistry(registry: any) {
  return new (registry["ClickHandler"])();
}
"#;
    let out = extract(src, Language::TypeScript, "src/newdyn.ts");
    assert!(
        !find_refs(&out, "ClickHandler", Confidence::DynamicCandidate).is_empty(),
        "new (registry[\"ClickHandler\"])() must yield DynamicCandidate; refs={:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn l0_calls_remain_exact_alongside_l1() {
    let src = r#"
export class UserService {}
export function bootstrap(c: any) {
  c.register(UserService);
  helper();
}
export function helper() { return 1; }
"#;
    let out = extract(src, Language::TypeScript, "src/mixed.ts");
    assert!(
        !find_refs(&out, "helper", Confidence::Exact).is_empty(),
        "L0 direct call stays Exact"
    );
    assert!(
        !find_refs(&out, "UserService", Confidence::Heuristic).is_empty(),
        "L1 DI edge coexists"
    );
}
