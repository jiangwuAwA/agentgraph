# Public Agent code-change task evals (P0-1)

Reproducible **public** mini-repos + `task.json` structure facts for agent
code-change tasks (blast radius / avoid wrong-file edits).

**Non-claims**

- Public synthetic fixtures only — **no private stock / proprietary corpus**.
- Scores measure **structure facts** (expected files, noise files, honesty
  fields) on these fixtures — **not** production monorepo precision.
- `window=sound` on fixtures is the **ast_modeled** engineering S gate, not
  ecosystem sound / production sound / a complete runtime graph.
- The harness baseline is a **name-grep** file set (symbol token in files),
  **not** a fabricated LLM baseline.

**Layout**

```text
fixtures/eval-agent-tasks/<task_id>/
  task.json     # issue text, symbol, expected structure facts, recommended tools
  <mini-repo>   # sources only (public domain shape)
```

**Run**

```bash
python scripts/eval_agent_tasks.py
# optional: cargo test --test agent_task_eval
```

Results: `target/agent_task_eval.json` + table in
[docs/eval-agent-tasks.md](../../docs/eval-agent-tasks.md).

**P0-5 (scripted tool-policy A/B/C):** same fixtures reused read-only —
see [docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md) and
replay trajectories under [`evals/agent-ab/`](../../evals/agent-ab/)
(**not** live LLM agents).

**P0-5b (live host-session A/B):** live trajectories under
[`evals/agent-ab-live/`](../../evals/agent-ab-live/) — host-session LLM
(`mimo-desktop-host-session`), **not** a public benchmark lab. Honesty limits:
[docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md).

**P0-5c (multi-runner hard tasks):** harder fixtures under
[`fixtures/eval-agent-tasks-hard/`](../eval-agent-tasks-hard/) — cross-crate
blast, real-noise, sound-scoped sibling, multi-root wrong-root. Trajectories:
[`evals/agent-ab-c/`](../../evals/agent-ab-c/). Protocol:
[docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md) § P0-5c.
