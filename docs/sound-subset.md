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
>    including Nest `@Module({ providers, controllers, imports })` and
>    constructor type-injection (`ts.nest.*`) — finite-domain identifiers
>    written in source metadata / type annotations  
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
| JS / TS / TSX / JSX / Python / Go / Rust only | `ast_modeled` (`SOUND_PROMISE_OK_AST`) | AST-modeled S (engineering gate on modeled edges — **not** ecosystem sound) |
| *(reserved)* lexical-v1 language only | `lexical_v1` (`SOUND_PROMISE_OK_LEXICAL_V1`) | Conservative **lexical v1** scanner — **not frozen**. **No currently shipped language selects this tier.** |
| *(reserved)* Mixed AST + lexical-v1 | `mixed_lexical_v1` (`SOUND_PROMISE_OK_MIXED_LEXICAL_V1`) | Weakest tier governs; both tiers named. **No currently shipped language is lexical-v1.** |
| Any S violations | `disabled` (`SOUND_PROMISE_DISABLED`) | No eligibility claim |

**Explicit:** an AST-modeled OK is still an **engineering S gate**, not a
proven runtime call-graph over-approx and not ecosystem sound. The
`lexical_v1` / `mixed_lexical_v1` arms remain in the enum for API stability;
no currently shipped language selects them. Honesty > green
checkmarks. 怕漏 users should inspect `promise_tier` / `promise_languages`.

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

`scan_rust` is a **tree-sitter AST walk** (see `scan_rust` / `walk_rs_s` in
`src/index/subset.rs`) that fails-closed on parse errors (`has_error` →
violation). Rust joins the `ast_modeled` promise tier.

**Rust (S_rs)** leaves S on:
`unsafe` blocks, `unsafe fn` / `unsafe impl` / `unsafe trait`,
`transmute` calls / `std::mem::transmute` / `core::mem::transmute` paths,
`asm!` / `global_asm!`, `std::ptr::*` / `core::ptr::*` paths.
Comments and strings do **not** trigger (AST advantage over lexical).

## S_py / S_go (AST scanners — tree-sitter)

See `scan_py` / `scan_go` in `src/index/subset.rs`. Both are **tree-sitter
AST walks** that fail-closed on parse errors (`has_error` → violation).

**Python (S_py)** leaves S on:
`eval` / `exec` calls and aliases, `__import__`, `setattr`,
`getattr` without a string-literal second arg, `__builtins__` access,
`__getattribute__` / `attrgetter` / `methodcaller` / `FunctionType` /
`__dict__` / `compile`, `vars`/`globals`/`locals` + subscript,
`importlib.import_module` with non-literal first arg.
Comments and strings do **not** trigger (AST advantage over lexical).

**Go (S_go)** leaves S on:
cgo `import "C"`, `//go:linkname`, `import "unsafe"` / `import "reflect"`,
`unsafe.` / `reflect.` selectors, `plugin.Open` / `syscall.NewCallback`.
Comments alone do not flag (except `//go:linkname`, a significant compiler
directive). Parse errors fail closed.

Python, Go, and Rust join the `ast_modeled` promise tier. This is still an
engineering S gate — **not** ecosystem sound.

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
2. Check `promise_tier`: `ast_modeled` is the strongest S claim (still an engineering gate). The `lexical_v1` / `mixed_lexical_v1` arms are reserved; no currently shipped language selects them.  
3. Or `--recall` / `--include-dynamic` for a wider heuristic window.  
4. Do **not** expect zero misses **and** zero extras on arbitrary code — see PLAN §0.2.

## Known over-approx / accepted false positives

- `obj['on']` / `obj['emit']` on **any** receiver is treated as the event API
  (e.g. a state machine `states['on']`). Over-approx is soundness-safe; L1 noise possible.
- Event **names** as DynamicCandidate callees (`emit('trade')` → name `trade`).
- Nest `@Module` arrays / ctor type annotations are treated as finite-domain
  registration (same class as `bind`/`register`). Registration ≠ runtime HTTP
  ServeHTTP; Nest internals that resolve the provider graph are outside the
  indexed program.

## Non-goals

- Soundness outside S  
- Completeness for unmodeled frameworks  
- Replacing CodeQL  

See [PLAN.md](../PLAN.md) §4 and [eval-l2.md](eval-l2.md).
