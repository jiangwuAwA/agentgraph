# Agent golden suites (P1-3)

**Status:** release-gate goldens under
[`fixtures/eval-agent-goldens/`](../fixtures/eval-agent-goldens/), enforced by
`tests/agent_goldens.rs` via `cargo test`.

**Purpose:** lock recipe / window / honesty **key shapes** so regressions do not
depend on human memory. Same tier as `docs_claims` for agent-facing payloads.

**What is locked**

| area | fixture / case | lock |
|---|---|---|
| blast_radius `window` | `clean-ts` vs `dirty-eval-js` | `sound` when `subset_ok`; `default` when dirty; never `recall` |
| dirty multi-root recommendation | `dirty-multi-root` | contains clean root + `impact <sym> --sound --workspace-root <id>`; never `window=sound` on union |
| scoped clean root | same workspace, root filter | `window=sound` allowed on clean root only |
| `edge_role` callers/registration | `ts-router-reg`, `ts-nest-reg` | registration rows tagged `registration` |
| who_calls implementors split | `rust-trait`, `go-iface` | default separates implementors; `--noisy` merges |
| high_freq demote | `rust-fmt-flood` | `high_freq_name=true` + implementor cap/truncate |
| honesty `note` | all recipe cases | always `not a complete runtime graph`; no oversell phrases |

Recommendation **text** is compared with contains/regex (flexible). Stable keys
are 勿改名 — see [agent-recipes.md](agent-recipes.md).

**Non-claim:** goldens prove key-shape + honesty regression on **public
synthetic** fixtures. They do **not** claim production monorepo precision.
This is **not** ecosystem sound and **not** a complete runtime graph.

**Reproduce**

```bash
cargo test --test agent_goldens
python scripts/check_docs_claims.py
```

Related: [eval-agent-tasks.md](eval-agent-tasks.md) (public task scores),
[eval-query-p95.md](eval-query-p95.md) (workspace perf budgets),
[workspace.md](workspace.md).
