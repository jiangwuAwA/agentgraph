//! Noise-dir source skips are counted (R4 minor: testdata vanished silently).

use agentgraph::index::walker::collect_source_files_with_stats;
use agentgraph::index::Indexer;
use std::path::PathBuf;

#[test]
fn testdata_source_files_are_counted_as_noise_skipped() {
    let root = std::env::temp_dir().join(format!("agentgraph-noise-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("testdata")).unwrap();
    std::fs::write(root.join("src/a.ts"), "export function a() { return 1; }\n").unwrap();
    std::fs::write(
        root.join("testdata/fixture.ts"),
        "export function fixture() { return 2; }\n",
    )
    .unwrap();
    let collected = collect_source_files_with_stats(&root).unwrap();
    assert_eq!(collected.files.len(), 1, "only src/a.ts indexed");
    assert!(
        collected.noise_skipped >= 1,
        "testdata source must be counted; got {:?}",
        collected
    );
    let indexer = Indexer::new(&root).unwrap();
    let stats = indexer.index(false).unwrap();
    assert!(
        stats.noise_skipped_files >= 1,
        "IndexStats.noise_skipped_files must surface the skip"
    );
    let _ = PathBuf::from(&root);
}
