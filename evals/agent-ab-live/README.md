# Public live Agent A/B trajectories (P0-5b)

Replay fixtures for the **P0-5b live** section of
[docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md).

**P0-5c multi-runner hard-task trajectories** (independent-session protocol,
≥2 runner kinds, extended metrics) live under
[`evals/agent-ab-c/`](../agent-ab-c/) — **not** in this directory. P0-5b rows
remain single host-session on the easier P0-1 public tasks.

**Honesty (mandatory):**

- Trajectories were produced by a **live host-session LLM agent**
  (`model_note=mimo-desktop-host-session`) — **not** a public benchmark model
  id, **not** a standardized lab harness.
- File sets were decided from **issue text + recorded tool/grep evidence**;
  structure-fact scores were **stamped offline after commitment** from
  `fixtures/eval-agent-tasks/*/task.json`.
- **Contamination risk is real:** the same session previously inspected fixture
  `expected` labels during P0-5 protocol work. Arms derive sets from tool
  evidence rather than by copying expected lists, but this is **not** a clean
  lab isolation.
- Same-session arm exposure: evidence order was Arm A tools → Arm B greps per
  task (not per-seed randomization). Not independent lab trials.
- Public fixtures only — no private corpus. **No oversell.**

## Layout

```text
evals/agent-ab-live/<task_id>/
  run-a-<seed>.json   # live Arm A — agentgraph recipes + reads
  run-b-<seed>.json   # live Arm B — grep/read only (no agentgraph)
```

Schema: `agentgraph.eval_agent_ab.trajectory.v1`
(documented live alias: `agentgraph.eval_agent_ab.live.v1` via `schema_alias`).

| arm | kind | tools |
|---|---|---|
| A | `live_llm_agent` | agentgraph index / blast-radius / who-calls / find / related / subset + read |
| B | `live_llm_agent` | walk / grep / read — **no agentgraph** |

## Offline replay

```bash
# shared scorer (trajectory.v1)
python scripts/eval_agent_ab.py score --traj-dir evals/agent-ab-live

# live-specific summary (arms / model_note)
python scripts/eval_agent_ab_live.py score --traj-dir evals/agent-ab-live

# cargo gate
cargo test --test agent_ab_live
```

Tasks reuse [`fixtures/eval-agent-tasks/`](../../fixtures/eval-agent-tasks/)
(read-only). agentgraph may write gitignored `.agentgraph/` index DBs when
tools are exercised; trajectories store **fixture-relative** paths only.
