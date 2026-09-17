# Large-repo evaluation — `stock-trading-app` (private, multi-language)

**Corpus:** private GitHub repo `jiangwuAwA/stock-trading-app` (not vendored here).  
**Languages (source bytes):** Rust ≈16.9 MB, TypeScript ≈0.9 MB, Python ≈0.4 MB, plus JS.  
**Checkout note:** shallow clone on Windows fails on a macOS `Icon?` path; ~995 source files still usable.

Reproduce (requires clone access):

```bash
agentgraph --root /path/to/stock-trading-app index
agentgraph --root ... callers <symbol> --exact-only
agentgraph --root ... callers <symbol>            # default: Exact+Heuristic
```

## Index performance (this machine, release build)

### After perf-plan P0 + P1 sid batch (this commit)

| op | result |
|---|---|
| Full index (`--force`) | **~27 s**（`resolve_sids_ms≈3.4s` 批量 relink；db≈13s, parse≈5s） |
| **Incremental noop** | **~1.0 s**（`noop_early_out`） |
| 1 file change (public `lib.rs`) | **~3.0 s** |
| 1k synthetic fixture noop | **~0.7 s** |
| Watch path-scoped | `index_paths`（≤64 路径；否则全量） |

Public NestJS `typescript-starter` (smoke): 8 files index OK.

TRACE (`AGENTGRAPH_TRACE=1`) 示例 noop：`walk≈164ms, read_hash≈40ms, phase=noop_early_out`。

### Baseline (before P0)

| op | result |
|---|---|
| Full index (`--force`) | **~39 s** → 995 files, **20 924** symbols, **167 600** refs, 0 parse failures |
| Incremental (no change) | **~21 s** |
| Languages | rust, typescript, tsx, python, javascript |

**P0 达成：** 1k noop **&lt; 2s**（真仓 0.8s / fixture 0.7s）。  
**未达成：** full `--force` 仍由全量 sid relink 主导（后续 P1 set-based SQL）。

## L0 vs L1 sample (callers, limit 500)

| symbol | Exact (L0) | Default (L0+L1) | Heuristic | Δ |
|---|---:|---:|---:|---|
| `order` | 7 | **24** | 17 | **+243%** |
| `execute` | 500* | 500* | 56 | L1 adds Heuristic under cap |
| `from_request_parts` | 3 | 4 | 1 | +33% |
| `authenticate` / `login` / `handle` / `token` / `run` / `pipeline` | n | n | 0 | L0 complete (direct calls) |
| `Strategy` | 0 | 0 | 0 | no bare-name refs (qualified elsewhere) |

\* hit `--limit 500` cap.

**Reading:** On this quant codebase, L1 helps where registry/route/map/impl patterns exist (`order`, `execute`); pure Rust call graphs are already mostly Exact. L1 is **additive candidates**, not a rewrite of L0.

## L2 `--sound` (sample)

Not fully golden-labeled on the private tree. Spot-check: clean Rust modules without `unsafe`/`reflect` report `subset_ok: true`; any `unsafe` block disables the eligibility claim (by design).

## Honesty

- Single private repo; **not** a published NestJS monorepo benchmark.
- No source code from the private tree is committed to agentgraph.
- Numbers are operator-run, not CI (CI uses public fixtures).

## Follow-ups

- Wire a CI job only if a **public** slice can be published.
- Incremental index ~21 s on ~1k files is a perf smell — **plan:** [perf-plan.md](perf-plan.md) (P0: mtime short-circuit, early-out, incremental sid, batch qualifiers).

---

# L1 sampling (this commit)

Operator-run L1 sampling on two **real** monorepos (not fixtures). Corpora stay outside agentgraph; no private source is committed.

**Machine:** Windows 10.0.26200, Intel Core i5-9300H @ 2.40 GHz, 15.8 GB RAM.  
**Binary:** `cargo build --release` → `target\release\agentgraph.exe` (v0.1.1 tree).  
**Date context:** operator session; numbers are wall-clock stopwatch around the CLI, not CI.

**Corpora:**

| corpus | path | notes |
|---|---|---|
| stock-trading-app | `D:\projects\eval-corpus\stock-trading-app` | private multi-language quant monorepo |
| nestjs-starter | `D:\projects\eval-corpus\nestjs-starter` | public NestJS starter (real Nest layout) |

Both trees already had `.agentgraph/` from prior runs; this session **force-reindexed** (`index --force`) then measured incremental noop.

## A. Index health

### stock-trading-app

| op | result |
|---|---|
| Full index (`--force`) | **~217 s** wall (single run; see machine note — slower laptop than earlier ~27 s baseline) |
| Incremental noop | **~1.4 s** |
| Files | 995 walked, **994 indexed**, **1 failed** (`crates/alpha-forge/src/adapter.rs`: not valid UTF-8) |
| Symbols | **20 924** |
| Refs | **168 351** |
| Languages | rust, typescript, tsx, python, javascript |
| Refs by confidence | exact **167 201** · heuristic **1 142** · dynamic_candidate **8** |
| Parse failures | 0 (the UTF-8 skip is a read failure, not a tree-sitter parse failure) |

Prior ~27 s full-force number above was measured on a different/faster machine configuration. Treat **217 s as this-machine truth**; noop remains in the ~1 s band.

### nestjs-starter

| op | result |
|---|---|
| Full index (`--force`) | **~4.9 s** |
| Incremental noop | **~0.8 s** |
| Files / symbols / refs | 8 / 7 / 64 |
| Languages | typescript |
| Refs by confidence | exact **64** · heuristic **0** · dynamic_candidate **0** |
| Parse failures | 0 |

## B. L0 vs L1 callers sampling

CLI: `callers <sym> --exact-only` vs default (Exact+Heuristic) vs `--include-dynamic`. Limit 500.

### stock-trading-app (≥8 interesting symbols + heuristic-heavy extras)

| symbol | lang/pattern | Exact (L0) | Default (Exact+Heur) | IncludeDynamic | Δ L1 | notes |
|---|---|---:|---:|---:|---|---|
| `order` | Rust trait `PipelineStage` | 7 | **24** | 24 | **+243%** | 17 Heuristic implementor edges (`rs.di.impl_trait`) |
| `execute` | Rust `ScheduledTask` / SQL | 500* | 500* | 500* | under cap | L1 adds Heuristic under the 500 limit |
| `from_request_parts` | Rust `FromRequestParts` | 3 | 4 | 4 | +33% | 1 Heuristic impl edge |
| `submit` | domain / trait | 16 | 23 | 23 | +44% | +7 Heuristic |
| `cancel` | domain / trait | 5 | 7 | 7 | +40% | +2 Heuristic |
| `from_ref` | axum `FromRef` | 7 | 17 | 17 | +143% | +10; 8× `impl FromRef for Arc` (generic qualifier) |
| `local_auth` | Rust trait `AuthState` | 4 | 7 | 7 | +75% | +3 implementors |
| `snapshot` | Rust trait method | 54 | 59 | 59 | +9% | +5 |
| `evaluate` | Rust trait `RiskEvaluator` | 42 | 49 | 49 | +17% | +7 |
| `create_run` | Rust repo trait | 25 | 26 | 26 | +4% | +1 |
| `cadence` | Rust trait method | 3 | 27 | 27 | **+800%** | L0 sparse; L1 is almost all implementors |
| `priority` | Rust trait method | 1 | 21 | 21 | **+2000%** | same |
| `cron_expr` | Rust trait field/method | 0 | 43 | 43 | **∞→43** | L0 empty; L1-only name |
| `schedule` | Rust trait method | 13 | 56 | 56 | **+331%** | +43 |
| `fmt` | std Display/Debug impls | 40 | 115 | 115 | +188% | high name-collision noise as “callers” |
| `drop` | std Drop impls | 142 | 179 | 179 | +26% | +37 |
| `default` | std Default impls | 500* | 500* | 500* | under cap | very common name |
| `authenticate` / `login` / `handle` / `token` / `run` / `pipeline` / `fill` / `route` / `register` | direct calls | n | n | n | 0 | L0 already complete |
| `Strategy` / `dispatch` | qualified names | 0 | 0 | 0 | 0 | bare-name miss |
| IncludeDynamic delta | — | — | — | +0 vs Default | 0 | all 8 DynamicCandidate names (`1`, `f`, `attr`, `key`, `default_socket`) did not enlarge these particular samples |

\* hit `--limit 500`.

**Reading (updated):** On this Rust-heavy monorepo, L1’s lift is **dominated by `rs.di.impl_trait`** (1 137 / 1 142 Heuristic edges). That is useful for *implementor / impact* discovery (`order`, `cadence`, `priority`, `cron_expr`), but those edges are stored as `kind=call` and therefore flood `callers` for high-collision names (`fmt`, `default`, `drop`, `execute`). Pure direct-call symbols stay L0-complete. `--include-dynamic` added nothing on these samples.

### impact --sound samples (stock-trading-app)

| query | depth | mode | subset_ok | promise_tier | note |
|---|---:|---|---|---|---|
| `impact order --sound` | 2 | sound | **false** | **disabled** | S violated — eligibility claim disabled (honest). Still returns best-effort sound-eligible edges (Exact + some Heuristic implementors at depth 1). |
| `impact from_request_parts --sound` | 2 | sound | **false** | **disabled** | Same; depth-2 Exact imports of `AuthUser` expand blast radius usefully. |
| `impact execute` (default) | 1 | default | n/a | n/a | Exact SQL/cron call sites + Heuristic `ScheduledTask` implementors. |

DISABLED is **honest** on this tree: 113 S-violations (unsafe / parse_error / std_ptr / py dynamic / nonliteral keys). See §D.

## C. Heuristic noise proxy (stock-trading-app)

No golden labels on private code. Counts + human classification of source lines only. **Not production precision.**

### Counts by rule

| confidence | rule_id | count |
|---|---|---:|
| heuristic | `rs.di.impl_trait` | **1 137** |
| heuristic | `ts.event.subscribe` | **5** |
| dynamic_candidate | `ts.dynamic.computed` | **5** |
| dynamic_candidate | `py.dynamic.getattr` | **3** |
| (exact) | — | 167 201 |

### Classified sample (n=25: 17 Heuristic + 8 DynamicCandidate)

Heuristic (n=17):

| verdict | n | examples |
|---|---:|---|
| **plausible** | 13 | `impl PipelineStage for MockStage` → `order`; `impl ScheduledTask for NorthboundSync` → `cadence`; `addEventListener('storage', listener)` → `listener`; `impl RiskEvaluator for ComplianceRule` → `evaluate` |
| **suspicious** | 3 | `subscribe('tokio_tasks', anon)` labeled `setLiveSnapshots` (name ≠ callback); 8× `impl FromRef for Arc` (qualifier too generic); `proxy.on('error', anon)` labeled `error` (event key as name) |
| **clearly wrong** | 1 | same `proxy.on('error', …)` case — claiming a *call* to a symbol named `error` is false |

DynamicCandidate (n=8, census):

| verdict | n | examples |
|---|---:|---|
| **plausible** | 1 | `getattr(bs_context, "default_socket", None)` → `default_socket` |
| **suspicious** | 1 | `METRIC_FORMAT[key]?.(…)` → name `key` (loop var, not a stable symbol) |
| **clearly wrong** | 6 | `result.current[1](…)` ×4 (numeric tuple index); `getattr(obj, f)` / `getattr(manager, attr)` (loop vars) |

**Rough precision proxy (sampled, not labeled):**

| set | n | plausible/n | not-clearly-wrong |
|---|---:|---:|---:|
| Heuristic only | 17 | **13/17 ≈ 76%** | 16/17 ≈ 94% |
| DynamicCandidate | 8 | **1/8 ≈ 13%** | 2/8 ≈ 25% |
| Combined L1 | 25 | **14/25 ≈ 56%** | 18/25 ≈ 72% |

**Interpretation:** Heuristic (mostly trait-impl) edges have **correct evidence snippets** and are mostly useful as *implementor* candidates; as *callers* they over-approximate and collide on common names. DynamicCandidate is **expected-noisy** (loop vars / numeric indices) — which is why default query windows exclude it.

## D. L2 spot-check

### subset violation kinds

**stock-trading-app** — `in_subset: false`, `promise_tier: disabled`, **113** violations:

| kind | count |
|---|---:|
| unsafe | 46 |
| parse_error | 31 |
| std_ptr | 12 |
| py_dynamic_attr | 6 |
| nonliteral_computed_key | 5 |
| py___import__ | 4 |
| nonliteral_event_key | 3 |
| transmute | 2 |
| py_getattr_dynamic | 2 |
| unmodeled_event_handler | 1 |
| monkey_patch | 1 |

**nestjs-starter** — `in_subset: true`, `promise_tier: ast_modeled`, **0** violations. `impact getHello --sound` and `callers AppService --sound` both report `subset_ok: true` with the AST-modeled promise string (registration ≠ HTTP ServeHTTP — still an engineering S gate).

**Honesty:** `promise_tier: disabled` on stock-trading-app is **honest and correct** — the tree has `unsafe`, `std::ptr`, tree-sitter ERROR nodes, and Python dynamic attrs. Do not market L2 soundness on this repo.

---

## nestjs-starter subsection (L1 on real Nest)

### Historical note (pre-Nest-module rules)

Measured **before** `ts.nest.module_*` / `ts.nest.ctor_inject` landed: 8 TS
files, 7 symbols, **64 Exact / 0 Heuristic**. Bare `@Injectable()` /
`@Controller()` and `@Module({ providers, controllers, imports })` produced
**no** L1 lift; L0 already had the import graph. That zero is preserved here
as the honest baseline.

### After Nest module + ctor DI rules (this commit)

| op | result |
|---|---|
| Full index (`--force`) | 8 files · 7 symbols · **68 refs** |
| Refs by confidence | exact **64** · **heuristic 4** · dynamic_candidate 0 |
| Parse failures | 0 |

Heuristic edges (all sound-allowlisted registration over-approx):

| rule_id | name | enclosing | site |
|---|---|---|---|
| `ts.nest.module_providers` | `AppService` | `AppModule` | `providers: [AppService]` |
| `ts.nest.module_controllers` | `AppController` | `AppModule` | `controllers: [AppController]` |
| `ts.nest.module_imports` | `ObserveModule` | `AppModule` | `imports: [ObserveModule.forRoot(...)]` |
| `ts.nest.ctor_inject` | `AppService` | `AppController` | `constructor(...: AppService)` |

### L0 vs Default callers (after)

| symbol | Exact (L0) | Default | Heuristic | Δ notes |
|---|---:|---:|---:|---|
| `AppService` | 3 | **5** | 2 | + providers (AppModule) + ctor_inject (AppController) |
| `AppController` | 2 | **3** | 1 | + controllers (AppModule) |
| `ObserveModule` | 0 | **1** | 1 | + imports forRoot (AppModule) — L0 empty |
| `getHello` / `AppModule` / `bootstrap` | n | n | 0 | L0 complete (direct calls / imports) |

`impact AppService` BFS expands through Heuristic edges to `AppController`
and `AppModule` (depth 2). Registration ≠ HTTP ServeHTTP still holds.

---

## S map + golden boundary (this session — see also dedicated docs)

- **Per-crate S-violation map + clean/dirty ranks:** [eval-stock-s-map.md](eval-stock-s-map.md)
- **Golden L0 vs L1 (31 hand-labeled edges) + claim policy:** [eval-stock-boundary.md](eval-stock-boundary.md)

Headline numbers after product rule `rs.di.inventory_submit` + force reindex:

| item | value |
|---|---|
| Refs | 168 088 (exact 166 900 · heuristic **1 180** · dyn 8) |
| Heuristic delta vs prior sample | 1 142 → 1 180 (**+36** inventory registration/factory) |
| Golden L0 Exact | **21/31 (68%)** on clean+mixed crates |
| Golden L1 Default (before inventory rule) | 27/31 (87%) |
| Golden L1 Default (after) | **31/31 (100%)** |
| Full-tree `subset_ok` | **false** (`promise_tier: disabled`, 115 violations) |
| Cleanest crates for scoped `--sound` trials | `repository`, `model-selection-replay`, `risk-intent-authority`, `data-sources`, `event-engine` |
| Dirtiest (no sound claim) | `nn-ranker`, `src-tauri`, `model-selection-installer`, `alpha-forge`, `scheduler` |

`tokio::spawn` inner calls and `async_trait` method bodies are already **Exact** at L0 — no extract change needed there.

## Honesty limits (L1 sampling)

- Single private monorepo + one public Nest starter; **not** a published multi-repo benchmark.
- Noise “precision” is a **manual source-line classification of 25 edges**, not golden labels. Do **not** ship as a production precision number.
- Heuristic `rs.di.impl_trait` edges are *implementor* relationships stored as `kind=call` — useful for impact, over-approximate for callers.
- Real NestJS starter: **L1 lift was 0%** before `ts.nest.module_*` / `ts.nest.ctor_inject`; after those rules, Heuristic **0 → 4** and Default callers for `AppService`/`AppController`/`ObserveModule` grow (see nestjs-starter subsection). Do not generalize fixture L1 recall to production Nest without re-measuring.
- DynamicCandidate remains excluded from default windows; its sampled precision is poor (loop vars / indices) as expected.
- Index timings are single-operator wall clock on a laptop CPU; not CI, not multi-trial.
- No source from either corpus is committed to agentgraph.

## Reproduce (operator)

```powershell
cargo build --release
$exe = "target\release\agentgraph.exe"

# stock-trading-app
& $exe --root D:\projects\eval-corpus\stock-trading-app index --force
& $exe --root D:\projects\eval-corpus\stock-trading-app index            # noop
& $exe --root D:\projects\eval-corpus\stock-trading-app callers order --exact-only
& $exe --root D:\projects\eval-corpus\stock-trading-app callers order
& $exe --root D:\projects\eval-corpus\stock-trading-app impact order --sound --depth 2
& $exe --root D:\projects\eval-corpus\stock-trading-app subset

# nestjs-starter
& $exe --root D:\projects\eval-corpus\nestjs-starter index --force
& $exe --root D:\projects\eval-corpus\nestjs-starter callers AppService
& $exe --root D:\projects\eval-corpus\nestjs-starter callers AppController
& $exe --root D:\projects\eval-corpus\nestjs-starter callers ObserveModule
& $exe --root D:\projects\eval-corpus\nestjs-starter impact AppService
& $exe --root D:\projects\eval-corpus\nestjs-starter impact getHello --sound
& $exe --root D:\projects\eval-corpus\nestjs-starter subset
```

Read-only SQLite helpers used for rule counts / edge sampling (operator scripts, not product code):

```powershell
python scripts\eval_heur_stats.py D:\projects\eval-corpus\stock-trading-app 50
python scripts\eval_dump_nest.py D:\projects\eval-corpus\nestjs-starter
python scripts\eval_sample_heur_edges.py D:\projects\eval-corpus\stock-trading-app 20
python scripts\eval_subset_kinds.py D:\projects\eval-corpus\stock-trading-app
```
