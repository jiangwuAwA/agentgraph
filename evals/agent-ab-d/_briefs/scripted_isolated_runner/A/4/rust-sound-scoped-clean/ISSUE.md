# Issue brief — rust-sound-scoped-clean

- task: `rust-sound-scoped-clean`
- title: Sound-disabled dirty sibling — scoped clean batch_write
- runner_id: `scripted_isolated_runner`
- arm: **A**
- seed: `4`
- language: rust
- symbol token (from issue): `batch_write`
- tools allowed: agentgraph MCP/CLI recipes + reads

## Issue

clean::batch_write is under change. The workspace also has a dirty package with unsafe code that disables union sound. Do not claim union sound; recommend scoped sound on the clean root. Review set must stay on true clean dependents — dirty name collisions are noise for this symbol.

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
