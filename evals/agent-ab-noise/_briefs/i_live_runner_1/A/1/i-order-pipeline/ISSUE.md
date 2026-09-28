# Issue brief — i-order-pipeline

- task: `i-order-pipeline`
- runner_id: `i_live_runner_1`
- arm: **A**
- seed: `1`

## Issue (no golden list)

We are auditing every place that participates in order creation or card charging. List ALL source files that define, import, call, or document createOrder or chargeCard (including historical/legacy paths, admin exports, and metrics/config helpers that mention them). Be exhaustive for the audit — prefer missing nothing. Return at most 8 files.

## Constraints

- Return **at most 8 files** in `file_set`.
- Prefer precision over coverage.
