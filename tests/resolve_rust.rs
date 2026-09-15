use agentgraph::index::resolve::resolve_rust_use;
use std::collections::HashSet;

fn known(paths: &[&str]) -> HashSet<String> {
    paths.iter().map(|s| s.to_string()).collect()
}

#[test]
fn super_single_from_leaf_file() {
    // src/a/b.rs is module crate::a::b; super::util → crate::a::util → src/a/util.rs
    let k = known(&["src/a/util.rs", "src/util.rs"]);
    let hit = resolve_rust_use("src/a/b.rs", "super::util", &k);
    assert_eq!(hit.as_deref(), Some("src/a/util.rs"));
}

#[test]
fn super_super_walks_two_levels_up() {
    // src/a/b.rs → super::super::util → crate::util → src/util.rs
    // (not src/a/util.rs — trim_start_matches would have stripped both supers
    // and then only tried the wrong dir)
    let k = known(&["src/a/util.rs", "src/util.rs"]);
    let hit = resolve_rust_use("src/a/b.rs", "super::super::util", &k);
    assert_eq!(hit.as_deref(), Some("src/util.rs"));
}

#[test]
fn super_from_mod_rs() {
    // src/a/mod.rs IS module crate::a; super::util → crate::util → src/util.rs
    let k = known(&["src/util.rs"]);
    let hit = resolve_rust_use("src/a/mod.rs", "super::util", &k);
    assert_eq!(hit.as_deref(), Some("src/util.rs"));
}

#[test]
fn super_super_from_mod_rs() {
    // src/a/b/mod.rs is crate::a::b; super::super::util → crate::util → src/util.rs
    let k = known(&["src/util.rs"]);
    let hit = resolve_rust_use("src/a/b/mod.rs", "super::super::util", &k);
    assert_eq!(hit.as_deref(), Some("src/util.rs"));
}

#[test]
fn three_supers_from_deep_file() {
    // src/a/b/c.rs → super::super::super::util → crate::util → src/util.rs
    let k = known(&["src/util.rs"]);
    let hit = resolve_rust_use("src/a/b/c.rs", "super::super::super::util", &k);
    assert_eq!(hit.as_deref(), Some("src/util.rs"));
}

#[test]
fn crate_path_still_works() {
    let k = known(&["src/util.rs"]);
    let hit = resolve_rust_use("src/a/b.rs", "crate::util", &k);
    assert_eq!(hit.as_deref(), Some("src/util.rs"));
}

#[test]
fn self_path_stays_in_dir() {
    let k = known(&["src/a/util.rs"]);
    let hit = resolve_rust_use("src/a/b.rs", "self::util", &k);
    assert_eq!(hit.as_deref(), Some("src/a/util.rs"));
}
