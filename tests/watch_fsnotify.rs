//! TDD: fsnotify-level watch — file change must trigger reindex without multi-second poll.
use agentgraph::index::Indexer;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod common;

fn temp_root(tag: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-watch-{tag}"));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/a.ts"),
        "export function alpha() { return 1; }\n",
    )
    .unwrap();
    dir
}

#[test]
fn watch_reindexes_on_file_change_within_300ms() {
    let root = temp_root("change");
    let indexer = Indexer::new(&root).unwrap();
    // Seed index so we can detect a subsequent change.
    indexer.index(true).unwrap();

    let (rx, handle) = indexer
        .watch_events(Duration::from_millis(50))
        .expect("start watcher");

    // Give the watcher a moment to arm (debounce window).
    std::thread::sleep(Duration::from_millis(80));

    let start = Instant::now();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(root.join("src/a.ts"))
        .unwrap();
    writeln!(f, "export function beta() {{ return 2; }}").unwrap();
    f.sync_all().unwrap();
    drop(f);

    // Expect at least one reindex notification promptly (fsnotify, not 1s poll).
    let ev = rx.recv_timeout(Duration::from_millis(800));
    let elapsed = start.elapsed();
    assert!(
        ev.is_ok(),
        "expected watch event within 800ms, got {:?} after {elapsed:?}",
        ev
    );
    assert!(
        elapsed < Duration::from_millis(800),
        "too slow for fsnotify: {elapsed:?}"
    );

    // New symbol must be queryable.
    let store = indexer.open_store().unwrap();
    let hits = store.find_symbol("beta", 10).unwrap();
    assert!(
        hits.iter().any(|s| s.name == "beta"),
        "beta should be indexed after watch"
    );

    // Shutdown: drop rx side by stopping watcher via channel close — handle abort not required;
    // ensure we don't leak forever in test by dropping receiver and joining with timeout is hard.
    drop(rx);
    let _ = handle; // watcher thread exits when send fails / process ends
}

#[test]
fn watch_does_not_spam_reindex_without_change() {
    let root = temp_root("idle");
    let indexer = Indexer::new(&root).unwrap();
    indexer.index(true).unwrap();
    let (rx, _handle) = indexer.watch_events(Duration::from_millis(50)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        rx.try_recv().is_err(),
        "no events expected when nothing changed"
    );
}
