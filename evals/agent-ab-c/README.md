# Public P0-5c multi-runner A/B trajectories (hard tasks)

Replay fixtures for the **P0-5c** section of
[docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md).

Hard public tasks live under
[`fixtures/eval-agent-tasks-hard/`](../../fixtures/eval-agent-tasks-hard/).

## Honesty (mandatory)

- **≥2 runner kinds** recorded in this directory:
  - `host_session_llm` — live host-session decisions
    (`model_note=mimo-desktop-host-session`). **Not** a public benchmark model,
    **not** a multi-model lab. Same session authored hard fixtures ⇒
    `independent_session=false` (contamination disclosed). Decision path uses
    tool/grep evidence (`saw_labels_before_commit=false`).
  - `scripted_external_runner` — deterministic P0-5 policy A/B invoked as an
    external runner on hard fixtures. **Not** a live LLM.
    `independent_session=true` on the decision path (script does not read
    `expected` when assembling file sets).
- File sets committed **before** structure-fact stamp; scores stamped offline
  from `task.json`.
- **No fabricated multi-model lab numbers. No oversell that live A beats B.**
  Recorded hard-task numbers are structure-fact metrics on public synthetic
  fixtures only.
- **No private corpus.**
- `approx_tokens` is **null** unless a runner reports it — never invented.
- Task-level randomization order recorded in
  [`task_randomization.json`](task_randomization.json).

## Layout

```text
evals/agent-ab-c/<task_id>/
  run-host_session_llm-{a|b}-<seed>.json
  run-scripted_external_runner-{a|b}-<seed>.json
evals/agent-ab-c/task_randomization.json
```

Schema: `agentgraph.eval_agent_ab.trajectory.v1`
(alias: `agentgraph.eval_agent_ab.c.v1`).

| arm | tools |
|---|---|
| A | agentgraph index / blast-radius / who-calls / find / related / subset + read |
| B | walk / grep / read — **no agentgraph** |

Extended metrics per run: `mcp_or_cli_calls`, `chose_correct_workspace_root`,
`file_budget`, `read_budget`, `approx_tokens`, `runner_id`, `model_note`,
`independent_session`, `saw_labels_before_commit`.

## Offline replay

```bash
python scripts/eval_agent_ab_c.py score --traj-dir evals/agent-ab-c
cargo test --test agent_ab_c_eval
```

P0-5b (easier public tasks, single host-session) remains under
[`evals/agent-ab-live/`](../agent-ab-live/) — separate slice, separate honesty
limits.
