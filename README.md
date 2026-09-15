# agentgraph

Agent-native code understanding: symbol graph, call graph, impact analysis — as a **CLI + MCP server**.

Not another embedding RAG. When an agent needs to know *who calls this*, *what breaks if I change this*, or *which files matter*, it needs **structural facts**, not similar text chunks.

## Why

| Approach | Problem |
|---|---|
| Chunk + embed RAG | Loses call/import structure |
| CodeQL / Sourcegraph | Heavy, enterprise-oriented, hard for agents to call |
| Raw LSP | Built for IDE hover, not multi-hop agent reasoning |

`agentgraph` is a local-first index: tree-sitter parse → SQLite symbol/reference store → query API for agents.

## Features

- **Languages**: TypeScript, **TSX**, JavaScript/**JSX**, Python, Go, Rust
- **Index**: symbols, call sites, imports (module-resolved when possible)
- **Import resolution**: TS/JS relative paths, Python packages, local Rust `crate::`/`super::`/`self::`, conservative Go package paths
- **Incremental**: content-hash skip; LLM descriptions **survive reindex**
- **Queries**:
  - `find_symbol` — definitions (exact / escaped fuzzy)
  - `callers` — call/import sites with `module` + `resolved`
  - `impact` — **true BFS** blast radius
  - `related_files` — definition + importers + references
  - `importers` — who imports this file
- **`enrich`**: concurrent OpenAI-compatible LLM labels (descriptions persisted)
- **`watch`**: poll mtime fingerprint and reindex
- **Type-aware calls (pragmatic)**: `qualifier` from param annotations, receivers, `New*` constructors, and return-type `define` edges; `callers` matches `Type.method` / `Type::method`. Not a full type checker.
- **`export scip`**: SCIP **protobuf binary** readable by the official `scip` CLI (`scip-<lang> <manager> agentgraph 0.0.0 <descriptor>`, `relativePath`, `symbolRoles`). Also `export scip-json` for JSON mapping. Validated against crate `scip` 0.10. LSIF remains a simplified JSONL dump.
- **Prebuilt binaries**: GitHub Actions release + `install.sh` / `install.ps1` (SHA256 fail-closed)
- **CI gate**: `cargo fmt --check` + `clippy -D warnings` are required on all platforms
- **Performance**: parallel parse (rayon), single-read files, batched SQLite writes, WAL + tuned pragmas, O(log n) line lookup
- **Windows-safe**: strips `\\?\` UNC prefix from canonical roots
- **Interfaces**: CLI + MCP (stdio)

## Install

### Prebuilt

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.sh | bash

# Windows PowerShell
iwr -useb https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.ps1 | iex
```

Tag `v*` to cut a release (CI builds linux/mac/windows artifacts).

### From source

```bash
cargo install --path .
# or
cargo build --release
```

## CLI

```bash
cd /path/to/your/repo
agentgraph index
agentgraph index --force
agentgraph watch --interval 5

agentgraph find validateEmail
agentgraph callers hashPassword
agentgraph impact validateEmail --depth 3
agentgraph related createUser
agentgraph importers src/auth.ts

export OPENAI_API_KEY=sk-...
# optional: OPENAI_BASE_URL, AGENTGRAPH_MODEL, AGENTGRAPH_ENRICH_CONCURRENCY
agentgraph enrich --limit 50
agentgraph export scip --out index.scip
agentgraph export lsif --out index.lsif
```

Index: `<root>/.agentgraph/index.db` (gitignore it).

### Export

`export scip` writes **protobuf binary** (what the official `scip` CLI reads). `export scip-json` writes the protobuf JSON mapping (for tests/debugging). Interop-tested with crate `scip` 0.10:

- symbols: `scip-typescript npm agentgraph 0.0.0 Store#save().`
- descriptors: `Type#`, `Type#method().`, `fn.`, `ns/`
- metadata: `toolInfo`, `file:///` project roots (no `schemaVersion` — not in current scip.proto)

See `tests/scip_interop.rs`. `export lsif` is a simplified JSONL dump.

## MCP

```bash
agentgraph --root /path/to/repo mcp
```

Tools: `index`, `find_symbol`, `callers`, `impact`, `related_files`, `importers`, `enrich`, `stats`.

## Demo

```bash
agentgraph --root fixtures/sample-app index --force
agentgraph --root fixtures/sample-app impact validate_email --depth 3
agentgraph --root fixtures/sample-app importers src/auth.ts
```

## Architecture

```
source files
    │  ignore (gitignore + segment noise filter)
    ▼
tree-sitter (TS / TSX / JS / JSX / Python / Go / Rust)
    │  rayon parallel parse
    ▼
symbols + refs (+ import resolve)
    │  single SQLite batch transaction
    ▼
.agentgraph/index.db
    │
    ├─ CLI
    ├─ MCP stdio
    └─ enrich (concurrent, descriptions preserved)
```

Call resolution is **name-based** with optional `qualifier` hints — pragmatic, **not full type inference**. Impact uses true BFS and expands via enclosing symbol leaf names.

## Tests

```bash
cargo test
# CLI E2E only
cargo test --test e2e_cli
# Full local gate + fixture smoke
pwsh scripts/e2e.ps1
```

Covers unit/integration (`tests/*.rs`), **CLI binary E2E** (`tests/e2e_cli.rs`: index/query/export/MCP), and optional official `scip lint` when the CLI is on PATH. CI runs fmt, clippy, tests, E2E, and `scip lint` on Linux.

**Process**: new feature work is TDD-first — see [AGENTS.md](AGENTS.md).

## License

MIT
