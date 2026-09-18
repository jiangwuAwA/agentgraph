//! TDD: enrich must persist successful descriptions even when later failures trigger bail.
//!
//! Issue 6: the original code bailed BEFORE writing results, losing all successes.

use agentgraph::index::extract::extract_file;
use agentgraph::index::llm::{enrich, LlmConfig};
use agentgraph::index::store::Store;
use agentgraph::model::Language;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

mod common;

/// Minimal mock OpenAI-compatible server: returns 200 for the first `ok_count`
/// requests, then 500 (server error) to trigger the abort path.
fn spawn_mock_llm(ok_count: usize) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let served = Arc::new(AtomicUsize::new(0));
    let served_clone = served.clone();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // Read the HTTP request (best-effort, we don't need the body).
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);

            let n = served_clone.fetch_add(1, Ordering::SeqCst);
            let response = if n < ok_count {
                let body = r#"{"choices":[{"message":{"content":"Does a thing."}}]}"#;
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                )
            } else {
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_string()
            };
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    (format!("http://{}", addr), served)
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = common::temp_root(&format!("agentgraph-enrich-{name}"));
    let _ = std::fs::create_dir_all(dir.join("src"));
    dir
}

fn seed_store(root: &Path) -> Store {
    let db = root.join(".agentgraph").join("index.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let mut store = Store::open(&db).unwrap();
    let known: HashSet<String> = HashSet::new();
    let src = r#"
export function alpha() { return 1; }
export function beta() { return 2; }
export function gamma() { return 3; }
export function delta() { return 4; }
export function epsilon() { return 5; }
"#;
    std::fs::write(root.join("src/lib.ts"), src).unwrap();
    let parsed = extract_file(src, Language::TypeScript, "src/lib.ts", &known).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file("src/lib.ts", "h", "typescript", &parsed)
        .unwrap();
    store.commit_batch().unwrap();
    store
}

#[test]
fn enrich_persists_successes_before_bail() {
    let dir = temp_dir("bail");
    let mut store = seed_store(&dir);

    // Mock: first request succeeds, then 500s (triggers abort at 3 failures).
    let (base_url, _served) = spawn_mock_llm(1);

    let cfg = LlmConfig {
        api_key: "test-key".into(),
        base_url,
        model: "mock-model".into(),
        concurrency: 1, // sequential so ordering is deterministic
    };

    // enrich should bail (too many failures) but MUST have persisted the 2 successes.
    let result = enrich(&dir, &mut store, &cfg, 10);
    assert!(
        result.is_err(),
        "enrich should bail after 3+ failures, got: {result:?}"
    );
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("too many") || err_msg.contains("abort"),
        "error should mention abort, got: {err_msg}"
    );

    // CRITICAL: the successful description(s) must be persisted.
    let described = store
        .find_symbol_exact("alpha", 1)
        .unwrap()
        .into_iter()
        .chain(store.find_symbol_exact("beta", 1).unwrap())
        .chain(store.find_symbol_exact("gamma", 1).unwrap())
        .chain(store.find_symbol_exact("delta", 1).unwrap())
        .chain(store.find_symbol_exact("epsilon", 1).unwrap())
        .filter(|s| {
            s.description
                .as_deref()
                .unwrap_or("")
                .contains("Does a thing")
        })
        .count();
    assert!(
        described >= 1,
        "at least 1 successful description must survive the bail, got {described}"
    );
}
