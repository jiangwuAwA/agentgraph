# Issue brief — ts-nest-user-repo

- task: `ts-nest-user-repo`
- title: Change UserRepository contract in a Nest-like DI app
- runner_id: `external_live_runner_2`
- arm: **B**
- seed: `0`
- language: typescript
- symbol token (from issue): `UserRepository`
- tools allowed: walk / grep / read only (no agentgraph)

## Issue

Product wants UserRepository.find to return null instead of throwing when a user is missing. Before editing, identify the blast radius: which files must be reviewed, and which files are noise that mention unrelated concerns.

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
