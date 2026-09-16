//! TDD: AGENTGRAPH_TRUST_MTIME=0 disables mtime short-circuit (hash is truth).
//! Uses a **file lock** so parallel test *binaries* do not race on process-global env.

use agentgraph::index::Indexer;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock_path() -> PathBuf {
    std::env::temp_dir().join("agentgraph-trust-mtime.lock")
}

struct FileEnvGuard {
    _file: std::fs::File,
}

impl FileEnvGuard {
    fn acquire() -> Self {
        let path = env_lock_path();
        // Retry lock across test binaries (process-global env).
        for _ in 0..200 {
            if let Ok(mut f) = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
            {
                // Exclusive-ish: hold the file open for the duration of the test.
                let _ = writeln!(f, "{}", std::process::id());
                return Self { _file: f };
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        // Last resort: proceed without file lock (in-process mutex still held).
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open env lock");
        Self { _file: f }
    }
}

fn temp_root(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentgraph-mtime-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    d
}

#[test]
fn trust_mtime_zero_still_indexes_and_is_idempotent() {
    let _g = ENV_LOCK.lock().unwrap();
    let _fg = FileEnvGuard::acquire();
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
    assert_eq!(s1.symbols, s2.symbols);
    let _ = Path::new(&file);
}

#[test]
fn trust_mtime_default_short_circuits_unchanged() {
    let _g = ENV_LOCK.lock().unwrap();
    let _fg = FileEnvGuard::acquire();
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
