# Adversarial loop changelog (R14–R25)

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

## R21 residual

- **index_paths outside-root:** deleted/rename paths that fail
  `rel_path_under_root` (UNC / short-name / alias forms) no longer silently
  miss prune; parent-canonicalize re-join recovers the leaf.
- Probe class: path-form edge cases under Windows notify.

## R22 residual

- **is_source_event both-sides path match:** event paths that differ lexically
  from the canonical root (macOS `/var`, Windows junctions) are accepted; the
  keep-set comes from the walker’s resolved rel paths — never a naive
  `strip_prefix` + `unwrap_or(abs)` that would put absolute paths in the keep
  set and prune every store row.

## R23 residual

- **Keep-set for oversized/minified:** walker mints them as S violations but
  omits them from `files`; prune used to CASCADE-delete the violation in the
  same pass, so `--sound` wrongly claimed `in_subset=true`. Fixed: keep-set
  includes `oversized_paths` ∪ `minified_paths`.
- **importers path forms:** `src\auth.ts` / `./src/auth.ts` / `/src/auth.ts`
  normalize to the store’s repo-relative `/` form.
- **file_uri percent-encoding:** unicode and literal `%` in roots encode once
  (`%` → `%25`, not double-encoded).

## R24 residual

- **UTF-8 / read failures mint parse_error** on full index and watch
  (`record_parse_error`). Previously silent skips left keep-set files looking
  certified.
- **index_paths size / `.min.` gate:** a watch event on a file that grew past
  1.5 MiB (or was renamed to `*.min.*`) must mint, not fully parse and certify.
  Sibling oversized/minified files re-mint from the walker when another path
  is dirty.
- **Go `//export`:** cgo export directive leaves S (same class as
  `//go:linkname`). `//export Foo`, `//export\tFoo`, bare `//export` covered.
- **S edge forms:** TS `export = eval` / `export default eval`, Python eval
  via namespace dict / bare value — leave S.
- **Nest forwardRef cycle:** A↔B both emit `module_imports` edges; the
  `forwardRef` helper itself is not a callee.
- **impact --sound** traverses `ts.nest.module_exports`.

## R25 residual (this round)

### A1 — `record_parse_error` completeness matrix

Enumerated every skip path in `index()` / `index_paths()`. **No Critical or
Major gap.** Contract, pinned by table-driven `tests/r25_adversarial.rs`
(`skip_path_completeness_matrix`):

| Skip path | Keep-set? | Mint? | Verdict |
|---|---|---|---|
| Oversized (>1.5 MiB) | yes | `parse_error` | OK (R13/R23/R24) |
| Minified (`*.min.*`) | yes | `parse_error` | OK (R13/R23/R24) |
| Read I/O error | yes | `parse_error` | OK (R24) |
| Non-UTF-8 | yes | `parse_error` | OK (R24) |
| Extract / parse failure | yes | `parse_error` | OK (R12/R24). Practical note: tree-sitter almost always returns a tree; `has_error` is caught by the subset scanner as a violation on the indexed file. |
| mtime+size short-circuit | yes | none (previously indexed) | OK — true in-S file |
| Content-hash unchanged | yes | none (previously indexed) | OK — true in-S file |
| Noise dirs (`node_modules`, `testdata`, …) | **no** | none | OK — out of corpus by design; not claimed in S |
| Unsupported extension | **no** | none | OK — out of corpus |
| Outside root (`index_paths`) | **no** | none | OK — out of corpus |
| DB write failure (savepoint rollback) | path already in keep | none | **Accepted residual (Minor):** previous rows stay; new content not ingested. Not a new S claim (no fresh certification). Not worth minting on a DB fault. |

Regression also pins recovery: a `parse_error` file that becomes valid UTF-8
is re-indexed and the violation is cleared (`parse_error_cleared_when_file_recovers`).

### A2 — Go `//export` forms

- `//export Foo`, `//export\tFoo`, `//export`, `//export  Foo` → leave S.
- `/*export Foo*/` → **stays in S** (not a valid cgo directive). Over-flagging
  block comments would false-disable S; under-flagging a real `//export` is
  the bug we fixed. Pinned by `go_block_comment_export_not_a_directive`.

### A3 — file_uri vs SCIP unicode

Already covered R23/R24 (`file_uri_encodes_unicode`,
`file_uri_encodes_literal_percent_not_double`). No new finding.

### A4 — stock-trading-app

Skipped: A1 found no gap.

### B — Docs honesty (this round)

- `docs/sound-subset.md`: uncertified-source → `parse_error` keep-set table;
  Nest allowlist ↔ `SOUND_HEURISTIC_RULES` sync note; non-nest allowlist ids listed.
- `docs/eval-l1.md`: removed phantom rule ids `ts.nest.forRootAsync` /
  `ts.nest.string_token` (they are shapes, not emitted ids).
- `tests/l2_subset.rs`: sound-eligible loop now includes `ts.nest.module_exports`.
- `AGENTS.md`: keep-set / `parse_error` certification one-liner.
- README / README.zh-CN: no “零漏报” / zero-miss overselling found.

## Known accepted over-approx (do not “fix” as bugs)

- `typeof Function` in type position fail-closes S.
- Event names as DynamicCandidate callee names.
- `rs.di.impl_trait` implementor edges stored as `kind=call` (useful for
  impact; noisy as callers on high-collision names).
- Nest registration edges are not HTTP ServeHTTP call-graph edges.
- `//go:linkname` inside a block comment over-flags S (fail-closed).
- DB write failure on an already-indexed file leaves prior rows (stale data,
  not a new soundness claim).
