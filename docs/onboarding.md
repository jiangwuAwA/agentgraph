# Onboarding kit (5 minutes)

**Status:** P0-2 shipped (onboarding / cold-start).  
**Non-claim / Honesty:** this kit gets you from install → index → MCP
`blast_radius` on a **sample fixture**. Results are **indexed L0/L1
candidates**, **not** a complete runtime graph. Scoped `--sound` is an
engineering S gate (modeled edges when `subset_ok`) — **not** ecosystem
sound, **not** production sound. Do **not** claim zero missed dynamic
edges; 禁止写「零漏报」.

Related: [agent-recipes.md](agent-recipes.md) ·
[workspace.md](workspace.md) · [sound-subset.md](sound-subset.md) ·
[agent-playbook/index.html](agent-playbook/index.html) ·
[product-improvement-backlog.md](product-improvement-backlog.md)

Repo fixtures used below (offline, no network):

| Fixture | Use |
|---|---|
| `fixtures/sample-app` | Classic single-root happy path (TS + Go + Python + Rust samples) |
| `fixtures/eval-goldens/ts-nest-mini` | Tiny Nest-like TS service (public synthetic shape) |
| `fixtures/eval-goldens/rust-inventory-mini` | Tiny Rust companion for workspace demo |

Demo scripts (same flow, automated):

```bash
# Windows PowerShell
pwsh -File scripts/demo_blast_radius.ps1
# or
powershell -File scripts/demo_blast_radius.ps1

# linux / macOS
bash scripts/demo_blast_radius.sh
```

Both exit `0` when index + `blast-radius` succeed on a temp copy of the
fixture. Offline from this repository.

---

## 0. Install

### A. GitHub release assets (v0.5.2)

CI publishes multi-OS artifacts on tag `v*` (linux gnu/musl, windows, macos)
with `SHA256SUMS`. Scripts verify checksum **fail-closed**.

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.sh | bash
# pin a tag:
#   curl -fsSL …/install.sh | bash -s -- v0.5.2

# Windows PowerShell
iwr -useb https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.ps1 | iex
# pin a tag:
#   .\install.ps1 -Version v0.5.2
```

Release asset names (see `install.sh` / `install.ps1`):

| Platform | Asset |
|---|---|
| linux x86_64 (gnu) | `agentgraph-linux-x86_64.tar.gz` |
| linux x86_64 (musl / Alpine) | `agentgraph-linux-x86_64-musl.tar.gz` |
| macOS x86_64 / aarch64 | `agentgraph-macos-*.tar.gz` |
| Windows x86_64 | `agentgraph-windows-x86_64.zip` |

If a tag is not published yet, use source install.

### B. From this repository (recommended for developers)

```bash
git clone https://github.com/jiangwuAwA/agentgraph.git
cd agentgraph
cargo install --path .
# or: cargo build --release   → target/release/agentgraph
agentgraph --version
```

Need Rust: [rustup.rs](https://rustup.rs). Offline after `cargo` deps are
cached; tree-sitter grammars ship as crates.

---

## 1. MCP host wiring (verified from source)

**Entry (from `src/cli.rs` + `src/mcp/server.rs` — do not invent a protocol):**

```bash
agentgraph --root /path/to/repo mcp
```

- **Transport:** MCP over **stdio**, newline-delimited **JSON-RPC 2.0**
- **protocolVersion:** `2024-11-05`
- **serverInfo:** `name=agentgraph`, `version=<crate version>`
- **Capabilities:** `{ "tools": {} }`
- **Methods:** `initialize`, `tools/list`, `tools/call`, `ping`
- **Security:** per-call `root` is jailed under the server’s initial `--root`
  unless `AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1` (path-escape / prompt-injection
  guard). Optional `workspace_db` / `root_id` args on query tools (default off).

Tools returned by `tools/list` (verified):

`index`, `find_symbol`, `callers`, `blast_radius`, `who_calls`, `graph`,
`impact`, `subset`, `graph_diff`, `related_files`, `importers`, `stats`,
`workspace_status`, `macro_status`, `macro_rebuild`, `enrich`

### Host snippets (committed)

| File | Audience |
|---|---|
| [`examples/mcp-claude.json`](../examples/mcp-claude.json) | Claude Desktop / Claude Code-style `mcpServers` |
| [`examples/mcp-generic.json`](../examples/mcp-generic.json) | Generic stdio MCP host (`command` / `args` / `env`) |

**Claude-style** (copy into your host config; replace the root path):

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

**Generic stdio host:**

```json
{
  "name": "agentgraph",
  "transport": "stdio",
  "command": "agentgraph",
  "args": ["--root", "${WORKSPACE}", "mcp"]
}
```

**Env (optional):**

| Var | Meaning |
|---|---|
| `AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1` | Allow tool `root` / graph `out` outside the server root jail (default off) |

After the host starts the server, the Agent happy path is:

1. `tools/call` → `{ "name": "index", "arguments": { "root": "…" } }`
2. `tools/call` → `{ "name": "blast_radius", "arguments": { "symbol": "createUser" } }`

`blast_radius` payload always includes `window`, `subset_ok`, `promise_tier`,
`recommendation`, `note` (not a complete runtime graph). Keys are stable
(勿改名) — see [agent-recipes.md](agent-recipes.md).

---

## 2. Classic happy path (single root)

Uses `fixtures/sample-app` (or any repo you index).

```bash
# clone once
git clone https://github.com/jiangwuAwA/agentgraph.git
cd agentgraph

# 1) index
agentgraph --root fixtures/sample-app index
# → SQLite at fixtures/sample-app/.agentgraph/index.db

# 2) blast radius (CLI recipe = MCP blast_radius)
agentgraph --root fixtures/sample-app blast-radius createUser --depth 3
# watch: "window": "sound" | "default", "recommendation", "note"

# 3) who-calls
agentgraph --root fixtures/sample-app who-calls validateEmail

# 4) neighborhood graph (offline HTML)
agentgraph --root fixtures/sample-app graph createUser --depth 2 --out graph.html
```

Nest-like alternative:

```bash
agentgraph --root fixtures/eval-goldens/ts-nest-mini index
agentgraph --root fixtures/eval-goldens/ts-nest-mini blast-radius getHello
agentgraph --root fixtures/eval-goldens/ts-nest-mini who-calls getHello
```

Expected CLI shape (honesty keys — always present):

```json
{
  "tool": "blast_radius",
  "window": "sound",
  "subset_ok": true,
  "promise_tier": "ast_modeled",
  "recommendation": "subset_ok: sound window over S-qualified edges; …",
  "note": "not a complete runtime graph (…)",
  "sound_candidates": []
}
```

Empty index contract: CLI **and** MCP fail loud if you query before `index`
(no silent `[]`).

---

## 3. Workspace happy path (two roots)

Tiny two-root store — same pattern as [workspace.md](workspace.md) appendix.
Relative paths in the manifest resolve against the **manifest directory**.
Write JSON as UTF-8 **without BOM**.

```text
ws-demo/
  workspace.json
  api/    ← fixtures/eval-goldens/ts-nest-mini
  core/   ← fixtures/eval-goldens/rust-inventory-mini
```

```bash
# from the agentgraph repo
WS=/tmp/agentgraph-ws-demo
mkdir -p "$WS"
cp -R fixtures/eval-goldens/ts-nest-mini "$WS/api"
cp -R fixtures/eval-goldens/rust-inventory-mini "$WS/core"
cat > "$WS/workspace.json" <<'EOF'
{
  "roots": [
    { "id": "api", "path": "./api" },
    { "id": "core", "path": "./core" }
  ]
}
EOF

# index both roots → one SQLite store under the manifest dir
agentgraph --workspace "$WS/workspace.json" index
agentgraph --workspace "$WS/workspace.json" workspace status

# scoped queries (rows tagged root_id)
agentgraph --workspace "$WS/workspace.json" --workspace-root "$WS/api" \
  blast-radius getHello --depth 3
agentgraph --workspace "$WS/workspace.json" --workspace-root "$WS/core" \
  who-calls bootstrap
```

MCP workspace (optional args on query tools; default = classic store):

```json
{ "name": "blast_radius", "arguments": {
  "symbol": "getHello",
  "workspace_db": "/tmp/agentgraph-ws-demo/.agentgraph/index.db",
  "root_id": "api"
}}
```

Honesty: multi-root `--sound` is **per-root** (union = **weakest** root).
Read `sound_candidates[]` / `recommendation` from `workspace status` /
`subset` before claiming scoped sound. Not a cross-root type merge.

---

## 4. Honest 30s — what sound does / does not claim

| Does claim (when true) | Does **not** claim (non-claims) |
|---|---|
| Indexed symbols + reference/call edges from tree-sitter parse | **not** a complete runtime graph of your program |
| Recipe auto-window: `window=sound` only when `subset_ok` (S gate) | **not** ecosystem sound; **not** full-language soundness |
| Default window = Exact + Heuristic (L0 + L1 candidates) | **not** production sound as a product label |
| `promise_tier` e.g. `ast_modeled` / `lexical_v1` / `disabled` | **not** zero missed dynamic edges (禁止写「零漏报」) |
| Honest `note` + `recommendation` on every recipe payload | a clean graph is **not** claimed to be a complete graph |
| Per-root scoped sound candidates when global window is off | union queries are **not** claimed sound because one root is clean |

**Read before acting:** `window`, `subset_ok`, `promise_tier`,
`recommendation`, `note`. If `window` is `default`/`disabled`, follow
`recommendation` / `sound_candidates` — do **not** treat the answer as a
runtime proof. Macro sidecar (`--with-macro`) stays **default OFF** and is
**not** sound-certified; it is mutually exclusive with `--sound`.

More: [sound-subset.md](sound-subset.md) ·
[noise-governance.md](noise-governance.md) ·
[agent-recipes.md](agent-recipes.md).

---

## 5. Five-minute checklist

1. Install (release script or `cargo install --path .`) → `agentgraph --version`
2. Point MCP host at `agentgraph --root <repo> mcp` (`examples/mcp-claude.json`)
3. `agentgraph --root fixtures/sample-app index`
4. `agentgraph --root fixtures/sample-app blast-radius createUser`
5. Optional: workspace two-root path in §3
6. Optional: `pwsh -File scripts/demo_blast_radius.ps1` (or `.sh`) — exit 0

You are ready for Agent hosts. Next: [agent-recipes.md](agent-recipes.md),
[agent-playbook/index.html](agent-playbook/index.html).
