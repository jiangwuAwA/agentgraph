//! R19 adversarial probes: file rename watch, Go type-switch pointer/qualified
//! multi-type, Python from-import eval alias, JS import eval alias, find LIKE.

use agentgraph::index::extract::{extract_file, ExtractedFile};
use agentgraph::index::subset::scan_subset;
use agentgraph::model::{Confidence, Language};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

fn extract_lang(src: &str, path: &str, lang: Language) -> ExtractedFile {
    let known = HashSet::new();
    extract_file(src, lang, path, &known).expect("extract")
}

fn dump_refs(out: &ExtractedFile) -> String {
    format!(
        "{:?}",
        out.references
            .iter()
            .map(|r| (r.name.clone(), r.module.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    )
}

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agentgraph-r19-watch-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

// ── Surface 1: watch rename file A→B (same content, new path) ────────

/// Renaming a single source file must replace symbols under the new path
/// and prune the old path (no duplicate `helper` rows).
#[test]
fn watch_file_rename_replaces_path_without_duplicates() {
    let root = temp_root("file-ren");
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    std::fs::write(&a, "export function helper() { return 1; }\n").unwrap();

    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let before = {
        let store = indexer.open_store().unwrap();
        store.find_symbol("helper", 20).unwrap()
    };
    assert_eq!(before.len(), 1, "seed: one helper under a.ts: {before:?}");
    assert!(before[0].path.contains("a.ts"));

    let (rx, _handle) = indexer
        .watch_events(Duration::from_millis(50))
        .expect("start watcher");
    std::thread::sleep(Duration::from_millis(80));

    std::fs::rename(&a, &b).unwrap();

    let ev = rx.recv_timeout(Duration::from_millis(2000));
    assert!(
        ev.is_ok(),
        "file rename must trigger watch reindex; got {:?}",
        ev
    );

    let store = indexer.open_store().unwrap();
    let helpers = store.find_symbol("helper", 20).unwrap();
    let under_a: Vec<_> = helpers.iter().filter(|s| s.path.contains("a.ts")).collect();
    let under_b: Vec<_> = helpers.iter().filter(|s| s.path.contains("b.ts")).collect();
    assert!(
        under_a.is_empty(),
        "old path a.ts must be pruned after file rename: {helpers:?}"
    );
    assert_eq!(
        under_b.len(),
        1,
        "new path b.ts must hold exactly one helper: {helpers:?}"
    );
}

/// index_paths on the destination only (notify may drop the From path) must
/// still prune the vanished source file when the tree no longer contains it.
#[test]
fn index_paths_rename_destination_prunes_old_path() {
    let root = temp_root("ip-ren");
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    std::fs::write(&a, "export function helper() { return 1; }\n").unwrap();

    let indexer = agentgraph::index::Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    std::fs::rename(&a, &b).unwrap();

    // Simulate a To-only rename event (From path missed).
    indexer
        .index_paths(std::slice::from_ref(&b))
        .expect("index_paths on rename dest");

    let store = indexer.open_store().unwrap();
    let helpers = store.find_symbol("helper", 20).unwrap();
    let under_a: Vec<_> = helpers.iter().filter(|s| s.path.contains("a.ts")).collect();
    assert!(
        under_a.is_empty(),
        "index_paths(dest) after rename must prune old path via tree walk: {helpers:?}"
    );
    assert!(
        helpers.iter().any(|s| s.path.contains("b.ts")),
        "new path must be indexed: {helpers:?}"
    );
}

// ── Surface 3: Go type-switch pointer / qualified / multi-type ────────

/// `case *pkg.Cat:` — pointer wrapping a qualified type. The type name must
/// still mint a ref (impact of pkg.Cat).
#[test]
fn go_type_switch_pointer_qualified_case_refs() {
    let src = r#"
package main

import "other"

func speak(a interface{}) string {
	switch a.(type) {
	case *other.Cat:
		return "cat"
	case other.Dog:
		return "dog"
	default:
		return "?"
	}
}
"#;
    let out = extract_lang(src, "main.go", Language::Go);
    let names: Vec<&str> = out.references.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"Cat"),
        "case *other.Cat must mint a Cat ref; got {}",
        dump_refs(&out)
    );
    assert!(
        names.contains(&"Dog"),
        "case other.Dog must mint a Dog ref; got {}",
        dump_refs(&out)
    );
}

/// Multi-type case `case A, B:` — both type names must be refs.
#[test]
fn go_type_switch_multi_type_case_refs() {
    let src = r#"
package main

type Cat struct{}
type Dog struct{}
type Bird struct{}

func speak(a interface{}) string {
	switch a.(type) {
	case Cat, Dog:
		return "pet"
	case *Bird:
		return "bird"
	default:
		return "?"
	}
}
"#;
    let out = extract_lang(src, "main.go", Language::Go);
    let names: Vec<&str> = out.references.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"Cat") && names.contains(&"Dog"),
        "case Cat, Dog must mint both refs; got {}",
        dump_refs(&out)
    );
    assert!(
        names.contains(&"Bird"),
        "case *Bird must mint Bird ref; got {}",
        dump_refs(&out)
    );
}

// ── Surface 5: Python from-import eval alias (any module) ─────────────

#[test]
fn py_from_any_module_import_eval_as_leaves_s() {
    for src in [
        "from evil import eval as e\ndef f(x):\n    return e(x)\n",
        "from mymod import eval as e\ndef f(x):\n    return e(x)\n",
        "from .rel import eval as e\ndef f(x):\n    return e(x)\n",
        "from evil import eval as e\ndef f(x):\n    return e\n",
    ] {
        let r = scan_subset(src, Language::Python, "a.py");
        assert!(
            !r.in_subset,
            "from <any> import eval as e must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

#[test]
fn py_from_any_module_import_eval_bare_leaves_s() {
    // Local name is still `eval` — call-site name check should fire.
    let src = "from evil import eval\ndef f(x):\n    return eval(x)\n";
    let r = scan_subset(src, Language::Python, "a.py");
    assert!(
        !r.in_subset,
        "from evil import eval + call must leave S: {:?}",
        r.violations
    );
}

// ── Surface 6: JS import eval / re-export alias ───────────────────────

/// `import { eval as e } from 'evil'` — local binding is `e`, so bare-identifier
/// and call-site checks never see `eval`. Must leave S.
#[test]
fn js_import_named_eval_as_leaves_s() {
    for src in [
        "import { eval as e } from 'evil';\ne('code');\n",
        "import { eval as runCode } from './mod';\nexport function f(x) { return runCode(x); }\n",
        "import { Function as Fn } from 'evil';\nnew Fn('return 1');\n",
    ] {
        let r = scan_subset(src, Language::TypeScript, "a.ts");
        assert!(
            !r.in_subset,
            "import {{ eval as e }} must leave S: {src} -> {:?}",
            r.violations
        );
    }
}

// ── Surface 13: CLI find LIKE special chars ───────────────────────────

#[test]
fn find_symbol_fuzzy_percent_underscore_no_panic() {
    use agentgraph::index::store::Store;

    let dir = std::env::temp_dir().join(format!("agentgraph-r19-find-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("index.db");
    let mut store = Store::open(&db).unwrap();
    store.begin_batch().unwrap();
    let src = "export function helper() { return 1; }\nexport function other() { return 2; }\n";
    let extracted = extract_lang(src, "a.ts", Language::TypeScript);
    store
        .replace_file("a.ts", "h", "typescript", &extracted)
        .unwrap();
    store.commit_batch().unwrap();

    for needle in ["%", "_", "%%", "h%l", "\\%", "_elper", "help_er", "he_p"] {
        let hits = store
            .find_symbol_fuzzy(needle, 10)
            .unwrap_or_else(|e| panic!("fuzzy find panicked/erred on {needle:?}: {e}"));
        // Must not match everything via unescaped LIKE wildcards.
        if needle == "%" || needle == "%%" {
            assert!(
                hits.is_empty(),
                "literal % must not act as wildcard: {needle} -> {hits:?}"
            );
        }
    }
}

// ── Surface 8: Go type-switch Exact edges stay sound ──────────────────

/// Type-switch case refs should be exact confidence so --sound impact
/// includes them when the type is in S.
#[test]
fn go_type_switch_refs_are_exact() {
    let src = r#"
package main

type Cat struct{}

func speak(a interface{}) {
	switch a.(type) {
	case Cat:
		_ = a
	}
}
"#;
    let out = extract_lang(src, "main.go", Language::Go);
    let cat: Vec<_> = out.references.iter().filter(|r| r.name == "Cat").collect();
    assert!(!cat.is_empty(), "need Cat refs: {}", dump_refs(&out));
    assert!(
        cat.iter().all(|r| r.confidence == Confidence::Exact),
        "type-switch case refs must be Exact: {cat:?}"
    );
}
