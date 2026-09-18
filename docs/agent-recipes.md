# Agent recipes (agentgraph)

**Status:** operator/Agent cookbook.  
**Non-claim:** recipes are **honest query patterns**, not soundness guarantees.
Scoped `--sound` eligibility is per-root / per-path S gate only — not ecosystem
sound, not production sound, not a complete runtime graph.

Related: [onboarding.md](onboarding.md) (5-min install → MCP `blast_radius`),
[sound-subset.md](sound-subset.md), [workspace.md](workspace.md),
[graph-diff.md](graph-diff.md), [eval-stock-boundary.md](eval-stock-boundary.md),
[eval-agent-tasks.md](eval-agent-tasks.md) (public code-change task scores),
[agent-goldens.md](agent-goldens.md) (P1-3 golden suites — stable-key release gate),
[ci-blast-radius-demo.md](ci-blast-radius-demo.md) (P2-2 CI blast-radius comment **demo**).

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

### Stable keys (勿改名)

Stable helpers other tools may read — **do not rename** (勿改名):

**CLI/MCP payload keys** (subset / workspace status / blast_radius / recipes)
— names are **勿改名** / do not rename:

| Key | Where | Meaning |
|---|---|---|
| payload key `window` | blast_radius / graph | `sound` \| `default` \| `disabled` |
| payload key `promise_tier` | all honesty payloads | `ast_modeled` \| `lexical_v1` \| `mixed_lexical_v1` \| `disabled` |
| payload key `subset_ok` | blast_radius / sound queries | Selected store/root S gate |
| payload key `recommendation` | all recipes + subset | Next legal step / one-liner |
| payload key `note` | all recipes | Always “not a complete runtime graph” |
| payload key `sound_candidates` | subset / status / blast_radius | Scoped `--sound` candidates, eligible first |
| payload keys `by_root` / `by_top_dir` | subset / blast_radius | Per-scope buckets (workspace roots / single-root top dirs) |
| payload key `sound_eligible` | candidate rows | True only when that scope has 0 S violations |
| payload key `example_command` | blast_radius (default window) | e.g. `impact <sym> --sound --workspace-root <id>` |
| payload keys `baseline_stale` / `sidecar_stale` / `sidecar_exists` | status / stats / recipes / MCP defaults | Honesty flags (never auto-refresh baseline; never create sidecar). **P1-2:** present by default on MCP `stats`, `blast_radius`, `who_calls`, `subset`, `graph_diff`, `macro_status` |
| payload keys `root_id` / `root_path` | query rows | Workspace multi-root tagging |

**Rust helpers** (do not rename):

- `agentgraph::index::subset::{scoped_sound_by_root, scoped_sound_by_top_dir, aggregate_scoped_sound}`
- `agentgraph::query::recipes::{decide_blast_window, build_blast_radius_payload, build_who_calls_payload, build_scoped_sound_guidance, scoped_sound_aggregation}`
- Honesty flags on status/stats/diff: `baseline_stale`, `sidecar_exists`,
  `sidecar_stale`

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

**P1-2 MCP defaults:** the same flags appear on default (no extra flag) MCP
payloads for `stats`, `blast_radius`, `who_calls`, `subset`, `graph_diff`,
and `macro_status`. After a dirty `watch` / `index_paths`, `baseline_stale=true`
is visible without reading docs. When no sidecar was built,
`sidecar_exists=false` (never auto-created).

**P1-1 workspace incremental:** after editing one multi-root package, reindex
only that root — sibling file hashes/mtimes stay unchanged:

```text
agentgraph index --workspace-root ./packages/api --workspace-db ./ws.db
agentgraph watch --workspace-root ./packages/api --workspace-db ./ws.db
```

See [workspace.md](workspace.md).

---

## High-level blast radius / who-calls (P3 — sibling track)

MCP `blast_radius` / `who_calls` and CLI `agentgraph blast-radius` /
`agentgraph who-calls` auto-select a confidence window and always return
honesty fields (`window`, `subset_ok`, `promise_tier`, `recommendation`,
`note` = not a complete runtime graph; plus `sound_candidates`):

```text
agentgraph blast-radius <sym> --depth 3
agentgraph who-calls <sym>
```

| Behavior | Detail |
|---|---|
| Window | `subset_ok` → `window=sound` (S-qualified edges); else `window=default` (Exact+Heuristic) — **never** blind `--recall` |
| Default-window recommendation (P0-4) | Names **next legal commands**: `sound_candidates[]` (eligible roots/dirs first) + example `impact <sym> --sound --workspace-root <id>`; single-root → `by_top_dir` path hint; no eligible root → say so + default blast_radius + review implementors — **never** blind `--recall` as default; mentions `baseline_stale` / `sidecar_stale` when true |
| Dirty union | **Never** labeled `window=sound` (weakest root wins) |
| `who-calls` behavior | Default separates implementors + demotes high-freq names; `--noisy` merges |
| `include_macro` behavior | Only when sidecar exists && !stale && !nested; refused under sound window |
| `include_macro` P2-1 repo config | When CLI/MCP omit the flag, repo/project `macro_default` may auto-request: primary file `.agentgraph/config.toml` (fallback `agentgraph.toml`), field `macro_default = "off"\|"if_fresh"\|"on"`; env `AGENTGRAPH_MACRO_DEFAULT` overrides file. Success auto reason: `repo_config_if_fresh` / `repo_config_on`. **Global default remains OFF** — never claim global macro on. Explicit `--include-macro` / `--no-include-macro` (MCP `include_macro`) wins over config. Sound window still refuses. See [macro-sidecar.md](macro-sidecar.md). |
| Honesty | `note` always: not a complete runtime graph |

Helpers live in `agentgraph::query::recipes` (`decide_blast_window`,
`build_blast_radius_payload`, `build_who_calls_payload`,
`build_scoped_sound_guidance`, `scoped_sound_aggregation`). CLI + MCP share
this payload builder. Pair with P4 `sound_candidates` from `subset` when the
**global** window is disabled but a sibling root is clean.

Example dirty multi-root payload shape (keys stable / 勿改名):

```json
{
  "tool": "blast_radius",
  "window": "default",
  "subset_ok": false,
  "promise_tier": "disabled",
  "sound_candidates": [
    {"root_id": "api", "sound_eligible": true, "promise_tier": "ast_modeled", "reason": "…"},
    {"root_id": "nn-ranker", "sound_eligible": false, "promise_tier": "disabled", "reason": "…"}
  ],
  "example_command": "impact createUser --sound --workspace-root api",
  "recommendation": "sound disabled because …; scoped sound candidates (eligible first): api; avoid nn-ranker; e.g. `impact createUser --sound --workspace-root api` — do not claim union --sound on dirty roots",
  "note": "not a complete runtime graph (…)"
}
```

---

## MCP `graph` — HTML for Agents (no shell-out)

MCP tool **`graph`** returns the same self-contained HTML as CLI
`agentgraph graph`, plus machine honesty fields, without spawning a CLI:

```text
agentgraph graph <sym> --depth 3 --out graph.html   # CLI file write
# MCP tools/call graph:
#   { symbol, depth, direction, sound, with_macro, auto_window,
#     include_recommendation, out, workspace_db, root_id }
# payload:
#   { html, html_bytes, sha256, path, window, subset_ok, promise_tier,
#     recommendation, note, node_count, edge_count }
```

| Behavior | Detail |
|---|---|
| Default delivery | **String-only** — `html` in JSON, `path=null`. File write only when `out` is set and resolves under the workspace root jail |
| Window | `sound=true` + `subset_ok` → `window=sound`; `sound=true` + dirty S → `window=disabled` + honest disabled HTML + `recommendation` (never labeled OK sound) |
| `auto_window` flag | Reuses blast_radius decision: sound only when `subset_ok`; else `window=default` — never blind recall |
| Mutex | `sound` + `with_macro` rejected (also sound vs `exact_only` / `include_dynamic`) |
| P2-1 repo macro_default | Omitted `with_macro` resolves from repo config (`if_fresh` may auto-include a fresh sidecar; reason `repo_config_if_fresh`). Explicit `with_macro`/`include_macro` wins. Sound walk still refuses macro. Payload adds `include_macro` + `include_macro_reason`. Global default remains **OFF**. |
| Honesty | `note` always: not a complete runtime graph; `recommendation` when `include_recommendation` is true (default) |

Helpers: `agentgraph::viz::graph_tool` (`run_graph_html`,
`build_graph_html_payload`, `resolve_out_under_root`). See
[graph-html.md](graph-html.md).

## Other patterns

- **Find → callers → impact** on one workspace root: pass
  `--workspace-root <dir> --workspace-db <shared.db>` (or MCP `root_id`).
- **怕漏:** prefer scoped `--sound` on clean roots; else `--recall` /
  `--include-dynamic` for a wider heuristic window **as an explicit choice**,
  never as the recipe default. Never invent zero-miss.
- **Macro sidecar:** optional, per-root, not sound. `callers --with-macro`
  unions candidates only after `macro status` shows `sidecar_exists=true`.
  **P2-1:** a repo may set `.agentgraph/config.toml` `macro_default = "if_fresh"`
  so `blast_radius` / `graph` auto-paths include a **fresh** sidecar
  (`include_macro_reason=repo_config_if_fresh`). **Global default remains OFF**;
  env `AGENTGRAPH_MACRO_DEFAULT` overrides the file; explicit CLI flags win.
  Sound window still refuses macro. See [macro-sidecar.md](macro-sidecar.md).

---

## CI blast-radius comment demo (P2-2)

Demo-only pattern — **not** a required product-quality gate and **not** a
soundness claim. Full adaptation guide for monorepos:
[ci-blast-radius-demo.md](ci-blast-radius-demo.md).

| Piece | Path |
|---|---|
| Live workflow (this repo) | [`.github/workflows/blast-radius-demo.yml`](../.github/workflows/blast-radius-demo.yml) |
| Copyable template | [`examples/ci/blast-radius.yml`](../examples/ci/blast-radius.yml) |
| Markdown helper / local smoke | `scripts/ci_blast_radius_markdown.py` · `scripts/ci_blast_radius_demo.sh` |

```text
agentgraph --root <fixture> index --force
agentgraph --root <fixture> blast-radius <symbol> --depth 3
agentgraph --root <fixture> who-calls <symbol>
# markdown → $GITHUB_STEP_SUMMARY
# optional PR comment — soft-fail when github.token lacks write permission
```

Always echo `window` / `subset_ok` / `promise_tier` / `recommendation` /
`note` in the comment. Do **not** install expand tooling for this demo.
Do **not** promote a monorepo job to required without measuring noise on that
tree first.
