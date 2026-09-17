# Macro-expanded sidecar index (P2, optional — CLI default OFF)

**Status:** shipped as an **opt-in dual-index**. Not a sound expand-graph.  
**Default product path remains source L0/L1** (and L2 `--sound` on the main index only).  
Related spike: [eval-macro-expand.md](eval-macro-expand.md). Soundness: [sound-subset.md](sound-subset.md).

---

## Non-goals

- **No sound expand-graph.** Expanded-only symbols never receive `subset_ok=true` claims.
- **No required CI expand.** Sidecar build is operator-driven; tests only lock the optional UX.
- **No path-rewrite magic.** Sidecar rows are unioned with `"origin": "macro_expanded"` — no silent path mapping back to source crates.
- **Do not edit operator corpora** (e.g. `eval-corpus/stock-trading-app`) from this feature.

---

## What it is

A **second SQLite store** at:

```text
<root>/.agentgraph/index.macro.db
```

Built by indexing an **expanded shadow tree** (e.g. `cargo expand` / `rustc -Zunpretty=expanded` output written to a sibling directory). Schema matches the main store (`files` / `symbols` / `refs` / `meta`); `meta.origin = 'macro_expanded'` marks the sidecar kind.

When the sidecar file is **absent**:

- All default queries behave **exactly** as before.
- `--with-macro` treats the sidecar as **empty** (graceful union; no error; file is not created).
- `macro status` reports `exists: false` without creating the DB.

---

## UX / commands

### Build sidecar (does not replace main index)

```bash
agentgraph index --force
agentgraph index --force --macro-expanded-root /path/to/expanded-shadow
```

With `--macro-expanded-root`, the CLI still indexes the **main** source tree first, then indexes the expanded tree into the sidecar. JSON shape:

```json
{
  "main": { "files": 1, "symbols": 3, "references": 2, "...": "IndexStats" },
  "macro_sidecar": {
    "files": 1,
    "symbols": 5,
    "references": 4,
    "path": "<root>/.agentgraph/index.macro.db",
    "expanded_root": "/path/to/expanded-shadow",
    "origin": "macro_expanded"
  },
  "note": "sidecar is optional dual-index (not sound); default callers/impact ignore it unless --with-macro"
}
```

**Keep the expanded tree outside the indexed project root** (sibling directory).

**Hard reject (R26):** `index --macro-expanded-root` fails closed when the expanded root equals `--root`, is **under** `--root`, or **contains** `--root`. Nested expanded trees are ingested by the main walker (graph pollution + possible main `subset_ok` flip from expanded `unsafe`/parse errors). Validation runs **before** the main reindex so a rejected command cannot dirty `index.db`. Use a sibling path (e.g. `/tmp/app` + `/tmp/app-expanded`).

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
  "subset_violation_count": 2
}
```

**Staleness:** sidecar rows are a snapshot. If the expanded tree is deleted or moved after build, `--with-macro` still unions the old rows (dual-index noise; no crash). `macro status` sets `expanded_root_missing: true` when the recorded `expanded_root` path no longer exists — rebuild or delete `<root>/.agentgraph/index.macro.db` to clear.

**Nesting re-validation (R27):** `macro status` re-checks nesting on every call. If the recorded `expanded_root` still exists but now canonicalizes equal / under / containing `--root` (dir move + junction, or equivalent), status sets `expanded_root_nested: true`. That is a **main-walker pollution hazard** — do not run a plain `index --force` until the shadow is outside `--root` again. Index-time validation of `--macro-expanded-root` remains the hard reject.

**Relative expanded roots (R27):** non-absolute `--macro-expanded-root` values are resolved against **`--root`**, not the process cwd. `expand-shadow` means `<root>/expand-shadow` (nested → rejected); use an absolute sibling path (or `../app-expanded` relative to `--root`) for dual-index targets.

**Sidecar S honesty (R27):** `subset_violation_count` is the S-violation count **inside the sidecar store only** (expanded `unsafe` / `eval` / parse_error, …). It does **not** claim or flip main `subset_ok`. Main `subset` continues to operate only on `index.db`. There is still no sidecar-only `subset` walk command.

### Query union (default OFF)

```bash
agentgraph callers helper                 # source L0/L1 only — unchanged
agentgraph callers helper --with-macro    # main ∪ sidecar; sidecar rows tagged
agentgraph impact helper --with-macro --depth 2
```

`--limit` / tool `limit` applies **per store**. The union may return up to **~2N** rows (main ≤ N + sidecar ≤ N). Budget agent context accordingly.

Sidecar hit tagging (callers):

```json
{
  "name": "helper",
  "enclosing": "fmt",
  "path": "src/core.rs",
  "origin": "macro_expanded",
  "at": "src/core.rs:15",
  "...": "other ReferenceRecord fields"
}
```

Sidecar hit tagging (impact): `origin=macro_expanded` **and** `at=path:line` (same location field as callers). MCP `impact` rows always carry `at` (with or without `with_macro`) so schema does not flip on the flag — mirrors callers.

Main-index rows are **not** tagged with `origin`. Expect **dual-index noise**: the same logical call can appear twice (once from source, once from expanded) with different paths/lines.

### Concurrent watch + sidecar index

SQLite opens both stores with `journal_mode=WAL` + `busy_timeout=5000`. A live `watch` on the main index and a concurrent `index --macro-expanded-root` (main write + sidecar write) serialize on locks; the main store remains queryable. If a writer holds the DB longer than 5s, the other process fails closed with a busy error — retry — it does not corrupt `index.db`. `tests/r28_adversarial.rs` locks this contract.

### Honesty gates

| Rule | Behavior |
|---|---|
| Default callers/impact | Never read the sidecar |
| `--with-macro` + missing sidecar | Empty union, success |
| `--sound --with-macro` | **Rejected** (mutually exclusive) |
| Main `subset` / `--sound` | Operates only on main index; expanded S-violations stay in the sidecar |
| Expanded-only symbols | May appear under `--with-macro` as ordinary array rows with `origin`; **never** as `subset_ok: true` |
| Expanded root nested with `--root` | **Rejected** (equal / under / contains) before main reindex |
| Relative `--macro-expanded-root` | Resolved against `--root` (not cwd); nested relatives reject |
| Deleted expanded tree | Sidecar rows still union; `macro status.expanded_root_missing=true` |
| Expanded root later nested (move/junction) | `macro status.expanded_root_nested=true` (re-validated on status) |
| Expanded tree has S violations | `macro status.subset_violation_count` (sidecar only; main subset unchanged) |

---

## MCP (optional, same defaults)

Tools `callers` / `impact` accept `with_macro: boolean` (default `false`).  
Tool `macro_status` reports sidecar existence + counts.  
Same mutual exclusion: `sound` + `with_macro` is an error.

---

## Example (fixture-shaped)

```bash
# sibling expanded tree with extra fn fmt / fn clone that call helper
agentgraph --root /tmp/app index --force
agentgraph --root /tmp/app index --force --macro-expanded-root /tmp/app-expanded
agentgraph --root /tmp/app macro status
agentgraph --root /tmp/app callers helper              # no fmt/clone
agentgraph --root /tmp/app callers helper --with-macro # fmt/clone tagged origin=macro_expanded
```

---

## Why dual-index (not merge)

The spike ([eval-macro-expand.md](eval-macro-expand.md)) showed:

- Expanded trees mint symbols that **do not exist in source** (`Debug::fmt`, derive `Clone`, …).
- Heuristic ref counts inflate on expand; paths diverge from source crates.
- Expanded output can inject `unsafe impl` → S violations even when the source crate is clean.

Merging would require a **sound path map + de-dup + subset story** that we do not have. The sidecar keeps that uncertainty **visible** (`origin=macro_expanded`) instead of laundering it into the main graph.

---

## Tests

`tests/macro_sidecar.rs` locks:

1. Default index/callers: no sidecar file; `--with-macro` graceful when absent.
2. Expanded-only `fmt`/`clone` appear only under `--with-macro`, tagged.
3. `macro status` after build reports path + counts + `origin`.
4. Main `index.db` ref/symbol counts unchanged after sidecar build.
5. No `subset_ok` claim from expanded-only content; `--sound --with-macro` fails closed.

`tests/r26_adversarial.rs` also locks: nested expanded-root reject (no main pollution), spaces in sibling paths, inventory path allowlist, stale `expanded_root_missing`, MCP `with_macro`/`macro_status` schema.

`tests/r27_adversarial.rs` locks: project-relative expanded roots (cwd decoy reject), `expanded_root_nested` after junction, sidecar `subset_violation_count`, index without `--force` still validates+writes sidecar, impact `--with-macro` sidecar-only independent BFS, delete-sidecar no-resurrect, MCP callers `at` schema stability with/without `with_macro`.

`tests/r28_adversarial.rs` locks: inventory alias grammar (rename/`s!`, brace rename, `pub use`, nested `mod`, cfg_attr, wildcard, foreign use-list fail-closed), CLI `--with-macro` ~2N help text, MCP impact `origin`+`at` symmetry, rust-only `ast_modeled` promise, concurrent watch+sidecar index no-corruption, SCIP lint on inventory+nest fixture.

Gates: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
