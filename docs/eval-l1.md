# L1 Evaluation Report

**Status:** shipped L1 candidate rules; fixture-scale + framework-shaped public corpora.
**Non-claim:** L1 edges are **candidates**, never sound. No zero-miss / ecosystem-sound / complete-runtime-graph product claim. Private stock numbers are operator-only (skipped in CI).
**Reproduce:** `cargo test --test l1_eval -- --nocapture` (also `l1_eval_real`). Public synthetic goldens: [`fixtures/eval-goldens/`](../fixtures/eval-goldens/).

Reproducible corpus + golden edges live in [`fixtures/eval-l1/`](../fixtures/eval-l1/).
Metrics are produced by `tests/l1_eval.rs` (run in CI via `cargo test`).

```bash
cargo test --test l1_eval -- --nocapture
```

## What is measured

| Metric | Definition |
|---|---|
| **L0 found** | Golden edges matched with `ConfidenceFilter::ExactOnly` (L0 syntactic graph only) |
| **L1 found** | Golden edges matched with Default (Exact+Heuristic) or IncludeDynamic when the golden edge requires DynamicCandidate |
| **L1 recall** | `L1 found / golden total` |
| **Heuristic edges** | Heuristic-confidence refs emitted in the corpus files |
| **matched** | Heuristic edges whose name/evidence aligns with some golden edge (noise proxy: `1 - matched/total`) |

## Honest limits

- Primary golden set in `fixtures/eval-l1` is **fixture-scale** (DI/event/getattr shapes).
- Multi-file **framework-idiom** corpus in `fixtures/eval-l1-real` (NestJS/Inversify-like TS, **real Nest `@Module` shape**, FastAPI tree, Gin-like Go) — not a vendored production monorepo, but multi-module and closer to real layout. Measured by `tests/l1_eval_real.rs`.
- L1 edges remain **candidates** — never sound.
- **Large private multi-language repo** (`stock-trading-app`) measured separately: [eval-large-repo.md](eval-large-repo.md).
- **Real-tree L1 sampling** (stock-trading-app + public `nestjs-starter`) lives in [eval-large-repo.md](eval-large-repo.md) § “L1 sampling (this commit)”. Historical: fixture L1 lift did **not** transfer to the Nest starter (0 Heuristic) until `ts.nest.module_*` / `ts.nest.ctor_inject` landed; Rust monorepo lift is almost entirely `rs.di.impl_trait` implementor edges. Noise proxy there is manual sampling, not golden labels.
- **Hand-labeled goldens on stock-trading-app clean/mixed crates** (31 edges): L0 68% → L1 **100%** after `rs.di.inventory_submit` (`inventory::submit!` registry/factory types). Details: [eval-stock-boundary.md](eval-stock-boundary.md). Common-name noise (`fmt`/`drop`/`default`) still floods callers — not a production precision number.

## Results (this commit)

### fixtures/eval-l1

| corpus | golden | L0 found | L1 found | L1 recall | Heuristic edges | matched |
|---|---:|---:|---:|---:|---:|---:|
| go-iface | 4 | 0 | 4 | 100% | 13 | 13 |
| go-routes | 3 | 0 | 3 | 100% | 4 | 4 |
| py-fastapi | 4 | 1 | 4 | 100% | 2 | 2 |
| py-plugins | 4 | 0 | 4 | 100% | 6 | 6 |
| rust-dyn | 4 | 1 | 4 | 100% | 8 | 8 |
| rust-shapes | 3 | 1 | 3 | 100% | 4 | 4 |
| ts-di | 8 | 2 | 8 | 100% | 8 | 8 |
| ts-router | 4 | 0 | 4 | 100% | 6 | 6 |
| **total** | **34** | **5 (15%)** | **34 (100%)** | **+580% relative** | **51** | **51 (0% unmatched fixture proxy)** |

### fixtures/eval-l1-real (framework-idiom multi-module)

| corpus | golden | L0 | L1 | heur | matched |
|---|---:|---:|---:|---:|---:|
| nestjs-inversify | 7 | 2 | **7** | 8 | 8 |
| nestjs-module | 5 | 0 | **5** | 6 | 6 |
| fastapi-app | 3 | 2 | **3** | 1 | 1 |
| ginlike-go | 3 | 1 | **3** | 3 | 3 |
| **total** | **18** | **5 (28%)** | **18 (100%)** | **18** | **18 (0% unmatched)** |

Relative lift on nestjs-inversify: 28% → 100% (**+250%** ≥15% M2).
`nestjs-module` is the real-Nest `@Module({ imports, controllers, providers })`
+ constructor-DI shape (mirrors nestjs-starter); L0=0 → L1=100%.

### M2 acceptance (PLAN §10)

| Criterion | Required | Observed |
|---|---|---|
| Heuristic recall lift vs L0 on ≥1 DI corpus | ≥15% relative | **ts-di: 25% → 100%** (+300% relative) |
| Heuristic noise (proxy: unmatched-by-golden) | ≤30% | **0%** on **fixture** corpora only — **not** a production noise rate |

CI asserts these thresholds (`l1_beats_l0_on_di_corpus`, `l1_recall_improvement_meets_m2_threshold_on_mixed_corpus`, `heuristic_noise_rate_below_m2_threshold`).

## Rules covered

| Language | Rule id | Pattern | Confidence | Sound-eligible |
|---|---|---|---|---|
| TS/JS | `ts.di.register` | `c.register(X)` | Heuristic | yes |
| TS/JS | `ts.di.bind` / `ts.di.to` | `c.bind(X).to(Y)` | Heuristic | yes |
| TS/JS | `ts.di.decorator` | `@Inject(X)` / `@Injectable(X)` | Heuristic | yes |
| TS/JS | `ts.nest.module_providers` | `@Module({ providers: [S, { provide, useClass, useExisting, useFactory, inject }] })` | Heuristic | yes |
| TS/JS | `ts.nest.module_controllers` | `@Module({ controllers: [C] })` | Heuristic | yes |
| TS/JS | `ts.nest.module_imports` | `@Module({ imports: [M, X.forRoot(), X.forRootAsync()] })` | Heuristic | yes |
| TS/JS | `ts.nest.module_exports` | `@Module({ exports: [E, 'TOKEN'] })` | Heuristic | yes |
| TS/JS | `ts.nest.ctor_inject` | `constructor(private x: T)` (type annotation, not primitives) | Heuristic | yes |
| TS/JS | `ts.framework.register` | Express/Fastify `router.get/post(..., h)`, `app.use(mw)`, `app.register(path, h)` | Heuristic | yes (finite handler idents at site) |
| TS/JS | `ts.event.subscribe` | `emitter.on(evt, handler)` | Heuristic | yes |
| TS/JS | `ts.dynamic.computed` | `obj['m']()` / `new (reg['X'])()` | DynamicCandidate | finite-domain only |
| Python | `py.di.depends` | FastAPI `Depends(fn\|Class)` / `Security(fn)` / `Annotated[..., Depends(fn)]` | Heuristic | yes |
| Python | `py.di.inject` | `@inject(...)` | Heuristic | yes |
| Python | `py.framework.init_subclass` | subclass of base defining `__init_subclass__` | Heuristic | yes |
| Python | `py.di.entry_points` | `entry_points(group="g")` / `iter_entry_points("g")` | Heuristic | **no** — plugins not enumerated at site |
| Python | `py.dynamic.getattr` | `getattr(obj, "m")` | DynamicCandidate | finite-domain only |
| Python | `py.dynamic.import_module` | `importlib.import_module("pkg.mod")` | DynamicCandidate | finite-domain only |
| Go | `go.di.handler_map` | `map[string]Handler{ "p": H }` | Heuristic | yes |
| Go | `go.di.interface_impl` | `func (t *T) Method` | Heuristic | yes |
| Go | `go.di.interface_assert` | `var _ I = (*T)(nil)` / `T{}` | Heuristic | yes |
| Go | `go.di.interface_impl_v2` | assertion + method-set name match for indexed types (M3-B) | Heuristic | yes (finite method-set in corpus) |
| Go | `go.di.route_register` | `e.GET(path, h)` / `mux.HandleFunc` | Heuristic | yes |
| Rust | `rs.di.impl_trait` | `impl Trait for Type { fn m }` | Heuristic | yes |
| Rust | `rs.di.inventory_submit` | `inventory::submit! { Reg { factory: \|\| Type::new(..) } }` | Heuristic | yes |
| Rust | `rs.di.dyn_trait_method` | `recv.method()` on `dyn Trait` → same-file `impl Trait for Type` methods (M3-A) | Heuristic | **no** — open dispatch, not finite registration |
| Rust | `rs.di.linkme_distributed_slice` | `#[distributed_slice(SLICE)] static X: Ty = ...` (M3-E) | Heuristic | yes (identifiers at attribute/static site) |

**Shapes, not separate rule ids:** `X.forRootAsync({ imports, inject, useFactory })`
emits under `ts.nest.module_imports` / `ts.nest.module_providers` (and
`ts.di.bind`/`ts.di.to` for bind chains). Bare string tokens
(`provide: 'CONFIG'` / `exports: ['CONFIG']`) emit under
`ts.nest.module_providers` / `ts.nest.module_exports`. There are no
`ts.nest.forRootAsync` or `ts.nest.string_token` rule ids.

### M3 package notes

| Package | Shipped rule(s) | Noise / flooding notes |
|---|---|---|
| **M3-A** dyn Trait | `rs.di.dyn_trait_method` | Emits **method name** edges with implementor `qualifier` (Circle/Rect). Only types from **same-file** `impl Trait for Type` — never invents implementors (`Box<dyn Handler>` with no impl → no edge). Common method names (`fmt`/`clone`/`default`) can flood `callers` when a trait has many impls; `impact` benefits more (blast radius via enclosing call site). **Not sound-eligible**: dyn dispatch is open (cross-crate / blanket impls / generic trait objects). |
| **M3-B** Go iface | `go.di.interface_impl_v2` | `var _ I = (*T)(nil)` now links each interface method the type provides; method-set name match (all methods of I present on T in this file) fires without assertion. Finite domain over indexed methods — allowlisted like `go.di.interface_impl`. |
| **M3-C** Python gaps | `py.di.entry_points`; Security folded into `py.di.depends` | Depends/`__init_subclass__` already covered. New: `entry_points(group=…)` / `iter_entry_points("g")`. Plugins loaded by the group are **not** named at the call site → not sound-eligible. Annotated Depends already fired under `py.di.depends`. |
| **M3-D** TS router | `ts.framework.register` | Express lowercase verbs + `app.use` / `app.register`. Finite handler identifiers at registration site — allowlisted. Uppercase Go-like verbs keep legacy `go.di.route_register`. |
| **M3-E** inventory/linkme | `rs.di.linkme_distributed_slice` | `inventory::submit!` already covered by `rs.di.inventory_submit` (no duplicate id). Gap closed: linkme `#[distributed_slice]` source rule (not sidecar). |

Bare `@Injectable()` / `@Controller()` (no args) intentionally produce **no**
edges — do not invent fake callee names. Module metadata arrays are the
product value for real Nest.

Nest unwrapping details (still Heuristic registration, not HTTP ServeHTTP):
`forwardRef(() => M)` unwraps to `M` (never the helper); `X.forRoot()` /
`X.forRootAsync()` emit the receiver module name; `useFactory` bodies
contribute identifiers and call/new targets only; `inject: [Dep, 'TOKEN']`
is a dependency list. See [sound-subset.md](sound-subset.md) § Nest.

## Query windows

| Flag / mode | Exact | Heuristic | DynamicCandidate |
|---|:---:|:---:|:---:|
| default `callers` / `impact` | ✓ | ✓ | |
| `--exact-only` | ✓ | | |
| `--include-dynamic` | ✓ | ✓ | ✓ |
| SCIP export (default) | ✓ | ✓ | excluded |

## Reproducing / extending

1. Add fixtures under `fixtures/eval-l1/<corpus>/`.
2. Record golden edges in `fixtures/eval-l1/golden.json`.
3. Run `cargo test --test l1_eval -- --nocapture` and paste the table here.

Do **not** market L1 as sound or “zero missed dynamic calls” — see [PLAN.md](../PLAN.md) §0.2.
