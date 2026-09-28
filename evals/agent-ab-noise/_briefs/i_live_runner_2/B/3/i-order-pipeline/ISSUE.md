# Issue brief — i-order-pipeline

- task: `i-order-pipeline`
- runner_id: `i_live_runner_2`
- arm: **B**
- seed: `3`

## Issue (no golden list)

createOrder in the orders module must change its return contract to include a payment reference. Identify the real definition and its immediate product dependents (order placement + charge). Do not treat metrics/config/util/admin/legacy helpers as dependents. Return at most 8 files.

## Constraints

- Return **at most 8 files** in `file_set`.
- Prefer precision over coverage.
