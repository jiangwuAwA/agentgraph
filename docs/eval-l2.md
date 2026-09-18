# L2 Evaluation Report — sound subset over-approx

**Status:** **S-qualified** for **modeled** edges when `subset_ok` (see
[sound-subset.md](sound-subset.md)). Outside S or unmodeled APIs: no claim.
**Non-claim:** not ecosystem sound; not a complete runtime graph; expand/sidecar edges are not in the sound set; no zero-miss guarantee outside modeled S.
**Reproduce:** `cargo test --test l2_subset --test l2_sound --test l2_lang_subset --test l2_py_diff --test l2_go_diff -- --nocapture` (optional manual tracer below).

Reproduce:

```bash
cargo test --test l2_subset --test l2_sound --test l2_py_diff -- --nocapture
# optional manual differential:
node scripts/diff_trace.cjs fixtures/eval-l2/s-js-auth/src/auth.js main
python scripts/py_trace.py fixtures/eval-l2/s-py-auth/src/auth.py main
```

## What `--sound` does

1. Walks only **sound-eligible** edges:
   - all `Exact` L0 edges
   - allowlisted Heuristic rules (`ts.di.*`, `ts.nest.*`, `py.di.*`, `go.di.*`, `rs.di.*`, `ts.event.subscribe|dispatch`)
   - finite-domain DynamicCandidate (`ts.dynamic.computed`, `py.dynamic.getattr`, `py.dynamic.import_module`, `ts.event.emit`)
2. Surfaces **S violations** stored at last index (`eval`, `new Function`, `with`, `Proxy`, `Reflect`, template computed keys, Rust `unsafe`/`transmute`, Python `eval`/`exec`/`compile`/`ctypes`, Go `reflect`/`unsafe`/`plugin`/cgo).
3. Sets `subset_ok: false` and a non-claiming `promise` string when any violation exists.
4. Selects the OK promise by **language tier** (`promise_tier`):
   all shipped languages (js/ts/tsx/jsx, python, go, rust) are **ast_modeled**;
   lexical_v1 is reserved (no shipped language selects it). Weakest tier wins.
   See [sound-subset.md](sound-subset.md) promise table.

CLI:

```bash
agentgraph subset                 # exit 2 if violations
agentgraph impact X --sound
agentgraph callers X --sound
```

MCP: `impact` with `sound: true`; tool `subset`.

## Promise table (must match `src/index/subset.rs`)

| Corpus languages | `promise_tier` | Constant |
|---|---|---|
| JS / TS / TSX / JSX / Python / Go / Rust only | `ast_modeled` | `SOUND_PROMISE_OK_AST` |
| *(reserved)* lexical-v1 language only | `lexical_v1` | `SOUND_PROMISE_OK_LEXICAL_V1` |
| *(reserved)* Mixed AST + lexical-v1 | `mixed_lexical_v1` | `SOUND_PROMISE_OK_MIXED_LEXICAL_V1` |
| Any S violations | `disabled` | `SOUND_PROMISE_DISABLED` |

Code: `is_ast_modeled_language` / `is_lexical_v1_language` / `sound_promise_tier`
in `src/index/subset.rs`. **文实一致:** scanners for py/go/rust are tree-sitter
AST (`scan_py` / `scan_go` / `scan_rust`); no shipped language is lexical-v1.

## Corpus

| corpus | purpose |
|---|---|
| `fixtures/eval-l2/s-js-auth` | clean S_js auth pipeline (validate → hash → authenticate → login → main) + literal registry key |
| `fixtures/eval-l2/s-js-evil` | `eval` escape — must leave S |
| `fixtures/eval-l2/s-js-esm` | multi-file ESM differential |
| `fixtures/eval-l2/s-go-auth` | clean S_go cover-profile differential |
| `fixtures/eval-l2/s-py-auth` | clean S_py auth pipeline + literal getattr / DI-shaped helpers |
| `fixtures/eval-l2/s-py-evil` | `eval` escape — `subset_ok=false`, `promise_tier=disabled` |

## Differential matrix (runtime edges ⊆ sound walk)

| Language | Fixture | Runtime oracle | Test | Result |
|---|---|---|---|---|
| JS/TS | `s-js-auth` | Node export tracer (`scripts/diff_trace.cjs`) | `tests/l2_sound.rs::differential_runtime_edges_subset_of_sound_impact` | ✅ 100% on fixture (exported-callable edges) |
| JS ESM | `s-js-esm` | multi-file ESM tracer | `tests/l2_esm_diff.rs` | ✅ |
| Go | `s-go-auth` | `go test -coverprofile` executed funcs | `tests/l2_go_diff.rs::go_cover_funcs_subset_of_sound_graph` | ✅ (skips honestly if `go` missing) |
| Python | `s-py-auth` | pure-Python `sys.setprofile` tracer (`scripts/py_trace.py`) | `tests/l2_py_diff.rs::py_runtime_edges_subset_of_sound_impact` | ✅ (skips honestly if `python` missing) |
| Python property | generated S_py programs | static Exact edges + `impact_sound` | `tests/l2_property.rs` (S_py generator) | ✅ |

**Containment claim (same class for all rows):** every runtime `from→to`
observed by the oracle appears as a sound-eligible edge in
`impact(to, --sound)` / `callers(to, --sound)` (name or enclosing match) on a
corpus with `subset_ok: true`. This is **not** full VM instrumentation; it
covers the modeled/direct-call class the fixtures exercise.

## Results

| check | result |
|---|---|
| S scan: clean corpus `in_subset` | **true** (0 violations) |
| S scan: eval corpus | **false** (≥1 violation, CLI exit 2) |
| `impact --sound` on clean corpus | `subset_ok: true`, non-empty caller graph |
| `impact --sound` on eval corpus | `subset_ok: false`, promise says claim **disabled** |
| **Differential (Node tracer)** | every runtime edge `from→to` observed by wrapping exports and calling `main` is **contained** in `impact(to, --sound)` |
| **Differential (Go cover)** | `go test -coverprofile` executed funcs (`tests/l2_go_diff.rs`) — `validateEmail` sound impact includes `Authenticate` |
| **Differential (Python tracer)** | `scripts/py_trace.py` edges on `s-py-auth` ⊆ sound impact/callers (`tests/l2_py_diff.rs`) |
| Type-only `typeof Function` / interface `Function` | **stays in S** (M2 over-flag fix); value-use still leaves S |
| Py/Go `promise_tier` on clean corpus | **`ast_modeled`** (not lexical_v1) — `tests/l2_promise_lang.rs` |

### Differential method

- **JS:** `scripts/diff_trace.cjs` wraps every `module.exports` function, runs
  `main`, records `from→to` while the wrapper stack is active.
- **Python:** `scripts/py_trace.py` loads the fixture module, enables
  `sys.setprofile`, calls `main`, records function-call edges whose
  `co_filename` matches the fixture file.
- **Go:** coverage profile executed-function names vs sound graph.

Rust tests assert containment against `impact --sound` / `callers --sound`.
Named function expressions populate `enclosing` so BFS can expand
(`tests/ts_named_fe.rs`).

This is **not** a full V8/CPython instrumentation of every internal call; it
covers exported/module-callable edges in the S corpora (the claim we make for
this fixture class).

## `subset_ok=false` cases (must-disable matrix)

| Fixture / shape | `subset_ok` | `promise_tier` | Violation kinds (representative) |
|---|---|---|---|
| `s-js-evil` (`eval`) | false | `disabled` | `eval` |
| `s-py-evil` (`eval`) | false | `disabled` | `py_eval_exec` |
| Py `getattr(obj, name)` non-literal | false | `disabled` | `py_getattr_dynamic` |
| Py `importlib.import_module(name)` | false | `disabled` | `py_import_module_dynamic` |
| Py `compile` / `__import__` / `__builtins__` | false | `disabled` | `py_dynamic_attr` / `py___import__` / `py_builtins` |
| Py `import ctypes` / `ctypes.CDLL` | false | `disabled` | `py_ctypes` |
| Go `import "unsafe"` / `unsafe.X` | false | `disabled` | `go_cgo_linkname` / `go_unsafe` |
| Go `reflect` import or selector | false | `disabled` | `go_reflect` / `go_cgo_linkname` |
| Go `plugin.Open` / `//go:linkname` / `//export` / `import "C"` | false | `disabled` | `go_dynamic_symbol` / `go_cgo_linkname` |
| Rust `unsafe` / `transmute` / `std::ptr` | false | `disabled` | `unsafe` / `transmute` / `std_ptr` |
| TS/JS value-use `Function` / `eval` | false | `disabled` | `Function` / `eval` |

**Must stay in S (false-positive guards):** literal `getattr(obj,"m")`,
literal `import_module("pkg.mod")`, FastAPI `Depends`, Go `map[string]Handler`
literal routes, JS/TS comments/strings mentioning escapes, **type-only
`typeof Function` / interface `Function` type positions**, Nest allowlist
registration shapes.

## M4 / M2 acceptance (PLAN §10 + Track M2 §2.6)

| criterion | required | observed |
|---|---|---|
| Runtime trace ⊆ `--sound` edges on S_js corpus | 100% | **100%** on `s-js-auth` differential |
| Runtime edges ⊆ `--sound` on S_py / S_go fixtures | 100% when tools present | **100%** on `s-py-auth` / `s-go-auth` (honest skip if toolchain missing) |
| S-outside cases documented | yes | `s-js-evil` / `s-py-evil` + sound-subset.md + subset_ok=false matrix above |
| Clean fixture `subset_ok=true` after over-flag fix | yes | type-only Function positions stay in S |
| Promise 文实一致 (no lexical_v1 lie for py/go) | yes | `promise_tier=ast_modeled`; `is_lexical_v1_language` always false |
| `subset` JSON fields stable | yes | `in_subset`, `violation_count`, `violations`, `promise_tier`, `promise`, `promise_languages`, `note` (`tests/l2_py_diff.rs`, e2e) |

## Explicit non-claims

- No soundness outside S.
- No completeness for any S language beyond the AST scanner (engineering gate — **not** a frozen soundness contract). AST-modeled `promise_tier` OK is **not** ecosystem sound / **not** a proven runtime call-graph over-approx.
- AST-modeled S (js/ts/tsx/jsx, python, go, rust) is an engineering gate, **not** ecosystem sound and **not** a proven runtime call-graph over-approx.
- Over-reporting is allowed (over-approx).
- Do not market `--sound` as “zero missed dynamic calls” on arbitrary repos.

## Adversarial review follow-up (this drop)

| Finding | Fix |
|---|---|
| **C1** runtime-call soundness promise too strong | Promise rewritten to **sound-eligible reference graph**; registration≠dispatch called out |
| **C2** S holes: `Function()` w/o `new`, `obj[k]()`, prototype patch | Scanner flags `Function`, `nonliteral_computed_key`, `monkey_patch` |
| **C3** DI/event allowlist ≠ runtime call | Documented in promise + sound-subset; still walkable as reference candidates |
| **C4** vacuous differential assert | Removed pre-insert of `to`; require callers/impact evidence |
| **C5** SQL LIMIT before sound filter | `callers_uncached_opt(None)` — fetch all then filter then take limit |
| Parallel test race on shared fixture | Per-test temp copies in `l2_sound.rs` and `l2_esm_diff.rs` |


## Next hardening

- Broader Node DI-container fixtures; property tests already cover S_js/S_py/S_go (`tests/l2_property.rs`).
- ~~Model framework dispatch edges (emit).~~ **Done:** `Store::link_event_dispatch` + `tests/l2_dispatch.rs` (once, idempotent, arrow multi-call, index_paths).
- ~~Hot-name p95 fixture.~~ **Done:** `gen_fixture.ps1 -HotName` + `bench-query --hot run --cold` (see eval-query-p95).
- ~~Python differential.~~ **Done (M2 residual):** `scripts/py_trace.py` + `tests/l2_py_diff.rs`.
- ~~Type-only `typeof Function` over-flag.~~ **Done (M2 residual):** type positions stay in S; value-use still fails closed.

## Adversarial review follow-up (this drop — C1/C2/M3–M7)

| Finding | Fix |
|---|---|
| **C1** MCP root jail `..` bypass | Canonicalize candidate **and** base before prefix test; E2E rejects escape and does not create `.agentgraph` outside |
| **C2** S_js false-negatives | Unwrap `(0,eval)` / `(eval)`; flag `window['eval']`; flag `require`/`import` non-literal |
| **M3** S_py under-flag | Non-literal `getattr`, spaced `eval (`, `__builtins__`; docs stop calling S_py/S_go frozen |
| **M4** l2_esm_diff shared fixture race | Per-test unique temp copies |
| **M5** doc contradictions | One source of truth: property tests exist; **S_py/S_go/S_rs are tree-sitter AST scanners** (`promise_tier=ast_modeled`); --sound stays weakened eligibility |
| **M6** MCP callers missing `sound` | Schema + handler, mutual exclusion, CLI-shaped promise JSON |
| **m8/m9/m10** | prune_missing clears cache; scan_js uses file Language; oversized skips counted in `IndexStats.oversized_files` |
| **M2 residual** over-flag + 文实一致 + py differential | type-only Function stays in S; ctypes/compile coverage locked; `l2_py_diff.rs`; docs/PLAN/AGENTS aligned to AST tier |
