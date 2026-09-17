//! Track M1 — expanded path map table (spec §1.5).
//!
//! `map_expanded_path(expanded_rel, expanded_root, source_root, crate_layout)`
//! strips expand-dir prefixes, aligns crate roots, honors explicit pairs,
//! and handles Windows drive letters / `../` sibling forms.

use agentgraph::index::macro_map::{
    map_expanded_path, path_map_from_meta, path_map_to_meta, CrateLayout, PathMap, PathMapPair,
};
use std::path::{Path, PathBuf};

fn temp_roots(tag: &str) -> (PathBuf, PathBuf) {
    let base =
        std::env::temp_dir().join(format!("agentgraph-pathmap-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let source = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    std::fs::create_dir_all(source.join("src")).unwrap();
    std::fs::create_dir_all(source.join("crates/event-engine/src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("event-engine")).unwrap();
    std::fs::create_dir_all(expanded.join("expanded-view/event-engine")).unwrap();
    std::fs::write(source.join("src/core.rs"), "pub fn helper() {}\n").unwrap();
    std::fs::write(
        source.join("crates/event-engine/src/lib.rs"),
        "pub fn helper() {}\n",
    )
    .unwrap();
    std::fs::write(expanded.join("src/core.rs"), "pub fn helper() {}\n").unwrap();
    std::fs::write(expanded.join("event-engine/lib.rs"), "pub fn helper() {}\n").unwrap();
    std::fs::write(
        expanded.join("expanded-view/event-engine/lib.rs"),
        "pub fn helper() {}\n",
    )
    .unwrap();
    (source, expanded)
}

fn map(rel: &str, source: &Path, expanded: &Path) -> Option<String> {
    map_expanded_path(rel, expanded, source, &CrateLayout::default())
}

/// Identity mapping when the expanded tree mirrors the source layout.
#[test]
fn identity_when_source_file_exists() {
    let (source, expanded) = temp_roots("identity");
    assert_eq!(
        map("src/core.rs", &source, &expanded).as_deref(),
        Some("src/core.rs")
    );
}

/// Prefixed shadow dir (`expanded-view/...`) strips to the source-relative form.
#[test]
fn strips_expand_dir_prefix() {
    let (source, expanded) = temp_roots("prefix");
    let mapped = map(
        "expanded-view/crates/event-engine/src/lib.rs",
        &source,
        &expanded,
    );
    // crates/... already exists under source after strip of expanded-view.
    // File may not exist at that exact expanded path on disk, but strip + identity
    // / crate align should still produce a source-relative crates path.
    let mapped = mapped.expect("expand-dir prefix must map");
    assert!(
        mapped.contains("event-engine"),
        "mapped={mapped} for expanded-view prefix"
    );
    assert!(
        mapped.ends_with("lib.rs"),
        "mapped={mapped} should keep leaf"
    );
}

/// Crate-root alignment: `event-engine/lib.rs` → `crates/event-engine/src/lib.rs`.
#[test]
fn crate_root_alignment_heuristics() {
    let (source, expanded) = temp_roots("crate-align");
    assert_eq!(
        map("event-engine/lib.rs", &source, &expanded).as_deref(),
        Some("crates/event-engine/src/lib.rs")
    );
    // Also when the expand-dir prefix wraps the crate dir.
    let mapped =
        map("expanded-view/event-engine/lib.rs", &source, &expanded).expect("prefixed crate path");
    assert_eq!(mapped, "crates/event-engine/src/lib.rs");
}

/// Explicit pairs in CrateLayout override heuristics (metadata override).
#[test]
fn explicit_pairs_override_heuristics() {
    let (source, expanded) = temp_roots("explicit");
    std::fs::create_dir_all(source.join("crates/custom/src")).unwrap();
    std::fs::write(source.join("crates/custom/src/lib.rs"), "pub fn x() {}\n").unwrap();
    let layout = CrateLayout {
        pairs: vec![PathMapPair {
            expanded: "event-engine/".into(),
            source: "crates/custom/src/".into(),
            prefix: true,
        }],
        ..CrateLayout::default()
    };
    let mapped =
        map_expanded_path("event-engine/lib.rs", &expanded, &source, &layout).expect("pair map");
    assert_eq!(mapped, "crates/custom/src/lib.rs");
    // Exact pair wins over prefix.
    let layout2 = CrateLayout {
        pairs: vec![
            PathMapPair {
                expanded: "event-engine/lib.rs".into(),
                source: "crates/custom/src/other.rs".into(),
                prefix: false,
            },
            PathMapPair {
                expanded: "event-engine/".into(),
                source: "crates/custom/src/".into(),
                prefix: true,
            },
        ],
        ..CrateLayout::default()
    };
    let mapped2 =
        map_expanded_path("event-engine/lib.rs", &expanded, &source, &layout2).expect("exact");
    assert_eq!(mapped2, "crates/custom/src/other.rs");
}

/// Absolute expanded path under expanded_root is stripped to a relative map.
#[test]
fn windows_drive_absolute_under_expanded_root() {
    let (source, expanded) = temp_roots("abs");
    let abs = expanded.join("src/core.rs");
    let abs_str = abs.to_string_lossy().replace('\\', "/");
    // Force a drive-letter style form on Windows; on Unix this is still absolute.
    let mapped = map(&abs_str, &source, &expanded).expect("abs under expanded_root");
    assert_eq!(mapped, "src/core.rs");
}

/// `../` sibling relative forms resolve against roots when possible.
#[test]
fn sibling_relative_parent_form() {
    let (source, expanded) = temp_roots("sibling");
    // expanded and source are siblings under base.
    let rel = "../src-root/src/core.rs";
    let mapped = map(rel, &source, &expanded).expect("sibling ../ form");
    assert_eq!(mapped, "src/core.rs");
}

/// Unmappable path returns None (caller keeps mapped=false).
#[test]
fn unmappable_returns_none() {
    let (source, expanded) = temp_roots("unmap");
    assert_eq!(map("mystery/weird.rs", &source, &expanded), None);
    assert_eq!(map("", &source, &expanded), None);
}

/// Path-map meta round-trip through sidecar storage form.
#[test]
fn path_map_meta_roundtrip_in_test() {
    let map = PathMap {
        pairs: vec![PathMapPair {
            expanded: "src/core.rs".into(),
            source: "src/core.rs".into(),
            prefix: false,
        }],
    };
    let raw = path_map_to_meta(&map);
    let back = path_map_from_meta(Some(&raw));
    assert_eq!(back.pairs.len(), 1);
    assert_eq!(back.pairs[0].expanded, "src/core.rs");
    assert!(path_map_from_meta(None).pairs.is_empty());
}

/// Golden shape from the productization spec: `event-engine/lib.rs` maps to
/// `crates/event-engine/src/lib.rs` when the source crate layout exists.
#[test]
fn golden_event_engine_lib_maps_to_crate_src() {
    let base =
        std::env::temp_dir().join(format!("agentgraph-pathmap-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
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
    let mapped = map_expanded_path(
        "event-engine/lib.rs",
        &expanded,
        &source,
        &CrateLayout::default(),
    )
    .expect("crate align when source crate dir exists");
    assert_eq!(mapped, "crates/event-engine/src/lib.rs");
}

/// Without a matching source crate directory, crate-align must not invent
/// `crates/<unknown>/src/...` — path stays unmappable (mapped=false).
#[test]
fn unknown_crate_dir_is_unmappable() {
    let base =
        std::env::temp_dir().join(format!("agentgraph-pathmap-unknown-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let source = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    std::fs::create_dir_all(source.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("mystery")).unwrap();
    std::fs::write(source.join("src/core.rs"), "pub fn helper() {}\n").unwrap();
    let mapped = map_expanded_path(
        "mystery/gen.rs",
        &expanded,
        &source,
        &CrateLayout::default(),
    );
    assert_eq!(mapped, None, "unknown crate must stay unmappable");
}
