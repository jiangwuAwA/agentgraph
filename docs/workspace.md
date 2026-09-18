# Workspace multi-root (`agentgraph index --workspace`)

**Status:** shipped (Track M4-W).  
**Non-claim / Honesty:** one CLI invocation can index **multiple project roots**
into a **single SQLite store** tagged with `root_id`. This is **not** a
cross-root type merge, not a remote multi-repo service, and not a claim that
union queries are sound across roots. Global `--sound` promise = **weakest
selected root**.

**Reproduce:** `cargo test --test workspace_index`

---

## Product sentence

One CLI invocation indexes multiple monorepo project roots into a queryable
graph; Agent queries can be scoped per root. Design: **single SQLite store +
`root_id` column** (not N separate `.agentgraph` dirs) so agents open **one**
connection.

---

## CLI

```bash
# Manifest form
agentgraph index --workspace workspace.json
agentgraph index --workspace workspace.json --workspace-db /path/ws.db --force

# Flag form (repeatable roots)
agentgraph index --workspace-root ./packages/api --workspace-root ./packages/web
agentgraph index --workspace-root ./api --workspace-root ./web --workspace-db ./ws.db

# Status
agentgraph workspace status --workspace-db ./ws.db
agentgraph workspace status --workspace workspace.json

# Queries — filter by root_id (union + tagged rows when omitted / multi-select)
agentgraph find createUser --workspace-root ./packages/api --workspace-db ./ws.db
agentgraph callers validateEmail --workspace-root ./packages/web --workspace-db ./ws.db
agentgraph impact createUser --workspace-db ./ws.db          # union all roots
agentgraph subset --workspace-root ./packages/api --workspace-db ./ws.db
agentgraph diff --workspace-root ./packages/api --workspace-db ./ws.db
```

| Flag | Meaning |
|---|---|
| `--workspace <manifest.json>` | Multi-root index / status / query DB resolution |
| `--workspace-root <dir>` | Repeatable. Index: roots to ingest. Query: `root_id` filter |
| `--workspace-db <path>` | Explicit shared SQLite path (overrides defaults) |

### Manifest JSON

Either form is accepted:

```json
{ "roots": [ { "id": "api", "path": "./packages/api" }, { "id": "web", "path": "./packages/web" } ] }
```

```json
[ "./packages/api", "./packages/web" ]
```

Relative paths resolve against the **manifest directory**. Bare paths get
`root_id` = directory basename (collisions append a short path hash).

---

## Store location (documented choice)

| Invocation | Default DB path |
|---|---|
| `--workspace-db <path>` | that path |
| `--workspace <manifest.json>` | `<manifest_dir>/.agentgraph/index.db` |
| `--workspace-root <dir> …` (no db/manifest) | **first root's** `<dir>/.agentgraph/index.db` |
| classic `agentgraph index` (no workspace flags) | `<root>/.agentgraph/index.db` (unchanged) |

All workspace roots share **one** store. Do not point two independent
workspaces at the same DB unless you intend a combined graph.

---

## Schema

- `files`, `symbols`, `refs`, `subset_violations` each carry `root_id TEXT NOT NULL DEFAULT ''`.
- `files` primary key is `(root_id, path)` so the same relative path may exist in two roots.
- Classic single-root rows keep `root_id=''` — **CLI JSON for non-workspace queries is unchanged** (`root_id` omitted when empty).
- Existing DBs migrate on open: legacy path-only PK tables are rebuilt; rows default `root_id=''`.
- Workspace re-index of a path under a named root **reclaims** legacy `root_id=''` rows for that relative path (migration safety).

### Row tagging

When the store has non-empty `root_id` values (workspace mode), query JSON rows
include `root_id`. Without workspace flags, `find`/`callers`/`impact` **union
all roots** and still tag each row with `root_id`.

---

## Semantics

| Topic | Behavior |
|---|---|
| Index order | Each root sequentially, same WAL, same DB |
| Paths | Stored **root-relative** per `root_id` (not confused across roots) |
| Duplicate root path | **Hard-reject** (fail-loud) |
| Nested roots | **Allow + warn** (stderr); not a hard reject |
| Query without `--workspace-root` | Union all roots; rows tagged `root_id` |
| Query with one `--workspace-root` | Filter SQL to that `root_id` |
| Query with multiple `--workspace-root` | Union of selected roots (rows tagged) |
| `subset` / `--sound` | Per-root when filtered; **union = weakest root** (any violation → `subset_ok=false`) |
| `stats` | Includes `by_root[]` when workspace rows exist |
| `workspace status` | Per-root files/symbols/refs/subset_violations |
| Macro sidecar | **Per-root path** `<each-root>/.agentgraph/index.macro.db` (unchanged M1). Workspace main index is single-db; sidecars stay per-root |
| Diff snapshots | Workspace full index writes **per-root** sidecars `<root>/.agentgraph/refs.snapshot.<root_id>.json`; classic single-root keeps `refs.snapshot.json` |
| Event dispatch edges | Linked **within** a root_id only (no cross-root emit↔on) |

### `--sound` + workspace

Document as **per-root `subset_ok`**:

- `subset --workspace-root <dir>` reports that root's violations only.
- `callers/impact --sound --workspace-root <dir>` uses that root's S gate.
- Unfiltered workspace sound/subset is **disabled** when **any** selected root
  violates S (global promise = weakest root).

---

## MCP

Tool `workspace_status`: optional `workspace_db` / `workspace_root` args;
returns the same payload as `agentgraph workspace status`.

---

## Non-goals

- No remote / multi-machine index service
- No cross-root type merge or unified symbol identity across packages
- No claim that union callers/impact are sound because one root is in S
- No automatic `cargo expand` for workspace macro sidecars (still manual / per-root)
- Not a monorepo build-system integration (no pnpm/nx/cargo-workspace discovery beyond explicit roots)

---

## See also

- [graph-diff.md](graph-diff.md) — indexed-edge diff + workspace snapshot note
- [macro-sidecar.md](macro-sidecar.md) — per-root optional expanded sidecar
- [sound-subset.md](sound-subset.md) — S gate / promise tiers
- [product-boundary-migration.md](product-boundary-migration.md) Track M4-W
