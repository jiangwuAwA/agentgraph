# Adversarial loop changelog (R14–R20)

Honest record of residual adversarial fixes. Not a marketing list — each item
is a **real** behavior change or an explicitly accepted over-approx.

## R14–R19 (extract / S / watch / Nest / Go / Python)

- **Nest full DI surface:** `@Module({ providers, controllers, imports, exports })`
  including `{ provide, useClass, useExisting, useFactory, inject }`, string
  tokens, `forwardRef`, `X.forRoot()` / `forRootAsync({ imports, inject, useFactory })`,
  and constructor type-injection (`ts.nest.ctor_inject`).
- **AST S scanners:** `scan_py` / `scan_go` / `scan_rust` are tree-sitter walks
  (fail-closed on parse error). Python dynamic attr / Go unsafe-reflect / Rust
  unsafe-transmute rules live in `src/index/subset.rs`.
- **Watch dir/file rename:** extensionless Remove/rename under root triggers
  reindex (Windows notify reports directories). File rename replaces path rows
  without duplicates; delete cascades Heuristic refs.
- **Dangling sids:** incremental delete / path-scoped reindex prunes stale
  `resolved_symbol_id` rows.
- **Go type-switch:** pointer / multi-type / qualified cases emit L0/L1 edges.
- **Eval aliases:** Python `from-import` eval, JS import eval, `(0, eval)`,
  `Function.bind` / constructor-chain — all leave S.
- **Property tests:** S_js + S_py + S_go generators (`tests/l2_property.rs`).

## R20 residual (this round)

### Correctness probes — no Critical/Major

| Probe | Result |
|---|---|
| Multi-process SQLite WAL (4 concurrent `index --force` + 3 concurrent `callers` on 200-file tree) | All succeeded; `PRAGMA integrity_check=ok`; no `database is locked`. `busy_timeout=5000` + WAL serialize writers. Residual: last writer wins; no advisory cross-process lock (documented, not a crash). |
| MCP tools/list vs CLI after Nest/promise/sound | callers/impact expose `sound` / `recall` / `exact_only` / `include_dynamic` matching CLI. **Fixed:** `enrich` default limit was MCP 30 vs CLI 50 — aligned to 50 (schema + runtime). `export` / `watch` / `bench-query` remain CLI-only (intentional). |
| Watch burst debounce (100 events / 50ms) | Debounce coalesces; single reindex after quiet window. No correctness bug (slow path only). |
| nestjs-starter reindex after R14–R19 | 8 files · 7 symbols · 68 refs (**64 exact / 4 heuristic**). `subset_ok: true`, `promise_tier: ast_modeled`. `AppService` Default callers = 5 (3 Exact + 2 Heuristic: `module_providers` + `ctor_inject`). `AppController` +1 (`module_controllers`). `ObserveModule` +1 (`module_imports` forRoot). Registration ≠ HTTP ServeHTTP. |

### Docs honesty pass

- `docs/sound-subset.md`: Nest `ts.nest.*` table; registration ≠ ServeHTTP;
  type-only `typeof Function` over-flag (fail-closed).
- `docs/eval-l1.md`: rules table lists `module_exports`, `forRootAsync`,
  `inject` / `useFactory`, string tokens, `ctor_inject`.
- `AGENTS.md`: L1/L2 one-liners mention Nest `ts.nest.*`, AST S, property tests,
  and the `typeof Function` over-flag.
- `formal/TODO.md`: S_py/S_go AST + property tests moved to done.

## Known accepted over-approx (do not “fix” as bugs)

- `typeof Function` in type position fail-closes S.
- Event names as DynamicCandidate callee names.
- `rs.di.impl_trait` implementor edges stored as `kind=call` (useful for
  impact; noisy as callers on high-collision names).
- Nest registration edges are not HTTP ServeHTTP call-graph edges.
