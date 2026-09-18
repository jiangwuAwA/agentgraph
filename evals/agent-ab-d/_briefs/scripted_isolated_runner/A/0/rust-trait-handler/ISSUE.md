# Issue brief — rust-trait-handler

- task: `rust-trait-handler`
- title: Change Handler::render trait method — impls + dyn call sites
- runner_id: `scripted_isolated_runner`
- arm: **A**
- seed: `0`
- language: rust
- symbol token (from issue): `render`
- tools allowed: agentgraph MCP/CLI recipes + reads

## Issue

Handler::render signature must change. Find implementors and dyn dispatch call sites that must be updated.

## Decision-path constraints (blind)

- Work only inside this cell's `workdir/` (isolated copy; no structure-fact metadata).
- Identify the set of source files that must be reviewed for the issue.
- Write `file_set.json` and `meta.json` into **this** directory (the brief dir).
- Do **not** read structure-fact labels, scored tables, or other arms'/runners' outputs.
- Do **not** share intermediate file-set files across arms or seeds.
- Arm A may invoke agentgraph recipes; arm B must not.

## Outputs (contract)

```
file_set.json  — schema agentgraph.eval_agent_ab_d.file_set.v1
meta.json      — schema agentgraph.eval_agent_ab_d.meta.v1
```

Harness never injects answer-key lists into brief packs. Scores are stamped offline after commitment.
