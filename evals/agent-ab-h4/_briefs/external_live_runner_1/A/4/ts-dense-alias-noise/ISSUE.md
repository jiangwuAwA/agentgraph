# Issue brief — ts-dense-alias-noise

- task: `ts-dense-alias-noise`
- title: Dense same-name + cross-root alias — pick true OrderHandler
- runner_id: `external_live_runner_1`
- arm: **A**
- seed: `4`
- language: typescript
- symbol token (from issue): `OrderHandler`
- tools allowed: agentgraph MCP/CLI recipes + reads

## Issue

OrderHandler.execute contract must change in the core package. Many decoy execute() methods exist; legacy has a same-named class. App consumes core via package name. Choose true dependents; do not treat name collisions or help prose as dependents.

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
