# Sound subset (L2 — production S, modeled dispatch)

> **Status:** production for **subset S + modeled patterns**.  
> When `subset_ok: true` (zero S violations), `--sound` walks a graph that
> over-approximates runtime edges **for the call kinds we model**:
>
> 1. Direct syntactic calls (Exact)  
> 2. Finite-domain computed keys (`obj['m']()`, `getattr(obj,"m")`, `import_module("lit")`)  
> 3. **Event dispatch** `emit('e')` ↔ `on('e', handler)` → Heuristic `ts.event.dispatch`  
> 4. DI / route **registration** edges (impact candidates; registration is not HTTP ServeHTTP)  
> 5. Go handler maps + gin-like `GET(path, h)`  
>
> **Still not claimed:** soundness outside S; unmodeled frameworks (custom
> proxies, `eval`, `unsafe` fn pointers, monkey-patching); over-reporting is
> allowed. CLI `subset_ok=false` **disables** the promise.

## Promise (production S)

For programs inside **S** with `subset_ok: true`:

```text
Runtime edges from { direct calls, literal-key dispatch, emit↔on pairs }
  ⊆  impact/callers --sound graph   (over-approx OK)
```

HTTP mux / FastAPI call sites appear as **registration** edges (sound-eligible
Heuristic). The framework’s internal `ServeHTTP` invocation is **not** asserted
as a call-graph edge (it lives outside the indexed program).

## S_js (TypeScript / JavaScript)

A program is in S_js when **all** of the following hold:

1. No `eval`, no `Function` (with or without `new`), no `with`.
2. No `Proxy` / `Reflect` metaprogramming that invents call targets.
3. Module graph is static ESM/CJS `import`/`require` of **string literals** only.
4. Computed call targets only with **string-literal** keys (non-literal / template `${}` leave S).
5. No monkey-patching (`prototype` / `globalThis` / `window` assignment).
6. Event use limited to string-literal `on`/`emit`/`subscribe` pairs we index.

## S_rs (Rust)

1. No `unsafe` fn-pointer tables or transmute-based dispatch.
2. No process-macro-generated call sites invisible after expansion.
3. Trait objects only with local `impl Trait for Type` in the corpus.

## S_py / S_go (conservative scanners)

See `scan_py` / `scan_go` in `src/index/subset.rs`. Prefer over-flag.
Dynamic `getattr` without literal, `eval`/`exec`, `unsafe`/`reflect` leave S.

## Verification

| Method | Status |
|---|---|
| Node export tracer | ✅ `tests/l2_sound.rs` |
| Multi-file ESM | ✅ `tests/l2_esm_diff.rs` |
| Go cover profile | ✅ `tests/l2_go_diff.rs` |
| Property tests (S_js generator) | ✅ `tests/l2_property.rs` |
| emit↔on dispatch edges | ✅ `tests/l2_dispatch.rs` + `Store::link_event_dispatch` |
| S violation scanners | ✅ `tests/l2_subset.rs`, `l2_lang_subset.rs` |

## Non-goals

- Soundness outside S  
- Completeness for unmodeled frameworks  
- Replacing CodeQL  

See [PLAN.md](../PLAN.md) §4 and [eval-l2.md](eval-l2.md).
