//! R30 adversarial probes — M4 diff snapshot, S re-cert, graph --sound, M1 path-map.
//!
//! Method: adversarial tests first. Only Critical/Major product lies get fixes.
//! Path-map contract (docs/macro-sidecar.md): "Crate align is accepted only when
//! the source crate dir/file exists (**no invented paths**)."

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentgraph::index::macro_map::{map_expanded_path, CrateLayout, PathMap, PathMapPair};
use agentgraph::index::subset::{is_sound_eligible, SoundClass};
use agentgraph::index::Indexer;
use agentgraph::model::Confidence;

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_root(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-r30-{name}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
    dir
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn parse_json(out: &Output) -> serde_json::Value {
    let text = stdout(out);
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("json parse ({e}): stdout={text} stderr={}", stderr(out)))
}

// ── M1 path-map: no invented paths when source leaf is missing ─────────

/// Crate dir exists but the leaf source file was deleted → must NOT invent
/// `crates/<crate>/src/<leaf>`. mapped=false (return None) is the honest result.
#[test]
fn path_map_does_not_invent_when_crate_dir_exists_but_file_deleted() {
    let base = common::temp_root("ag-r30-invent-file");
    let source = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    // Crate directory exists; the specific leaf file does not.
    std::fs::create_dir_all(source.join("crates/event-engine/src")).unwrap();
    std::fs::create_dir_all(expanded.join("event-engine")).unwrap();
    std::fs::write(
        source.join("crates/event-engine/src/lib.rs"),
        "pub fn helper() {}\n",
    )
    .unwrap();
    // Expanded tree still has ghost.rs — source leaf was deleted.
    std::fs::write(
        expanded.join("event-engine/ghost.rs"),
        "pub fn ghost() {}\n",
    )
    .unwrap();

    let mapped = map_expanded_path(
        "event-engine/ghost.rs",
        &expanded,
        &source,
        &CrateLayout::default(),
    );
    assert_eq!(
        mapped, None,
        "must not invent crates/event-engine/src/ghost.rs when that file does not exist \
         (docs/macro-sidecar.md: no invented paths); got {mapped:?}"
    );
}

/// First-component directory exists under source root but the mapped leaf
/// does not → must return None (no fail-open invent via `source/<first>.is_dir()`).
#[test]
fn path_map_does_not_invent_via_first_component_dir_only() {
    let base = common::temp_root("ag-r30-invent-dir");
    let source = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    // Source has a top-level `pluginx/` dir (unrelated leaf).
    std::fs::create_dir_all(source.join("pluginx")).unwrap();
    std::fs::write(source.join("pluginx/keep.rs"), "pub fn keep() {}\n").unwrap();
    std::fs::create_dir_all(expanded.join("pluginx")).unwrap();
    std::fs::write(expanded.join("pluginx/ghost.rs"), "pub fn ghost() {}\n").unwrap();

    let mapped = map_expanded_path(
        "pluginx/ghost.rs",
        &expanded,
        &source,
        &CrateLayout::default(),
    );
    assert_eq!(
        mapped, None,
        "first-component dir must not invent a crates/pluginx/src/ghost.rs path; got {mapped:?}"
    );
}

/// Heuristic prefix pair must not invent a leaf that does not exist under source.
/// Explicit **exact** operator pairs remain trusted (operator override).
#[test]
fn path_map_prefix_pair_does_not_invent_missing_leaf() {
    let base = common::temp_root("ag-r30-prefix");
    let source = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    std::fs::create_dir_all(source.join("crates/event-engine/src")).unwrap();
    std::fs::create_dir_all(expanded.join("event-engine")).unwrap();
    std::fs::write(
        source.join("crates/event-engine/src/lib.rs"),
        "pub fn helper() {}\n",
    )
    .unwrap();
    std::fs::write(expanded.join("event-engine/lib.rs"), "pub fn helper() {}\n").unwrap();
    std::fs::write(
        expanded.join("event-engine/ghost.rs"),
        "pub fn ghost() {}\n",
    )
    .unwrap();

    let layout = CrateLayout {
        pairs: vec![PathMapPair {
            expanded: "event-engine/".into(),
            source: "crates/event-engine/src/".into(),
            prefix: true,
        }],
        ..CrateLayout::default()
    };

    // Existing leaf still maps.
    let ok = map_expanded_path("event-engine/lib.rs", &expanded, &source, &layout);
    assert_eq!(
        ok.as_deref(),
        Some("crates/event-engine/src/lib.rs"),
        "existing leaf under prefix pair must still map"
    );

    // Missing leaf must not invent.
    let ghost = map_expanded_path("event-engine/ghost.rs", &expanded, &source, &layout);
    assert_eq!(
        ghost, None,
        "prefix pair must not invent crates/event-engine/src/ghost.rs; got {ghost:?}"
    );

    // Explicit exact operator pair is still trusted even if source file missing
    // (documented operator override — not a heuristic invent).
    let exact_layout = CrateLayout {
        pairs: vec![PathMapPair {
            expanded: "event-engine/ghost.rs".into(),
            source: "crates/event-engine/src/ghost.rs".into(),
            prefix: false,
        }],
        ..CrateLayout::default()
    };
    let exact = map_expanded_path("event-engine/ghost.rs", &expanded, &source, &exact_layout);
    assert_eq!(
        exact.as_deref(),
        Some("crates/event-engine/src/ghost.rs"),
        "explicit exact pair remains an operator override"
    );
}

/// Alternate existing layout (`<crate>/src/lib.rs` without workspace `crates/`)
/// must still map when the file actually exists (fix must not over-fail-close).
#[test]
fn path_map_maps_existing_flat_crate_src_layout() {
    let base = common::temp_root("ag-r30-flat");
    let source = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    std::fs::create_dir_all(source.join("event-engine/src")).unwrap();
    std::fs::create_dir_all(expanded.join("event-engine")).unwrap();
    std::fs::write(
        source.join("event-engine/src/lib.rs"),
        "pub fn helper() {}\n",
    )
    .unwrap();
    std::fs::write(expanded.join("event-engine/lib.rs"), "pub fn helper() {}\n").unwrap();

    let mapped = map_expanded_path(
        "event-engine/lib.rs",
        &expanded,
        &source,
        &CrateLayout::default(),
    );
    assert_eq!(
        mapped.as_deref(),
        Some("event-engine/src/lib.rs"),
        "existing flat crate/src layout must map; got {mapped:?}"
    );
}

// ── M3 sound allowlist: dyn-trait stays Unsound ────────────────────────

#[test]
fn r30_dyn_trait_rule_stays_unsound() {
    assert!(
        !is_sound_eligible(Confidence::Heuristic, Some("rs.di.dyn_trait_method")),
        "rs.di.dyn_trait_method must remain Unsound"
    );
    assert_eq!(
        SoundClass::of(Confidence::Heuristic, Some("rs.di.dyn_trait_method")),
        SoundClass::Unsound
    );
    // Allowlisted registration rules stay sound.
    assert!(is_sound_eligible(
        Confidence::Heuristic,
        Some("rs.di.inventory_submit")
    ));
    assert!(is_sound_eligible(
        Confidence::Heuristic,
        Some("rs.di.linkme_distributed_slice")
    ));
    assert!(!is_sound_eligible(
        Confidence::Heuristic,
        Some("py.di.entry_points")
    ));
}

// ── M4 diff / write-snapshot / S re-cert / graph --sound probes ────────

fn write_ts(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, body).unwrap();
}

/// After watch/index_paths drift, `diff --write-snapshot` promotes live as
/// baseline; an immediate re-diff must be empty (not invent "added" from the
/// watch edges that were just baselined).
#[test]
fn r30_write_snapshot_after_watch_resets_baseline() {
    let root = temp_root("write-snap");
    write_ts(
        &root,
        "src/a.ts",
        "export function helper(x: number) { return x + 1; }\nexport function createUser(e: string) { helper(1); return { e }; }\n",
    );
    write_ts(
        &root,
        "src/b.ts",
        "import { createUser } from './a';\nexport function login(e: string) { return createUser(e); }\n",
    );
    let idx = Indexer::new(&root).unwrap();
    idx.index(true).unwrap();

    write_ts(
        &root,
        "src/watch_add.ts",
        "import { createUser } from './a';\nexport function watchCaller(e: string) { createUser(e); }\n",
    );
    idx.index_paths(std::slice::from_ref(&root.join("src/watch_add.ts")))
        .expect("index_paths");

    let d1 = run(&root, &["diff"]);
    assert!(d1.status.success(), "{}", stderr(&d1));
    let p1 = parse_json(&d1);
    assert!(
        p1["summary"]["added"].as_u64().unwrap_or(0) >= 1,
        "watch-added edge must appear before --write-snapshot: {p1}"
    );

    let ws = run(&root, &["diff", "--write-snapshot"]);
    assert!(ws.status.success(), "{}", stderr(&ws));

    let d2 = run(&root, &["diff"]);
    assert!(d2.status.success(), "{}", stderr(&d2));
    let p2 = parse_json(&d2);
    assert_eq!(
        p2["summary"]["added"].as_u64().unwrap_or(0),
        0,
        "immediate re-diff after --write-snapshot must have zero added: {p2}"
    );
    assert_eq!(
        p2["summary"]["removed"].as_u64().unwrap_or(0),
        0,
        "immediate re-diff after --write-snapshot must have zero removed: {p2}"
    );
}

/// parse_error file recovered on disk + index_paths must clear S debt
/// (promise restored) — product recovery path, not store-API-only refresh.
#[test]
fn r30_parse_error_recovers_via_index_paths() {
    let root = temp_root("parse-recover");
    let f = root.join("src/rec.ts");
    std::fs::write(&f, [0xffu8, 0xfe, 0x00, 0x01]).unwrap();
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();

    let dirty = run(&root, &["subset"]);
    let dj = parse_json(&dirty);
    assert_eq!(dj["in_subset"], serde_json::json!(false), "{dj}");
    assert_eq!(dj["promise_tier"].as_str(), Some("disabled"), "{dj}");

    std::fs::write(&f, "export function rec() { return 1; }\n").unwrap();
    indexer
        .index_paths(std::slice::from_ref(&f))
        .expect("index_paths recover");

    let fixed = run(&root, &["subset"]);
    let fj = parse_json(&fixed);
    assert_eq!(
        fj["in_subset"],
        serde_json::json!(true),
        "recovered parse_error file must clear S debt via index_paths: {fj}"
    );
    assert_eq!(
        fj["promise_tier"].as_str(),
        Some("ast_modeled"),
        "promise must restore after recovery: {fj}"
    );
}

/// Deleting a violating file via index_paths (not only full index) must clear S debt.
#[test]
fn r30_deleted_violating_file_clears_via_index_paths() {
    let root = temp_root("del-viol");
    write_ts(
        &root,
        "src/clean.ts",
        "export function ok() { return 1; }\n",
    );
    let bad = root.join("src/bad.ts");
    write_ts(
        &root,
        "src/bad.ts",
        "export function evil(x: string) { return eval(x); }\n",
    );
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();
    let dirty = parse_json(&run(&root, &["subset"]));
    assert_eq!(dirty["in_subset"], serde_json::json!(false), "{dirty}");

    std::fs::remove_file(&bad).unwrap();
    indexer
        .index_paths(std::slice::from_ref(&bad))
        .expect("index_paths delete");

    let after = parse_json(&run(&root, &["subset"]));
    assert_eq!(
        after["in_subset"],
        serde_json::json!(true),
        "deleted violating file must not leave S debt after index_paths: {after}"
    );
}

/// `graph --sound` with subset_ok=true and empty neighborhood: exit 0,
/// HTML written, honesty present, not labeled as complete runtime graph.
#[test]
fn r30_graph_sound_empty_neighborhood_exit0_when_subset_ok() {
    let root = temp_root("sound-empty");
    write_ts(
        &root,
        "src/solo.ts",
        "export function lonely() { return 1; }\n",
    );
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = root.join("empty-sound.html");
    let out_s = out.to_string_lossy().to_string();
    let g = run(
        &root,
        &["graph", "lonely", "--sound", "--out", out_s.as_str()],
    );
    assert!(
        g.status.success(),
        "subset_ok=true empty sound graph must exit 0: stdout={} stderr={}",
        stdout(&g),
        stderr(&g)
    );
    assert!(out.exists(), "HTML must be written");
    let html = std::fs::read_to_string(&out).unwrap();
    assert!(
        html.contains("subset_ok=true")
            || html.contains("subset_ok = true")
            || html.contains("subset_ok"),
        "sound page must report subset_ok; snippet missing"
    );
    assert!(
        html.contains("not a complete runtime graph"),
        "honesty line required even for empty sound graph"
    );
}

/// `graph --sound` when S is violated: HTML still written, exit non-zero.
#[test]
fn r30_graph_sound_violated_exit_nonzero_but_writes_html() {
    let root = temp_root("sound-bad");
    write_ts(
        &root,
        "src/app.ts",
        "export function evil(x: string) { return eval(x); }\n",
    );
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let out = root.join("bad-sound.html");
    let out_s = out.to_string_lossy().to_string();
    let g = run(
        &root,
        &["graph", "evil", "--sound", "--out", out_s.as_str()],
    );
    assert!(
        !g.status.success(),
        "subset_ok=false graph --sound must exit non-zero"
    );
    assert!(out.exists(), "HTML must still be written when S violated");
    let html = std::fs::read_to_string(&out).unwrap();
    assert!(
        html.to_lowercase().contains("disabled") || html.contains("NOT a sound"),
        "page must be marked not-a-sound-graph; got snippet"
    );
}

/// Snapshot is written only after a successful full index; deleting it → diff fail-loud.
#[test]
fn r30_diff_fail_loud_without_snapshot() {
    let root = temp_root("nosnap");
    write_ts(&root, "src/a.ts", "export function f() { return 1; }\n");
    let d0 = run(&root, &["diff"]);
    assert!(!d0.status.success(), "diff before index must fail");
    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    assert!(
        root.join(".agentgraph/refs.snapshot.json").exists(),
        "successful index must write snapshot"
    );
    std::fs::remove_file(root.join(".agentgraph/refs.snapshot.json")).ok();
    std::fs::remove_file(root.join(".agentgraph/refs.snapshot.prev.json")).ok();
    let d1 = run(&root, &["diff"]);
    assert!(!d1.status.success(), "diff without baseline must fail-loud");
}

/// S re-cert: dirty eval file toggles promise_tier disabled ↔ ast_modeled.
#[test]
fn r30_s_recert_promise_follows_disk() {
    let root = temp_root("s-toggle");
    let js = root.join("src/app.js");
    write_ts(
        &root,
        "src/app.js",
        "export function safe(x) { return x + 1; }\nexport function main() { return safe(1); }\n",
    );
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();
    assert_eq!(
        parse_json(&run(&root, &["subset"]))["promise_tier"].as_str(),
        Some("ast_modeled")
    );

    write_ts(
        &root,
        "src/app.js",
        "export function evil(x) { return eval(x); }\n",
    );
    indexer
        .index_paths(std::slice::from_ref(&js))
        .expect("dirty");
    let dirty = parse_json(&run(&root, &["subset"]));
    assert_eq!(dirty["promise_tier"].as_str(), Some("disabled"), "{dirty}");
    assert_eq!(dirty["in_subset"], serde_json::json!(false), "{dirty}");

    let sound = parse_json(&run(&root, &["impact", "evil", "--sound"]));
    assert_eq!(sound["subset_ok"], serde_json::json!(false), "{sound}");
    assert_eq!(sound["promise_tier"].as_str(), Some("disabled"), "{sound}");
}

/// PathMap empty → path_map_from_meta stays empty (no silent pairs).
#[test]
fn r30_path_map_meta_empty_is_empty() {
    use agentgraph::index::macro_map::{path_map_from_meta, path_map_to_meta};
    assert!(path_map_from_meta(None).pairs.is_empty());
    assert!(path_map_from_meta(Some("not-json")).pairs.is_empty());
    let m = PathMap { pairs: vec![] };
    assert!(path_map_from_meta(Some(&path_map_to_meta(&m)))
        .pairs
        .is_empty());
}
