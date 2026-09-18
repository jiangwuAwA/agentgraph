# Agent recipes (agentgraph)

**Status:** operator/Agent cookbook.  
**Non-claim:** recipes are **honest query patterns**, not soundness guarantees.
Scoped `--sound` eligibility is per-root / per-path S gate only — not ecosystem
sound, not production sound, not a complete runtime graph.

Related: [sound-subset.md](sound-subset.md), [workspace.md](workspace.md),
[graph-diff.md](graph-diff.md), [eval-stock-boundary.md](eval-stock-boundary.md).

---

## One-click scoped sound (P4)

When a multi-root workspace is globally `promise_tier=disabled` because one
crate violates S, **do not** claim union `--sound`. Read machine-readable
candidates instead:

```text
agentgraph subset --workspace workspace.json
# payload.sound_candidates[] — eligible first
# payload.recommendation — e.g. scoped --sound on repository/event-engine/auth; avoid nn-ranker

agentgraph impact <symbol> --sound --workspace-root <eligible-root> --workspace workspace.json
```

Same fields on `agentgraph workspace status --workspace …` and MCP `subset` /
`workspace_status`.

Stable helpers other tools may read (do not rename):

- CLI/MCP payload keys: `sound_candidates`, `recommendation`, `by_root`,
  `by_top_dir`, `sound_eligible`, `promise_tier`
- Rust: `agentgraph::index::subset::{scoped_sound_by_root, scoped_sound_by_top_dir}`
- Honesty flags on status/stats/diff: `baseline_stale`, `sidecar_exists`,
  `sidecar_stale` (never auto-refresh baseline; never create sidecar)

Stock honesty: private corpus per-crate tables in
[eval-stock-boundary.md](eval-stock-boundary.md) / [eval-stock-s-map.md](eval-stock-s-map.md)
are **operator trial evidence**, not CI product claims.

---

## Watch / diff staleness (P5)

```text
agentgraph index                 # writes baseline; baseline_stale=false
# edit sources; agentgraph watch   # or index_paths on dirty files
agentgraph diff                  # payload.baseline_stale=true when dirty reindex ran
```

When `baseline_stale=true`, `diff` still compares live edges vs the last
**full-index** snapshot (watch drift is visible by design). Refresh with a
full `agentgraph index` or lock current edges via `diff --write-snapshot`.

`macro status` / `stats` / `workspace status` echo `sidecar_stale` /
`sidecar_exists` via a cheap meta read — they **never** build the sidecar.

---

## High-level blast radius / who-calls (P3 — sibling track)

MCP `blast_radius` / `who_calls` and CLI `agentgraph blast-radius` /
`agentgraph who-calls` auto-select a confidence window and always return
honesty fields (`window`, `subset_ok`, `promise_tier`, `recommendation`,
`note` = not a complete runtime graph):

```text
agentgraph blast-radius <sym> --depth 3
agentgraph who-calls <sym>
```

| Behavior | Detail |
|---|---|
| Window | `subset_ok` → `window=sound` (S-qualified edges); else `window=default` (Exact+Heuristic) — **never** blind `--recall` |
| `who-calls` | Default separates implementors + demotes high-freq names; `--noisy` merges |
| `include_macro` | Only when sidecar exists && !stale && !nested; refused under sound window |
| Honesty | `note` always: not a complete runtime graph |

Helpers live in `agentgraph::query::recipes` (`decide_blast_window`,
`build_blast_radius_payload`, `build_who_calls_payload`). Pair with P4
`sound_candidates` from `subset` when the **global** window is disabled but a
sibling root is clean.

## Other patterns

- **Find → callers → impact** on one workspace root: pass
  `--workspace-root <dir> --workspace-db <shared.db>` (or MCP `root_id`).
- **怕漏:** prefer scoped `--sound` on clean roots; else `--recall` /
  `--include-dynamic` for a wider heuristic window. Never invent zero-miss.
- **Macro sidecar:** optional, per-root, not sound. `callers --with-macro`
  unions candidates only after `macro status` shows `sidecar_exists=true`.
