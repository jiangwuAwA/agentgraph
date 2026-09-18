# Noise governance — callers vs implementor + high-frequency demotion

**Status:** shipped (L1 noise cut).  
**Non-claim / Honesty:** separation is **query-time presentation only**. The store
still keeps implementor / registration / dynamic edges. `impact` still expands them
(blast radius). L1 remains **not sound** — this cut does not delete edges or make
`callers` a complete runtime call graph.

**Reproduce:** `cargo test --test noise_roles`

---

## Why

Stock monorepo L1 lift was dominated by `rs.di.impl_trait` implementor edges stored
as `kind=call`. Default `callers fmt` / `drop` / `default` flooded with Display/Debug/
Drop implementors (75+ heuristic rows on `fmt` alone), drowning real Exact call sites.

M3 added `rs.di.dyn_trait_method` (Unsound Heuristic) which made the flood worse.
Operators need **callers** (who calls me) separated from **implementors** (who
implements this trait method / interface method).

---

## Design (locked)

### A. Edge role classification (query-time, no DB migration)

`edge_role` is derived from `confidence` + `evidence.rule_id`:

| role | rules / conditions |
|---|---|
| `implementor` | `rs.di.impl_trait`, `rs.di.dyn_trait_method`, `go.di.interface_impl`, `go.di.interface_impl_v2`, `go.di.interface_assert` |
| `registration` | Nest `ts.nest.*`, `ts.di.*`, `ts.framework.register`, `ts.event.subscribe/dispatch`, `py.di.*`, `py.framework.init_subclass`, `go.di.handler_map/route_register`, `rs.di.inventory_submit`, `rs.di.linkme_distributed_slice` |
| `dynamic` | any `DynamicCandidate` confidence |
| `call` | default Exact (and unknown Heuristic ids — still references, not implementors) |

Source of truth: `src/model.rs` (`edge_role_for`, mapping tables). Unit-tested in
`tests/noise_roles.rs`.

Callers / impact / HTML graph rows carry JSON field **`edge_role`**.

### B. Default `callers` payload shape

| condition | JSON shape |
|---|---|
| zero implementors | **plain array** (back-compat) with `edge_role` + `at` on each row |
| any implementor present | object `{callers, implementors, implementor_count, implementors_truncated, truncated, note}` |

- `callers[]` = Exact calls + registration (not mixed with implementor flood)
- `implementors[]` = trait/interface implementor candidates
- `--limit N` applies **per section** in Separate mode
- `--exact-only` stays pure Exact calls (plain array; no implementor section)
- `--include-implementors` merges all roles into one array (old noisy shape + tags)
- `--implementors-only` returns `{implementors, implementor_count, implementors_truncated, …}`
- `--include-implementors` + `--implementors-only` are mutually exclusive

CLI + MCP share the same payload builder (`src/query/mod.rs::build_callers_payload`).

### C. High-frequency name demotion

Constant list `HIGH_FREQ_NAMES` in `src/model.rs`:

`fmt, debug, clone, drop, default, eq, hash, new, into, from, as_ref, to_string, get, set, call, execute, run, handle`

For these names under default `callers`:

- implementors section capped at **20** (`HIGH_FREQ_IMPLEMENTOR_CAP`)
- payload sets `implementors_truncated: true` + `implementor_count` (full count)
- Exact user calls in `callers[]` are **never dropped**
- `--include-implementors` restores merge-all (limit applies to the merged list;
  high-freq demote does **not** apply when merging)

Configurable name list is **backlog** (not shipped).

### D. Impact / HTML graph

- `impact` still expands implementor edges (blast radius) but every node carries `edge_role`
- HTML graph badges: **IMP** implementor / **REG** registration / **DYN** dynamic
- Node/edge `data-edge-role` attributes; legend documents roles
- Confidence colors unchanged (role is an additional badge/stroke accent)

### E. Honesty

- Store keeps all edges — no silent deletion
- `impact --sound` / L2 sound walk unchanged (roles are presentation)
- `--sound` callers stay sound-eligible rows; sound is mutually exclusive with
  role-merge flags
- Do not treat default `callers <std-ish name>` as a call graph even after
  separation — implementors are still L1 candidates

---

## Stock sample numbers (operator private corpus)

Before (docs/eval-stock-boundary.md § Noise proxy):

| name | exact | heuristic (mostly impl_trait) |
|---|---:|---:|
| `fmt` | 11 | **75** |
| `drop` | 142 | **37** |
| `default` | 515* | **55** |

After this cut (same store; query-time only):

| name | payload shape | callers section | implementors section |
|---|---|---|---|
| `fmt` | wrapped object | Exact calls (role=call) | capped 20 + truncated (count=75) |
| `drop` | wrapped object | Exact calls | capped 20 + truncated |
| `default` | wrapped object | Exact calls | capped 20 + truncated |

\* Exact counts unchanged — we did not rewrite the store.

Reproduce (operator, private corpus):

```bash
agentgraph --root D:\projects\eval-corpus\stock-trading-app callers fmt
agentgraph --root D:\projects\eval-corpus\stock-trading-app callers fmt --exact-only
agentgraph --root D:\projects\eval-corpus\stock-trading-app callers fmt --include-implementors
```

---

## MCP

Tool `callers` accepts `include_implementors` / `implementors_only` with the same
shape as CLI. Tool descriptions document the wrapped object + `edge_role`.

---

## Non-goals

- No schema migration / `edge_role` column (query-time classification)
- No removal of L1 implementor edges from the store
- No change to L2 sound eligibility tables
- No automatic std-impl suppression beyond the high-freq implementor cap
- Not a complete runtime call graph (unchanged honesty line)

See also: [eval-stock-boundary.md](eval-stock-boundary.md), [sound-subset.md](sound-subset.md),
[graph-html.md](graph-html.md).
