# Sound subset (L2 — S-qualified modeled edges)

> **Status:** **S-qualified** (not a blanket “production” label). When
> `subset_ok: true`, `--sound` walks Exact + allowlisted Heuristic +
> finite-domain dynamic edges **for the call kinds we model**:
>
> 1. Direct syntactic calls  
> 2. Finite-domain computed keys (`obj['m']()`, string `getattr` / `import_module`)  
> 3. Event dispatch `emit`/`once`/`on` with **string-literal** keys and
>    identifier / fn-expr / string-subscript handlers → `ts.event.dispatch`  
> 4. DI / route registration (impact candidates; registration ≠ HTTP ServeHTTP)  
>
> **Explicitly outside the claim:** unmodeled bus aliases, non-ident handlers
> (leave S), RxJS `next`, custom frameworks, anything with S violations.
> CLI promise is disabled when `subset_ok: false`.

## Promise (S-qualified)

For programs inside **S** with `subset_ok: true`:

```text
Runtime edges from { direct calls, literal-key dispatch, emit↔on pairs }
  ⊆  impact/callers --sound graph   (over-approx OK)
```

HTTP mux / FastAPI call sites appear as **registration** edges (sound-eligible
Heuristic). The framework’s internal `ServeHTTP` invocation is **not** asserted
as a call-graph edge (it lives outside the indexed program).

### Promise table by language (honesty — weakest tier wins)

CLI/MCP select the promise string from the **indexed corpus languages**, not
from a single global OK. Shared constants live in `src/index/subset.rs`
(re-exported by `mcp::server`).

| Corpus languages | `promise_tier` | Assurance |
|---|---|---|
| JS / TS / TSX / JSX / Rust only | `ast_modeled` (`SOUND_PROMISE_OK_AST`) | AST-modeled S (full S-qualified claim on modeled edges) |
| Python / Go only | `lexical_v1` (`SOUND_PROMISE_OK_LEXICAL_V1`) | Conservative **lexical v1** scanners — **not frozen**, weaker than AST S_js/S_rs |
| Mixed AST + Python/Go | `mixed_lexical_v1` (`SOUND_PROMISE_OK_MIXED_LEXICAL_V1`) | Weakest tier governs; both tiers named |
| Any S violations | `disabled` (`SOUND_PROMISE_DISABLED`) | No eligibility claim |

**Explicit:** a lexical-v1 OK does **not** mean the same assurance as AST
S_js. Honesty > green checkmarks. 怕漏 users should inspect
`promise_tier` / `promise_languages` and prefer AST-only corpora when they
need the strongest S claim.

Payload fields on `impact/callers --sound` and `subset`:
`promise`, `promise_tier`, `promise_languages` (plus `subset_ok` /
`in_subset`).

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

## S_py / S_go (conservative scanners — lexical v1)

See `scan_py` / `scan_go` in `src/index/subset.rs`. Prefer over-flag.
Dynamic `getattr` without literal, `eval`/`exec`, `unsafe`/`reflect` leave S.

These scanners are **lexical v1** (line-oriented, fail-closed on known
escapes) — not a full AST freeze and **not** a frozen soundness contract.
`subset_ok: true` on a py/go-only or mixed corpus emits the
`lexical_v1` / `mixed_lexical_v1` promise tier, never the AST OK.

## Verification

| Method | Status |
|---|---|
| Node export tracer | ✅ `tests/l2_sound.rs` |
| Multi-file ESM | ✅ `tests/l2_esm_diff.rs` |
| Go cover profile | ✅ `tests/l2_go_diff.rs` |
| Property tests (S_js generator) | ✅ `tests/l2_property.rs` |
| emit↔on dispatch edges | ✅ `tests/l2_dispatch.rs` + `Store::link_event_dispatch` |
| S violation scanners | ✅ `tests/l2_subset.rs`, `l2_lang_subset.rs` |

## If you fear missed edges (怕漏)

1. Prefer `impact/callers --sound` when `subset_ok: true` (S-qualified over-approx).  
2. Check `promise_tier`: `ast_modeled` is the strongest S claim; `lexical_v1` / `mixed_lexical_v1` are honest weaker tiers for py/go scanners.  
3. Or `--recall` / `--include-dynamic` for a wider heuristic window.  
4. Do **not** expect zero misses **and** zero extras on arbitrary code — see PLAN §0.2.

## Known over-approx / accepted false positives

- `obj['on']` / `obj['emit']` on **any** receiver is treated as the event API
  (e.g. a state machine `states['on']`). Over-approx is soundness-safe; L1 noise possible.
- Event **names** as DynamicCandidate callees (`emit('trade')` → name `trade`).

## Non-goals

- Soundness outside S  
- Completeness for unmodeled frameworks  
- Replacing CodeQL  

See [PLAN.md](../PLAN.md) §4 and [eval-l2.md](eval-l2.md).
