# Issue brief — rust-cross-crate-blast

- task: `rust-cross-crate-blast`
- title: Cross-crate normalize_id blast — stay on true dependents
- runner_id: `scripted_isolated_runner`
- arm: **B**
- seed: `3`
- language: rust
- symbol token (from issue): `normalize_id`
- tools allowed: walk / grep / read only (no agentgraph)

## Issue

core::normalize_id contract must change (trim + case rules). Using the multi-root workspace, find the true blast radius across packages. Prefer scoped queries on the definition root and true consumers; do not pull unrelated packages that merely share the symbol name or mention it in comments.

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
