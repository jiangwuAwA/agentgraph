# Public scripted A/B/C trajectories (P0-5)

Replay fixtures for [docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md).

**Honesty:** trajectories are produced by **scripted deterministic
tool-policy agents**, not live LLM agents. Public fixtures only — no private
corpus. Offline score:

```bash
python scripts/eval_agent_ab.py score --traj-dir evals/agent-ab
cargo test --test agent_ab_eval
```

## Layout

```text
evals/agent-ab/<task_id>/
  run-a-<seed>.json   # policy A — agentgraph MCP/CLI recipe policy (scripted)
  run-b-<seed>.json   # policy B — read/grep policy, no agentgraph (scripted)
  run-c-<seed>.json   # policy C — name-grep control (P0-1)
```

Schema: `agentgraph.eval_agent_ab.trajectory.v1`.

| policy | kind | tools |
|---|---|---|
| A | `scripted_tool_policy` | index + blast-radius / who-calls / find / related / subset |
| B | `scripted_read_grep_policy` | walk + grep + bounded reads + same-dir + secondary tokens |
| C | `name_grep_control` | symbol token in source files |

Tasks reuse [`fixtures/eval-agent-tasks/`](../../fixtures/eval-agent-tasks/) (read-only).
