//! TDD: AGENTGRAPH_TRUST_MTIME=0 disables mtime short-circuit (hash is truth).

use agentgraph::index::Indexer;
use std::path::PathBuf;

fn temp_root(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentgraph-mtime-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    d
}

#[test]
fn trust_mtime_zero_still_indexes_and_is_idempotent() {
    let root = temp_root("off");
    let file = root.join("src/a.ts");
    std::fs::write(&file, "export function a() { return 1; }\n").unwrap();
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(false).unwrap();

    std::fs::write(&file, "export function a() { return 2; }\n").unwrap();
    std::env::set_var("AGENTGRAPH_TRUST_MTIME", "0");
    let s1 = indexer.index(false).unwrap();
    let s2 = indexer.index(false).unwrap();
    std::env::remove_var("AGENTGRAPH_TRUST_MTIME");
    assert!(s1.files >= 1);
    // With trust off, every index rehashes; noop still succeeds (dirty=0).
    assert_eq!(s1.symbols, s2.symbols);
}

#[test]
fn trust_mtime_default_short_circuits_unchanged() {
    let root = temp_root("on");
    std::fs::write(root.join("src/a.ts"), "export function a() { return 1; }\n").unwrap();
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(false).unwrap();
    let s2 = indexer.index(false).unwrap();
    assert!(
        s2.skipped_files >= 1,
        "default trust_mtime should meta-skip unchanged"
    );
}
