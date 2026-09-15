# L3 formal track

Research / quality-gate track. **Does not block releases.**

## Status (this drop)

| Item | Status |
|---|---|
| `IncrementalIndex.tla` + `.cfg` | ✅ TLC-checked |
| TLC run | ✅ **No error** — 568 states, 63 distinct, depth 6 (`tlc-results.txt`) |
| I1–I3 executable invariants | ✅ `tests/l3_invariants.rs` |
| I4 small-language soundness | ✅ executable IR + exhaustive/property tests (`tests/l4_mini_lang.rs`, `src/formal/mini_lang.rs`) — **not** Lean |
| Theorem-level I4 (Lean/Rocq) | ❌ backlog — see [TODO.md](TODO.md) |

## Running TLC

Requires Java 17+ and `tla2tools.jar` (not committed; ~4.5 MB):

```bash
# download once
curl -L -o formal/tools/tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar

# from repo root (Windows)
formal\run-tlc.cmd
# or
cd formal && java -cp tools/tla2tools.jar tlc2.TLC -config IncrementalIndex.cfg IncrementalIndex
```

Last successful run summary is in [tlc-results.txt](tlc-results.txt).

## Invariants

| ID | Statement | How checked |
|---|---|---|
| **I1** | Every DirectCall AST node yields ≥1 Exact call ref | `tests/l3_invariants.rs` |
| **I2** | After C→C', DB rows for that path ≅ extract(C') | `tests/l3_invariants.rs` + TLC `ReindexCorrect` |
| **I3** | `impact(s,d)` = depth-≤d callers (per expansion rules) | `tests/l3_invariants.rs` |
| **I4** | Mini-language runtime calls ⊆ static call-closure | `tests/l4_mini_lang.rs` (bounded exhaustive) |

## I4 mini-language (executable formal)

`src/formal/mini_lang.rs` defines a tiny imperative IR:

- functions with direct calls and **string-literal** table dispatch (`tbl["m"]()`)
- no reflection / eval / computed non-literal keys (subset S_L)

Analysis: transitive call-closure over direct + literal-dispatch edges.
Semantics: small-step interpreter collecting runtime call edges.
Property: for every generated program in the bounded space,
`runtime_edges ⊆ static_closure` (over-approx allowed).

This is **not** a Lean/Rocq development. A theorem-prover port remains optional.

## Non-goals

- Verify tree-sitter / LLVM / browser engines
- Prove L1 heuristics sound
- Require TLC or Lean on the main CI (CI only checks artifacts + Rust tests)

## Relationship to L2

L2 `--sound` is an engineering over-approx with an S-violation gate.
L3 pins index/BFS invariants and a **toy language** containment property —
it does not upgrade L2 into a full call-graph theorem.
