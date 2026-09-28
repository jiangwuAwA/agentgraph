# Issue brief — go-store-api

- task: `go-store-api`
- title: Change Go Repository.Get — consumers and implementors
- runner_id: `external_live_runner_1`
- arm: **A**
- seed: `3`
- language: go
- symbol token (from issue): `Get`
- tools allowed: agentgraph MCP/CLI recipes + reads

## Issue

API change: Repository.Get must accept a context. Find implementors (MemRepo/RedisCache Get) and call sites (api handlers) that must be updated; ignore package noise that does not use these methods.

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
