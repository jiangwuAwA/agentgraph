//! R23 adversarial probes: S-violation keep-set, impact BFS expand gate,
//! importers path forms, export file_uri encoding.

use agentgraph::index::export::file_uri;
use agentgraph::index::extract::extract_file;
use agentgraph::index::store::Store;
use agentgraph::index::Indexer;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-r23-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-r23-db-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("index.db")
}

// ── Surface: oversized / minified S violations must survive prune ─────

/// R13 mints parse_error S violations for oversized sources. `prune_missing`
/// keep-set is the walker's normal file list, so those paths are not kept —
/// the violation is created and CASCADE-deleted in the same index pass.
/// `--sound` then wrongly claims in_subset=true.
#[test]
fn oversized_source_s_violation_survives_index() {
    let root = temp_root("oversized");
    std::fs::write(
        root.join("src/ok.ts"),
        "export function helper() { return 1; }\n",
    )
    .unwrap();
    // > 1.5 MiB
    let big = format!(
        "export function big() {{ return '{}'; }}\n",
        "x".repeat(1_600_000)
    );
    std::fs::write(root.join("src/big.ts"), big).unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    let stats = indexer.index(true).expect("index1");
    assert_eq!(stats.oversized_files, 1, "walker must count oversized");

    let store = indexer.open_store().unwrap();
    let viols = store.subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("big.ts")),
        "oversized source must keep an S violation after index; got {viols:?}"
    );

    // Second force index: violation must still be present (not wiped by prune).
    indexer.index(true).expect("index2");
    let store = indexer.open_store().unwrap();
    let viols2 = store.subset_violations().unwrap();
    assert!(
        viols2
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("big.ts")),
        "oversized S violation must survive a second index; got {viols2:?}"
    );
}

#[test]
fn minified_bundle_s_violation_survives_index() {
    let root = temp_root("minified");
    std::fs::write(
        root.join("src/ok.ts"),
        "export function helper() { return 1; }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/app.min.js"),
        "function foo(){return 1}function bar(){return 2}\n",
    )
    .unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("index1");
    let store = indexer.open_store().unwrap();
    let viols = store.subset_violations().unwrap();
    assert!(
        viols
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("app.min.js")),
        "minified bundle must keep an S violation; got {viols:?}"
    );

    indexer.index(true).expect("index2");
    let store = indexer.open_store().unwrap();
    let viols2 = store.subset_violations().unwrap();
    assert!(
        viols2
            .iter()
            .any(|v| v.kind == "parse_error" && v.path.contains("app.min.js")),
        "minified S violation must survive a second index; got {viols2:?}"
    );
}

/// index_paths (watch) must not wipe previously minted oversized/minified
/// violations when a different file changes.
#[test]
fn index_paths_does_not_wipe_oversized_violations() {
    let root = temp_root("ip-oversized");
    let ok = root.join("src/ok.ts");
    std::fs::write(&ok, "export function helper() { return 1; }\n").unwrap();
    let big = format!(
        "export function big() {{ return '{}'; }}\n",
        "y".repeat(1_600_000)
    );
    std::fs::write(root.join("src/big.ts"), big).unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("seed");
    assert!(
        indexer
            .open_store()
            .unwrap()
            .subset_violations()
            .unwrap()
            .iter()
            .any(|v| v.path.contains("big.ts")),
        "seed must have oversized violation"
    );

    std::fs::write(&ok, "export function helper() { return 2; }\n").unwrap();
    indexer
        .index_paths(std::slice::from_ref(&ok))
        .expect("scoped index");

    let viols = indexer.open_store().unwrap().subset_violations().unwrap();
    assert!(
        viols.iter().any(|v| v.path.contains("big.ts")),
        "scoped watch index must not wipe oversized S violations; got {viols:?}"
    );
}

// ── Surface: impact BFS visited_names poison across languages ─────────

/// If the first enclosing-leaf encounter is from a language where that leaf
/// is NOT a real symbol, `visited_names` marks it and a later same-language
/// expandable encounter is skipped. Impact under-reports.
#[test]
fn impact_expands_enclosing_when_same_lang_symbol_exists() {
    let db = temp_db("impact-poison");
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();

    // Python file: call helper() from a function whose leaf name collides
    // with a TypeScript class that we DO want to expand later.
    // No Python symbol named `Service` exists.
    let py = r#"
def helper():
    pass
def not_service():
    helper()
"#;
    // TypeScript: Service.run calls helper; expanding Service should reach run callers.
    let ts = r#"
class Service {
  run() { helper(); }
}
function helper() {}
function trigger() {
  const s = new Service();
  s.run();
}
"#;
    let py_p = extract_file(py, Language::Python, "src/a.py", &known).unwrap();
    let ts_p = extract_file(ts, Language::TypeScript, "src/b.ts", &known).unwrap();

    store.begin_batch().unwrap();
    store
        .replace_file("src/a.py", "hpy", "python", &py_p)
        .unwrap();
    store
        .replace_file("src/b.ts", "hts", "typescript", &ts_p)
        .unwrap();
    store.commit_batch().unwrap();

    let impact = store.impact("helper", 4, 50).unwrap();
    // Depth-1 must include both call sites of helper.
    let d1_paths: HashSet<_> = impact
        .iter()
        .filter(|i| i.depth == 1)
        .map(|i| i.path.as_str())
        .collect();
    assert!(
        d1_paths.contains("src/a.py"),
        "py helper call site: {impact:?}"
    );
    assert!(
        d1_paths.contains("src/b.ts"),
        "ts helper call site: {impact:?}"
    );

    // Service.run is a same-language expandable enclosing for the TS helper
    // call. Impact must include depth-2 nodes under Service / run.
    let d2: Vec<_> = impact.iter().filter(|i| i.depth >= 2).collect();
    assert!(
        !d2.is_empty(),
        "impact must expand TS Service.run enclosing to depth>=2; got {impact:?}"
    );
}

/// Cross-language first encounter of a leaf must not permanently suppress
/// a later same-language expandable leaf of the same bare name.
#[test]
fn impact_visited_names_not_poisoned_by_cross_lang_leaf() {
    let db = temp_db("impact-xlang-poison");
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();

    // Python: helper() called from a nested scope; enclosing leaf will be
    // something that does NOT exist as a python symbol named `Engine`.
    // We craft enclosing via a class method that only exists in TS as Engine.
    let py = r#"
def helper():
    pass

def wrapper():
    helper()
"#;
    // Go-like / TS Engine with a method that calls helper, plus a caller of that method.
    let ts = r#"
class Engine {
  ignite() { helper(); }
}
function helper() {}
function start() {
  const e = new Engine();
  e.ignite();
}
"#;
    let py_p = extract_file(py, Language::Python, "src/wrap.py", &known).unwrap();
    let ts_p = extract_file(ts, Language::TypeScript, "src/engine.ts", &known).unwrap();

    store.begin_batch().unwrap();
    store
        .replace_file("src/wrap.py", "hpy", "python", &py_p)
        .unwrap();
    store
        .replace_file("src/engine.ts", "hts", "typescript", &ts_p)
        .unwrap();
    store.commit_batch().unwrap();

    let impact = store.impact("helper", 4, 50).unwrap();
    // ignite is a real TS method symbol; expanding it must surface start.
    let names: HashSet<String> = impact.iter().map(|i| i.name.clone()).collect();
    // start calls e.ignite — that may appear as a ref named ignite.
    assert!(
        impact.iter().any(|i| i.depth >= 2),
        "must expand ignite enclosing; impact={impact:?} names={names:?}"
    );
}

// ── Surface: importers path forms ─────────────────────────────────────

fn seed_importer_store(tag: &str) -> (PathBuf, Store) {
    let db = temp_db(tag);
    let mut store = Store::open(&db).unwrap();
    let mut known = HashSet::new();
    known.insert("src/api.ts".to_string());
    known.insert("src/auth.ts".to_string());
    let api = r#"import { createUser } from "./auth";
export function loginHandler() { createUser("a","b"); }
"#;
    let auth = r#"export function createUser(e: string, p: string) { return {e,p}; }
"#;
    let p_api = extract_file(api, Language::TypeScript, "src/api.ts", &known).unwrap();
    let p_auth = extract_file(auth, Language::TypeScript, "src/auth.ts", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/api.ts", "h1", "typescript", &p_api)
        .unwrap();
    store
        .replace_file("src/auth.ts", "h2", "typescript", &p_auth)
        .unwrap();
    store.commit_batch().unwrap();
    (db, store)
}

#[test]
fn importers_accepts_dot_slash_prefix() {
    let (_db, store) = seed_importer_store("imp-dot");
    let hits = store.importers_of_file("./src/auth.ts", 20).unwrap();
    assert!(
        !hits.is_empty(),
        "importers must accept ./src/auth.ts (CLI/Windows common form)"
    );
}

#[test]
fn importers_accepts_backslash_rel() {
    let (_db, store) = seed_importer_store("imp-back");
    let hits = store.importers_of_file("src\\auth.ts", 20).unwrap();
    assert!(
        !hits.is_empty(),
        "importers must accept src\\auth.ts after normalize"
    );
}

#[test]
fn e2e_importers_accepts_absolute_path_under_root() {
    let root = temp_root("imp-abs");
    std::fs::write(
        root.join("src/auth.ts"),
        "export function createUser(e: string, p: string) { return {e,p}; }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/api.ts"),
        "import { createUser } from \"./auth\";\nexport function loginHandler() { createUser(\"a\",\"b\"); }\n",
    )
    .unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("index");

    let abs = root.join("src/auth.ts");
    let store = indexer.open_store().unwrap();
    // CLI/MCP now strip the root prefix before the store lookup.
    let lookup = agentgraph::index::parser::rel_path_under_root(&abs, &indexer.root)
        .expect("abs under root");
    assert_eq!(lookup, "src/auth.ts");
    let hits = store.importers_of_file(&lookup, 20).unwrap();
    assert!(
        !hits.is_empty(),
        "abs-under-root importers lookup; got {hits:?}"
    );
}

// ── Surface: export file_uri percent-encoding ─────────────────────────

#[test]
fn file_uri_percent_encodes_spaces_and_specials() {
    let root = Path::new("C:/My Project/app");
    let uri = file_uri(root, "src/my file.ts");
    assert_eq!(
        uri, "file:///C:/My%20Project/app/src/my%20file.ts",
        "file_uri must percent-encode spaces (RFC 8089 / URI)"
    );

    let posix = Path::new("/home/user/my proj");
    let uri2 = file_uri(posix, "src/a#b.ts");
    assert!(
        uri2.contains("my%20proj") && !uri2.contains("my proj"),
        "posix spaces must be encoded: {uri2}"
    );
}
