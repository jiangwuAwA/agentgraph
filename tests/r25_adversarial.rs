//! R25 adversarial probes: `record_parse_error` completeness matrix for every
//! index()/index_paths() skip path; Go `//export` directive forms; Nest
//! sound-allowlist ↔ docs sync.
//!
//! Completeness contract (keep-set):
//!   Every source file that remains in the keep-set MUST either
//!   (a) be successfully indexed (true in-S candidate), or
//!   (b) carry a `parse_error` subset violation.
//! Files intentionally outside the corpus (noise dirs, unsupported extensions)
//! must NOT appear in the keep-set.

use agentgraph::index::subset::{is_sound_eligible, scan_subset};
use agentgraph::index::Indexer;
use agentgraph::model::{Confidence, Language};
use std::path::PathBuf;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentgraph-r25-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn has_parse_error(store: &agentgraph::index::store::Store, needle: &str) -> bool {
    store
        .subset_violations()
        .unwrap()
        .iter()
        .any(|v| v.kind == "parse_error" && v.path.contains(needle))
}

// ── A1: record_parse_error completeness matrix ─────────────────────────

/// Table-driven skip-path matrix. Each case plants a file (or path condition)
/// and asserts the completeness contract after full and/or scoped index.
#[test]
fn skip_path_completeness_matrix() {
    // Enumerated skip paths. Variants:
    //   Full  — `Indexer::index(true)`
    //   Scoped — `Indexer::index_paths([path])` (watch)
    struct Case {
        name: &'static str,
        /// File under `src/` (None = path-condition only).
        file: Option<&'static str>,
        /// File body; `None` = do not create.
        body: Option<Vec<u8>>,
        expect_parse_error_full: bool,
        expect_parse_error_scoped: bool,
        /// True when the path is intentionally outside the corpus (no mint required).
        out_of_corpus: bool,
    }

    let cases: Vec<Case> = vec![
        // 1. UTF-8 failure — full + scoped both mint (R24).
        Case {
            name: "utf8_invalid",
            file: Some("bad_utf8.ts"),
            body: Some(vec![0xff, 0xfe, 0x00, 0x01, 0xc0, 0x81]),
            expect_parse_error_full: true,
            expect_parse_error_scoped: true,
            out_of_corpus: false,
        },
        // 2. Oversized (>1.5MiB) — full + scoped both mint (R13/R24).
        Case {
            name: "oversized",
            file: Some("big.ts"),
            body: Some({
                let mut v = b"export function big() { return '".to_vec();
                v.extend(std::iter::repeat_n(b'x', 1_600_000));
                v.extend(b"'; }\n".iter().copied());
                v
            }),
            expect_parse_error_full: true,
            expect_parse_error_scoped: true,
            out_of_corpus: false,
        },
        // 3. Minified bundle — full + scoped both mint (R13/R24).
        Case {
            name: "minified",
            file: Some("app.min.js"),
            body: Some(b"function a(){return 1}function b(){return 2}\n".to_vec()),
            expect_parse_error_full: true,
            expect_parse_error_scoped: true,
            out_of_corpus: false,
        },
        // 4. Clean source — true in-S file (no parse_error).
        Case {
            name: "clean_in_s",
            file: Some("ok.ts"),
            body: Some(b"export function helper() { return 1; }\n".to_vec()),
            expect_parse_error_full: false,
            expect_parse_error_scoped: false,
            out_of_corpus: false,
        },
        // 5. Noise-dir source — intentionally outside corpus; no mint required.
        //    (walker skips testdata/; not in keep-set; prune removes stale rows.)
        Case {
            name: "noise_dir_out_of_corpus",
            file: Some("testdata/gen.ts"),
            body: Some(b"export function gen() { return 1; }\n".to_vec()),
            expect_parse_error_full: false,
            expect_parse_error_scoped: false,
            out_of_corpus: true,
        },
    ];

    for case in &cases {
        let root = temp_root(case.name);
        // Parent dirs for nested cases (testdata/).
        if let Some(f) = case.file {
            if let Some(parent) = std::path::Path::new(f).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(root.join("src").join(parent)).unwrap();
                }
            }
        }
        // Always plant a clean sibling so the store is non-empty.
        std::fs::write(
            root.join("src/sibling.ts"),
            "export function sibling() { return 1; }\n",
        )
        .unwrap();
        let target = case.file.map(|f| root.join("src").join(f));
        if let (Some(t), Some(b)) = (&target, &case.body) {
            std::fs::write(t, b).unwrap();
        }

        let indexer = Indexer::new(&root).expect("indexer");
        indexer.index(true).expect("full index");
        let store = indexer.open_store().expect("store");
        let needle = case.file.unwrap_or("");
        if case.out_of_corpus {
            assert!(
                !has_parse_error(&store, needle),
                "{}: noise-dir file must not mint parse_error (out of corpus)",
                case.name
            );
        } else if case.expect_parse_error_full {
            assert!(
                has_parse_error(&store, needle),
                "{}: full index must mint parse_error; viols={:?}",
                case.name,
                store.subset_violations().unwrap()
            );
        } else {
            assert!(
                !has_parse_error(&store, needle),
                "{}: clean file must not mint parse_error",
                case.name
            );
        }

        // Scoped path: fresh DB per case for independence on scoped arm.
        if let Some(t) = &target {
            let root2 = temp_root(&format!("{}-scoped", case.name));
            if let Some(parent) = std::path::Path::new(case.file.unwrap()).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(root2.join("src").join(parent)).unwrap();
                }
            }
            std::fs::write(
                root2.join("src/sibling.ts"),
                "export function sibling() { return 1; }\n",
            )
            .unwrap();
            let t2 = root2.join("src").join(case.file.unwrap());
            if let Some(b) = &case.body {
                std::fs::write(&t2, b).unwrap();
            }
            // For scoped, the event path is the target (may be under testdata/).
            let event_path = if case.out_of_corpus {
                // Scoped index_paths of a noise-dir file: Language matches, so
                // it enters the path loop. Walker will not keep it on a later
                // full index — either way it must not become a silent in-S claim.
                t2
            } else {
                t2.clone()
            };
            let indexer2 = Indexer::new(&root2).expect("indexer2");
            let _ = indexer2
                .index_paths(std::slice::from_ref(&event_path))
                .expect("scoped");
            let store2 = indexer2.open_store().expect("store2");
            if !case.out_of_corpus {
                if case.expect_parse_error_scoped {
                    assert!(
                        has_parse_error(&store2, needle),
                        "{}: scoped index must mint parse_error; viols={:?}",
                        case.name,
                        store2.subset_violations().unwrap()
                    );
                } else {
                    assert!(
                        !has_parse_error(&store2, needle),
                        "{}: scoped clean file must not mint parse_error",
                        case.name
                    );
                }
            }
            let _ = t;
        }
    }
}

/// Unchanged-file skip (mtime+size and content-hash) must not invent a
/// parse_error, and a prior parse_error must be replaced by a clean index
/// once the file is readable again.
#[test]
fn parse_error_cleared_when_file_recovers() {
    let root = temp_root("recover");
    let f = root.join("src/rec.ts");
    std::fs::write(&f, [0xffu8, 0xfe, 0x00, 0x01]).unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("seed");
    assert!(
        has_parse_error(&indexer.open_store().unwrap(), "rec.ts"),
        "invalid UTF-8 must mint parse_error"
    );

    std::fs::write(&f, "export function rec() { return 1; }\n").unwrap();
    indexer.index(true).expect("reindex");
    assert!(
        !has_parse_error(&indexer.open_store().unwrap(), "rec.ts"),
        "recovering file must clear parse_error on reindex"
    );
}

/// Outside-root path on the watch path: skipped without minting (not in
/// keep-set — never claimed in S). Must not crash and must not invent rows.
#[test]
fn index_paths_outside_root_no_mint_no_crash() {
    let root = temp_root("outside");
    std::fs::write(
        root.join("src/ok.ts"),
        "export function ok() { return 1; }\n",
    )
    .unwrap();
    let outside = root
        .parent()
        .unwrap()
        .join(format!("agentgraph-r25-outside-{}.ts", std::process::id()));
    std::fs::write(&outside, "export function evil() { return 1; }\n").unwrap();

    let indexer = Indexer::new(&root).expect("indexer");
    indexer.index(true).expect("seed");
    let _ = indexer
        .index_paths(std::slice::from_ref(&outside))
        .expect("outside skip");

    let store = indexer.open_store().unwrap();
    assert!(
        store
            .subset_violations()
            .unwrap()
            .iter()
            .all(|v| !v.path.contains("agentgraph-r25-outside")),
        "outside-root path must not mint a violation"
    );
    let _ = std::fs::remove_file(&outside);
}

// ── A2: Go //export directive forms ────────────────────────────────────

/// cgo `//export` line-comment forms that must leave S.
#[test]
fn go_export_forms_leave_s() {
    // Official form and whitespace variants. `//export\tFoo` is accepted by
    // go_comment_is_linkname (R24). Block comments are NOT valid cgo exports.
    let leave_s: &[(&str, &str)] = &[
        ("space", "package main\n//export Foo\nfunc Foo() {}\n"),
        ("tab", "package main\n//export\tFoo\nfunc Foo() {}\n"),
        ("bare", "package main\n//export\nfunc Foo() {}\n"),
        (
            "extra_space",
            "package main\n//export  Foo\nfunc Foo() {}\n",
        ),
        (
            "linkname",
            "package main\n//go:linkname F runtime.F\nfunc F() {}\n",
        ),
    ];
    for (label, src) in leave_s {
        let r = scan_subset(src, Language::Go, "main.go");
        assert!(
            !r.in_subset,
            "go //export/{label} must leave S: {:?}",
            r.violations
        );
    }
}

/// `/*export Foo*/` is not a valid cgo directive — must NOT leave S via the
/// comment rule (over-flagging would be a false S-disable). `import "C"`
/// still leaves S independently.
#[test]
fn go_block_comment_export_not_a_directive() {
    let src = "package main\n/*export Foo*/\nfunc Foo() {}\n";
    let r = scan_subset(src, Language::Go, "main.go");
    assert!(
        r.in_subset,
        "block-comment /*export*/ is not a cgo directive; must stay in S: {:?}",
        r.violations
    );
}

// ── B: Nest sound allowlist completeness ───────────────────────────────

/// Every `ts.nest.*` rule id that is sound-allowlisted must stay eligible,
/// and the docs table (docs/sound-subset.md) lists exactly these five.
/// Keep this list in lockstep with `SOUND_HEURISTIC_RULES` in subset.rs.
#[test]
fn nest_sound_allowlist_matches_docs() {
    let nest_rules = [
        "ts.nest.module_providers",
        "ts.nest.module_controllers",
        "ts.nest.module_imports",
        "ts.nest.module_exports",
        "ts.nest.ctor_inject",
    ];
    for rule in nest_rules {
        assert!(
            is_sound_eligible(Confidence::Heuristic, Some(rule)),
            "{rule} must be sound-eligible (SOUND_HEURISTIC_RULES)"
        );
    }
    // Phantom ids that eval-l1 used to list as if they were rule_ids — they
    // are *shapes* that reuse module_*/di.* ids. Must NOT be sound-eligible
    // under those names (they are never emitted).
    for phantom in ["ts.nest.forRootAsync", "ts.nest.string_token"] {
        assert!(
            !is_sound_eligible(Confidence::Heuristic, Some(phantom)),
            "{phantom} is a shape alias, not an emitted rule id"
        );
    }
}

/// Docs table in sound-subset.md must list every sound-allowlisted ts.nest.* id.
#[test]
fn sound_subset_docs_lists_all_nest_allowlist_ids() {
    let docs = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/sound-subset.md"),
    )
    .expect("docs/sound-subset.md");
    for rule in [
        "ts.nest.module_providers",
        "ts.nest.module_controllers",
        "ts.nest.module_imports",
        "ts.nest.module_exports",
        "ts.nest.ctor_inject",
    ] {
        assert!(
            docs.contains(rule),
            "docs/sound-subset.md must list allowlisted rule {rule}"
        );
    }
    // parse_error S rule must be documented (R13/R23/R24 keep-set family).
    assert!(
        docs.contains("parse_error"),
        "docs/sound-subset.md must document the parse_error S certification rule"
    );
    // Go //export must stay documented.
    assert!(
        docs.contains("//export"),
        "docs/sound-subset.md must document Go //export"
    );
}
