# Indexed-edge diff (`agentgraph diff`)

**Status:** shipped (Track M4).  
**Non-claim / Honesty:** `diff` compares **indexed edge sets** (SQLite `refs` rows).  
It is **not** a runtime call-graph diff, not a semantic equivalence check, and not a
git-aware patch analyzer. Payload note is always:

> `indexed edges only; not a runtime call-graph diff (name+path+line+confidence+enclosing)`

**Reproduce:** `cargo test --test graph_diff` or:

```bash
agentgraph index
# edit sources (add/remove call sites)
agentgraph index
agentgraph diff
```

**Perf:** loose latency budgets (CLI `diff` cold/warm on a mid-size fixture; process spawn included) + reproduce commands: [eval-query-p95.md](eval-query-p95.md) § M4 diff / S-recert budgets (`tests/perf_m4_diff.rs`).

---

## Command

```bash
agentgraph diff \
  [--exact-only] \
  [--limit N] \
  [--snapshot path.json] \
  [--write-snapshot]
```

| Flag | Meaning |
|---|---|
| *(default)* | Exact + Heuristic edges in the set difference |
| `--exact-only` | Only Exact (L0) edges |
| `--limit N` | Cap **rows** returned per side; `summary` keeps full counts |
| `--snapshot PATH` | Explicit baseline JSON (skips dual sidecar selection) |
| `--write-snapshot` | After printing the diff, promote **current live refs** as the new baseline |
| `--workspace-root <dir>` | Optional workspace filter: diff only that `root_id` (see [workspace.md](workspace.md)) |
| `--workspace-db <path>` | Shared workspace SQLite store |
| `--workspace <manifest>` | Resolve workspace DB from a manifest |

### Output JSON

```json
{
  "added": [ { "name": "...", "path": "...", "line": 12, "confidence": "exact", "enclosing": "..." } ],
  "removed": [ ... ],
  "summary": { "added": 3, "removed": 1 },
  "exact_only": false,
  "note": "indexed edges only; not a runtime call-graph diff (...)",
  "baseline_index_seq": 1,
  "current_index_seq": 2,
  "baseline_source": "previous"
}
```

JSON fields: `added` / `removed` (edge rows), `summary` (full counts),
`exact_only`, `note` (honesty), plus optional baseline metadata.

### Exit contract

| Case | Behavior |
|---|---|
| Baseline present | exit `0`, JSON payload on stdout |
| **No baseline** | exit **non-zero**, fail-loud: hint to run `agentgraph index` first |
| Empty index / store | fail-loud via `ensure_indexed` (same as other queries) |

---

## Snapshot workflow (dual sidecar)

Track M4 avoids a large `refs` schema migration. Instead:

| File | Role |
|---|---|
| `<root>/.agentgraph/refs.snapshot.json` | Baseline written at **full `index`** time |
| `<root>/.agentgraph/refs.snapshot.prev.json` | Previous generation (one step) |
| SQLite `meta.index_seq` | Monotonic full-index counter |

### When snapshots update

| Event | Snapshot effect |
|---|---|
| `agentgraph index` (full, including noop early-out) | Copy current snapshot → `.prev.json`; write live refs → `.json`; bump `index_seq` |
| `agentgraph index_paths` / `watch` dirty reindex | **Does not** refresh baseline (live drifts; next `diff` sees watch-added edges) |
| `agentgraph diff --write-snapshot` | Write live refs to **both** snapshot files (reset baseline to “now”) |

### Baseline selection for default `diff`

1. `--snapshot PATH` → that file.
2. Else if live refs **match** `refs.snapshot.json` **and** `.prev.json` exists →  
   compare against **previous** generation (changes since the previous full index).
3. Else → compare against `refs.snapshot.json` (changes since last full-index write,
   including watch / `index_paths` drift).

### Typical workflows

**Second full index (who changed between indexes?):**

```bash
agentgraph index                 # baseline A
# edit call sites
agentgraph index                 # baseline B; A kept as .prev
agentgraph diff                  # live vs A → added/removed edges
agentgraph diff --exact-only
agentgraph diff --limit 20
```

**Watch / incremental (who newly depends on X after dirty reindex?):**

```bash
agentgraph index                 # baseline written
agentgraph watch                 # or: code edit + index_paths
agentgraph diff                  # live vs last full-index baseline
# optional: lock the post-watch state as the new baseline
agentgraph diff --write-snapshot
```

**Missing baseline:**

```bash
agentgraph diff
# error: no refs snapshot baseline at ... — run `agentgraph index` first
```

---

## Semantics (locked)

- **Edge key:** `name + path + line + confidence + enclosing`
- **added:** present in **current index**, absent from baseline
- **removed:** present in baseline, absent from **current index**
- **`--exact-only`:** both sides filtered to `confidence=exact`
- **summary:** full set sizes (not limited)
- **Honesty:** indexed edges only — **not** runtime semantics

Line-number churn on an otherwise identical call counts as remove+add for that key.
This is intentional: the product claim is *indexed-edge set difference*, not AST identity.

---

## MCP

Tool `graph_diff` mirrors the CLI payload (optional `exact_only` / `limit` /
`snapshot` / `write_snapshot`). Same honesty note; same fail-loud without baseline.

---

## Workspace multi-root (shipped — Track M4-W)

Multi-root workspace indexing uses a **single SQLite store + `root_id` column**
(not one `.agentgraph` per root) so agents keep one connection.

```bash
agentgraph index --workspace workspace.json
agentgraph index --workspace-root ./api --workspace-root ./web
agentgraph diff --workspace-root ./api --workspace-db <shared.db>
```

Workspace full index writes **per-root** snapshot sidecars:
`<root>/.agentgraph/refs.snapshot.<root_id>.json` (+ `.prev.json`).
Classic single-root `diff` is unchanged (`refs.snapshot.json`).

Details: [workspace.md](workspace.md).

---

## Non-goals

- Runtime call-graph diff / semantic equivalence
- Git integration (commit ranges, blame)
- Cross-repo remote diff service
- Claiming L1/L2 soundness from a diff

See also: [graph-html.md](graph-html.md), [sound-subset.md](sound-subset.md),
[workspace.md](workspace.md),
[product-boundary-migration.md](product-boundary-migration.md) Track M4.
