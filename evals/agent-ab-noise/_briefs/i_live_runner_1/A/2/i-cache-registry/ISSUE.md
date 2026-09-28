# Issue brief — i-cache-registry

- task: `i-cache-registry`
- runner_id: `i_live_runner_1`
- arm: **A**
- seed: `2`

## Issue (no golden list)

Security review: every file that defines, constructs, wraps, or documents CacheRegistry must be listed (including admin/legacy/shared registries and logger helpers). Do not miss any occurrence of the class name. Return at most 8 files.

## Constraints

- Return **at most 8 files** in `file_set`.
- Prefer precision over coverage.
