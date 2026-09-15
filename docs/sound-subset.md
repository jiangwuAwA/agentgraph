# Sound subset (L2 v1 — experimental)

> **Status:** implemented as `impact --sound` / `callers --sound` / `agentgraph subset`
> (experimental). Soundness claim applies **only** when the indexed corpus has
> **zero** S violations (`subset_ok: true`). See [eval-l2.md](eval-l2.md).
>
> Do **not** market this as production-complete sound analysis.

## Promise (once implemented)

For programs that stay inside subset **S**, every runtime call edge that
can occur is **contained** in the static over-approximation returned by
`impact --sound` / `callers --sound` (over-approx OK; **no misses**).

Outside S: no guarantee. DynamicCandidate edges are reported as
**warnings**, not trusted as sound.

## S_js (TypeScript / JavaScript v1)

A program is in S_js when **all** of the following hold:

1. No `eval`, no `new Function`, no `with`.
2. No `Proxy` / `Reflect` metaprogramming that invents call targets.
3. Module graph is mostly static ESM/CJS `import`/`require` of string literals.
4. Computed property access used as a call target only with **string-literal**
   keys (`obj['m']()`), never with template keys containing `${}`.
5. DI / registries only via **patterns agentgraph already recognizes**
   (`register` / `bind().to` / FastAPI `Depends` / Go handler maps / …).
6. No monkey-patching of built-ins that redirects known callees.

## S_rs (Rust v1)

1. No `unsafe` fn-pointer tables or transmute-based dispatch.
2. No process-macro-generated call sites that are invisible after expansion
   (or macros must be expanded before index).
3. Trait objects (`dyn Trait`) only with **local** `impl Trait for Type`
   blocks present in the indexed corpus.

## Analysis ingredients (implemented v1)

1. Type-constraint propagation (L0 `qualifier`) — existing.
2. Class / interface / trait implementation closure — partial (Rust `impl Trait` Heuristic).
3. Explicit registry closure — DI rule allowlist (`src/index/subset.rs`).
4. String-literal key finite domain — `ts.dynamic.computed` / `py.dynamic.getattr` / `py.dynamic.import_module` treated as `SoundFiniteDomain`.
5. Abstract interpretation — **not** implemented.

## Verification (implemented v1)

| Method | Role | Status |
|---|---|---|
| Differential vs runtime traces | Node export-wrapper tracer | ✅ `scripts/diff_trace.cjs` + `tests/l2_sound.rs` |
| Property tests | random S programs | ❌ planned |
| Golden corpus | S_js auth fixture | ✅ `fixtures/eval-l2/s-js-auth` |
| Invariant unit tests | call sites have Exact edges | ✅ `tests/l2_subset.rs` |

## Non-goals

- Zero misses on S ∪ (not S).
- Full pointer analysis.
- Replacing CodeQL.

See [PLAN.md](../PLAN.md) §4 and [eval-l2.md](eval-l2.md).
