# Issue brief — py-plugin-registry

- task: `py-plugin-registry`
- title: Change PluginRegistry.register — plugins + bootstrap
- runner_id: `external_live_runner_1`
- arm: **B**
- seed: `2`
- language: python
- symbol token (from issue): `PluginRegistry`
- tools allowed: walk / grep / read only (no agentgraph)

## Issue

PluginRegistry.register must validate plugin names. Find the files that actually construct or call the registry.

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
