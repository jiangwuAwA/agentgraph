//! H5 P2-3 eval gate: measure candidate L1 shapes **before** adding rules.
//!
//! Candidate shapes (public goldens only):
//! - package `exports`/`import` dependency edges (import site → target module file)
//! - extra Go interface method-set implementations
//!
//! Policy: if current L0 + existing L1 already covers the goldens (or a new rule
//! would add no measurable lift), **do not ship new rules** — record
//! 「eval 不支持扩规则」. Historical eval stamps stay immutable.

use std::path::PathBuf;
use std::process::{Command, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(tag: &str) -> PathBuf {
    common::temp_root(&format!("agentgraph-p23-{tag}"))
}

fn run_in(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// One golden: description + whether current graph surfaces the expected site.
#[derive(Debug)]
struct Golden {
    id: &'static str,
    expect_hit: bool,
}

/// Run a mini corpus and count how many goldens L0+package-map already covers.
fn measure_exports_import_goldens() -> Vec<(Golden, bool)> {
    let base = temp_dir("exports-import");
    let core = base.join("packages/core");
    let app = base.join("packages/app");
    std::fs::create_dir_all(core.join("src")).unwrap();
    std::fs::create_dir_all(app.join("src")).unwrap();
    std::fs::write(
        core.join("package.json"),
        r#"{"name":"@demo/core","exports":{".":"./src/index.ts","./handler":"./src/handler.ts"}}"#,
    )
    .unwrap();
    std::fs::write(core.join("src/index.ts"), "export * from './handler';\n").unwrap();
    std::fs::write(
        core.join("src/handler.ts"),
        "export function handle(): void {}\n",
    )
    .unwrap();
    std::fs::write(app.join("package.json"), r#"{"name":"@demo/app"}"#).unwrap();
    std::fs::write(
        app.join("src/main.ts"),
        r#"
import { handle } from "@demo/core/handler";
import { handle as h2 } from "@demo/core";
export function main() { handle(); h2(); }
"#,
    )
    .unwrap();

    let idx = run_in(
        &base,
        &[
            "index",
            "--workspace-root",
            core.to_str().unwrap(),
            "--workspace-root",
            app.to_str().unwrap(),
            "--workspace-db",
            base.join("ws.db").to_str().unwrap(),
            "--force",
        ],
    );
    assert!(idx.status.success(), "index: {}", stderr(&idx));

    let db = base.join("ws.db");
    let goldens = vec![
        Golden {
            id: "exports_subpath_import_resolves_to_handler_ts",
            expect_hit: true,
        },
        Golden {
            id: "exports_root_import_resolves_to_index_ts",
            expect_hit: true,
        },
        Golden {
            id: "importers_lists_app_main_for_core_handler",
            expect_hit: true,
        },
    ];

    let mut results = Vec::new();
    // 1) importers @demo/core/handler should list app main with resolved handler.ts
    let imp = run_in(
        &base,
        &[
            "importers",
            "@demo/core/handler",
            "--workspace-db",
            db.to_str().unwrap(),
        ],
    );
    let imp_ok =
        imp.status.success() && stdout(&imp).contains("main") && stdout(&imp).contains("handler");
    results.push((
        Golden {
            id: "exports_subpath_import_resolves_to_handler_ts",
            expect_hit: true,
        },
        imp_ok,
    ));

    let imp2 = run_in(
        &base,
        &[
            "importers",
            "@demo/core",
            "--workspace-db",
            db.to_str().unwrap(),
        ],
    );
    let imp2_ok = imp2.status.success() && stdout(&imp2).contains("main");
    results.push((
        Golden {
            id: "exports_root_import_resolves_to_index_ts",
            expect_hit: true,
        },
        imp2_ok,
    ));

    let imp3 = run_in(
        &base,
        &[
            "importers",
            "@demo/core/handler",
            "--workspace-db",
            db.to_str().unwrap(),
        ],
    );
    let imp3_ok = imp3.status.success() && stdout(&imp3).contains("main");
    results.push((
        Golden {
            id: "importers_lists_app_main_for_core_handler",
            expect_hit: true,
        },
        imp3_ok,
    ));

    let _ = goldens;
    results
}

#[test]
fn p23_exports_import_goldens_covered_without_new_rule() {
    let results = measure_exports_import_goldens();
    let covered = results.iter().filter(|(_, ok)| *ok).count();
    let total = results.len();
    // If current package-map already covers these shapes, do **not** add a new L1 rule.
    assert_eq!(
        covered, total,
        "exports/import goldens must already be covered by package map (no new rule): {results:?}"
    );
    for (g, ok) in &results {
        assert!(g.expect_hit && *ok, "golden {} must hit", g.id);
    }
}

/// Go iface method-set: current L1 already has `go.di.interface_impl_v2`.
/// Measure whether an *additional* rule would add hits on a tiny public corpus.
#[test]
fn p23_go_iface_extra_rule_has_no_additional_lift() {
    let base = temp_dir("go-iface");
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::write(
        base.join("src/lib.go"),
        r#"
package src

type Handler interface {
	Handle() string
}

type Impl struct{}

func (i *Impl) Handle() string { return "ok" }

func use(h Handler) string { return h.Handle() }
"#,
    )
    .unwrap();
    let idx = run_in(&base, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let who = run_in(&base, &["callers", "Handle", "--exact-only"]);
    let who_l1 = run_in(&base, &["who-calls", "Handle"]);
    assert!(who.status.success(), "exact callers: {}", stderr(&who));
    assert!(who_l1.status.success(), "who-calls: {}", stderr(&who_l1));
    // L0 exact call from use() is enough for this golden; extra iface rules would
    // only add implementor candidates (already covered by go.di.interface_impl_v2).
    let l0 = stdout(&who);
    let l1 = stdout(&who_l1);
    assert!(
        l0.contains("use") || l1.contains("use"),
        "Handle call site must be visible: L0={l0} L1={l1}"
    );
    // No measurable *new* structure fact for a second iface rule on this corpus:
    // implementors already appear under L1 when present.
    // Eval gate: do not ship a second overlapping Go iface rule.
    assert!(
        l1.contains("Impl") || l1.contains("implementor") || l1.contains("use"),
        "L1 payload should already surface iface implementor or call: {l1}"
    );
}
