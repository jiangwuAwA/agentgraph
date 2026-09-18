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

**Agent-facing polish (Track M4-W):**

| Surface | Behavior |
|---|---|
| Default query JSON (find/callers/impact/diff/subset/sound) | Every row that can appear in a multi-root DB includes `root_id` (empty legacy rows serialize as `"default"`). Classic single-root path may omit `root_id`. |
| `find` path display | Rows also carry `root_path` (recorded workspace root path) so agents do not confuse `src/main.ts` in two roots |
| `workspace status` | Per-root `files`/`symbols`/`references`/`exact_refs`/`heuristic_refs`/`subset_violations` + `promise_tier` + `index_seq` + `missing:true` when the recorded path is gone; payload-level `promise_tier` + `weakest_root` (union sound = weakest root). **P4:** `sound_candidates[]` (eligible first) + `recommendation` + enriched `by_root[]` (`violations` / `top_kinds` / `promise_tier` / `sound_eligible`). **P5:** `baseline_stale`, `sidecar_exists`, `sidecar_stale` (cheap meta reads; never create sidecar / never refresh baseline) |
| Partial re-index | `index --workspace-root api --workspace-db ws.db` re-indexes/prunes **that** `root_id` only; sibling roots remain in the store and meta |
| `graph` / `graph --sound` | Global `--workspace-root` filters the neighborhood; HTML nodes show a `root_id` badge (`data-root-id`) when present |
| Nested roots | Allowed + warned on index; `workspace status` lists **both** roots. Resolution: each root stores **root-relative** paths under its own `root_id`; overlapping files are indexed twice (once per root) — not a shared identity |
| Macro sidecar | **Per-root** `<root>/.agentgraph/index.macro.db`. `--with-macro` + multi-root workspace **without** a single `--workspace-root` filter is rejected with a clear error |
| MCP | Tool `workspace_status`; `find_symbol`/`callers`/`impact`/`subset`/`graph_diff` accept optional `workspace_db` + `root_id` (default off) |
| Watch | Remains classic `--root` (not multi-root rewrite) |

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
| `workspace status` | Per-root files/symbols/refs/exact_refs/heuristic_refs/subset_violations + `promise_tier` + `index_seq` + `missing` + **`sound_candidates` / `recommendation`** (scoped --sound, eligible first) + **`baseline_stale` / `sidecar_exists` / `sidecar_stale`** |
| `subset` / MCP `subset` | Always include `sound_candidates[]` + `recommendation`; workspace → `by_root[]`; single-root → `by_top_dir[]`. `subset --by-root` forces root buckets |
| Macro sidecar | **Per-root path** `<each-root>/.agentgraph/index.macro.db` (unchanged M1). Workspace main index is single-db; sidecars stay per-root. `--with-macro` + multi-root without root filter → **rejected** |
| Diff snapshots | Workspace full index writes **per-root** sidecars `<root>/.agentgraph/refs.snapshot.<root_id>.json`; classic single-root keeps `refs.snapshot.json` |
| Event dispatch edges | Linked **within** a root_id only (no cross-root emit↔on) |
| Watch | Classic `--root` only (not multi-root) |

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

Query tools (`find_symbol`, `callers`, `impact`, `subset`, `graph_diff`) accept
optional `workspace_db` + `root_id` (default off). `with_macro` + multi-root
without `root_id` is rejected (sidecar is per-root).

---

## Non-goals

- No remote / multi-machine index service
- No cross-root type merge or unified symbol identity across packages
- No claim that union callers/impact are sound because one root is in S
- No automatic `cargo expand` for workspace macro sidecars (still manual / per-root)
- Not a monorepo build-system integration (no pnpm/nx/cargo-workspace discovery beyond explicit roots)

---

## Operator appendix — v0.5.1 demos

Private corpora are **not** committed. Numbers are operator-run on the release
binary (`agentgraph 0.5.1`), not CI.

### A. Two-root mini workspace (Nest TS + Rust dyn)

Roots: Nest starter `src` (`id=api`) + `fixtures/eval-l1-real/rust-dyn-trait`
(`id=core`). Single store under the workspace dir.

| Metric | Value |
|---|---|
| files / symbols / refs | 7 / 18 / 57 |
| per-root promise | both `ast_modeled`, 0 violations |
| `find bootstrap` (union) | **api** `src/main.ts` + **core** `src/lib.rs` |
| `find AppService --workspace-root api` | 1 row, `root_id=api`, `root_path` set |
| `graph AppService --workspace-root api` | 9 nodes / 9 edges; HTML `root-badge=api` |

**Manifest gotcha:** JSON must be UTF-8 **without BOM**. A PowerShell
`Set-Content -Encoding UTF8` BOM yields `parse workspace manifest … line 1
column 1`. Write with `UTF8Encoding($false)` if scripting.

### B. Stock crates as workspace roots (stress)

Six crates copied under one workspace (operator paths omitted):

| root_id | files | refs | subset_violations | promise_tier |
|---|---:|---:|---:|---|
| auth | 5 | 471 | 0 | ast_modeled |
| event-engine | 5 | 384 | 0 | ast_modeled |
| repository | 27 | 915 | 0 | ast_modeled |
| strategy-plugins | 30 | 3836 | 3 | disabled |
| scheduler | 42 | 13986 | 5 | disabled |
| nn-ranker | 24 | 7417 | 19 | disabled |
| **workspace total** | **133** | **27009** | union | **disabled** (weakest root) |

- Full workspace `index --force`: **~27 s** on the operator laptop
- `callers insert --workspace-root repository`: Exact call + **implementor**
  `PgKlineRepo` (`edge_role=implementor`) — noise split works in multi-root
- `find decide` union: only `event-engine` (correct for this crate slice)
- `subset` without root filter: `in_subset=false` when any root violates
  (nn-ranker/scheduler) — honest weakest-root rule
- `graph insert --workspace-root repository`: 5 nodes; HTML shows **IMP** +
  root badges

### C. Practical recipe

```text
agentgraph index --workspace workspace.json --force
agentgraph workspace status --workspace workspace.json
agentgraph find <sym> --workspace workspace.json --workspace-root ./packages/api
agentgraph callers <sym> --workspace workspace.json --workspace-root ./packages/core
agentgraph graph <sym> --workspace workspace.json --workspace-root ./packages/api --out g.html
```

Use **scoped** `--sound` only on roots whose `promise_tier` is not `disabled`.
Machine-readable candidates come from `workspace status` / `subset`
`sound_candidates` (eligible first) — see [sound-subset.md](sound-subset.md)
and [agent-recipes.md](agent-recipes.md):

```text
agentgraph subset --workspace workspace.json
agentgraph impact X --sound --workspace-root <eligible> --workspace workspace.json
```

Honesty: per-root cleanliness is a **trial candidate**, not product soundness
(private stock corpus evidence lives in
[eval-stock-boundary.md](eval-stock-boundary.md)).

---

## See also

- [graph-diff.md](graph-diff.md) — indexed-edge diff + workspace snapshot note
- [macro-sidecar.md](macro-sidecar.md) — per-root optional expanded sidecar
- [sound-subset.md](sound-subset.md) — S gate / promise tiers
- [noise-governance.md](noise-governance.md) — callers vs implementors
- [product-boundary-migration.md](product-boundary-migration.md) Track M4-W

