# Public P0-5d isolated-lab artifacts

Replay + isolation harness for
[docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md) § **P0-5d**.

## Status (S2)

- **Isolated live matrix complete.** `lab_ready=**true**`: `mimo-pro` +
  `mimo-flash` × arms A/B × N=5 × easy+hard, `independent_session=true`,
  non-author `model_note`.
- **Non-claim:** This is **not** ecosystem sound. It is **not** a claim that
  live arm A always beats live arm B on every corpus.
- Offline fill (`scripted_isolated_runner`) is protocol parity only —
  **not** live LLM evidence.
- Product-superiority claims remain **out of scope**; cite recorded
  trajectories + honest score table in
  [docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md).

## Task set (easy≥4 + hard≥4)

| tier | task ids |
|---|---|
| easy | `ts-nest-user-repo`, `rust-trait-handler`, `py-plugin-registry`, `go-store-api` |
| hard | `rust-cross-crate-blast`, `rust-real-noise-dense`, `rust-sound-scoped-clean`, `ts-multi-root-client` |

Selection rationale: [`task_selection.json`](task_selection.json).
Randomization order: [`task_randomization.json`](task_randomization.json).

## Isolation layout

```text
evals/agent-ab-d/
  task_selection.json
  task_randomization.json
  _briefs/<runner_id>/<arm>/<seed>/<task_id>/
    ISSUE.md              # issue text only — no structure-fact labels
    RUNNER_CONTRACT.md
    workdir/              # isolated fixture copy (no task.json); gitignored
    file_set.json         # runner writes (after live/scripted run)
    meta.json             # runner honesty fields
  <task_id>/
    run-<runner_id>-<arm>-<seed>.json   # harness stamp output (after run)
```

Brief packs must **not** contain structure-fact label keys or golden lists.
The decision path cannot read other arms' file sets or scores.

## Runner contract (summary)

1. `python scripts/eval_agent_ab_d.py prepare`
2. External runner executes one **independent session** per cell using
   `ISSUE.md` + `workdir/` only.
3. Runner writes `file_set.json` + `meta.json` into the brief dir.
4. `python scripts/eval_agent_ab_d.py stamp --traj-dir evals/agent-ab-d`
5. `python scripts/eval_agent_ab_d.py score --traj-dir evals/agent-ab-d`
6. `python scripts/eval_agent_ab_d.py lab-ready --traj-dir evals/agent-ab-d`

Harness **never injects goldens**. Offline stamp uses fixture `task.json`.

## Metrics (aligned P0-5c)

`recall` · `extra-noise` · `cwr` (`chose_correct_workspace_root`) ·
`mcp_or_cli_calls` · `file_budget` · `read_budget` · `approx_tokens` (null-allowed)

## Runner ids (recorded)

| runner_id | kind | status |
|---|---|---|
| `scripted_isolated_runner` | scripted offline | protocol parity / **not** live LLM |
| `external_live_runner_1` | live (`xiaomi/mimo-pro`) | **done** N=5 A/B |
| `external_live_runner_2` | live (`xiaomi/mimo-flash`) | **done** N=5 A/B |

Seeds recorded: 0, 1, 2, 3, 4 (N=5).

## Offline replay

```bash
python scripts/eval_agent_ab_d.py score --traj-dir evals/agent-ab-d
python scripts/eval_agent_ab_d.py lab-ready --traj-dir evals/agent-ab-d
cargo test --test agent_ab_d_eval
```

Schema: `agentgraph.eval_agent_ab.trajectory.v1` (alias `agentgraph.eval_agent_ab.d.v1`).

## Honesty (mandatory)

- Scripted offline runner is **not** a live LLM / multi-model lab.
- Incomplete matrix ⇒ `lab_ready=false` + gap list. Never invent cells.
- Author-session models (e.g. host-session fixture author) are **not**
  lab-eligible and force `lab_ready=false`.
- No private corpus paths.
- `lab_ready=true` means **complete isolated matrix + isolation protocol**.
  It does **not** mean ecosystem sound, or that MCP beats grep on every corpus.
