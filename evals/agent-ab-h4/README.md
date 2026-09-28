# H4-live trajectories (public)

Isolated live LLM cells on **easy + hard** public tasks.

## Matrix

- 2 live runners (`xiaomi/mimo-v2.6-pro` / `xiaomi/mimo-v2.6-flash`)
- arms A/B, seeds 0–4, **9 tasks** (4 easy + 5 hard incl. `ts-dense-alias-noise`)
- Target N≥5 / cell — real N labeled (31/36 cells at N=5)

## Honesty

- Live A/B **noise not separated**. Primary: **recall + cwr**.
- `lab_ready=false` while any cell <5.
- Historical P0-5c/d scores **not** rewritten.
