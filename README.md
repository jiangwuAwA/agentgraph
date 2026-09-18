# agentgraph

[![CI](https://github.com/jiangwuAwA/agentgraph/actions/workflows/ci.yml/badge.svg)](https://github.com/jiangwuAwA/agentgraph/actions/workflows/ci.yml)

English | [简体中文](README.zh-CN.md)

Agent-native code understanding: **symbol graph, call graph, impact analysis** — CLI + MCP server.

Not another embedding RAG. When an agent needs *who calls this*, *what breaks if I change this*, or *which files matter*, it needs **structural facts**, not similar text chunks.

### Positioning (one-liner narrative)

| Approach | vs agentgraph |
|---|---|
| Chunk + embed RAG | Loses call / import structure — similar text is not a call graph |
| CodeQL / enterprise platforms | Heavier to install and host; agentgraph is **CLI + MCP first** for agent hosts |
| Bare LSP | Hover / goto-oriented — not multi-hop blast-radius or agent recipes |
| Raw SCIP export | Interop format only — agentgraph **adds** L1/L2 honesty fields (`window` / `subset_ok` / `note`) + recipes + watch |

**Honesty:** this is a positioning table for agent tooling, **not** a soundness ranking. agentgraph reports indexed L0/L1 candidates (S-qualified edges only when `subset_ok`) — **not** a complete runtime graph, **not** ecosystem sound, **not** production sound. Public structure-fact scores: [docs/eval-agent-tasks.md](docs/eval-agent-tasks.md). 5-minute cold start: [docs/onboarding.md](docs/onboarding.md).

**Pipeline:** tree-sitter parse → SQLite symbol/reference store → query API for agents.

## Agent path

What an Agent host should call, in order. Deep CLI / MCP tables stay below — this is the one-pager.

```bash
agentgraph index                    # once per repo (MCP: index)
agentgraph blast-radius <sym>       # MCP: blast_radius
agentgraph who-calls <sym>          # MCP: who_calls
agentgraph graph <sym> --depth 2    # MCP: graph → offline HTML + honesty fields
```

| Step | Call | What you get |
|---|---|---|
| 1. Index | `agentgraph index` / MCP `index` | SQLite store at `<root>/.agentgraph/index.db`; query before index fails loud |
| 2. Blast radius | MCP `blast_radius` / CLI `blast-radius` | Auto `window=sound\|default` + `recommendation` / honesty `note` |
| 3. Who calls | MCP `who_calls` / CLI `who-calls` | Implementors separated by default (`noisy=false`); high-freq names demoted |
| 4. Graph | MCP `graph` / CLI `graph` | Self-contained HTML + `window` / `subset_ok` / `promise_tier` / `note` |

### Honesty (window / subset_ok / note)

| Field | When sound applies | Non-claim |
|---|---|---|
| **window=sound** | Only when `subset_ok` (S-qualified **modeled** edges); else `window=default` (Exact+Heuristic) | Never blind `--recall` labeled as sound; **not** a complete runtime graph |
| **subset_ok** | Per-root / per-path S gate for modeled edges | **Not** ecosystem sound; **not** production sound |
| **note** | Always present on recipe / graph payloads | `note` = **not a complete runtime graph** |

**Agents:** read `window`, `subset_ok`, and `recommendation` before acting. Scoped sound may be available on a clean workspace root even when the global window is off — see [docs/agent-recipes.md](docs/agent-recipes.md).

**More:** [docs/onboarding.md](docs/onboarding.md) (5-min install → MCP `blast_radius`) · [docs/agent-recipes.md](docs/agent-recipes.md) · [docs/agent-playbook/index.html](docs/agent-playbook/index.html)

**Advanced** (links, not an inline dump):

- Onboarding kit / MCP host snippets — [docs/onboarding.md](docs/onboarding.md) · [examples/mcp-claude.json](examples/mcp-claude.json)
- Flags, windows, recipes — [Queries](#queries) below · [docs/agent-recipes.md](docs/agent-recipes.md)
- Workspace multi-root — [docs/workspace.md](docs/workspace.md)
- Macro sidecar (default OFF; optional repo `macro_default` in `.agentgraph/config.toml`) — [docs/macro-sidecar.md](docs/macro-sidecar.md)
- Sound subset / S gate — [docs/sound-subset.md](docs/sound-subset.md)
- Noise / implementors — [docs/noise-governance.md](docs/noise-governance.md)
- Public Agent code-change task evals — [docs/eval-agent-tasks.md](docs/eval-agent-tasks.md)
- Scripted tool-policy A/B/C + replay (P0-5; **not** live LLM) + P0-5b live host-session A/B (not a public benchmark lab) — [docs/eval-agent-baseline.md](docs/eval-agent-baseline.md)
- CI blast-radius comment **demo** (non-required) — [docs/ci-blast-radius-demo.md](docs/ci-blast-radius-demo.md)

## Quick start

```bash
cargo install --path .
cd /path/to/your/repo

agentgraph index
agentgraph find createUser
agentgraph callers validateEmail
agentgraph impact validateEmail --depth 3
agentgraph graph validateEmail --depth 2   # offline HTML graph (default .agentgraph/graph.html)
agentgraph importers src/auth.ts
agentgraph export scip --out index.scip   # official scip CLI can read this
```

Index lives at `<root>/.agentgraph/index.db` (add to `.gitignore`).

### HTML graph (visual, offline)

```bash
agentgraph graph helper --depth 2 --out graph.html
agentgraph graph helper --sound --out graph-sound.html   # S-qualified edges + subset_ok header
# open graph.html in a browser — no network
```

Impact-style BFS neighborhood with confidence colors (Exact / Heuristic / DynamicCandidate),
bilingual UI, click-to-inspect nodes. **Shows indexed L0/L1 candidates, not a complete
runtime graph.** Empty neighborhood still writes a page. `--sound` shows only
sound-eligible modeled edges when `subset_ok`; violated S still writes HTML but marks
the page disabled (exit non-zero) — not labeled a sound graph.
Details: [docs/graph-html.md](docs/graph-html.md).

### Indexed-edge diff

```bash
agentgraph index
# edit call sites
agentgraph index
agentgraph diff                 # added/removed indexed edges vs snapshot baseline
agentgraph diff --exact-only --limit 20
```

Snapshot written at full `index` time (`<root>/.agentgraph/refs.snapshot.json` + previous generation). No baseline → fail-loud. **Honesty:** indexed edges only; not a runtime call-graph diff. Details: [docs/graph-diff.md](docs/graph-diff.md).

Incremental index skips rehash when **mtime+size match**. On network/FAT volumes
or tools that preserve mtime across content edits, set `AGENTGRAPH_TRUST_MTIME=0`
to force content-hash every file (hash remains the source of truth).

## Features

### Languages

TypeScript, TSX, JavaScript, JSX, Python, Go, Rust.

### Queries

| Command | Purpose |
|---|---|
| `find` | Symbol definitions (exact; `--fuzzy` for LIKE). Workspace rows carry `root_id` + `root_path` |
| `callers` | Call/import sites + L1 candidates (`module`, `resolved`, `qualifier`, `confidence`, **`edge_role`**, `root_id`). Default separates implementors into `{callers, implementors, …}` when present; `--include-implementors` / `--implementors-only` / high-freq demote — [docs/noise-governance.md](docs/noise-governance.md) |
| `impact` | True BFS blast radius (default Exact+Heuristic) |
| `graph` | Local self-contained HTML code-graph (impact BFS + optional callers view); `--sound` renders S-qualified edges with `subset_ok` / `promise_tier` header; `--workspace-root` scopes + `root_id` badge — see [docs/graph-html.md](docs/graph-html.md) |
| `diff` | Indexed-edge set difference vs snapshot baseline written at `index` time (not a runtime call-graph diff) — see [docs/graph-diff.md](docs/graph-diff.md) |
| `related` | Definition + importers + references (scope retrieval) |
| `importers` | Who imports a given file |
| `workspace status` | Multi-root workspace index health (per-root counts, exact/heur, violations, `promise_tier`, `index_seq`, `missing`) — one-liner: `agentgraph index --workspace-root api --workspace-root web --workspace-db ./ws.db` then `workspace status` — [docs/workspace.md](docs/workspace.md) |
| `blast-radius` | High-level blast-radius recipe: auto `window=sound\|default` (sound only when `subset_ok`; default Exact+Heuristic otherwise — never blind `--recall`) + `recommendation` / honesty `note` — [docs/agent-recipes.md](docs/agent-recipes.md) |
| `who-calls` | High-level who-calls recipe: implementors separated/collapsed by default (`--noisy` merges); high-freq names demoted — [docs/agent-recipes.md](docs/agent-recipes.md) |
| `macro status` | Optional macro-expanded sidecar (P2/M1, default OFF) — path + counts + `expanded_root_missing` / `expanded_root_nested` / `subset_violation_count` / `stale` / `path_map_present` / `dedup_stats` / `rebuild_policy`. Sidecar is **per-root**; workspace multi-root `--with-macro` needs a single `--workspace-root` filter |
| `macro rebuild` | Re-index recorded expanded shadow into sidecar (idempotent; no `cargo expand`; not sound) |

Confidence windows on `callers` / `impact` (and MCP tools):

- default: **Exact + Heuristic** (L1 DI/factory/event candidates). **Noise governance:** rows carry `edge_role` (`call`|`implementor`|`registration`|`dynamic`); when implementors are present `callers` returns `{callers, implementors, implementor_count, implementors_truncated, truncated, note}` (plain array when zero implementors). `--include-implementors` merges; `--implementors-only` isolates implementors. High-frequency names (`fmt`/`drop`/`clone`/…) cap implementors at 20. Store keeps all edges; `impact` still expands implementors (tagged). See [docs/noise-governance.md](docs/noise-governance.md).
- `--exact-only`: L0 syntactic edges only
- `--include-dynamic` / **`--recall`**: also DynamicCandidate (reflection / computed keys — noisier)
- `--sound` (L2, S-qualified): modeled edges (direct, literal-key, **emit↔on dispatch**, DI/route registration) when `subset_ok`; registration ≠ HTTP ServeHTTP. See [docs/sound-subset.md](docs/sound-subset.md). Query p95: [docs/eval-query-p95.md](docs/eval-query-p95.md). **Promise tier:** shipped languages (js/ts/tsx/jsx/python/go/rust) are **`ast_modeled`** (tree-sitter AST S gate — engineering subset, **not** ecosystem sound). Type-only `typeof Function` stays **in S**; value-use of `Function`/`eval` leaves S.
- `--with-macro` (P2/M1, **default OFF**): union optional macro-expanded sidecar hits tagged `origin=macro_expanded` with **mapped source paths** when the path map applies. De-dup ON by default (same logical edge as main Exact/Heuristic keeps the main row); `--no-macro-dedup` is a debug escape. `--exact-only --with-macro` ignores the sidecar. Stale sidecars warn and still union (`macro rebuild` repairs). Not sound; mutually exclusive with `--sound`. See [docs/macro-sidecar.md](docs/macro-sidecar.md).

**怕漏（missed edges）时：** 优先 `--sound`（`subset_ok` 时）或 `--recall`。干净的图 ≠ 完整的图。

Every non-Exact edge carries `evidence` (`rule_id` + source snippet). SCIP export defaults to Exact+Heuristic (DynamicCandidate omitted). Numbers: [docs/eval-l1.md](docs/eval-l1.md), [docs/eval-l2.md](docs/eval-l2.md). Public agent code-change task evals (blast radius / wrong-file noise vs name-grep baseline): [docs/eval-agent-tasks.md](docs/eval-agent-tasks.md).

### Index quality

- **Import resolution** — TS/JS relative paths, Python packages, local Rust `crate::`/`super::`/`self::` (segment-counted), conservative Go packages
- **Incremental** — content-hash skip; LLM descriptions survive reindex
- **Type-aware (pragmatic)** — `qualifier` from param annotations, receivers, `New*` constructors, return-type `define` edges; `callers` accepts `Type.method` / `Type::method`. Not a full type checker
- **Empty-index contract** — CLI *and* MCP fail loudly if you query before `index` (no silent `[]`)

### Export (SCIP)

`export scip` writes **protobuf binary** (what the official `scip` CLI reads).  
`export scip-json` writes protobuf JSON for debugging.

Official descriptors (validated with `scip lint` exit 0):

- type: `Store#`
- method: `Store#save().`
- function: `loginHandler.`
- namespace: `ns/`

Interop-tested against crate [`scip`](https://crates.io/crates/scip) 0.10 and the official CLI (`print` / `lint` / `stats`).  
`export lsif` is a simplified JSONL dump (not a full LSIF implementation).

### Enrich (optional LLM)

```bash
export OPENAI_API_KEY=sk-...
# optional: OPENAI_BASE_URL, AGENTGRAPH_MODEL, AGENTGRAPH_ENRICH_CONCURRENCY
agentgraph enrich --limit 50
```

Concurrent OpenAI-compatible labels; successes are persisted even if the run later bails.

### Watch

```bash
agentgraph watch --interval 5
```

Uses **fsnotify** with debounce; falls back to poll (mtime nanos + size) if the watcher fails. Reindexes changed source paths (path-scoped when possible). Dirty-file reindex re-certifies S subset violations for those paths so `--sound` / `subset` reflect current disk ([docs/sound-subset.md](docs/sound-subset.md)).

## MCP server

```bash
agentgraph --root /path/to/repo mcp
```

Tools: `index`, `find_symbol`, `callers`, `impact`, **`blast_radius`** / **`who_calls`** (high-level agent recipes — auto window + implementor separation; [docs/agent-recipes.md](docs/agent-recipes.md)), **`graph`** (self-contained HTML neighborhood + honesty payload — string-only by default, optional jailed `out`; [docs/graph-html.md](docs/graph-html.md)), `related_files`, `importers`, `enrich`, `stats`, **`subset`** (S-violation report that gates `--sound`), **`graph_diff`** (indexed-edge snapshot diff; not runtime semantics — [docs/graph-diff.md](docs/graph-diff.md)), **`workspace_status`** (multi-root health — [docs/workspace.md](docs/workspace.md)), optional **`macro_status`** / **`macro_rebuild`** / `with_macro` + `no_macro_dedup` (P2/M1 sidecar, default off — [docs/macro-sidecar.md](docs/macro-sidecar.md)). Query tools accept optional `workspace_db` / `root_id` filters (default off).

**Security:** per-call `root` is jailed under the server’s initial root unless `AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1`.

Example client config (also committed as [examples/mcp-claude.json](examples/mcp-claude.json) / [examples/mcp-generic.json](examples/mcp-generic.json)):

```json
{
  "mcpServers": {
    "agentgraph": {
      "command": "agentgraph",
      "args": ["--root", "C:/path/to/repo", "mcp"]
    }
  }
}
```

5-minute cold start: [docs/onboarding.md](docs/onboarding.md) · demo `scripts/demo_blast_radius.ps1` / `.sh`.

## Install

### From source (recommended until a tagged release exists)

```bash
cargo install --path .
# or
cargo build --release
```

### Prebuilt (after a `v*` release is published)

```bash
# macOS / Linux (SHA256 fail-closed)
curl -fsSL https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.sh | bash

# Windows PowerShell
iwr -useb https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.ps1 | iex
```

CI builds multi-OS artifacts on tag `v*` (linux gnu/musl, windows, macos) with `SHA256SUMS`.

## Demo (fixture)

```bash
agentgraph --root fixtures/sample-app index --force
agentgraph --root fixtures/sample-app impact validate_email --depth 3
agentgraph --root fixtures/sample-app importers src/auth.ts
agentgraph --root fixtures/sample-app export scip --out /tmp/index.scip
scip lint /tmp/index.scip   # exit 0
```

## Architecture

```
source files
    │  ignore (gitignore + segment noise filter)
    ▼
tree-sitter (TS / TSX / JS / JSX / Python / Go / Rust)
    │  rayon parallel parse + LineIndex (UTF-16 cols)
    ▼
symbols + refs (+ import resolve + qualifier)
    │  single SQLite batch transaction
    ▼
.agentgraph/index.db
    │
    ├─ CLI
    ├─ MCP stdio
    ├─ enrich (concurrent)
    └─ export scip / scip-json / lsif
```

Call resolution is **name-based** with optional type `qualifier` — pragmatic, **not full type inference**. Impact uses true BFS over enclosing-symbol promotion.

## Tests & process

```bash
cargo test
cargo test --test e2e_cli          # real binary E2E (CLI + MCP + scip)
cargo test --test agent_task_eval  # P0-1 public agent-task eval harness
python scripts/eval_agent_tasks.py # scores → target/agent_task_eval.json
powershell -File scripts/e2e.ps1   # full local gate + fixture smoke
```

CI (ubuntu / windows / macos): `fmt` + `clippy -D warnings` + `build` + `test` + CLI E2E; Linux also runs official `scip lint`.

**Development is TDD-first** — see [AGENTS.md](AGENTS.md). Write a failing test, implement, refactor.

**Roadmap (L0–L3):** analysis capability plan — [PLAN.md](PLAN.md). L1 DI/dynamic **candidate** edges are shipped (not sound). L2 `--sound` is **experimental** with a weakened eligibility promise (see [docs/sound-subset.md](docs/sound-subset.md)); L3 formal track is non-blocking research.

## License

MIT
