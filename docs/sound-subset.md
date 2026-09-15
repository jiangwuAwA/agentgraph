# Sound subset (L2 v1 — experimental)

> **Status:** implemented as `impact --sound` / `callers --sound` / `agentgraph subset`
> (experimental). When the indexed corpus has **zero** S violations (`subset_ok: true`),
> CLI emits a **weakened eligibility promise**:
> edges are sound-eligible *reference* candidates (Exact calls + allowlisted
> DI/event **registrations** + finite-domain string keys).
>
> **This is NOT a proven runtime call-graph over-approx.** Registration of a
> handler (`emitter.on`, DI `bind`, Go route map) is not the same edge as
> framework dispatch at runtime. Do **not** market as production-complete sound analysis.

## Promise (weakened — as implemented)

For programs inside **S** with `subset_ok: true`, the `--sound` walk returns
the **sound-eligible reference graph**: Exact syntactic calls, allowlisted
DI/event *registration* references, and finite-domain string-key candidates.

**Not claimed:** completeness of runtime *dispatch* (framework `emit` /
FastAPI dependency call / HTTP mux invocation are not modeled as call edges).
Over-reporting is allowed. Outside S: no guarantee.

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

## S_py (Python v1 — conservative lexical scanner)

A program is in S_py when **all** of the following hold:

1. No `eval` / `exec` / `__import__` (including spaced forms like `eval (`).
2. No `setattr` on callables / functions (monkey-patching call targets).
3. No non-literal `getattr(obj, name)` (dynamic attribute call targets).
4. No `__builtins__` eval/exec access.
5. DI only via recognized patterns (`Depends`, `@inject`) with static argument names.
6. Dynamic import only via `importlib.import_module("literal.path")` (finite domain).

Scanner: **v1 conservative lexical** (`scan_py` in `src/index/subset.rs`) — **not**
a frozen soundness contract. Prefer over-flag (false violation) over a missed escape.

## S_go (Go v1 — conservative lexical scanner)

A program is in S_go when **all** of the following hold:

1. No `unsafe.*` (Pointer / Sizeof / Add) and no `unsafe` blocks.
2. No `reflect.*` (Value.Call / MethodByName invents edges).
3. No `plugin.Open` / `syscall.NewCallback`.
4. Route/DI tables only as composite `map[string]…Handler…` literals
   recognized by `go.di.handler_map`.

Scanner: **v1 conservative lexical** (`scan_go`) — not a frozen soundness contract.
Same over-flag bias as S_py.

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
| Property tests | random S programs | ✅ `tests/l2_property.rs` (deterministic S_js generator) |
| Golden corpus | S_js auth fixture | ✅ `fixtures/eval-l2/s-js-auth` |
| Invariant unit tests | call sites have Exact edges | ✅ `tests/l2_subset.rs` |

## Non-goals

- Zero misses on S ∪ (not S).
- Full pointer analysis.
- Replacing CodeQL.

See [PLAN.md](../PLAN.md) §4 and [eval-l2.md](eval-l2.md).
