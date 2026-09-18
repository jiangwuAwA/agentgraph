# fixtures/eval-agent-goldens — P1-3 Golden Agent Suites (release gate)

**Status:** shipped as **public synthetic** recipe/window/honesty goldens.
Locked by `tests/agent_goldens.rs` (CI `cargo test`).

**Non-claim / Honesty:**
- `window=sound` here is the **`ast_modeled` engineering S gate** only —
  not ecosystem sound, not production sound.
- Recipe payloads are **not a complete runtime graph**.
- `edge_role` tags are **query-time presentation**, not runtime proof.
- Recommendation prose is locked via **contains / regex substrings** (flexible
  text); only **stable keys** are rename-frozen (勿改名 — see
  [agent-recipes.md](../../docs/agent-recipes.md)).
- Public synthetic fixtures only — **no private stock corpus**.

**Reproduce:**

```bash
cargo test --test agent_goldens
# optional: unit-style recipe builders + CLI e2e already covered by
cargo test --test agent_recipes
```

## Layout

| path | purpose |
|---|---|
| `goldens.json` | expected key-shape locks (window / edge_role / recommendation / note / who_calls) |
| `clean-ts/` | clean TS → blast_radius `window=sound` |
| `dirty-eval-js/` | eval → blast_radius `window=default`, honest recommendation |
| `dirty-multi-root/` | dirty union + clean sibling root → scoped `impact … --sound --workspace-root api` |
| `rust-trait/` | who_calls implementors split + `edge_role=implementor` |
| `rust-fmt-flood/` | `high_freq_name=true` + implementor cap / truncate |
| `ts-router-reg/` | Express-style registration → `edge_role=registration` |
| `ts-nest-reg/` | Nest-like providers/controllers → `edge_role=registration` |
| `go-iface/` | Go interface method → who_calls honesty + high-freq `Get` |

## What is locked (stable keys 勿改名)

blast_radius / recipes payloads must always carry:

`window`, `promise_tier`, `subset_ok`, `recommendation`, `note`,
`sound_candidates` (plus `example_command` / `by_root` / `by_top_dir` when
scoped guidance applies).

who_calls payloads must always carry:

`tool`, `symbol`, `noisy`, `window`, `subset_ok`, `promise_tier`,
`high_freq_name`, `callers`, `implementors`, `implementor_count`,
`recommendation`, `note`.

`note` must always contain `not a complete runtime graph` and must **not**
oversell (zero-miss / ecosystem sound / production sound / macro-complete).

Dirty multi-root recommendation must name the **clean root** and an example
scoped command (`impact <sym> --sound --workspace-root <id>`) — never
`window=sound` on a dirty union, never blind `--recall` as the default step.

## Related

- [docs/agent-goldens.md](../../docs/agent-goldens.md) — short pointer
- [docs/eval-agent-tasks.md](../../docs/eval-agent-tasks.md) — public task evals
- [docs/agent-recipes.md](../../docs/agent-recipes.md) — stable key table
- [docs/eval-query-p95.md](../../docs/eval-query-p95.md) — workspace perf budgets
