# Runner contract — P0-5d isolated cell

- cell: `scripted_isolated_runner` / arm `B` / seed `4` / task `rust-sound-scoped-clean`
- harness: `agentgraph.eval_agent_ab_d.harness.v1`
- independent session required: **true** for lab-grade live runners
- decision path must not see structure-fact labels or other cells' outputs

## You must write

### `file_set.json`

```json
{
  "schema": "agentgraph.eval_agent_ab_d.file_set.v1",
  "task_id": "rust-sound-scoped-clean",
  "runner_id": "scripted_isolated_runner",
  "arm": "B",
  "seed": 4,
  "file_set": ["path/relative/to/workdir.ts"],
  "tool_calls": [
    {"tool": "read", "args": ["src/x.ts"], "ok": true, "summary": {}, "note": ""}
  ],
  "chose_correct_workspace_root": null,
  "approx_tokens": null
}
```

### `meta.json`

```json
{
  "schema": "agentgraph.eval_agent_ab_d.meta.v1",
  "runner_id": "scripted_isolated_runner",
  "kind": "live_llm_agent",
  "model_note": "<public-model-id-or-operator-note>",
  "independent_session": true,
  "harness_version": "agentgraph.eval_agent_ab_d.harness.v1",
  "saw_labels_before_commit": false,
  "arm_isolated": true,
  "read_budget": null,
  "approx_tokens": null,
  "lab_ready_claim": false
}
```

## Rules

- `approx_tokens`: real runner value or `null` — never invented.
- `saw_labels_before_commit`: must be `false` on a clean decision path.
- `lab_ready_claim`: leave `false`; the harness computes lab-ready from the matrix.
- Scripted offline fills must set `kind=scripted_external_runner` and a non-live `model_note`.
- Harness `stamp` scores offline from fixture metadata after you commit `file_set.json`.
