//! R22 adversarial probes: path-form resolution for watch / index_paths.
//!
//! macOS notify reports `/var/...` while Indexer root is `/private/var/...`;
//! Windows may report `RUNNER~1` short names. A directory junction/symlink
//! alias reproduces the same lexical-mismatch class on every platform.
//!
//! Unit tests cover `is_source_event` (src/index/mod.rs). This file locks the
//! end-to-end `index_paths` path so a mismatched-form event still updates the
//! graph and does not wipe the store via a bad keep-set.

use agentgraph::index::Indexer;
use std::path::{Path, PathBuf};

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-r22-e2e-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("real").join("src")).unwrap();
    dir
}

fn make_alias(target: &Path, alias: &Path) {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, alias).expect("symlink alias");
    }
    #[cfg(windows)]
    {
        let ok = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                alias.to_str().expect("utf8 alias"),
                target.to_str().expect("utf8 target"),
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            std::os::windows::fs::symlink_dir(target, alias).expect("junction/symlink alias");
        }
    }
    assert!(
        alias.join("src").is_dir(),
        "alias must resolve: {} -> {}",
        alias.display(),
        target.display()
    );
}

/// index_paths must accept a path that is lexically outside the canonical
/// root but resolves under it (junction / macOS /var form). Content change
/// under the alias must update symbols under the store's real rel path.
#[test]
fn index_paths_accepts_alias_form_path() {
    let base = temp_root("alias-ip");
    let real = base.join("real");
    let alias = base.join("link");
    make_alias(&real, &alias);

    let real_a = real.join("src/a.ts");
    std::fs::write(&real_a, "export function helper() { return 1; }\n").unwrap();

    let indexer = Indexer::new(&real).expect("indexer");
    indexer.index(true).expect("seed index");

    let before = {
        let store = indexer.open_store().unwrap();
        store.find_symbol("helper", 20).unwrap()
    };
    assert_eq!(before.len(), 1, "seed helper: {before:?}");
    assert!(
        before[0].path.replace('\\', "/").ends_with("src/a.ts"),
        "store rel path expected src/a.ts: {:?}",
        before[0].path
    );

    // Modify via the REAL file, deliver the ALIAS path (notify form mismatch).
    std::fs::write(&real_a, "export function helper() { return 2; }\n").unwrap();
    let alias_a = alias.join("src/a.ts");
    assert!(
        !alias_a.starts_with(&indexer.root),
        "test setup: alias path must not lexically start with root"
    );

    indexer
        .index_paths(std::slice::from_ref(&alias_a))
        .expect("index_paths on alias-form path must succeed");

    let store = indexer.open_store().unwrap();
    let after = store.find_symbol("helper", 20).unwrap();
    assert_eq!(
        after.len(),
        1,
        "alias-form index_paths must not wipe/duplicate helper: {after:?}"
    );
    assert!(
        after[0].path.replace('\\', "/").ends_with("src/a.ts"),
        "rel path must remain src/a.ts: {:?}",
        after[0].path
    );
}

/// Deleted leaf delivered under an alias parent must prune via index_paths.
#[test]
fn index_paths_alias_form_deleted_leaf_prunes() {
    let base = temp_root("alias-del");
    let real = base.join("real");
    let alias = base.join("link");
    make_alias(&real, &alias);

    let real_a = real.join("src/a.ts");
    std::fs::write(&real_a, "export function helper() { return 1; }\n").unwrap();

    let indexer = Indexer::new(&real).expect("indexer");
    indexer.index(true).expect("seed index");
    assert_eq!(
        indexer
            .open_store()
            .unwrap()
            .find_symbol("helper", 20)
            .unwrap()
            .len(),
        1
    );

    std::fs::remove_file(&real_a).unwrap();
    let alias_gone = alias.join("src/a.ts");
    indexer
        .index_paths(std::slice::from_ref(&alias_gone))
        .expect("index_paths on deleted alias leaf");

    let after = indexer
        .open_store()
        .unwrap()
        .find_symbol("helper", 20)
        .unwrap();
    assert!(
        after.is_empty(),
        "deleted alias-form leaf must prune helper: {after:?}"
    );
}

/// A batch containing both an alias-form live file and an outside path must
/// update the live one and skip the outside one (no store wipe).
#[test]
fn index_paths_alias_batch_skips_outside_keeps_store() {
    let base = temp_root("alias-batch");
    let real = base.join("real");
    let alias = base.join("link");
    make_alias(&real, &alias);

    let real_a = real.join("src/a.ts");
    let real_b = real.join("src/b.ts");
    std::fs::write(&real_a, "export function alpha() { return 1; }\n").unwrap();
    std::fs::write(&real_b, "export function beta() { return 1; }\n").unwrap();
    std::fs::create_dir_all(base.join("outside")).unwrap();
    let outside = base.join("outside/evil.ts");
    std::fs::write(&outside, "export function evil() { return 1; }\n").unwrap();

    let indexer = Indexer::new(&real).expect("indexer");
    indexer.index(true).expect("seed index");

    std::fs::write(&real_a, "export function alpha() { return 2; }\n").unwrap();
    let paths = vec![alias.join("src/a.ts"), outside];
    indexer.index_paths(&paths).expect("mixed batch");

    let store = indexer.open_store().unwrap();
    let alpha = store.find_symbol("alpha", 20).unwrap();
    let beta = store.find_symbol("beta", 20).unwrap();
    let evil = store.find_symbol("evil", 20).unwrap();
    assert_eq!(alpha.len(), 1, "alpha must remain: {alpha:?}");
    assert_eq!(beta.len(), 1, "unrelated file must not be wiped: {beta:?}");
    assert!(
        evil.is_empty(),
        "outside path must not be indexed: {evil:?}"
    );
}
