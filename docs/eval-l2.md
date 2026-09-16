# L2 Evaluation Report — sound subset over-approx

**Status:** production S for **modeled** edges when `subset_ok` (see
[sound-subset.md](sound-subset.md)). Outside S or unmodeled APIs: no claim.

Reproduce:

```bash
cargo test --test l2_subset --test l2_sound -- --nocapture
# optional manual differential:
node scripts/diff_trace.cjs fixtures/eval-l2/s-js-auth/src/auth.js main
```

## What `--sound` does

1. Walks only **sound-eligible** edges:
   - all `Exact` L0 edges
   - allowlisted Heuristic rules (`ts.di.*`, `py.di.*`, `go.di.handler_map`, `rs.di.impl_trait`, `ts.event.subscribe`)
   - finite-domain DynamicCandidate (`ts.dynamic.computed`, `py.dynamic.getattr`, `py.dynamic.import_module`)
2. Surfaces **S violations** stored at last index (`eval`, `new Function`, `with`, `Proxy`, `Reflect`, template computed keys, Rust `unsafe`/`transmute`, Python `eval`/`exec`).
3. Sets `subset_ok: false` and a non-claiming `promise` string when any violation exists.

CLI:

```bash
agentgraph subset                 # exit 2 if violations
agentgraph impact X --sound
agentgraph callers X --sound
```

MCP: `impact` with `sound: true`; tool `subset`.

## Corpus

| corpus | purpose |
|---|---|
| `fixtures/eval-l2/s-js-auth` | clean S_js auth pipeline (validate → hash → authenticate → login → main) + literal registry key |
| `fixtures/eval-l2/s-js-evil` | `eval` escape — must leave S |

## Results

| check | result |
|---|---|
| S scan: clean corpus `in_subset` | **true** (0 violations) |
| S scan: eval corpus | **false** (≥1 violation, CLI exit 2) |
| `impact --sound` on clean corpus | `subset_ok: true`, non-empty caller graph |
| `impact --sound` on eval corpus | `subset_ok: false`, promise says claim **disabled** |
| **Differential (Node tracer)** | every runtime edge `from→to` observed by wrapping exports and calling `main` is **contained** in `impact(to, --sound)` (name or enclosing match) |
| **Differential (Go cover)** | `go test -coverprofile` executed funcs (`tests/l2_go_diff.rs`) — `validateEmail` sound impact includes `Authenticate` |

### Differential method

`scripts/diff_trace.cjs` wraps every `module.exports` function, runs `main`,
records `from→to` while the wrapper stack is active. The Rust test
`differential_runtime_edges_subset_of_sound_impact` asserts containment
against `impact --sound` / `callers --sound`.

This is **not** a full V8 instrumentation of every internal call; it covers
exported-callable edges in the S corpus (the claim we make for this fixture
class). Named function expressions now populate `enclosing` so BFS can expand
(`tests/ts_named_fe.rs`).

## M4 acceptance (PLAN §10)

| criterion | required | observed |
|---|---|---|
| Runtime trace ⊆ `--sound` edges on S_js corpus | 100% | **100%** on `s-js-auth` differential |
| S-outside cases documented | yes | `s-js-evil` + sound-subset.md |

## Explicit non-claims

- No soundness outside S.
- No completeness for Python/Go S (v1 conservative lexical scanners — **not** frozen soundness contracts).
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

- Broader Node DI-container fixtures; property tests for S_py/S_go (S_js property tests ship in `tests/l2_property.rs`).
- ~~Model framework dispatch edges (emit).~~ **Done:** `Store::link_event_dispatch` + `tests/l2_dispatch.rs` (once, idempotent, arrow multi-call, index_paths).
- ~~Hot-name p95 fixture.~~ **Done:** `gen_fixture.ps1 -HotName` + `bench-query --hot run --cold` (see eval-query-p95).

## Adversarial review follow-up (this drop — C1/C2/M3–M7)

| Finding | Fix |
|---|---|
| **C1** MCP root jail `..` bypass | Canonicalize candidate **and** base before prefix test; E2E rejects escape and does not create `.agentgraph` outside |
| **C2** S_js false-negatives | Unwrap `(0,eval)` / `(eval)`; flag `window['eval']`; flag `require`/`import` non-literal |
| **M3** S_py under-flag | Non-literal `getattr`, spaced `eval (`, `__builtins__`; docs stop calling S_py/S_go frozen |
| **M4** l2_esm_diff shared fixture race | Per-test unique temp copies |
| **M5** doc contradictions | One source of truth: property tests exist; S_py/S_go are v1 lexical scanners; --sound stays weakened eligibility |
| **M6** MCP callers missing `sound` | Schema + handler, mutual exclusion, CLI-shaped promise JSON |
| **m8/m9/m10** | prune_missing clears cache; scan_js uses file Language; oversized skips counted in `IndexStats.oversized_files` |
