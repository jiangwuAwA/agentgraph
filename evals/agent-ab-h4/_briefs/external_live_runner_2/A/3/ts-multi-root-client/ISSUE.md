# Issue brief — ts-multi-root-client

- task: `ts-multi-root-client`
- title: Multi-root RegistryClient — pick true roots, drop docs/help noise
- runner_id: `external_live_runner_2`
- arm: **A**
- seed: `3`
- language: typescript
- symbol token (from issue): `RegistryClient`
- tools allowed: agentgraph MCP/CLI recipes + reads

## Issue

RegistryClient.fetch return contract must change. Multi-root workspace: registry defines the client; service consumes it. cli/docs only mention the name in help/prose. Using workspace index, choose the correct roots for review; do not treat name mentions as dependents.

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
