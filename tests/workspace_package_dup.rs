//! Duplicate package.json name must fail-loud or require CLI override.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn temp_dir(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "ag-dup-pkg-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn write_tree(base: &Path) -> PathBuf {
    let r1 = base.join("packages/a");
    let r2 = base.join("packages/b");
    std::fs::create_dir_all(r1.join("src")).unwrap();
    std::fs::create_dir_all(r2.join("src")).unwrap();
    std::fs::write(
        r1.join("package.json"),
        r#"{"name":"@demo/registry","version":"0.0.1"}"#,
    )
    .unwrap();
    std::fs::write(
        r2.join("package.json"),
        r#"{"name":"@demo/registry","version":"0.0.1"}"#,
    )
    .unwrap();
    std::fs::write(r1.join("src/index.ts"), "export class A {}\n").unwrap();
    std::fs::write(r2.join("src/index.ts"), "export class B {}\n").unwrap();
    let ws = base.join("workspace.json");
    let body = String::from(
        "{\"roots\":[{\"id\":\"aa\",\"path\":\"packages/a\"},{\"id\":\"bb\",\"path\":\"packages/b\"}]}",
    );
    std::fs::write(&ws, body).unwrap();
    ws
}

#[test]
fn duplicate_package_name_fails_loud_without_override() {
    let base = temp_dir("fail");
    let ws = write_tree(&base);
    let out = run(&["index", "--workspace", ws.to_str().unwrap(), "--force"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.status.success(),
        "duplicate package.json name must fail-loud (no silent first-root): {text}"
    );
    assert!(
        text.contains("@demo/registry")
            && (text.contains("duplicate") || text.contains("ambiguous")),
        "error must name the package and say duplicate/ambiguous: {text}"
    );
}

#[test]
fn duplicate_package_name_cli_override_indexes() {
    let base = temp_dir("ok");
    let ws = write_tree(&base);
    let out = run(&[
        "index",
        "--workspace",
        ws.to_str().unwrap(),
        "--workspace-alias",
        "@demo/registry=aa",
        "--force",
    ]);
    assert!(
        out.status.success(),
        "CLI override must allow index: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
