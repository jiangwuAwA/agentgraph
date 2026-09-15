# Sound subset (draft — L2 prep)

> **Status:** draft for L2. Not implemented as `impact --sound` yet.
> This document freezes the *intended* subset S so L1 rules and future
> sound analysis share one vocabulary. Do **not** claim L2/sound until
> PLAN.md §4 acceptance criteria are met.

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

## Analysis ingredients (planned, not implemented)

1. Type-constraint propagation (extend L0 `qualifier`).
2. Class / interface / trait implementation closure.
3. Explicit registry closure (key → implementation).
4. Finite-domain enumeration of string-literal keys.
5. Optional abstract interpretation over S only.

## Verification plan (L2)

| Method | Role |
|---|---|
| Differential vs runtime traces | Node/Jest, `go test -cover`, Python coverage hooks |
| Property tests | random S programs: interpret vs graph |
| Golden corpus | hand-labeled complete edge sets |
| Invariant unit tests | every AST `call_expression` in S has an edge |

## Non-goals

- Zero misses on S ∪ (not S).
- Full pointer analysis.
- Replacing CodeQL.

See [PLAN.md](../PLAN.md) §4 for milestones.
