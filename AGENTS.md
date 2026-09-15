# AGENTS.md — agentgraph

## Development process (mandatory)

**All new code work follows TDD** unless the user explicitly asks otherwise in the same request.

1. **Write a failing test first** that captures the intended behavior (unit, integration, or CLI E2E).
2. **Run the test** and confirm it fails for the expected reason.
3. **Implement the minimum code** to make the test pass.
4. **Refactor** only after green (`cargo test` + `cargo clippy --all-targets -- -D warnings` + `cargo fmt`).
5. **Do not** land feature code without a corresponding test, except pure docs/CI config.

Explicit user override examples that waive TDD for a change: "skip tests", "just prototype", "don't write tests for X".

## Quality gates (must stay green locally before push)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same three (plus multi-OS build/test).

## Layout

- Library: `src/lib.rs` + `src/index/*`, `src/mcp/*`, `src/query/*`
- CLI binary: `src/main.rs` + `src/cli.rs`
- Unit/integration: `tests/*.rs`
- CLI end-to-end: `tests/e2e_cli.rs` (uses `CARGO_BIN_EXE_agentgraph`)
- Local one-shot: `scripts/e2e.ps1`
- Fixture sample app: `fixtures/sample-app/`

## SCIP export

`export scip` writes **protobuf binary** readable by the official `scip` CLI.
Descriptors follow official grammar (`Type#`, `fn.`, `method().`).
Validate with: `scip lint index.scip` (exit 0 required).

## Commits

- Prefer small, test-backed commits.
- Message focuses on *why*, not only *what*.
