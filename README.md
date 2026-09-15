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

- **Languages**: TypeScript / TSX, Python, **Go**, **Rust**
- **Index**: symbols (functions, classes, methods, structs, traits, interfaces, types), call sites, imports
- **Import resolution**: relative TS/JS paths, Python packages, best-effort Go/Rust module paths → `module` + `resolved` file edges
- **Incremental**: content-hash skip — only changed files re-parsed
- **Queries**:
  - `find_symbol` — definitions by exact / fuzzy name
  - `callers` — call/import sites (with module/resolved when known)
  - `impact` — multi-hop blast radius (BFS on call graph)
  - `related_files` — definition + importers + references (scope retrieval)
  - `importers` — who imports this file (module-level)
- **Optional LLM enrich** (`enrich`): one-line responsibility labels on symbols
- **Interfaces**: CLI + MCP (stdio)

## Real-repo validation

Indexed `codex-rs` (OpenAI Codex Rust workspace):

| Metric | Value |
|---|---|
| Files | 4,860 |
| Symbols | 66,830 |
| References | 691,646 |
| Languages | rust, typescript, python |
| Index time | ~234 s (full, release build) |

Sample hits on that index:

- `find connect_websocket` → qualified names like `CodexClient::connect_websocket`, `ModelClient::connect_websocket`
- `impact connect_websocket` → real call sites with enclosing test/function names
- `related parse_host_url` → definition file + `app-server/src/lib.rs` via **resolved import edge**

## Install

```bash
cargo install --path .
# or
cargo build --release
```

Requires a C toolchain (tree-sitter grammars) and Rust stable.

## CLI

```bash
cd /path/to/your/repo
agentgraph index                 # incremental
agentgraph index --force         # full rebuild
agentgraph stats

agentgraph find validateEmail
agentgraph callers hashPassword
agentgraph impact validateEmail --depth 3
agentgraph related createUser
agentgraph importers src/auth.ts

# Optional LLM labels (OpenAI-compatible API)
export OPENAI_API_KEY=sk-...
export OPENAI_BASE_URL=https://api.openai.com/v1   # optional
export AGENTGRAPH_MODEL=gpt-4o-mini                # optional
agentgraph enrich --limit 50
```

Index lives at `<root>/.agentgraph/index.db` (add to `.gitignore`).

## MCP server

```bash
agentgraph --root /path/to/repo mcp
```

Example client config:

```json
{
  "mcpServers": {
    "agentgraph": {
      "command": "agentgraph",
      "args": ["--root", "C:/path/to/your/repo", "mcp"]
    }
  }
}
```

### Tools

| Tool | Purpose |
|---|---|
| `index` | Build/refresh index |
| `find_symbol` | Locate definitions |
| `callers` | Who depends on this symbol |
| `impact` | Transitive blast radius |
| `related_files` | Scope which files to read |
| `importers` | Who imports this file |
| `enrich` | LLM responsibility labels |
| `stats` | Index size / languages |

## Demo (fixture, 4 languages)

```bash
agentgraph --root fixtures/sample-app index --force
agentgraph --root fixtures/sample-app impact validate_email --depth 3
agentgraph --root fixtures/sample-app importers src/auth.ts
```

## Architecture

```
source files
    │  ignore (gitignore-aware walk)
    ▼
tree-sitter parse (TS / Python / Go / Rust)
    │
    ▼
symbols + references (+ import resolve)
    │
    ▼
SQLite (.agentgraph/index.db)
    │
    ├─ CLI (clap, JSON stdout)
    ├─ MCP stdio (JSON-RPC 2.0)
    └─ enrich (OpenAI-compatible, optional)
```

Call resolution is **name-based** (not full type inference) — honest MVP trade-off that already scales to ~5k-file monorepos. Multi-hop impact uses enclosing-symbol promotion as a practical approximation.

## Roadmap

- [ ] Type-aware / receiver-aware call resolution
- [ ] Java / C# / Kotlin grammars
- [ ] Watch mode / daemon
- [ ] SCIP / LSIF export
- [ ] Batch LLM enrich with concurrency + cache

## License

MIT
