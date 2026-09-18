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
python scripts/check_docs_claims.py   # M5 docs claims gate (also via tests/docs_claims.rs)
```

CI runs the same three (plus multi-OS build/test) and the M5 docs claims check.

## Release checklist (M5 credibility gate)

Before tagging a release, confirm:

- [ ] `python scripts/check_docs_claims.py` green (and `cargo test --test docs_claims`)
- [ ] README / README.zh-CN / AGENTS capability sentences ⊆ implementation + `docs/eval-*.md`
- [ ] Every CLI flag/command mentioned in docs exists in clap (`src/cli.rs`) — checker verifies this
- [ ] Eval numbers live in `docs/eval-*.md`; README only cites them (no new slogans)
- [ ] Public goldens under `fixtures/eval-goldens/` (no private stock source / expand artifacts)
- [ ] Private eval paths are skipped in CI, not required
- [ ] No banned oversell: 零漏报 / 生态 sound / 宏完整 / production sound / complete runtime graph as a **product** guarantee
- [ ] Honesty lines that explicitly deny those claims are preserved (checker must stay green on them)
- [ ] `cargo fmt` + `clippy -D warnings` + `cargo test` + CLI E2E (+ `scip lint` where available)

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

**L1 (shipped):** heuristic/dynamic-candidate edges with `confidence` + `evidence`; CLI/MCP `--exact-only` / `--include-dynamic`; eval corpus in `fixtures/eval-l1` + [docs/eval-l1.md](docs/eval-l1.md). Includes Nest `ts.nest.*` (module providers/controllers/imports/exports, forRootAsync, ctor_inject, string tokens); Rust `rs.di.impl_trait` / `rs.di.inventory_submit` / `rs.di.linkme_distributed_slice` (`#[distributed_slice]` source rule, M3-E; eval fixture `fixtures/eval-l1/rust-linkme/`) / `rs.di.dyn_trait_method` (dyn Trait method → same-file implementors only — **not sound-eligible**); Go `go.di.interface_impl_v2` (assertion + method-set name match); Python `py.di.entry_points` + Security-as-Depends; TS `ts.framework.register` (Express/Fastify router/use/register). Multi-idiom real-shaped corpora: `fixtures/eval-l1-real/` (dyn-trait, go iface v2, express router, entry_points, linkme). Public M3/M2 goldens: `fixtures/eval-goldens/`. Real-tree goldens + S map on private stock-trading-app: [docs/eval-stock-boundary.md](docs/eval-stock-boundary.md). L1 is **not** sound — candidates only. **Noise governance (shipped):** query-time `edge_role` (`call`|`implementor`|`registration`|`dynamic`) from `rule_id`+confidence; default `callers` separates implementors into `{callers, implementors, implementor_count, implementors_truncated}` when any implementor is present (plain array when zero); `--include-implementors` merges; `--implementors-only` isolates; `--exact-only` stays pure Exact; `HIGH_FREQ_NAMES` (`fmt`/`drop`/`clone`/…) cap implementors at 20. Store keeps all edges; `impact` still expands implementors (tagged). See [docs/noise-governance.md](docs/noise-governance.md).

**L2 (S-qualified):** `impact --sound` / `callers --sound` / `agentgraph subset`; S-violation scan (tree-sitter AST for js/ts/tsx/jsx/python/go/rust); emit↔on dispatch closure; Nest `ts.nest.*` registration allowlist (registration ≠ HTTP ServeHTTP) and Rust `rs.di.impl_trait` / `rs.di.inventory_submit` (crate-relative inventory paths + `use inventory::submit` / `use inventory::{submit as s}` / `use inventory::*` aliases; **foreign use-lists like `use evil::{inventory::submit}` do not unlock bare `submit!`**); Node/ESM/Go/Python differential + property tests (S_js/S_py/S_go; Python tracer `scripts/py_trace.py` + `tests/l2_py_diff.rs`). Claim **only** when `subset_ok` for **modeled** edges inside S ([docs/sound-subset.md](docs/sound-subset.md), [docs/eval-l2.md](docs/eval-l2.md)). Promise is **language-aware**: all shipped languages select **ast_modeled** (engineering S gate, not ecosystem sound); LexicalV1 tier is reserved (no shipped language currently selects it). Type-only `typeof Function` / interface `Function` type positions **stay in S** (M2 over-flag fix); value-use of `Function`/`eval` still leaves S. Uncertified sources (oversized / `.min.` / non-UTF-8 / read or extract failure) mint `parse_error` and stay in the keep-set on both full index and watch — noise dirs and unsupported extensions are out of corpus (not claimed). Query p95 SLO: [docs/eval-query-p95.md](docs/eval-query-p95.md). Still not a full ecosystem theorem.

**L3 (research, non-blocking):** `formal/IncrementalIndex.tla` TLC-checked (no errors); I1–I3 in `tests/l3_invariants.rs`; I4 mini-language containment in `src/formal/mini_lang.rs` + `tests/l4_mini_lang.rs`; Lean 4 theorem `runtime_subset_static` in [`formal/lean/`](formal/lean/README.md) (`lake build` green, stdlib only). TLC/Lean optional locally; main CI does not require theorem provers.

**P2/M1 macro sidecar (optional, default OFF; not sound):** `index --macro-expanded-root` writes `<root>/.agentgraph/index.macro.db` from an **already-produced** expanded shadow tree (never auto `cargo expand`). `callers`/`impact`/`graph --with-macro` union sidecar hits tagged `origin=macro_expanded` (+ `at=path:line`, `mapped`/`mapped_path` when path map applies). **M1 path map:** expanded paths map back to source (explicit `meta.path_map` pairs + crate-root heuristics; unmappable rows keep `mapped=false`). **De-dup ON by default** on `name+enclosing+mapped_path` (main Exact/Heuristic wins; `--no-macro-dedup` debug); `--exact-only --with-macro` **ignores** the sidecar. Sidecar build writes `meta.source_fingerprint`; `macro status` exposes `stale` / `path_map_present` / `dedup_stats` / `rebuild_policy=manual`; stale + `--with-macro` warns and still unions (`stale:true` in wrapped payload). `macro rebuild` / MCP `macro_rebuild` re-index the recorded `expanded_root` (idempotent; fails if missing/nested). Nested expanded roots remain **rejected before main reindex**; relative expanded roots resolve against `--root` (not cwd). `limit` is per store (~2N without de-dup). Absent sidecar: default queries unchanged (plain arrays); `--with-macro` graceful empty. `--sound && --with-macro` stay mutually exclusive — expanded edges are never sound-certified. See [docs/macro-sidecar.md](docs/macro-sidecar.md).

**HTML graph viz (shipped):** `agentgraph graph <symbol>` writes self-contained offline HTML (default `<root>/.agentgraph/graph.html`); impact BFS primary view; colors by confidence; honesty line “L0/L1 candidates, not a complete runtime graph”; empty neighborhood still writes a page (exit 0 + note). **`graph --sound` (M4):** only sound-eligible modeled edges; header shows `subset_ok` + `promise_tier`; when `subset_ok=false` HTML is still written but marked disabled / **not** a sound graph (exit non-zero); mutually exclusive with `--with-macro` and with `--exact-only`/`--include-dynamic`. See [docs/graph-html.md](docs/graph-html.md).

**Indexed-edge diff (M4 shipped):** `agentgraph diff [--exact-only] [--limit N] [--snapshot PATH] [--write-snapshot]` compares the **indexed ref edge set** (name+path+line+confidence+enclosing) against a dual sidecar snapshot written at full `index` time (`refs.snapshot.json` + `.prev.json`, `meta.index_seq`). Watch/`index_paths` do **not** refresh the baseline. No snapshot → fail-loud (run `index` first). Honesty: **indexed edges only; not a runtime call-graph diff**. MCP tool: `graph_diff`. Workspace multi-root is **backlog** (prefer single store + `root_id` when done). See [docs/graph-diff.md](docs/graph-diff.md).

**S re-cert after watch (M4):** dirty-file `index_paths` re-scans subset violations for those paths (`scan_subset` + `Store::refresh_subset_for_paths`); recovered files clear stale rows; deleted violating files are pruned from `subset_violations`. `impact/callers --sound`, `subset`, and `graph --sound` reflect **current disk**. Regression: `tests/s_recert_watch.rs`. See [docs/sound-subset.md](docs/sound-subset.md).

## Commits

- Prefer small, test-backed commits.
- Message focuses on *why*, not only *what*.
