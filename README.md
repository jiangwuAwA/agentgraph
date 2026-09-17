# agentgraph

[![CI](https://github.com/jiangwuAwA/agentgraph/actions/workflows/ci.yml/badge.svg)](https://github.com/jiangwuAwA/agentgraph/actions/workflows/ci.yml)

English | [简体中文](README.zh-CN.md)

Agent-native code understanding: **symbol graph, call graph, impact analysis** — CLI + MCP server.

Not another embedding RAG. When an agent needs *who calls this*, *what breaks if I change this*, or *which files matter*, it needs **structural facts**, not similar text chunks.

| Approach | Problem |
|---|---|
| Chunk + embed RAG | Loses call/import structure |
| CodeQL / Sourcegraph | Heavy, enterprise-oriented, awkward for agents to call |
| Raw LSP | Built for IDE hover, not multi-hop agent reasoning |

**Pipeline:** tree-sitter parse → SQLite symbol/reference store → query API for agents.

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
# open graph.html in a browser — no network
```

Impact-style BFS neighborhood with confidence colors (Exact / Heuristic / DynamicCandidate),
bilingual UI, click-to-inspect nodes. **Shows indexed L0/L1 candidates, not a complete
runtime graph.** Empty neighborhood still writes a page. Details: [docs/graph-html.md](docs/graph-html.md).

Incremental index skips rehash when **mtime+size match**. On network/FAT volumes
or tools that preserve mtime across content edits, set `AGENTGRAPH_TRUST_MTIME=0`
to force content-hash every file (hash remains the source of truth).

## Features

### Languages

TypeScript, TSX, JavaScript, JSX, Python, Go, Rust.

### Queries

| Command | Purpose |
|---|---|
| `find` | Symbol definitions (exact; `--fuzzy` for LIKE) |
| `callers` | Call/import sites + L1 candidates (`module`, `resolved`, `qualifier`, `confidence`) |
| `impact` | True BFS blast radius (default Exact+Heuristic) |
| `graph` | Local self-contained HTML code-graph (impact BFS + optional callers view) — see [docs/graph-html.md](docs/graph-html.md) |
| `related` | Definition + importers + references (scope retrieval) |
| `importers` | Who imports a given file |
| `macro status` | Optional macro-expanded sidecar (P2, default OFF) — path + counts + `expanded_root_missing` / `expanded_root_nested` / sidecar `subset_violation_count` |

Confidence windows on `callers` / `impact` (and MCP tools):

- default: **Exact + Heuristic** (L1 DI/factory/event candidates)
- `--exact-only`: L0 syntactic edges only
- `--include-dynamic` / **`--recall`**: also DynamicCandidate (reflection / computed keys — noisier)
- `--sound` (L2, S-qualified): modeled edges (direct, literal-key, **emit↔on dispatch**, DI/route registration) when `subset_ok`; registration ≠ HTTP ServeHTTP. See [docs/sound-subset.md](docs/sound-subset.md). Query p95: [docs/eval-query-p95.md](docs/eval-query-p95.md).
- `--with-macro` (P2, **default OFF**): union optional macro-expanded sidecar hits tagged `origin=macro_expanded`. Not sound; mutually exclusive with `--sound`. See [docs/macro-sidecar.md](docs/macro-sidecar.md).

**怕漏（missed edges）时：** 优先 `--sound`（`subset_ok` 时）或 `--recall`。干净的图 ≠ 完整的图。

Every non-Exact edge carries `evidence` (`rule_id` + source snippet). SCIP export defaults to Exact+Heuristic (DynamicCandidate omitted). Numbers: [docs/eval-l1.md](docs/eval-l1.md), [docs/eval-l2.md](docs/eval-l2.md).

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

Uses **fsnotify** with debounce; falls back to poll (mtime nanos + size) if the watcher fails. Reindexes changed source paths (path-scoped when possible).

## MCP server

```bash
agentgraph --root /path/to/repo mcp
```

Tools: `index`, `find_symbol`, `callers`, `impact`, `related_files`, `importers`, `enrich`, `stats`, **`subset`** (S-violation report that gates `--sound`), optional **`macro_status`** / `with_macro` (P2 sidecar, default off — [docs/macro-sidecar.md](docs/macro-sidecar.md)).

**Security:** per-call `root` is jailed under the server’s initial root unless `AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1`.

Example client config:

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
powershell -File scripts/e2e.ps1   # full local gate + fixture smoke
```

CI (ubuntu / windows / macos): `fmt` + `clippy -D warnings` + `build` + `test` + CLI E2E; Linux also runs official `scip lint`.

**Development is TDD-first** — see [AGENTS.md](AGENTS.md). Write a failing test, implement, refactor.

**Roadmap (L0–L3):** analysis capability plan — [PLAN.md](PLAN.md). L1 DI/dynamic **candidate** edges are shipped (not sound). L2 `--sound` is **experimental** with a weakened eligibility promise (see [docs/sound-subset.md](docs/sound-subset.md)); L3 formal track is non-blocking research.

## License

MIT
