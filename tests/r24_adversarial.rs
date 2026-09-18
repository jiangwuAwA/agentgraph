//! R24 adversarial probes: keep-set completeness for UTF-8/read failures,
//! watch (index_paths) oversized/minified S certification, S edge forms,
//! file_uri unicode / already-encoded, nest circular, impact sound mix.

use agentgraph::index::export::file_uri;
use agentgraph::index::parser::rel_path_under_root;
use agentgraph::index::subset::scan_subset;
use agentgraph::index::Indexer;
use agentgraph::model::Language;
use std::path::{Path, PathBuf};

mod common;

fn temp_root(tag: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-r24-{tag}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
    dir
}

// ── Surface 1: keep-set completeness — UTF-8 / read failures ───────────

/// A binary / non-UTF-8 source file is collected by the walker (extension
/// matches) but full `index()` skips it without minting an S violation.
/// `--sound` then claims in_subset=true while the file was never certified.
#[test]
fn utf8_fail_mints_parse_error_violation() {
    let root = temp_root("utf8");
    std::fs::write(
        root.join("src/ok.ts"),
        "export function helper() { return 1; }\n",
    )
    .unwrap();
    // Valid .ts extension, invalid UTF-8 bytes.
    std::fs::write(
        root.join("src/bad.ts"),
        [0xffu8, 0xfe, 0x00, 0x01, 0xc0, 0x81],
    )
    .unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    let stats = indexer.index(true).expect("index");
    assert!(
        stats.failed_files >= 1,
        "UTF-8 fail must count as failed; {stats:?}"
    );

    let store = indexer.open_store().unwrap();
    let viols = store.subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("bad.ts")),
        "non-UTF-8 source must mint parse_error S violation; got {viols:?}"
    );

    // Second force index: violation must survive prune.
    indexer.index(true).expect("index2");
    let viols2 = indexer.open_store().unwrap().subset_violations().unwrap();
    assert!(
        viols2
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("bad.ts")),
        "UTF-8 parse_error must survive reindex; got {viols2:?}"
    );
}

/// Same hole on the watch path: index_paths UTF-8 fail is silent.
#[test]
fn index_paths_utf8_fail_mints_parse_error() {
    let root = temp_root("utf8-ip");
    let bad = root.join("src/bad.ts");
    std::fs::write(&bad, [0xffu8, 0xfe, 0x00, 0x01]).unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index_paths(std::slice::from_ref(&bad)).expect("ip");

    let viols = indexer.open_store().unwrap().subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("bad.ts")),
        "index_paths UTF-8 fail must mint parse_error; got {viols:?}"
    );
}

// ── Surface 8: watch + file grown past 1.5MiB ──────────────────────────

/// index_paths has no size / .min. gate. A file that grows past 1.5MiB is
/// fully parsed and indexed (symbols in the graph, no S violation) while a
/// full index would refuse to certify it. `--sound` then wrongly claims
/// in_subset=true for an oversized file.
#[test]
fn index_paths_mints_oversized_violation_when_file_grows() {
    let root = temp_root("grow-oversized");
    let big = root.join("src/big.ts");
    std::fs::write(&big, "export function big() { return 1; }\n").unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("seed");
    assert!(
        !indexer
            .open_store()
            .unwrap()
            .subset_violations()
            .unwrap()
            .iter()
            .any(|v| v.path.contains("big.ts")),
        "seed must be clean"
    );

    // Grow past the 1.5 MiB cap.
    let grown = format!(
        "export function big() {{ return '{}'; }}\n",
        "x".repeat(1_600_000)
    );
    std::fs::write(&big, grown).unwrap();

    indexer
        .index_paths(std::slice::from_ref(&big))
        .expect("watch index");

    let store = indexer.open_store().unwrap();
    let viols = store.subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("big.ts")),
        "watch index of a file grown past 1.5MiB must mint oversized S violation; got {viols:?}"
    );
    // Symbols from the pre-growth index must be wiped (same as full index).
    let syms = store
        .all_symbols_for_export()
        .unwrap()
        .into_iter()
        .filter(|s| s.path.contains("big.ts"))
        .count();
    assert_eq!(
        syms, 0,
        "oversized file must not keep symbols after watch index"
    );
}

/// Same for a newly created .min. bundle under watch.
#[test]
fn index_paths_mints_minified_violation_for_new_min_bundle() {
    let root = temp_root("grow-min");
    std::fs::write(
        root.join("src/ok.ts"),
        "export function helper() { return 1; }\n",
    )
    .unwrap();
    let min = root.join("src/app.min.js");
    std::fs::write(&min, "function foo(){return 1}function bar(){return 2}\n").unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index_paths(std::slice::from_ref(&min)).expect("ip");

    let viols = indexer.open_store().unwrap().subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("app.min.js")),
        "watch index of .min. bundle must mint minified S violation; got {viols:?}"
    );
}

/// A different file changes while a sibling has grown oversized: the walker
/// keep-set keeps the oversized path, but index_paths never re-mints the
/// violation / never wipes the stale symbols.
#[test]
fn index_paths_mints_oversized_violation_from_walker() {
    let root = temp_root("sib-oversized");
    let ok = root.join("src/ok.ts");
    let big = root.join("src/big.ts");
    std::fs::write(&ok, "export function helper() { return 1; }\n").unwrap();
    std::fs::write(&big, "export function big() { return 1; }\n").unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("seed");

    // big.ts grows; ok.ts is what the watch event reports.
    std::fs::write(
        &big,
        format!(
            "export function big() {{ return '{}'; }}\n",
            "y".repeat(1_600_000)
        ),
    )
    .unwrap();
    std::fs::write(&ok, "export function helper() { return 2; }\n").unwrap();
    indexer
        .index_paths(std::slice::from_ref(&ok))
        .expect("scoped index");

    let store = indexer.open_store().unwrap();
    let viols = store.subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("big.ts")),
        "scoped watch index must mint oversized violation for sibling grown file; got {viols:?}"
    );
}

// ── Surface 7: S edge forms ────────────────────────────────────────────

#[test]
fn ts_export_assignment_eval_leaves_s() {
    for src in [
        "export = eval;\n",
        "export = eval('x');\n",
        "export default eval;\n",
        "export default eval('x');\n",
    ] {
        let r = scan_subset(src, Language::TypeScript, "src/e.ts");
        assert!(!r.in_subset, "must leave S: {src:?} {:?}", r.violations);
    }
}

#[test]
fn py_eval_in_namespace_leaves_s() {
    // eval reached via locals()/globals() subscript, or assigned into a ns dict.
    for src in [
        "ns = {}\nns['eval'] = eval\n",
        "def f():\n    return eval\n",
        "handlers = {'eval': eval}\n",
    ] {
        let r = scan_subset(src, Language::Python, "src/e.py");
        assert!(!r.in_subset, "must leave S: {src:?} {:?}", r.violations);
    }
}

#[test]
fn go_export_directive_leaves_s() {
    // //export Foo is a cgo compiler directive (exports Go func to C).
    let src = "package main\n//export Foo\nfunc Foo() {}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(!r.in_subset, "go //export must leave S: {:?}", r.violations);
}

// ── Surface 3: file_uri unicode / already-encoded ──────────────────────

#[test]
fn file_uri_encodes_unicode() {
    let root = Path::new("/home/用户/my-proj");
    let uri = file_uri(root, "src/文件.ts");
    assert!(
        uri.contains("%") && !uri.contains("用户") && !uri.contains("文件"),
        "unicode path must be percent-encoded: {uri}"
    );
    assert!(
        uri.starts_with("file:///"),
        "unicode posix root keeps file:/// form: {uri}"
    );
}

#[test]
fn file_uri_encodes_literal_percent_not_double() {
    // A filesystem path that literally contains `%20` must encode `%` → `%25`.
    let root = Path::new("C:/proj%20x");
    let uri = file_uri(root, "src/a.ts");
    assert!(
        uri.contains("proj%2520x"),
        "literal % in path must become %25 (not left as %20): {uri}"
    );
}

// ── Surface 2: subset_violations cleared when eval is removed ──────────

#[test]
fn removing_eval_clears_stored_violation() {
    let root = temp_root("clear-eval");
    let f = root.join("src/a.ts");
    std::fs::write(&f, "eval('x');\n").unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("seed");
    assert!(
        indexer
            .open_store()
            .unwrap()
            .subset_violations()
            .unwrap()
            .iter()
            .any(|v| v.path.contains("a.ts")),
        "seed must have eval violation"
    );

    std::fs::write(&f, "export function ok() { return 1; }\n").unwrap();
    indexer
        .index_paths(std::slice::from_ref(&f))
        .expect("reindex");

    let viols = indexer.open_store().unwrap().subset_violations().unwrap();
    assert!(
        !viols.iter().any(|v| v.path.contains("a.ts")),
        "removing eval must clear stored violation; got {viols:?}"
    );
}

// ── Surface 5: Nest circular forwardRef both directions ────────────────

#[test]
fn nest_forward_ref_cycle_edges_both_ways() {
    use agentgraph::index::extract::extract_file;
    use std::collections::HashSet;

    let a = r#"
import { Module, forwardRef } from '@nestjs/common';
import { BModule } from './b';
@Module({ imports: [forwardRef(() => BModule)] })
export class AModule {}
"#;
    let b = r#"
import { Module, forwardRef } from '@nestjs/common';
import { AModule } from './a';
@Module({ imports: [forwardRef(() => AModule)] })
export class BModule {}
"#;
    let known: HashSet<String> = HashSet::new();
    let ea = extract_file(a, Language::TypeScript, "src/a.ts", &known).unwrap();
    let eb = extract_file(b, Language::TypeScript, "src/b.ts", &known).unwrap();

    let hits =
        |refs: &[agentgraph::index::extract::ExtractedRef], name: &str, rule: &str| -> bool {
            refs.iter().any(|r| {
                r.name == name
                    && r.evidence
                        .as_ref()
                        .map(|e| e.rule_id == rule)
                        .unwrap_or(false)
            })
        };

    assert!(
        hits(&ea.references, "BModule", "ts.nest.module_imports"),
        "A→B forwardRef must emit BModule; {:?}",
        ea.references
    );
    assert!(
        hits(&eb.references, "AModule", "ts.nest.module_imports"),
        "B→A forwardRef must emit AModule; {:?}",
        eb.references
    );
    let nest_forward = |refs: &[agentgraph::index::extract::ExtractedRef]| {
        refs.iter().any(|r| {
            r.name == "forwardRef"
                && r.evidence
                    .as_ref()
                    .map(|e| e.rule_id.starts_with("ts.nest."))
                    .unwrap_or(false)
        })
    };
    assert!(
        !nest_forward(&ea.references) && !nest_forward(&eb.references),
        "must not invent ts.nest.* forwardRef edge"
    );
}

// ── Surface 6: impact sound includes nest module_exports Heuristic ─────

#[test]
fn impact_sound_traverses_nest_module_exports() {
    use agentgraph::index::extract::extract_file;
    use agentgraph::index::store::Store;
    use std::collections::HashSet;

    let db_dir = temp_root("impact-exports");
    let db = db_dir.join("index.db");
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();

    let mod_src = r#"
import { Module } from '@nestjs/common';
import { SharedService } from './shared';
@Module({ exports: [SharedService] })
export class SharedModule {}
"#;
    let use_src = r#"
import { SharedService } from './shared';
export function run(s: SharedService) { s.work(); }
"#;
    let shared_src = r#"
export class SharedService { work() { return 1; } }
"#;
    let m = extract_file(
        mod_src,
        Language::TypeScript,
        "src/shared.module.ts",
        &known,
    )
    .unwrap();
    let u = extract_file(use_src, Language::TypeScript, "src/run.ts", &known).unwrap();
    let s = extract_file(shared_src, Language::TypeScript, "src/shared.ts", &known).unwrap();

    store.begin_batch().unwrap();
    store
        .replace_file("src/shared.module.ts", "hm", "typescript", &m)
        .unwrap();
    store
        .replace_file("src/run.ts", "hu", "typescript", &u)
        .unwrap();
    store
        .replace_file("src/shared.ts", "hs", "typescript", &s)
        .unwrap();
    store.commit_batch().unwrap();

    let (impact, viols) = store.impact_sound("SharedService", 3, 50).unwrap();
    assert!(
        viols.is_empty(),
        "fixture must be in S for this probe; {viols:?}"
    );
    let paths: Vec<_> = impact.iter().map(|i| i.path.as_str()).collect();
    assert!(
        paths.contains(&"src/shared.module.ts"),
        "impact --sound must include ts.nest.module_exports edge; got {impact:?}"
    );
}

// ── Surface 10: file_uri + spaces in root (export form) ────────────────

#[test]
fn file_uri_spaces_root_three_slash_windows() {
    let root = Path::new("C:/My Project/app");
    let uri = file_uri(root, "");
    assert_eq!(uri, "file:///C:/My%20Project/app");
    let rel = rel_path_under_root(Path::new("C:/My Project/app/src/a.ts"), root);
    // May be None on non-Windows canonicalize; only assert when Some.
    if let Some(rel) = rel {
        assert_eq!(rel, "src/a.ts");
    }
}
