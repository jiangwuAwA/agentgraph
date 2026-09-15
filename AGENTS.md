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

## Capability roadmap

Phased analysis plan (L0 index → L1 dynamic candidates → L2 sound subset → L3 formal track) lives in [PLAN.md](PLAN.md). Do not market L1/L2/L3 as complete until their acceptance criteria in PLAN.md are met.

**L1 (shipped):** heuristic/dynamic-candidate edges with `confidence` + `evidence`; CLI/MCP `--exact-only` / `--include-dynamic`; eval corpus in `fixtures/eval-l1` + [docs/eval-l1.md](docs/eval-l1.md). L1 is **not** sound — candidates only.

**L2 (experimental):** `impact --sound` / `callers --sound` / `agentgraph subset`; S-violation scan; Node differential (single + multi-file ESM); property tests (`tests/l2_property.rs`). S_js is a conservative AST scanner; S_py/S_go/S_rs are **v1 conservative lexical scanners** (not full freezes). Soundness claim **only** when `subset_ok` (see [docs/sound-subset.md](docs/sound-subset.md) and [docs/eval-l2.md](docs/eval-l2.md)). Not production-complete.

**L3 (research, non-blocking):** `formal/IncrementalIndex.tla` TLC-checked (no errors); I1–I3 in `tests/l3_invariants.rs`; I4 mini-language containment in `src/formal/mini_lang.rs` + `tests/l4_mini_lang.rs`. TLC optional locally (Java + tla2tools.jar); main CI does not require theorem provers.

## Commits

- Prefer small, test-backed commits.
- Message focuses on *why*, not only *what*.
