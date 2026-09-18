//! R32+: multi-root path alias / re-export recall gap (P0-5d review follow-up).
//!
//! `export { RegistryClient } from "./client"` must yield findable structure
//! so blast/who-calls file sets can include `index.ts` (not only `client.ts`).
//! Package-style imports (`@demo/registry`) in another root must still
//! connect via ctor/type names already extracted.

use agentgraph::index::extract::{extract_file, ExtractedFile};
use agentgraph::model::Language;
use std::collections::HashSet;

fn extract(src: &str, path: &str) -> ExtractedFile {
    let known = HashSet::from([path.to_string(), "src/client.ts".to_string()]);
    extract_file(src, Language::TypeScript, path, &known).expect("extract ts")
}

#[test]
fn ts_export_from_mints_reexport_symbols_for_find() {
    let src = r#"export { RegistryClient, createClient } from "./client";
export { Other } from "./other";
"#;
    let out = extract(src, "src/index.ts");
    let names: Vec<_> = out.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"RegistryClient"),
        "re-export must mint symbol RegistryClient at index.ts, got {names:?}"
    );
    assert!(
        names.contains(&"createClient"),
        "re-export must mint createClient, got {names:?}"
    );
}

#[test]
fn ts_export_from_with_as_alias() {
    let src = r#"export { RegistryClient as Rc } from "./client";
"#;
    let out = extract(src, "src/index.ts");
    assert!(
        out.symbols.iter().any(|s| s.name == "Rc"),
        "export-as must mint the public alias name Rc, got {:?}",
        out.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn ts_export_from_mints_import_or_export_refs() {
    let src = r#"export { RegistryClient } from "./client";
"#;
    let out = extract(src, "src/index.ts");
    let has_ref = out.references.iter().any(|r| r.name == "RegistryClient");
    assert!(
        has_ref,
        "export-from should record a ref for RegistryClient; refs={:?}",
        out.references
            .iter()
            .map(|r| (&r.name, format!("{:?}", r.kind)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_name_import_still_extracts_type_refs() {
    let src = r#"
import { RegistryClient } from "@demo/registry";

export class OrderService {
  constructor(private client: RegistryClient) {}
  load(id: string) { return this.client.fetch(id); }
}
"#;
    let out = extract(src, "src/order.service.ts");
    assert!(
        out.references
            .iter()
            .any(|r| r.name == "RegistryClient" && r.enclosing.as_deref() == Some("OrderService")),
        "package-name import + ctor type must yield RegistryClient ref; refs={:?}",
        out.references
            .iter()
            .map(|r| (&r.name, format!("{:?}", r.kind), r.enclosing.clone()))
            .collect::<Vec<_>>()
    );
}
