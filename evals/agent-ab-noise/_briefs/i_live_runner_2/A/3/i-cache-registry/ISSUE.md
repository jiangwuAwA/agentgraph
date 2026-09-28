# Issue brief — i-cache-registry

- task: `i-cache-registry`
- runner_id: `i_live_runner_2`
- arm: **A**
- seed: `3`

## Issue (no golden list)

CacheRegistry.register must validate keys before write. Find the real class and the files that construct/use it. Exclude admin/legacy/shared helpers and name-only registry decoys. Return at most 8 files.

## Constraints

- Return **at most 8 files** in `file_set`.
- Prefer precision over coverage.
