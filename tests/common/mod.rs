//! Shared integration-test temp hygiene helpers.
//!
//! Parallel `cargo test` races on fixed paths under `std::env::temp_dir()`.
//! Every helper here returns a path that includes pid + atomic counter +
//! wall-clock nanos so same-process threads and concurrent test binaries
//! never share a directory or SQLite file.

#![allow(dead_code)] // each test binary uses a subset of these helpers

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Process-unique tag: pid + monotonic counter + wall-clock nanos.
///
/// Counter alone is unique within a process; pid isolates concurrent
/// test binaries; nanos keep leftovers from prior runs distinguishable.
pub fn unique_tag() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{n}-{c}", std::process::id())
}

/// Unique path under the OS temp dir. Does **not** create the directory.
pub fn unique_temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{tag}-{}", unique_tag()))
}

/// Create a unique temp directory (`create_dir_all`) and return it.
pub fn temp_root(tag: &str) -> PathBuf {
    let dir = unique_temp_dir(tag);
    std::fs::create_dir_all(&dir).expect("create unique temp_root");
    dir
}

/// Unique directory containing `index.db` (parent directory is created).
pub fn temp_db(tag: &str) -> PathBuf {
    temp_root(tag).join("index.db")
}

/// Recursive copy that always skips `.agentgraph` (index DB / sidecars).
pub fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("copy_dir create dst");
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        if name == ".agentgraph" {
            continue;
        }
        let t = dst.join(&name);
        if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            copy_dir(&e.path(), &t);
        } else {
            let _ = std::fs::copy(e.path(), &t);
        }
    }
}

/// Copy fixture tree into a fresh unique temp dir; skips `.agentgraph`.
pub fn copy_fixture_to_temp(src: &Path, tag: &str) -> PathBuf {
    let dst = unique_temp_dir(tag);
    let _ = std::fs::remove_dir_all(&dst);
    copy_dir(src, &dst);
    dst
}

/// Best-effort recursive remove; ignore errors (Windows file locks / AV).
pub fn remove_dir_all_ignore(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

/// RAII guard: creates a unique temp dir and removes it on drop (best-effort).
pub struct TempDirGuard(pub PathBuf);

impl TempDirGuard {
    pub fn new(tag: &str) -> Self {
        Self(temp_root(tag))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        remove_dir_all_ignore(&self.0);
    }
}

impl std::ops::Deref for TempDirGuard {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}
