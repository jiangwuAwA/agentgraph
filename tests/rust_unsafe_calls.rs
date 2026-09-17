//! Track A: calls inside `unsafe` blocks / `unsafe` fns must be L0-visible.
//!
//! Corpus pattern (stock-trading-app, private — not committed):
//!   `unsafe { File::from_raw_fd(fd) }`, `unsafe { libc::geteuid() }`,
//!   `UnixStream::from_raw_fd`, `libc::fcntl`.
//!
//! Policy (honesty):
//! - Extract should mint Exact call refs (tree-sitter already visits call nodes
//!   under `unsafe_block` / function bodies).
//! - S scanner must still flag the same file (`unsafe` → promise disabled).
//!   Graph edges AND sound stays off — do not enable `--sound` on unsafe crates.

use agentgraph::index::extract::extract_file;
use agentgraph::index::subset::scan_subset;
use agentgraph::model::{Confidence, EdgeKind, Language};
use std::collections::HashSet;

fn extract(src: &str, path: &str) -> agentgraph::index::extract::ExtractedFile {
    let known = HashSet::new();
    extract_file(src, Language::Rust, path, &known).expect("extract")
}

fn exact_calls<'a>(
    out: &'a agentgraph::index::extract::ExtractedFile,
    name: &str,
) -> Vec<&'a agentgraph::index::extract::ExtractedRef> {
    out.references
        .iter()
        .filter(|r| r.name == name && r.kind == EdgeKind::Call && r.confidence == Confidence::Exact)
        .collect()
}

fn any_calls<'a>(
    out: &'a agentgraph::index::extract::ExtractedFile,
    name: &str,
) -> Vec<&'a agentgraph::index::extract::ExtractedRef> {
    out.references
        .iter()
        .filter(|r| r.name == name && r.kind == EdgeKind::Call)
        .collect()
}

/// `File::from_raw_fd` inside an `unsafe` block → Exact Call with enclosing fn.
#[test]
fn unsafe_block_from_raw_fd_is_exact_call() {
    let src = r#"
use std::os::unix::io::RawFd;
use std::fs::File;

fn open_owned(fd: RawFd) -> File {
    let file = unsafe { File::from_raw_fd(fd) };
    file
}
"#;
    let out = extract(src, "src/security.rs");
    let hits = exact_calls(&out, "from_raw_fd");
    assert!(
        !hits.is_empty(),
        "unsafe-block File::from_raw_fd must be Exact Call; all refs={:?}",
        out.references
            .iter()
            .filter(|r| r.kind == EdgeKind::Call)
            .map(|r| (
                r.name.clone(),
                r.qualifier.clone(),
                r.enclosing.clone(),
                r.confidence.as_str(),
                r.evidence.as_ref().map(|e| e.rule_id.clone()),
            ))
            .collect::<Vec<_>>()
    );
    let hit = hits
        .iter()
        .find(|r| r.enclosing.as_deref() == Some("open_owned"))
        .expect("call must carry enclosing function name");
    assert_eq!(
        hit.qualifier.as_deref(),
        Some("File"),
        "system-API qualifier File preserved: {hit:?}"
    );
}

/// `libc::geteuid()` inside `unsafe fn` body → Exact Call, enclosing = fn name.
#[test]
fn unsafe_fn_libc_geteuid_is_exact_call() {
    let src = r#"
unsafe fn current_uid() -> u32 {
    libc::geteuid()
}
"#;
    let out = extract(src, "src/uid.rs");
    let hits = exact_calls(&out, "geteuid");
    assert!(
        !hits.is_empty(),
        "libc::geteuid inside unsafe fn must be Exact Call; all refs={:?}",
        out.references
            .iter()
            .filter(|r| r.kind == EdgeKind::Call)
            .map(|r| (
                r.name.clone(),
                r.qualifier.clone(),
                r.enclosing.clone(),
                r.confidence.as_str(),
            ))
            .collect::<Vec<_>>()
    );
    let hit = hits
        .iter()
        .find(|r| r.enclosing.as_deref() == Some("current_uid"))
        .expect("geteuid call must carry enclosing fn name");
    assert_eq!(
        hit.qualifier.as_deref(),
        Some("libc"),
        "system-API qualifier libc preserved"
    );
}

/// `libc::geteuid()` / `libc::fcntl` inside a normal fn's `unsafe` block.
#[test]
fn unsafe_block_libc_calls_are_exact() {
    let src = r#"
fn drop_cloexec(fd: i32) -> i32 {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) }
}

fn owner_uid(path: &std::path::Path) -> u32 {
    let _ = path;
    unsafe { libc::geteuid() }
}
"#;
    let out = extract(src, "src-tauri/src/lib.rs");
    let fcntl = exact_calls(&out, "fcntl");
    assert!(
        !fcntl.is_empty(),
        "libc::fcntl in unsafe block must be Exact; refs={:?}",
        out.references
            .iter()
            .filter(|r| r.kind == EdgeKind::Call)
            .map(|r| (r.name.clone(), r.enclosing.clone(), r.confidence.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(
        fcntl
            .iter()
            .any(|r| r.enclosing.as_deref() == Some("drop_cloexec")),
        "fcntl must be attributed to drop_cloexec"
    );
    let geteuid = exact_calls(&out, "geteuid");
    assert!(
        geteuid
            .iter()
            .any(|r| r.enclosing.as_deref() == Some("owner_uid")),
        "geteuid must be attributed to owner_uid"
    );
}

/// Multiple system APIs in one unsafe block (corpus security.rs shape).
#[test]
fn unsafe_block_unix_stream_from_raw_fd_is_exact() {
    let src = r#"
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::fs::File;
use std::os::unix::net::UnixStream;

fn wrap_stream(fd: RawFd) -> UnixStream {
    let descriptor = unsafe { File::from_raw_fd(fd) };
    let mut channel = unsafe { UnixStream::from_raw_fd(descriptor.as_raw_fd()) };
    channel
}
"#;
    let out = extract(src, "crates/api/src/security.rs");
    // Both File::from_raw_fd and UnixStream::from_raw_fd mint `from_raw_fd`.
    let raw = any_calls(&out, "from_raw_fd");
    assert!(
        raw.len() >= 2,
        "expected both File::from_raw_fd and UnixStream::from_raw_fd; got {raw:?} / refs={:?}",
        out.references
            .iter()
            .filter(|r| r.kind == EdgeKind::Call)
            .map(|r| (
                r.name.clone(),
                r.qualifier.clone(),
                r.enclosing.clone(),
                r.confidence.as_str()
            ))
            .collect::<Vec<_>>()
    );
    assert!(
        raw.iter()
            .all(|r| r.confidence == Confidence::Exact
                && r.enclosing.as_deref() == Some("wrap_stream")),
        "both unsafe system-API calls must be Exact under wrap_stream"
    );
    let quals: Vec<_> = raw.iter().filter_map(|r| r.qualifier.clone()).collect();
    assert!(
        quals.iter().any(|q| q == "File") && quals.iter().any(|q| q == "UnixStream"),
        "qualifiers must distinguish File vs UnixStream: {quals:?}"
    );
}

/// Prefer Exact — do not invent a Heuristic rule id for already-visited calls.
#[test]
fn unsafe_calls_are_not_relabelled_heuristic() {
    let src = r#"
fn open_fd(fd: i32) -> *mut u8 {
    unsafe { File::from_raw_fd(fd) };
    unsafe { libc::geteuid() as *mut u8 }
}
"#;
    let out = extract(src, "src/security.rs");
    for r in out
        .references
        .iter()
        .filter(|r| r.kind == EdgeKind::Call && (r.name == "from_raw_fd" || r.name == "geteuid"))
    {
        assert_eq!(
            r.confidence,
            Confidence::Exact,
            "unsafe call sites stay Exact L0 (no new heuristic rule): {r:?}"
        );
        assert!(
            r.evidence.is_none(),
            "Exact unsafe-call edges carry no L1 rule evidence: {r:?}"
        );
    }
}

/// Graph has edges AND sound stays off: same source still violates S via `unsafe`.
#[test]
fn unsafe_file_with_call_edges_still_leaves_s() {
    let src = r#"
use std::fs::File;
use std::os::unix::io::RawFd;

fn open_owned(fd: RawFd) -> File {
    unsafe { File::from_raw_fd(fd) }
}

unsafe fn evil_uid() -> u32 {
    libc::geteuid()
}
"#;
    let out = extract(src, "src/security.rs");
    assert!(
        !exact_calls(&out, "from_raw_fd").is_empty(),
        "edges must exist in the L0 graph"
    );
    assert!(
        !exact_calls(&out, "geteuid").is_empty(),
        "unsafe-fn system API must exist in the L0 graph"
    );

    let r = scan_subset(src, Language::Rust, "src/security.rs");
    assert!(
        !r.in_subset,
        "unsafe must still leave S (promise disabled) even when call edges are indexed: {:?}",
        r.violations
    );
    assert!(
        r.violations.iter().any(|v| v.kind == "unsafe"),
        "S scanner must report unsafe violation: {:?}",
        r.violations
    );
}

/// Nested closures / async blocks inside unsafe — still attributed to outer fn
/// when no intermediate scope symbol is minted.
#[test]
fn nested_scope_inside_unsafe_still_visible() {
    let src = r#"
fn load(fd: i32) {
    unsafe {
        let f = File::from_raw_fd(fd);
        drop(f);
    }
}
"#;
    let out = extract(src, "src/loader.rs");
    let hits = exact_calls(&out, "from_raw_fd");
    assert!(
        !hits.is_empty(),
        "nested unsafe-block call must remain visible; refs={:?}",
        out.references
            .iter()
            .filter(|r| r.kind == EdgeKind::Call)
            .map(|r| (r.name.clone(), r.enclosing.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(hits[0].enclosing.as_deref(), Some("load"));
}
