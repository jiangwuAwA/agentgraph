//! P2 optional macro-expanded side-index (CLI default OFF).
//! TDD contract: sidecar is dual-index only — not sound, not default.
//!
//! See docs/macro-sidecar.md. Do not claim subset_ok from expanded-only rows.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_pair(tag: &str) -> (PathBuf, PathBuf) {
    let base = common::temp_root(&format!("agentgraph-macro-{tag}"));
    let root = base.join("src-root");
    let expanded = base.join("expanded-shadow");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(expanded.join("src")).unwrap();
    (root, expanded)
}

/// Source L0/L1 tree: helper + two callers. No macro-only symbols.
fn write_main_fixture(root: &Path) {
    std::fs::write(
        root.join("src/core.rs"),
        r#"
pub fn helper() -> i32 {
    1
}

pub fn process() -> i32 {
    helper() + 1
}

pub fn validate() -> bool {
    helper() > 0
}
"#,
    )
    .unwrap();
}

/// Expanded shadow tree: same symbols + macro-generated `fmt` / `clone`
/// that also call `helper`. `fmt` uses `unsafe` so the *sidecar* would leave S
/// — main index must not inherit that claim either way.
fn write_expanded_fixture(expanded: &Path) {
    std::fs::write(
        expanded.join("src/core.rs"),
        r#"
pub fn helper() -> i32 {
    1
}

pub fn process() -> i32 {
    helper() + 1
}

pub fn validate() -> bool {
    helper() > 0
}

// Macro-generated stand-ins (proc-macro / derive-like).
pub fn fmt() -> i32 {
    unsafe { helper() }
}

pub fn clone() -> i32 {
    helper()
}
"#,
    )
    .unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn parse_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).unwrap_or_else(|e| {
        panic!(
            "invalid JSON ({e}): stdout={} stderr={}",
            stdout(out),
            stderr(out)
        )
    })
}

fn callers_enclosings(hits: &serde_json::Value) -> Vec<String> {
    union_rows(hits)
        .iter()
        .filter_map(|r| r["enclosing"].as_str().map(|s| s.to_string()))
        .collect()
}

/// M1: `--with-macro` with a present sidecar returns a wrapped object
/// (`callers`/`impact` + dedup_stats/stale); absent sidecar stays a plain array.
fn union_rows(v: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = v.as_array() {
        return arr.clone();
    }
    if let Some(arr) = v.get("callers").and_then(|x| x.as_array()) {
        return arr.clone();
    }
    if let Some(arr) = v.get("impact").and_then(|x| x.as_array()) {
        return arr.clone();
    }
    panic!("expected array or wrapped with-macro payload: {v}");
}

fn has_origin_macro(hits: &serde_json::Value) -> bool {
    union_rows(hits)
        .iter()
        .any(|r| r["origin"] == "macro_expanded")
}

/// 1. Default index + callers: no sidecar required.
///    `--with-macro` when sidecar is missing is graceful (same hits, no error,
///    no sidecar file created).
#[test]
fn default_callers_no_sidecar_and_with_macro_graceful() {
    let (root, _expanded) = temp_pair("default");
    write_main_fixture(&root);

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "index stderr={}", stderr(&idx));
    // Default index JSON is still plain IndexStats (no sidecar field required).
    let stats = parse_json(&idx);
    assert!(stats["symbols"].as_u64().unwrap() >= 3);
    assert!(stats.get("macro_sidecar").is_none());

    let sidecar = root.join(".agentgraph").join("index.macro.db");
    assert!(
        !sidecar.exists(),
        "default index must not create sidecar at {}",
        sidecar.display()
    );

    let plain = run(&root, &["callers", "helper"]);
    assert!(plain.status.success(), "{}", stderr(&plain));
    let plain_hits = parse_json(&plain);
    let plain_enc = callers_enclosings(&plain_hits);
    assert!(
        plain_enc.iter().any(|e| e == "process"),
        "expected process: {plain_enc:?}"
    );
    assert!(
        plain_enc.iter().any(|e| e == "validate"),
        "expected validate: {plain_enc:?}"
    );
    assert!(
        !plain_enc.iter().any(|e| e == "fmt" || e == "clone"),
        "source-only callers must not include expanded symbols: {plain_enc:?}"
    );
    assert!(
        !has_origin_macro(&plain_hits),
        "default callers must not tag origin=macro_expanded"
    );

    // --with-macro with absent sidecar: same content, still success.
    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(
        with.status.success(),
        "--with-macro must be graceful when sidecar missing: {}",
        stderr(&with)
    );
    let with_hits = parse_json(&with);
    let with_enc = callers_enclosings(&with_hits);
    assert_eq!(
        plain_enc, with_enc,
        "absent sidecar must not change callers union"
    );
    assert!(!has_origin_macro(&with_hits));
    assert!(
        !sidecar.exists(),
        "--with-macro must not create the sidecar file"
    );

    // macro status reports absent without creating the DB.
    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let status = parse_json(&st);
    assert_eq!(status["exists"], false, "status={status}");
    assert!(
        !sidecar.exists(),
        "macro status must not create the sidecar file"
    );
}

/// 2. Build sidecar from expanded shadow tree; `--with-macro` finds extra
///    symbols; default callers does not.
#[test]
fn with_macro_finds_expanded_only_callers() {
    let (root, expanded) = temp_pair("union");
    write_main_fixture(&root);
    write_expanded_fixture(&expanded);

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let exp_str = expanded.to_string_lossy().into_owned();
    let side_idx = run(
        &root,
        &["index", "--force", "--macro-expanded-root", &exp_str],
    );
    assert!(
        side_idx.status.success(),
        "sidecar index stderr={}",
        stderr(&side_idx)
    );
    let payload = parse_json(&side_idx);
    assert!(
        payload["macro_sidecar"]["origin"] == "macro_expanded",
        "payload={payload}"
    );
    assert!(
        payload["macro_sidecar"]["files"].as_u64().unwrap() >= 1,
        "payload={payload}"
    );

    // Default callers: still source-only.
    let plain = run(&root, &["callers", "helper"]);
    assert!(plain.status.success(), "{}", stderr(&plain));
    let plain_hits = parse_json(&plain);
    let plain_enc = callers_enclosings(&plain_hits);
    assert!(
        !plain_enc.iter().any(|e| e == "fmt" || e == "clone"),
        "default callers must stay source-only after sidecar build: {plain_enc:?}"
    );
    assert!(!has_origin_macro(&plain_hits));

    // --with-macro: union includes expanded-only enclosings, tagged.
    let with = run(&root, &["callers", "helper", "--with-macro"]);
    assert!(with.status.success(), "{}", stderr(&with));
    let with_hits = parse_json(&with);
    let with_enc = callers_enclosings(&with_hits);
    assert!(
        with_enc.iter().any(|e| e == "fmt"),
        "with-macro must include fmt: {with_enc:?} raw={}",
        stdout(&with)
    );
    assert!(
        with_enc.iter().any(|e| e == "clone"),
        "with-macro must include clone: {with_enc:?}"
    );
    assert!(
        has_origin_macro(&with_hits),
        "sidecar rows must be tagged origin=macro_expanded: {}",
        stdout(&with)
    );

    // impact --with-macro also unions (expanded-only enclosing reachable).
    let imp = run(&root, &["impact", "helper", "--depth", "2", "--with-macro"]);
    assert!(imp.status.success(), "{}", stderr(&imp));
    let imp_hits = parse_json(&imp);
    let imp_rows = union_rows(&imp_hits);
    assert!(
        imp_rows.iter().any(|n| n["enclosing"] == "fmt"
            || n["enclosing"] == "clone"
            || n["origin"] == "macro_expanded"),
        "impact --with-macro should surface expanded rows: {}",
        stdout(&imp)
    );
}

/// 3. `macro status` reports sidecar after build.
#[test]
fn macro_status_reports_sidecar_after_build() {
    let (root, expanded) = temp_pair("status");
    write_main_fixture(&root);
    write_expanded_fixture(&expanded);

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    let exp_str = expanded.to_string_lossy().into_owned();
    let side_idx = run(
        &root,
        &["index", "--force", "--macro-expanded-root", &exp_str],
    );
    assert!(side_idx.status.success(), "{}", stderr(&side_idx));

    let st = run(&root, &["macro", "status"]);
    assert!(st.status.success(), "{}", stderr(&st));
    let status = parse_json(&st);
    assert_eq!(status["exists"], true, "status={status}");
    assert!(
        status["path"]
            .as_str()
            .unwrap_or("")
            .contains("index.macro.db"),
        "status={status}"
    );
    assert!(status["files"].as_u64().unwrap() >= 1, "status={status}");
    assert!(
        status["symbols"].as_u64().unwrap() >= 5,
        "expanded tree should mint helper/process/validate/fmt/clone: status={status}"
    );
    assert!(status["refs"].as_u64().unwrap() >= 2, "status={status}");
    assert_eq!(status["origin"], "macro_expanded", "status={status}");
}

/// 4. Main `.agentgraph/index.db` ref count unchanged after sidecar index.
#[test]
fn main_index_ref_count_unchanged_after_sidecar() {
    let (root, expanded) = temp_pair("main-isolated");
    write_main_fixture(&root);
    write_expanded_fixture(&expanded);

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));
    let before = run(&root, &["stats"]);
    assert!(before.status.success(), "{}", stderr(&before));
    let before_stats = parse_json(&before);
    let before_refs = before_stats["references"].as_u64().unwrap();
    let before_symbols = before_stats["symbols"].as_u64().unwrap();
    assert!(before_refs > 0);

    let exp_str = expanded.to_string_lossy().into_owned();
    let side_idx = run(
        &root,
        &["index", "--force", "--macro-expanded-root", &exp_str],
    );
    assert!(side_idx.status.success(), "{}", stderr(&side_idx));

    let after = run(&root, &["stats"]);
    assert!(after.status.success(), "{}", stderr(&after));
    let after_stats = parse_json(&after);
    assert_eq!(
        after_stats["references"].as_u64().unwrap(),
        before_refs,
        "main index refs must be unchanged by sidecar build: before={before_stats} after={after_stats}"
    );
    assert_eq!(
        after_stats["symbols"].as_u64().unwrap(),
        before_symbols,
        "main index symbols must be unchanged by sidecar build"
    );

    // Sidecar itself has more symbols than main (fmt/clone only exist there).
    let st = parse_json(&run(&root, &["macro", "status"]));
    assert!(
        st["symbols"].as_u64().unwrap() > before_symbols,
        "sidecar should hold expanded-only symbols: status={st} main={before_symbols}"
    );
}

/// 5. Expanded-only symbols do not get `subset_ok` true claims.
///    - `--sound --with-macro` is rejected (no combined sound claim).
///    - `--with-macro` JSON is a plain array (no subset_ok field).
///    - Main `subset` is not flipped by sidecar S-violations (`unsafe` in fmt).
#[test]
fn expanded_only_symbols_no_subset_ok_claims() {
    let (root, expanded) = temp_pair("subset-claim");
    write_main_fixture(&root);
    write_expanded_fixture(&expanded);

    let idx = run(&root, &["index", "--force"]);
    assert!(idx.status.success(), "{}", stderr(&idx));

    // Main source is clean Rust — subset should be OK before sidecar.
    let subset_before = run(&root, &["subset"]);
    assert!(
        subset_before.status.success(),
        "main fixture should be in-subset: {}",
        stderr(&subset_before)
    );
    let subset_before_json = parse_json(&subset_before);
    assert_eq!(subset_before_json["in_subset"], true);

    let exp_str = expanded.to_string_lossy().into_owned();
    let side_idx = run(
        &root,
        &["index", "--force", "--macro-expanded-root", &exp_str],
    );
    assert!(side_idx.status.success(), "{}", stderr(&side_idx));

    // Main subset claim unchanged (sidecar S-violations stay in sidecar).
    let subset_after = run(&root, &["subset"]);
    assert!(
        subset_after.status.success(),
        "sidecar must not flip main subset: {}",
        stderr(&subset_after)
    );
    let subset_after_json = parse_json(&subset_after);
    assert_eq!(
        subset_after_json["in_subset"], true,
        "main in_subset must stay true; expanded unsafe stays out of main claims: {subset_after_json}"
    );

    // Sound walk on main remains main-only (subset_ok refers to main S only).
    let sound_main = run(&root, &["callers", "helper", "--sound"]);
    assert!(sound_main.status.success(), "{}", stderr(&sound_main));
    let sound_json = parse_json(&sound_main);
    assert_eq!(sound_json["mode"], "sound");
    // Even if subset_ok is true here, it must NOT include expanded-only rows.
    let sound_callers = &sound_json["callers"];
    let sound_enc = callers_enclosings(sound_callers);
    assert!(
        !sound_enc.iter().any(|e| e == "fmt" || e == "clone"),
        "sound walk must not absorb expanded-only symbols: {sound_enc:?}"
    );
    assert!(
        !sound_callers
            .as_array()
            .map(|a| a.iter().any(|r| r["origin"] == "macro_expanded"))
            .unwrap_or(false),
        "sound payload must not tag macro_expanded rows"
    );

    // --sound --with-macro is rejected — never a combined subset_ok claim.
    let combo = run(&root, &["callers", "helper", "--sound", "--with-macro"]);
    assert!(!combo.status.success(), "sound+with_macro must fail closed");
    let err = stderr(&combo).to_lowercase();
    assert!(
        err.contains("with-macro") || err.contains("with_macro") || err.contains("mutually"),
        "stderr={err}"
    );

    // Expanded-only symbol query via --with-macro: no subset_ok in payload.
    let fmt_with = run(&root, &["callers", "fmt", "--with-macro"]);
    // fmt is not in main index; callers may return [] or only sidecar hits.
    // Either way the CLI must not invent a sound claim.
    if fmt_with.status.success() {
        let fmt_json = parse_json(&fmt_with);
        let text = stdout(&fmt_with);
        assert!(
            !text.contains("\"subset_ok\""),
            "with-macro callers payload must not carry subset_ok: {text}"
        );
        // If any rows appear, they must be origin-tagged when from sidecar.
        if let Some(arr) = fmt_json.as_array() {
            for r in arr {
                if r.get("origin").is_some() {
                    assert_eq!(r["origin"], "macro_expanded");
                }
            }
        } else if let Some(arr) = fmt_json.get("callers").and_then(|x| x.as_array()) {
            for r in arr {
                if r.get("origin").is_some() {
                    assert_eq!(r["origin"], "macro_expanded");
                }
            }
        }
    } else {
        // Empty main index for `fmt` can also fail loudly — acceptable as long
        // as it is not a false subset_ok=true claim.
        let err = stderr(&fmt_with).to_lowercase();
        assert!(
            !err.contains("subset_ok"),
            "error path must not claim subset_ok: {err}"
        );
    }
}
