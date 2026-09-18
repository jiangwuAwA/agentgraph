# stock-trading-app boundary — S map, golden L0/L1, claim policy

**Status:** operator boundary report on a **private** quant monorepo; source never committed; CI skips.
**Non-claim:** per-crate “clean” is a **trial candidate**, not product soundness. No zero-miss / ecosystem-sound / macro-complete / production-sound guarantee. Not a complete runtime graph.
**Reproduce:** operator-only scripts below (private checkout required). Public synthetic stand-in: [`fixtures/eval-goldens/`](../fixtures/eval-goldens/).

**Corpus:** private multi-language quant monorepo at `D:\projects\eval-corpus\stock-trading-app`  
(≈757 `.rs` + TS/TSX/Python; **source is never committed** to agentgraph).  
**Index:** `.agentgraph/index.db` (operator force-reindex after product change).  
**Machine note:** operator laptop; numbers are wall-clock / SQLite counts, not CI.

Related:

- Per-crate S table: [eval-stock-s-map.md](eval-stock-s-map.md) (+ `.tsv`)
- Prior large-repo sampling: [eval-large-repo.md](eval-large-repo.md)
- S / promise rules: [sound-subset.md](sound-subset.md)
- Machine-readable scoped candidates (`sound_candidates` / one-click recipe): [agent-recipes.md](agent-recipes.md), [workspace.md](workspace.md)

Reproduce scripts (operator):

```powershell
python scripts\stock_s_map.py
python scripts\stock_golden_eval.py D:\projects\eval-corpus\stock-trading-app
python scripts\eval_heur_stats.py D:\projects\eval-corpus\stock-trading-app 20
python scripts\eval_subset_kinds.py D:\projects\eval-corpus\stock-trading-app
```

---

## A. S map (summary)

Full crate table lives in [eval-stock-s-map.md](eval-stock-s-map.md). Highlights after this session’s force reindex:

| metric | value |
|---|---:|
| Indexed files | 995 (994 OK, 1 UTF-8 fail `alpha-forge/adapter.rs`) |
| Symbols / refs | 20 896 / **168 088** |
| Exact / Heuristic / DynamicCandidate | 166 900 / **1 180** / 8 |
| `subset_violations` | **115** (`promise_tier: disabled` on full tree; type-only `Function` over-flag **fixed in M2 residual** — remaining violations are real unsafe/reflect/dynamic) |

Violation kinds (authority = AST `subset_violations`):

| kind | count |
|---|---:|
| unsafe | 46 |
| parse_error | 32 |
| std_ptr | 12 |
| py_dynamic_attr | 6 |
| nonliteral_computed_key | 5 |
| py___import__ | 4 |
| nonliteral_event_key | 3 |
| transmute | 2 |
| py_getattr_dynamic | 2 |
| unmodeled_event_handler | 1 |
| monkey_patch | 1 |
| Function | 1 |
| **total** | **115** |

### Top-5 cleanest Rust crates (0 unsafe_db, 0 unsafe_src, 0 parse_error)

Preferred for **scoped `--sound` trials** (single-crate root or path-scoped index). Full-repo `subset_ok` stays **false**.

| rank | crate | rs files | viol_total | async_fn | async_trait | derive | why interesting |
|---:|---|---:|---:|---:|---:|---:|---|
| 1 | `repository` | 27 | 0 | 130 | 44 | 32 | real trait + `RepoRegistry` DI |
| 2 | `model-selection-replay` | 22 | 0 | 0 | 0 | 46 | derive/trait heavy, sync |
| 3 | `risk-intent-authority` | 22 | 0 | 0 | 0 | 41 | domain authority traits |
| 4 | `data-sources` | 18 | 0 | 29 | 0 | 12 | async ingest |
| 5 | `event-engine` | 17 | 0 | 0 | 0 | 22 | pure domain decide/evolve |

Also clean and useful: `auth`, `trading`, `pipeline`, `execution-traits`, `common*`.

### Top-5 dirtiest Rust crates (do **not** claim `--sound`)

| rank | crate | rs | unsafe_db | ptr | parse_error | viol_total |
|---:|---|---:|---:|---:|---:|---:|
| 1 | `nn-ranker` | 28 | 11 | 4 | 4 | 19 |
| 2 | `src-tauri` | 4 | 12 | 6 | 0 | 18 |
| 3 | `model-selection-installer` | 23 | 17 | 0 | 0 | 17 |
| 4 | `alpha-forge` | 13 | 0 | 4 | 2 | 6 |
| 5 | `scheduler` | 48 | 2 | 0 | 3 | 5 |

**Mixed (parse_error only, no unsafe):** `strategy-plugins` (3 tree-sitter ERROR files: `ai_signal.rs`, `debate_signal.rs`, `ml_selector.rs`), `storage` (5 parse_error), `service` (2), `api` (3). These still leave full-tree S, but **product code outside the ERROR files extracts normally** — honest for L0/L1 recall measurement, not for `--sound`.

`parse_error` is mostly `tree-sitter ERROR nodes — cannot certify S` (fail-closed), plus one non-UTF-8 read (`alpha-forge/src/adapter.rs`).

---

## B. Golden samples — L0 vs L1 on real edges

**Method:** operator labeled ≥15 call/impl/registry edges from private source (not committed), then measured Exact-only vs Default (Exact+Heuristic) against `index.db` refs. Script: `scripts/stock_golden_eval.py`.

**Crates chosen:**

| bucket | crates | why |
|---|---|---|
| clean | `repository`, `event-engine`, `auth` | 0 S violations; trait + async + pure domain |
| mixed | `strategy-plugins` (+ neighbor `pipeline`) | inventory registry + trait impls; 3 parse_error files |
| noise proxy | corpus-wide | `fmt` / `drop` / `default` |

### Results (this commit, after `rs.di.inventory_submit`)

| set | labeled | L0 Exact | L1 Default | Δ |
|---|---:|---:|---:|---|
| clean-repo (`repository`) | 11 | 8 (73%) | **11 (100%)** | trait impls L1-only |
| clean-event (`event-engine`) | 5 | 5 (100%) | 5 (100%) | L0 complete |
| clean-auth (`auth`) | 3 | 3 (100%) | 3 (100%) | L0 complete |
| mixed-plugins | 12 | 5 (42%) | **12 (100%)** | impl_trait + inventory |
| **total labeled** | **31** | **21 (68%)** | **31 (100%)** | **+32 pp** |

**Baseline before inventory rule (same labels):** L0 21/31 (68%), L1 **27/31 (87%)** — the four inventory registry goldens (P8–P11) were honest misses.

### Golden edge table

| id | crate | kind | name | L0 | L1 | notes |
|---|---|---|---|---:|---:|---|
| R1–R3 | clean-repo | direct_call | `normalize_freq` | ✅ | ✅ | called inside `PgKlineRepo` methods |
| R4–R6 | clean-repo | trait_impl | `insert` / `query_range` / `latest_close` | ❌ | ✅ | `impl KlineRepository for PgKlineRepo` via `rs.di.impl_trait` |
| R7 | clean-repo | direct_call | `latest_close` | ✅ | ✅ | `TradingServiceImpl::submit_order` |
| R8–R9 | clean-repo | direct_call | `query_range` / `find_by_code` | ✅ | ✅ | market service |
| R10 | clean-repo | registry | `RepoRegistry` | ✅ | ✅ | `import` into service (DI field `Arc<RepoRegistry>`) |
| R11 | clean-repo | import | `KlineRepository` | ✅ | ✅ | trait import at impl site |
| E1–E5 | clean-event | call/import | `decide` / `evolve` / `submit` | ✅ | ✅ | includes qualified `event_engine::evolve` |
| A1–A3 | clean-auth | direct_call | `expose_secret` / `constant_time_eq` / `digest` | ✅ | ✅ | token module |
| P1–P3 | mixed | trait_impl | `generate` on Momentum/Vwap/Trend | ❌ | ✅ | `SignalGenerator` implementors |
| P4–P5 | mixed | direct_call | `last_return` | ✅ | ✅ | strategy generate bodies |
| P6–P7 | mixed | direct_call | `registered_strategies` / `run_registered_fusion` | ✅ | ✅ | registry **reader** / adapter |
| P8–P10 | mixed | registry | `AiSignalStrategy` / `MomentumRotationStrategy` / `VwapStrategy` | ❌ | ✅ | `rs.di.inventory_submit` factory types |
| P11 | mixed | registry | `StrategyRegistration` | ❌ | ✅ | registration type ×18 sites |
| P12 | mixed | direct_call | `build_default_pipeline` | ✅ | ✅ | pipeline tests |

### Noise proxy (honest — L1 invents call-kind edges on common names)

| name | exact `kind=call` | heuristic (mostly `rs.di.impl_trait`) | reading |
|---|---:|---:|---|
| `fmt` | 11 | **75** | Display/Debug impls flood `callers fmt` |
| `drop` | 142 | **37** | Drop impls |
| `default` | 515* | **55** | Default impls / name collision |

\* sample cap in prior runs hit 500; full exact-call count is larger.

**Do not treat Default `callers <std-ish name>` as a call graph.** Prefer `--exact-only` or qualified impact. L1 is **implementor / registration candidates**, not a rewrite of L0.

### Noise governance after L1 cut (query-time separation + high-freq demote)

Same private store; **no store rewrite** — default `callers` now separates
implementors and caps high-frequency names (docs/noise-governance.md).

| name | default payload | `callers[]` | `implementors[]` | `implementor_count` | truncated |
|---|---|---:|---:|---:|---|
| `fmt` | wrapped object | 40 (Exact call/import + registration) | **20** (cap) | **75** | true |
| `drop` | wrapped object | 50 | **20** | **37** | true |
| `default` | wrapped object | 50 | **20** | **48** | true |
| `fmt --exact-only` | plain array | 40 | — | 0 | — |

Reading after the cut:

- Default is **no longer a 75-row mixed implementor flood** — implementors live
  in a separate `implementors[]` section with `edge_role=implementor`.
- High-frequency names (`fmt`/`drop`/`default`) cap implementors at 20 +
  `implementors_truncated=true` while keeping `implementor_count` honest.
- Exact user/import rows stay in `callers[]` and are **not dropped**.
- `--include-implementors` restores merge-all (old noisy shape + role tags).
- Store still holds every edge; `impact` still expands implementors for blast
  radius (rows tagged `edge_role`).

Reproduce (operator, private corpus):

```powershell
cargo run -- --root D:\projects\eval-corpus\stock-trading-app callers fmt
cargo run -- --root D:\projects\eval-corpus\stock-trading-app callers fmt --exact-only
cargo run -- --root D:\projects\eval-corpus\stock-trading-app callers fmt --include-implementors
```

Public fixture regression: `tests/noise_roles.rs`.


### What L1 did **not** invent

- Pure direct-call symbols (`decide`, `evolve`, `normalize_freq`, `last_return`, `registered_strategies`) stay L0-complete.
- Qualified Rust paths (`event_engine::evolve`) are Exact under bare name `evolve`.
- `tokio::spawn(async { … })` **inner calls are already Exact** (tree-sitter walks the block) — verified on `trading/persist.rs` (`create`/`upsert`/`is_transient_db_err` inside spawn) and `auth/tests/security_core.rs` (`authorize` inside spawn). **No product change needed for tokio.**
- `async_trait` method **bodies** extract as normal async fns; implementor edges come from `rs.di.impl_trait` (L1), not missing L0.
- **`unsafe` block / `unsafe` fn call sites are already Exact L0** (Track A pin). `walk_rust` visits `call_expression` under `unsafe_block` and unsafe-fn bodies the same as safe code. Corpus system APIs (`File::from_raw_fd`, `UnixStream::from_raw_fd`, `libc::geteuid`, `libc::fcntl` in `api/src/security.rs`, `src-tauri/src/lib.rs`, model-selection storage/installer) extract as Exact `call` refs with enclosing fn + qualifier. **S still flags `unsafe`** — full-tree `subset_ok` remains **false**; do **not** enable `--sound` on unsafe crates. Product change not required; tests: `tests/rust_unsafe_calls.rs`. Honesty note in [sound-subset.md](sound-subset.md).

---

## C. Product change (justified)

### Shipped: `rs.di.inventory_submit` (Heuristic)

**Gap (real, this repo):** strategy-plugins registers ~18 strategies via

```rust
inventory::submit! { StrategyRegistration { name: "ai", factory: || Box::new(AiSignalStrategy::new("ai")), } }
```

Tree-sitter sees the macro call as `macro_invocation` + `token_tree`; L0 did **not** mint edges to factory types. Golden P8–P11 were misses before the rule.

**Change (TDD `tests/l1_rules_inventory.rs`):** on `inventory::submit` macro bodies, emit Heuristic edges (`kind=call`, rule `rs.di.inventory_submit`) for:

1. Registration type identifier (e.g. `StrategyRegistration`)
2. Factory constructor types (`Type::new` inside the body; skip `Box`/`Self`)

**Explicitly not done:**

- No general proc-macro expansion
- No unsound Heuristic beyond finite-domain identifiers **written at the call site**
- No restoring `--sound` on unsafe crates
- `tokio::spawn` / async_trait already fine at L0 — left alone

**Sound allowlist:** `rs.di.inventory_submit` added to `SOUND_HEURISTIC_RULES` (finite-domain registration over-approx, registration ≠ runtime call) — same class as `ts.nest.module_*`. Full-tree `subset_ok` remains **false** regardless.

**Index delta (force reindex):** Heuristic 1 142 → **1 180** (**+36** = 18× registration + 18× factory type). Exact unchanged at 166 900.

**CLI after change:**

```text
callers AiSignalStrategy
  exact import @ lib.rs:12
  heuristic @ lib.rs:47  rule=rs.di.inventory_submit  snippet=inventory::submit! { StrategyRegistration { name: "ai", …
callers MomentumRotationStrategy
  exact import @ lib.rs:12
  heuristic @ lib.rs:58  rule=rs.di.inventory_submit
```

---

## D. Claim policy for this corpus

### Will claim

- **Index scale:** ~995 files / ~21k symbols / ~168k refs indexable on a laptop; incremental noop ~1 s band.
- **L0 Exact recall on clean crates:** high for direct calls, including async/async_trait bodies and qualified paths (golden clean-event/auth 100%).
- **L1 as implementor/registry candidates:** trait impls (`rs.di.impl_trait`) + inventory registration (`rs.di.inventory_submit`) lift labeled goldens from **68% → 100%** on this hand-labeled set (31 edges).
- **Honest S gate:** full-tree `promise_tier: disabled` while violations exist; per-crate clean set is a **trial candidate**, not a product soundness claim.

### Will **not** claim

- Full-repo `impact/callers --sound` with `subset_ok: true` — **false** on this tree (unsafe + parse_error + ptr + py dynamic).
- Soundness on `nn-ranker`, `src-tauri`, `model-selection-installer`, or any crate with `unsafe` / `transmute` / `std_ptr`.
- Production precision from fixture L1 noise numbers; stock L1 still floods `callers` on `fmt`/`default`/`drop`.
- Macro-complete call graphs (`inventory` edges are registration candidates only; `inventory::iter` runtime fan-out is not modeled).
- DynamicCandidate quality (`py.dynamic.getattr` / `ts.dynamic.computed` still expected-noisy; excluded from default windows).

**Track A (unsafe-call L0 visibility) is a pin, not a soundness claim.** Operator smoke on the existing stock index:

```text
callers from_raw_fd --exact-only
  exact call @ crates/api/src/security.rs:118  enclosing=LocalBootstrap::deliver_to_inherited_fd  qualifier=File
  exact call @ crates/api/src/security.rs:128  enclosing=LocalBootstrap::deliver_to_inherited_fd  qualifier=UnixStream
  … more OwnedFd/File sites in model-selection-installer/storage …

callers geteuid --exact-only
  exact call @ src-tauri/src/lib.rs:171  enclosing=open_private_regular_file  qualifier=libc
  exact call @ crates/model-selection-installer/src/local_install.rs:469  … qualifier=libc
  …

subset (full tree)
  in_subset=false  promise_tier=disabled  violation_count=115
  unsafe @ crates/api/src/security.rs:118  snippet=unsafe { File::from_raw_fd(fd) }
  unsafe @ crates/api/src/security.rs:128  snippet=unsafe { UnixStream::from_raw_fd(...) }
```

Same sites appear as Exact call refs **and** `unsafe` S violations — graph edges + sound off.

### Operator next steps (optional)

1. Path-scoped or single-crate index of `repository` / `event-engine` for `--sound` demos where `subset_ok` can be true on that slice.
2. Investigate tree-sitter ERROR files (often very large generated/contract tests) — recovery would shrink `parse_error`, not unsafe.
3. Do not market L2 on the full monorepo.
4. Macro-expand **coverage notes** (pre-M1 spike dual-root experiments) — see [eval-macro-expand.md](eval-macro-expand.md). Operator expand remains optional; **product path** is M1 `--macro-expanded-root` + path_map + de-dup + `macro rebuild` (default OFF; **not sound**; not required CI). Dual-root-only language in older spike notes is historical.

---

## Macro expand spike (summary)

Full write-up: [eval-macro-expand.md](eval-macro-expand.md). Shadow root (operator, never commit):
`D:\projects\eval-corpus\stock-trading-app-expanded\`.

| finding | value |
|---|---|
| Real expand tooling here | `cargo expand` / nightly blocked; **`RUSTC_BOOTSTRAP=1 cargo rustc -Zunpretty=expanded` works** |
| Stock crate real expand | blocked on crates.io this machine |
| Synthetic + real mini-spike index | **no parser panic** after UTF-8 |
| Source-view vs expanded-view (4 crates) | symbols **1100 → 2140**; `fmt` +131, `clone` +121, inventory registrar-shaped +36 |
| Dual-index noise (pre-M1 spike) | operator dual-root workflow had **no automatic path map**; heuristic refs inflate on expand |
| **M1 product path (this repo)** | `index --macro-expanded-root` + `meta.path_map` + de-dup ON + `macro rebuild` / stale fingerprint — product queries map sidecar hits to source crate paths (e.g. `crates/foo/src/lib.rs`). Dual-root **spike notes remain operator history**, not the product UX. Still **not** sound / not expand-complete. |
| S on expanded trees | **false** — expand injects `unsafe TrivialClone` / large-file `parse_error` |
| Product `src/` change | none from the spike itself; Track M1 later productized path map / de-dup / rebuild (see [macro-sidecar.md](macro-sidecar.md), [eval-macro-expand.md](eval-macro-expand.md) §8) |

---

## Gates

| gate | result |
|---|---|
| `cargo fmt --check` | ✅ |
| `cargo clippy --all-targets -- -D warnings` | ✅ |
| `cargo test` (full) | ✅ green (includes `l1_rules_inventory`, `rust_unsafe_calls`) |
| Force reindex stock corpus | ✅ 994 files / 168 088 refs / heuristic 1 180 |
| Track A smoke `callers from_raw_fd/geteuid --exact-only` | ✅ Exact hits on unsafe system-API sites |
| Track A smoke `subset` | ✅ `in_subset=false` / `promise_tier=disabled` (unsafe still flags) |
| Corpus source committed? | **No** |
