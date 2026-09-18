# Hard public Agent tasks (P0-5c)

Harder structure-fact fixtures for **multi-runner live A/B**对照 (P0-5c).

**Non-claims**

- Public synthetic fixtures only — **no private corpus**.
- Scores are **structure-fact** file sets — not production monorepo precision.
- `window=sound` is the **ast_modeled** engineering S gate — not ecosystem sound.
- Hard tasks are designed to stress cross-crate blast, real name noise, and
  sound-disabled scoping — **not** to prove live A “beats” B.

**Layout**

```text
fixtures/eval-agent-tasks-hard/<task_id>/
  task.json     # issue, symbol, workspace?, expected structure facts
  <mini-repo>   # public sources only
```

**Tasks (P0-5c hard slice)**

| task_id | stress | multi-root | sound notes |
|---|---|---|---|
| `rust-cross-crate-blast` | symbol used across package roots + unrelated name collision | yes | clean union possible |
| `rust-real-noise-dense` | dense implementors / fmt-like flood + clone noise | no | single crate |
| `rust-sound-scoped-clean` | sound-disabled dirty sibling + clean scoped root | yes | union not sound; scoped `clean` |
| `ts-multi-root-client` | multi-root TS; wrong-root temptation + docs/help noise | yes | clean roots |

See [docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md) **P0-5c**.
