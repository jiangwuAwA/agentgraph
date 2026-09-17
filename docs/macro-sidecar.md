# Macro-expanded sidecar index (P2 / Track M1)

**Status:** productized **opt-in** dual-index path (Track M1): path map + de-dup + fingerprint/stale + rebuild.  
**Not** a sound expand-graph. Origin stays **`macro_expanded`** (non-sound).  
**Default product path remains source L0/L1** (and L2 `--sound` on the main index only).  
Related: [product-boundary-migration.md](product-boundary-migration.md) §Track M1, [eval-macro-expand.md](eval-macro-expand.md), [sound-subset.md](sound-subset.md).

---

## What it is (M1 product path)

A **second SQLite store** at:

```text
<root>/.agentgraph/index.macro.db
```

Built by indexing an **already-produced** expanded shadow tree (operator-supplied). Schema matches the main store; `meta.origin = 'macro_expanded'` marks the sidecar kind.

M1 productization (no more manual dual-root diff as the only UX):

| Capability | Behavior |
|---|---|
| **Path map** | Sidecar paths mapped back to source (`src/…`, `crates/<crate>/src/…`) via explicit pairs + crate-root heuristics. Unmappable rows stay `mapped=false`. |
| **De-dup** | `--with-macro` union de-dups on `name + enclosing + mapped_path`. Main Exact/Heuristic wins; `dedup_stats` counted. Default **ON**. |
| **Fingerprint / stale** | Sidecar build writes `meta.source_fingerprint` (main files mtime+size aggregate). Status exposes `stale`. Stale + `--with-macro` → **warn + still union** + `stale:true` in payload. |
| **Rebuild** | `agentgraph macro rebuild` re-indexes the recorded `expanded_root` into the sidecar (idempotent). Does **not** call `cargo expand`. |
| **Exact-only** | `--exact-only --with-macro` **ignores** the sidecar (exact-only semantics). |
| **Debug** | `--no-macro-dedup` keeps duplicate sidecar rows (default dedup on). |

When the sidecar file is **absent**:

- All default queries behave **exactly** as before (plain JSON arrays).
- `--with-macro` treats the sidecar as **empty** (graceful union; no error; file is not created).
- `macro status` reports `exists: false` without creating the DB.

---

## Non-goals

- **No sound expand-graph.** Expanded-only symbols never receive `subset_ok=true` claims.
- **`--sound && --with-macro` remain mutually exclusive.**
- **No required CI expand.** Sidecar build is operator-driven; we never auto-run `cargo expand` at index time.
- **No arbitrary proc-macro completeness.** Failed-to-compile crates stay skipped.
- **Do not commit** operator corpora or expand products into this repo.
- Origin tag stays **`macro_expanded`** (documented choice for M1); rows may also carry `mapped` / `mapped_path` / `expanded_path`.

---

## UX / commands

### Build sidecar (does not replace main index)

```bash
agentgraph index --force
agentgraph index --force --macro-expanded-root /path/to/expanded-shadow
```

Keep the expanded tree **outside** the indexed project root (sibling directory).

**Hard reject (R26/R27, unchanged):** equal / under / contains `--root` fails closed **before** main reindex. Relative `--macro-expanded-root` resolves against `--root`, not cwd.

Index payload includes `macro_sidecar` with `source_fingerprint`, `path_map_present`, `stale`.

### Status

```bash
agentgraph macro status
```

```json
{
  "exists": true,
  "path": "<root>/.agentgraph/index.macro.db",
  "files": 1,
  "symbols": 5,
  "refs": 4,
  "origin": "macro_expanded",
  "expanded_root": "/path/to/expanded-shadow",
  "expanded_root_missing": false,
  "expanded_root_nested": false,
  "subset_violation_count": 0,
  "stale": false,
  "source_fingerprint": "…",
  "path_map_present": true,
  "path_map": [["src/core.rs:exact", "src/core.rs"]],
  "dedup_stats": {
    "merged_exact": 0,
    "merged_heuristic": 0,
    "kept_sidecar": 0,
    "unmapped": 0,
    "main_rows": 0,
    "sidecar_rows": 0
  },
  "rebuild_policy": "manual"
}
```

New fields are serde-default (old sidecar JSON still loads).

- `stale`: main source fingerprint ≠ fingerprint recorded at sidecar build.
- `expanded_root_missing` / `expanded_root_nested`: existing honesty flags (R26/R27).
- `subset_violation_count`: S noise **inside the sidecar only** — never flips main `subset_ok`.
- `dedup_stats`: last `--with-macro` union counters (zeros until a union runs).

### Rebuild

```bash
agentgraph macro rebuild
```

Re-indexes the **recorded** `expanded_root` into the sidecar (idempotent). Fails when:

- no sidecar / no recorded `expanded_root`
- `expanded_root` missing on disk
- `expanded_root` currently nests with `--root` (R27)

Does **not** invoke `cargo expand` (non-hermetic toolchain stays out of the product path).

### Query union (default OFF; de-dup ON when ON)

```bash
agentgraph callers helper                 # source L0/L1 only — unchanged
agentgraph callers helper --with-macro    # main ∪ sidecar; mapped + de-duped
agentgraph callers helper --with-macro --no-macro-dedup   # debug: keep dups
agentgraph callers helper --with-macro --exact-only        # sidecar ignored
agentgraph impact helper --with-macro --depth 2
agentgraph graph helper --with-macro                      # HTML: MACRO badge + mapped path
```

When the sidecar is present, `--with-macro` returns a **wrapped object**:

```json
{
  "callers": [ /* main rows + kept sidecar rows */ ],
  "sidecar_present": true,
  "stale": false,
  "origin": "macro_expanded",
  "path_map_present": true,
  "dedup_stats": { "merged_exact": 1, "kept_sidecar": 2, "...": "..." },
  "note": "sidecar union is optional candidates (not sound); de-dup ON unless --no-macro-dedup"
}
```

Absent sidecar → plain array (schema-stable with pre-M1).

Sidecar hit tagging:

```json
{
  "name": "helper",
  "enclosing": "fmt",
  "path": "src/core.rs",
  "mapped_path": "src/core.rs",
  "expanded_path": "src/core.rs",
  "origin": "macro_expanded",
  "mapped": true,
  "at": "src/core.rs:15"
}
```

Unmappable rows keep the expanded `path`, `mapped=false`, `origin=macro_expanded`.

`--limit` / tool `limit` applies **per store**. Without de-dup the union may return up to **~2N** rows; with de-dup, duplicate logical edges are dropped.

### De-dup table (locked in tests/macro_dedup.rs)

| Scenario | Expectation |
|---|---|
| Sidecar mapped path + name + enclosing = main Exact | Keep main only; `merged_exact += 1` |
| Sidecar path unmappable | Keep sidecar; `origin=macro_expanded`; `mapped=false` |
| Sidecar-only symbols (`fmt`/`clone`/…) | Kept as candidates (not noise-dropped) |
| `--exact-only --with-macro` | **Ignore sidecar** |
| Main Heuristic + sidecar same logical key | Main Heuristic wins; merge counted |
| Stale fingerprint + `--with-macro` | Warn + still union + `stale:true` |
| Nested expanded root | Hard-reject (R26/R27) |

### Path map

Heuristics + explicit pairs (sidecar `meta.path_map`):

- Strip expand-dir prefixes (`expanded-view/`, `real-expanded/`, `expand-shadow/`, …)
- Identity when the relative path exists under the source root
- Rust crate-root align: `event-engine/lib.rs` → `crates/event-engine/src/lib.rs`
- Explicit pairs override heuristics (longest prefix wins)
- Absolute paths under `expanded_root` and `../` sibling forms
- Crate align is accepted only when the source crate dir/file exists (no invented paths)

### Concurrent watch + sidecar index

Unchanged: WAL + `busy_timeout`; `tests/r28_adversarial.rs` locks no-corruption.

### Honesty gates

| Rule | Behavior |
|---|---|
| Default callers/impact | Never read the sidecar; JSON stays plain array |
| `--with-macro` + missing sidecar | Empty union, success, plain array |
| `--sound --with-macro` | **Rejected** (mutually exclusive) |
| `--exact-only --with-macro` | Sidecar **ignored** |
| Origin | Always `macro_expanded` (M1 choice) + `mapped` flag; **not sound** |
| Main `subset` / `--sound` | Operate only on main index |
| Expanded-only symbols | May appear under `--with-macro`; **never** `subset_ok: true` |
| Nested expanded root | **Rejected** (R26/R27) |
| Stale sidecar | Warn + union + `stale:true`; `macro rebuild` repairs |
| Fingerprint missing (pre-M1 sidecar) | `stale=false`; rebuild upgrades meta |

---

## MCP

Tools `callers` / `impact` accept `with_macro` (default `false`) and `no_macro_dedup` (default `false`).  
Tool `macro_status` returns the full M1 status JSON.  
Tool `macro rebuild` counterpart: `macro_rebuild`.  
Same mutual exclusion: `sound` + `with_macro` is an error.  
Wrapped payload when sidecar present (same shape as CLI).

---

## Example (fixture-shaped)

```bash
# sibling expanded tree with extra fn fmt / fn clone that call helper
agentgraph --root /tmp/app index --force
agentgraph --root /tmp/app index --force --macro-expanded-root /tmp/app-expanded
agentgraph --root /tmp/app macro status
agentgraph --root /tmp/app callers helper              # no fmt/clone
agentgraph --root /tmp/app callers helper --with-macro # fmt/clone tagged; dups merged
agentgraph --root /tmp/app macro rebuild               # after main source edits
```

Operator stock expand trees live outside this repo (e.g. `eval-corpus/.../real-expanded/`). **Never commit them.** Product tests use synthetic fixtures only.

---

## Why dual-index (not merge into main)

The spike ([eval-macro-expand.md](eval-macro-expand.md)) showed expanded trees mint symbols that **do not exist in source** (`Debug::fmt`, derive `Clone`, …), inflate Heuristic counts, and can inject `unsafe impl` → S violations. M1 maps + de-dups for **query UX**, but keeps uncertainty visible (`origin=macro_expanded`, `stale`, non-sound) instead of laundering expand edges into the main graph or `--sound`.

---

## Tests

- `tests/macro_pathmap.rs` — map table (prefix, crate align, Windows abs, `../`, explicit pairs, unmappable)
- `tests/macro_dedup.rs` — full §1.4 table + `--no-macro-dedup` + status fields
- `tests/macro_rebuild.rs` — fingerprint/stale/rebuild idempotence + nested still rejects
- `tests/macro_sidecar.rs` — absent-sidecar grace; union tagging; no subset_ok
- `tests/r26_adversarial.rs` / `r27` / `r28` — nesting, relative roots, MCP schema, help text
- `tests/graph_html.rs` — MACRO badge + mapped source path
- `tests/e2e_cli.rs` — flag matrix; sound+with_macro still fails

Gates: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
