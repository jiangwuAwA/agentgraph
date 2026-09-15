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
- **Performance**: parallel parse (rayon), single-read files, batched SQLite writes, WAL + tuned pragmas, O(log n) line lookup
- **Windows-safe**: strips `\\?\` UNC prefix from canonical roots
- **Interfaces**: CLI + MCP (stdio)

## Install

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
```

Index: `<root>/.agentgraph/index.db` (gitignore it).

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

Call resolution is **name-based** (not full type inference) — a pragmatic MVP trade-off. Impact uses true BFS and expands via enclosing symbol leaf names.

## Tests

```bash
cargo test
```

Covers line index, TS import resolve, BFS impact, and description preservation across reindex.

## License

MIT
