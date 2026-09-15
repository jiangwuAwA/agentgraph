# L3 formal track

Research / quality-gate track. **Does not block releases.**

Goal (PLAN §5): prove **implementation invariants** and a **small-language core**,
not “JS ecosystem soundness”. L1 heuristics are **not** proven sound here.

## Layout

| File | What it models |
|---|---|
| [IncrementalIndex.tla](IncrementalIndex.tla) | Incremental index + `resolved_symbol_id` full relink (I2) |
| [README.md](README.md) | This file |

## Invariants (PLAN §5.2)

| ID | Statement | How we check it |
|---|---|---|
| **I1** | Every DirectCall AST node yields ≥1 Exact call ref | `tests/l3_invariants.rs::i1_…` + extract unit tests |
| **I2** | After content change C→C', DB rows for that path ≅ extract(C') | `tests/l3_invariants.rs::i2_…` + TLA+ `TypeInvariant` / `ReindexCorrect` |
| **I3** | `impact(s,d)` = callers reachable within depth ≤ d (per expansion rules) | `tests/l3_invariants.rs::i3_…` |
| **I4** | Small imperative language call-closure over-approx | **not started** (Lean/Rocq optional, not in CI) |

## Running the TLA+ model (optional)

Install [TLA+ Tools](https://github.com/tlaplus/tlaplus) or Apalache, then:

```bash
# TLC (Java)
tlc2 formal/IncrementalIndex.tla

# or Apalache
apalache-mc check --config= formal/IncrementalIndex.tla
```

CI only asserts these files exist and Rust invariant tests stay green.
Nightly TLC is welcome but not required.

## Non-goals

- Verify tree-sitter
- Verify LLVM / browser engines
- Prove L1 heuristics sound

## Relationship to L2

L2 `--sound` is an *engineering over-approx* with an S-violation gate.
L3 does not upgrade that claim; it only pins index/BFS invariants the
implementation must not break.
