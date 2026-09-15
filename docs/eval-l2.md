# L2 Evaluation Report — sound subset over-approx

**Status:** experimental. Soundness claim applies **only** inside subset S
(see [sound-subset.md](sound-subset.md)). Outside S, `--sound` disables the
claim and reports violations.

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
- No completeness for Python/Go S (not frozen; scanner is conservative lexical).
- Over-reporting is allowed (over-approx).
- Do not market `--sound` as “zero missed dynamic calls” on arbitrary repos.

## Next hardening (not in this drop)

- Property tests: random S programs interpret vs graph.
- Broader Node differential (multi-file ESM, DI container fixtures).
- Go/Python S freeze + language-specific S scanners via full AST.
