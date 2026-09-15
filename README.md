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

## Features (MVP)

- **Languages**: TypeScript / TSX, Python
- **Index**: symbols (functions, classes, methods, interfaces, types), call sites, imports
- **Incremental**: content-hash skip — only changed files re-parsed
- **Queries**:
  - `find_symbol` — definitions by exact / fuzzy name
  - `callers` — call/import sites
  - `impact` — multi-hop blast radius (BFS on call graph)
  - `related_files` — files to scope retrieval before reading code
- **Interfaces**: CLI + MCP (stdio) — same queries, JSON output

## Install

```bash
cargo install --path .
# or
cargo build --release
# binary: target/release/agentgraph.exe
```

Requires a C toolchain (for tree-sitter grammars) and Rust stable.

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
```

Index is stored at `<root>/.agentgraph/index.db` (add to `.gitignore`).

## MCP server

```bash
agentgraph --root /path/to/repo mcp
```

Example `mcp.json` / Claude Desktop config:

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
| `index` | Build/refresh index (call once before queries) |
| `find_symbol` | Locate definitions |
| `callers` | Who depends on this symbol |
| `impact` | Transitive blast radius |
| `related_files` | Scope which files to read |
| `stats` | Index size / languages |

## Demo

```bash
agentgraph --root fixtures/sample-app index
agentgraph --root fixtures/sample-app impact validateEmail --depth 3
```

Expected (abridged): `validateEmail` → `createUser` → `authenticate` → `loginHandler`.

## Architecture

```
source files
    │  ignore (gitignore-aware walk)
    ▼
tree-sitter parse (TS / Python)
    │
    ▼
symbols + references
    │
    ▼
SQLite (.agentgraph/index.db)
    │
    ├─ CLI (clap, JSON stdout)
    └─ MCP stdio (JSON-RPC 2.0)
```

Call resolution is **name-based** (not full type inference) — honest MVP trade-off. Multi-hop impact uses enclosing-symbol promotion as a practical approximation.

## Roadmap

- [ ] Go / Rust / Java grammars
- [ ] Import-path resolution (module-accurate edges)
- [ ] LLM-enriched symbol intent labels (optional)
- [ ] Watch mode / daemon
- [ ] SCIP / LSIF export

## License

MIT
