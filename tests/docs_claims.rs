//! Track M5 credibility gate — docs claims checker wired into `cargo test`.
//!
//! Spec: `docs/product-boundary-migration.md` § Track M5.
//!
//! Behavior under test (TDD):
//! 1. Fixture README with oversell phrase → checker red.
//! 2. README mentions `--with-macro` but flag absent from a **stub** list → red
//!    (logic tested with strings / stub known-flags — product `src/cli.rs` is untouched).
//! 3. Honesty negations ("not a complete runtime graph", 禁止「零漏报」…) → green.
//! 4. Main product docs (README / AGENTS / PLAN / key docs) → green.
//! 5. Public synthetic goldens under `fixtures/eval-goldens/` exist + parse.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn checker_script() -> PathBuf {
    repo_root().join("scripts").join("check_docs_claims.py")
}

fn python_command() -> Command {
    let mut last_err = String::from("python not found");
    for cand in ["python", "python3", "py"] {
        let mut cmd = Command::new(cand);
        cmd.arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match cmd.status() {
            Ok(st) if st.success() => {
                let mut run = Command::new(cand);
                // Keep checker messages UTF-8 even on Windows consoles.
                run.env("PYTHONIOENCODING", "utf-8");
                run.env("PYTHONUTF8", "1");
                return run;
            }
            Ok(st) => last_err = format!("{cand} exited {st}"),
            Err(e) => last_err = format!("{cand}: {e}"),
        }
    }
    panic!("no python interpreter available for docs claims check: {last_err}");
}

struct CheckResult {
    code: i32,
    stdout: String,
    stderr: String,
}

impl CheckResult {
    fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

fn run_checker(args: &[&str]) -> CheckResult {
    let script = checker_script();
    assert!(
        script.is_file(),
        "missing checker script at {}",
        script.display()
    );
    let mut cmd = python_command();
    let output = cmd
        .arg(&script)
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("failed to spawn docs claims checker");
    CheckResult {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn write_temp_doc(name: &str, contents: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agentgraph-m5-docs-{}-{}",
        std::process::id(),
        name.replace(['/', '\\', ' '], "_")
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("doc.md");
    std::fs::write(&path, contents).expect("write temp doc");
    path
}

/// Main product docs must stay green under the M5 claim checker.
#[test]
fn main_product_docs_are_green() {
    let res = run_checker(&[]);
    assert_eq!(
        res.code, 0,
        "main docs claims check must pass.\nstdout:\n{}\nstderr:\n{}",
        res.stdout, res.stderr
    );
}

/// TDD red: oversell phrases in a fixture README trip the checker.
#[test]
fn oversell_phrases_are_red() {
    let cases: &[(&str, &str)] = &[
        (
            "zero_miss_en",
            "# p\nWe guarantee zero-miss coverage for all dynamic edges.\n",
        ),
        ("zero_miss_zh", "# p\n本产品保证零漏报。\n"),
        ("ecosystem_sound", "# p\nThis product is ecosystem sound.\n"),
        (
            "macro_complete",
            "# p\nMacro-complete call graphs on every crate.\n",
        ),
        (
            "production_sound",
            "# p\nShip with production sound analysis.\n",
        ),
        (
            "complete_runtime_graph",
            "# p\nProvides a complete runtime graph of your codebase.\n",
        ),
    ];
    for (name, body) in cases {
        let path = write_temp_doc(name, body);
        let doc = path.display().to_string();
        let res = run_checker(&["--doc", &doc, "--no-flag-check"]);
        assert_ne!(
            res.code,
            0,
            "oversell fixture `{name}` must be red; got green.\n{}",
            res.combined()
        );
        assert!(
            res.combined().contains("banned oversell") || res.combined().contains("FAILED"),
            "fixture `{name}` should report banned oversell; got:\n{}",
            res.combined()
        );
    }
}

/// Honesty / negation lines must NOT trip the checker.
#[test]
fn honesty_negations_are_green() {
    let path = write_temp_doc(
        "honesty",
        r#"# honesty
Shows indexed L0/L1 candidates, **not** a complete runtime graph.
honesty line “L0/L1 candidates, not a complete runtime graph”
非生态 sound。不得出现「零漏报」。禁止写「宏完整」。
Not production sound. engineering S gate — **not** ecosystem sound.
Do **not** market L1 as sound or “zero missed dynamic calls”.
**Status:** S-qualified (not a blanket production label). Soundness not claimed.
### Will not claim
- Macro-complete call graphs are not promised.
- **Arbitrary proc-macro completeness:** **non-goal.**
### Non-goals
- 宏完整 is not a product guarantee.
- 不证明 JS/Py/Go 全生态 sound。
- 声称「宏完整」「动态零漏报」 — **禁止**
"#,
    );
    let doc = path.display().to_string();
    let res = run_checker(&["--doc", &doc, "--no-flag-check"]);
    assert_eq!(
        res.code,
        0,
        "honesty/negation lines must stay green.\n{}",
        res.combined()
    );
}

/// TDD red: README mentions `--with-macro` but the flag is missing from a stub list.
/// Product clap source is NOT modified — checker logic is tested with `--known-flags`.
#[test]
fn readme_flag_missing_from_stub_list_is_red() {
    let path = write_temp_doc(
        "flag_missing",
        r#"# flags

```bash
agentgraph callers x --with-macro
```

Optional sidecar via `--with-macro` and `macro status`.
"#,
    );
    let doc = path.display().to_string();
    // Stub known-flags deliberately omit `with-macro`.
    let res = run_checker(&[
        "--doc",
        &doc,
        "--known-flags",
        "sound,exact-only,include-dynamic,recall",
        "--known-commands",
        "callers,impact,macro,macro status",
    ]);
    assert_ne!(
        res.code,
        0,
        "README flag not in stub list must be red; got green.\n{}",
        res.combined()
    );
    assert!(
        res.combined().contains("with-macro"),
        "violation must name the missing flag; got:\n{}",
        res.combined()
    );
}

/// Same README is green when the stub list includes the mentioned flags/commands.
#[test]
fn readme_flag_present_in_stub_list_is_green() {
    let path = write_temp_doc(
        "flag_present",
        r#"# flags

```bash
agentgraph callers x --with-macro --sound
agentgraph graph foo
agentgraph macro status
```
"#,
    );
    let doc = path.display().to_string();
    let res = run_checker(&[
        "--doc",
        &doc,
        "--known-flags",
        "with-macro,sound,exact-only",
        "--known-commands",
        "callers,graph,macro,macro status",
    ]);
    assert_eq!(
        res.code,
        0,
        "flags present in stub list must be green.\n{}",
        res.combined()
    );
}

/// CLI flags mentioned in README must exist in clap (`src/cli.rs`).
#[test]
fn readme_flags_exist_in_clap_cli() {
    let cli = repo_root().join("src").join("cli.rs");
    assert!(cli.is_file(), "src/cli.rs must exist for flag verification");
    let res = run_checker(&[
        "--doc",
        "README.md",
        "--flag-doc",
        "README.md",
        "--flag-doc",
        "README.zh-CN.md",
        "--cli",
        "src/cli.rs",
    ]);
    assert_eq!(
        res.code,
        0,
        "README CLI mentions must match clap.\n{}",
        res.combined()
    );
}

/// Public synthetic goldens (M5.4) — structure + JSON shape, no private corpus.
#[test]
fn eval_goldens_fixtures_exist_and_parse() {
    let root = repo_root().join("fixtures").join("eval-goldens");
    assert!(root.is_dir(), "missing fixtures/eval-goldens/");
    assert!(
        root.join("README.md").is_file(),
        "eval-goldens needs a README with non-claims"
    );
    assert!(
        root.join("ts-nest-mini")
            .join("src")
            .join("app.module.ts")
            .is_file(),
        "missing ts-nest-mini app.module.ts"
    );
    assert!(
        root.join("ts-nest-mini")
            .join("src")
            .join("app.controller.ts")
            .is_file(),
        "missing ts-nest-mini app.controller.ts"
    );
    assert!(
        root.join("ts-nest-mini")
            .join("src")
            .join("app.service.ts")
            .is_file(),
        "missing ts-nest-mini app.service.ts"
    );
    assert!(
        root.join("rust-inventory-mini")
            .join("src")
            .join("lib.rs")
            .is_file(),
        "missing rust-inventory-mini lib.rs"
    );

    let golden_path = root.join("golden.json");
    let raw = std::fs::read_to_string(&golden_path).expect("read golden.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("golden.json must be valid JSON");
    let corpora = v
        .get("corpora")
        .and_then(|c| c.as_object())
        .expect("golden.json corpora object");
    assert!(
        corpora.contains_key("ts-nest-mini"),
        "golden.json must include ts-nest-mini"
    );
    assert!(
        corpora.contains_key("rust-inventory-mini"),
        "golden.json must include rust-inventory-mini"
    );
    for (name, corpus) in corpora {
        let edges = corpus
            .get("edges")
            .and_then(|e| e.as_array())
            .unwrap_or_else(|| panic!("corpus {name} missing edges[]"));
        assert!(!edges.is_empty(), "corpus {name} edges must be non-empty");
        for edge in edges {
            assert!(
                edge.get("symbol").and_then(|s| s.as_str()).is_some(),
                "{name}: edge missing symbol"
            );
            assert!(
                edge.get("site_file").and_then(|s| s.as_str()).is_some(),
                "{name}: edge missing site_file"
            );
            assert!(
                edge.get("site_contains").and_then(|s| s.as_str()).is_some(),
                "{name}: edge missing site_contains"
            );
        }
    }

    // Non-claim marker required in goldens README (credibility gate hygiene).
    let readme = std::fs::read_to_string(root.join("README.md")).expect("read goldens README");
    let lower = readme.to_lowercase();
    assert!(
        lower.contains("non-claim") || lower.contains("not sound") || lower.contains("候选"),
        "eval-goldens README must state non-claims"
    );
}

/// Goldens sources must be public synthetic shapes — no private stock paths.
#[test]
fn eval_goldens_have_no_private_stock_paths() {
    let root = repo_root().join("fixtures").join("eval-goldens");
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for banned in [
                "stock-trading-app",
                "eval-corpus",
                "D:\\projects\\eval-corpus",
                "model-selection-replay",
                "model-selection-installer",
            ] {
                assert!(
                    !text.contains(banned),
                    "{} must not reference private corpus token `{banned}`",
                    path.display()
                );
            }
        }
    }
}

/// Helper used by CI-oriented assertions: checker accepts `--print-known`.
#[test]
fn checker_print_known_clap_flags() {
    let res = run_checker(&["--print-known"]);
    assert_eq!(res.code, 0, "print-known failed:\n{}", res.combined());
    let out = res.combined();
    for flag in [
        "with-macro",
        "sound",
        "exact-only",
        "include-dynamic",
        "recall",
    ] {
        assert!(
            out.contains(flag),
            "clap parse must expose --{flag}; got:\n{out}"
        );
    }
    for cmd in ["callers", "impact", "graph", "macro status", "subset"] {
        assert!(
            out.contains(cmd),
            "clap parse must expose command `{cmd}`; got:\n{out}"
        );
    }
}

#[test]
fn docs_claims_checker_script_is_present() {
    let p: &Path = &checker_script();
    assert!(p.is_file());
    let src = std::fs::read_to_string(p).expect("read checker");
    assert!(
        src.contains("BANNED_PHRASES"),
        "checker must define banned phrases"
    );
    assert!(
        src.contains("parse_clap_cli"),
        "checker must parse clap CLI"
    );
}
