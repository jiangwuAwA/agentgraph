# Issue brief — rust-real-noise-dense

- task: `rust-real-noise-dense`
- title: Dense Encode noise — separate real call sites from collisions
- runner_id: `scripted_isolated_runner`
- arm: **B**
- seed: `3`
- language: rust
- symbol token (from issue): `encode`
- tools allowed: walk / grep / read only (no agentgraph)

## Issue

Encode::encode payload format must change. Dense implementors exist alongside unrelated name collisions (metrics Encode enum, encode_label, legacy encode, clone helpers). Identify true structure files for the trait + real callers; exclude collision/noise crates. high-freq implementor demotion may apply.

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
