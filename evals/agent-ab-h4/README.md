# H4-live hard-slice trajectories (public)

Isolated live LLM cells on **hard** public tasks including `ts-dense-alias-noise`.

## Runners (non-author)

| runner_id | model_note | kind | independent_session |
|---|---|---|---|
| `external_live_runner_1` | `xiaomi/mimo-v2.6-pro` | live_llm_agent | true |
| `external_live_runner_2` | `xiaomi/mimo-v2.6-flash` | live_llm_agent | true |

Each cell is a **fresh `mimo run` process**.

## Matrix (H4-live)

- Tasks (hard): `ts-dense-alias-noise`, `ts-multi-root-client`, `rust-real-noise-dense`, `rust-cross-crate-blast`, `rust-sound-scoped-clean`
- Arms: A (agentgraph tools) / B (read-grep)
- Target N≥5; **real recorded N is labeled** in the score table.

## Honesty

- Live A/B noise **not separated** (extra-noise ≈ 0 both arms). Primary: **recall** + **cwr**.
- `lab_ready=false` until matrix complete.
- Historical P0-5c/d scores are **not** rewritten.
