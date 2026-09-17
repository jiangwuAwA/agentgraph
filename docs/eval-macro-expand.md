# Macro-expand indexing spike (Track B)

**Status:** spike complete — **partial production readiness**, soundness **not** claimed  
**Corpus:** private `D:\projects\eval-corpus\stock-trading-app` (source never committed)  
**Shadow tree (operator machine, never commit):** `D:\projects\eval-corpus\stock-trading-app-expanded\`  
**Reproduce:** `powershell -File scripts\stock_macro_expand_spike.ps1`  
**Diff helper:** `python scripts\expand_index_diff.py prepare|diff`

Related: [eval-stock-boundary.md](eval-stock-boundary.md) (L0/L1 stock goldens), [sound-subset.md](sound-subset.md), [eval-l1.md](eval-l1.md).

---

## 1. Question

Can agentgraph capture **proc-macro / derive-generated edges** by indexing a
**post-expansion** shadow tree — without claiming the resulting graph is sound?

Secondary: what is left before any production default?

---

## 2. Tooling available?

| tool | result on this operator machine |
|---|---|
| `cargo expand` | **Missing.** `cargo install cargo-expand` failed (crates.io via local proxy `127.0.0.1` unreachable / too slow). |
| `rustup` nightly `cargo +nightly rustc -- -Zunpretty=expanded` | **Blocked / incomplete.** nightly channel listed but install failed (`static.rust-lang.org` TCP timeouts; `missing manifest` after partial download). |
| **`RUSTC_BOOTSTRAP=1 cargo rustc -- -Zunpretty=expanded`** | **Works** on stable (`stable-x86_64-pc-windows-gnu`). Used for all real expands in this spike. |
| Workspace dep resolve for stock crates (`rust_decimal`, `sqlx`, `async-trait`, …) | **Blocked** — crates.io index download fails. Cannot expand `event-engine` / `repository` / `model-selection-replay` with rustc on this machine right now. |
| Offline crates in cargo cache | `thiserror`, `serde*`, `sha2`, `ring` present → enough for a **mini-spike** crate. |

**Failure modes documented for the script:**

1. `cargo expand` not installed  
2. nightly toolchain broken / missing manifest  
3. crates.io / rustup network proxy failures  
4. PowerShell `>` redirection writes **UTF-16 LE** → agentgraph `skip …: not valid UTF-8`  
5. Oversized expanded files (`>1.5MiB`) skipped and mint `parse_error` (R23)  
6. Expanded output can inject **`unsafe impl … TrivialClone`** → S violations on expand even when source crate is clean  

---

## 3. Spike measurements

### 3.1 Baseline — stock index (source root, not expanded)

Full corpus remains: **995 files / 20 896 symbols / 168 088 refs** (see eval-stock-boundary).

Selected crate slice (source index.db filters):

| crate | files | symbols | refs | heuristic refs |
|---|---:|---:|---:|---:|
| `event-engine` | 17 | 199 | 1 133 | 10 |
| `model-selection-replay` | 22 | 392 | 1 550 | 4 |
| `repository` | 27 | 246 | 915 | 65 |
| `strategy-plugins` (inventory mix) | 34 | 518 | 4 448 | 76 |

### 3.2 B1 — Expand

**Real expand (rustc `-Zunpretty=expanded`):**

| artifact | method | bytes | notes |
|---|---|---:|---|
| mini-spike `thiserror`+derive crate | RUSTC_BOOTSTRAP unpretty | 9 113 UTF-8 | Real `Debug::fmt` / `Clone` / `Default` / `Display::fmt`; thiserror routes `Error` via `::thiserror::__private20::Error` (no bare `source`/`description` methods). |
| agentgraph own `src/lib.rs` (tool path proof, not stock) | RUSTC_BOOTSTRAP unpretty | ~1.1 MiB UTF-8 | clap/serde derive expansion; indexes successfully after UTF-8 fix. |
| stock `event-engine` / `repository` / `model-selection-replay` | blocked | — | deps not resolvable offline. |

**Synthetic expand (labeled, not rustc):** `scripts/expand_index_diff.py prepare`
appends post-expansion-shaped impls derived from source patterns for the four crates
(`event-engine`, `model-selection-replay`, `repository`, `strategy-plugins`):

| pattern emitted | count |
|---|---:|
| derive targets seen | 93 (+34 plugins) |
| `Debug` impls | 93 (+31) |
| `Clone` impls | 87 (+34) |
| `PartialEq`/`Eq` | 56/55 |
| `thiserror::Error`-shaped Display+Error | 7 |
| `async_trait` attrs / method shapes | 44 / 606 (+20/22 plugins) |
| `inventory::submit!` sites | 18 (plugins) |
| synthetic inventory registrar types | 36 |

Shadow layout:

```text
D:\projects\eval-corpus\stock-trading-app-expanded\
  source-view\          # unexpanded copies of spike crates
  expanded-view\        # same trees + synthetic expand suffix
  mini-source-view\     # real mini-spike source
  mini-expanded-view\   # real -Zunpretty output
  real-expand-root\     # real expanded agentgraph lib + mini-spike
  mini-spike\           # crate used for offline real expand
  metrics\              # logs / stats (operator)
```

**Never commit any of the above into agentgraph.**

### 3.3 B2 — Index + measure

CLI supports **one root per index** (`agentgraph --root <root> index --force`).
We used **separate shadow roots** (preferred) rather than indexing corpus+expanded together.

| root | files | symbols | refs | exact | heuristic | failed |
|---|---:|---:|---:|---:|---:|---:|
| `source-view` (4 crates, unexpanded) | 73 | **1 100** | **6 009** | 5 863 | 146 | 0 |
| `expanded-view` (same + synthetic expand) | 73 | **2 140** | **6 618** | 6 114 | **504** | 0 |
| `mini-source-view` (real source) | 1 | 12 | 9 | 7 | 2 | 0 |
| `mini-expanded-view` (real rustc expand) | 1 | **35** | **59** | 34 | **25** | 0 |
| `real-expand-root` (real 1.1 MiB expand + mini) | 3 | 751 | 9 595 | 9 317 | 278 | 0 |

**Parser:** no panic on expanded Rust. After UTF-8 conversion, index/force succeeds.
`subset` on expanded roots **does not claim S** — expected.

#### Macro-generated edges that show up after expand

Fair 4-crate compare (`source-view` vs `expanded-view`, both 73 files):

| name | src symbols | exp symbols | src refs | exp refs | Δ sym | Δ ref |
|---|---:|---:|---:|---:|---:|---:|
| `fmt` (Debug/Display) | 1 | **132** | 3 | 134 | **+131** | +131 |
| `clone` | 0 | **121** | 191 | 312 | **+121** | +121 |
| `eq` | 0 | **67** | 0 | 67 | **+67** | +67 |
| `default` | 4 | 13 | 63 | 72 | +9 | +9 |
| `hash` | 0 | 10 | 18 | 28 | +10 | +10 |
| `source` / `description` (Error) | 1 / 0 | 8 / 7 | 0 / 0 | 7 / 7 | +7 / +7 | +7 / +7 |
| `*_async_trait_boxed_shape` | 0 | **628** | 0 | 0 | +628 | 0 |
| `*inventory_registrar*` | 0 | **36** | 0 | 0 | +36 | 0 |
| `SyntheticInventoryRegistrar` | 0 | 18 | 0 | 0 | +18 | 0 |

Symbol-name set: source **830** names ⊂ expanded **905**; **only-in-expanded=75**,
**only-in-source=0**. Expanded adds derive/inventory/async_trait-shape names; it does not
drop source-level names when expand is appended to the same modules.

**Real rustc expand (mini-spike):** only-in-expanded names include
`fmt`, `clone`, `default`, `eq`, `hash`, `cmp`, `partial_cmp`, plus thiserror-internal helpers.
`callers fmt` becomes non-empty **only** after expand — same L1 noise class already documented
on stock source (`fmt`/`drop`/`default` floods in eval-stock-boundary § noise proxy).

#### Honest noise / caveats

1. **Dual-index duplication:** if an operator indexes stock corpus **and** a shadow tree,
   every symbol/ref can appear twice under different paths
   (`crates/foo/src/bar.rs` vs `source-view/crates/foo/src/bar.rs` or
   `expanded-view/crates/foo/src/bar.rs`). Path mapping is **not** automatic.
2. **Synthetic ≠ rustc:** synthetic expand proves *indexability of expanded-shaped Rust*,
   not exact proc-macro output. Real mini-spike expand validates the rustc path on this machine.
3. **async_trait:** method **names** already extract at L0 from source (eval-stock-boundary:
   async_trait bodies are Exact). Expand mainly changes **signatures** (BoxFuture) and can
   add shadow symbol names in synthetic mode — not a large L0 recall gap for `insert` /
   `query_range` / `latest_close`.
4. **inventory:** L1 `rs.di.inventory_submit` already lifts goldens on **source**.
   Expand would add registrar statics; synthetic spike shows +36 registrar-shaped symbols,
   not a new unsound call graph.
5. **Heuristic inflation:** expanded-view heuristic refs 146 → 504 (impl-trait-like
   Debug/Error/Clone implementor edges). Same L1 noise family; **do not** treat Default
   `callers <name>` as call graph.
6. **S gate on expand:** real expanded mini-spike mints
   `unsafe impl ::core::clone::TrivialClone` → `subset` `unsafe` violations.
   Large real expand (`agentgraph_lib_expanded.rs`) mints `parse_error`
   (tree-sitter ERROR nodes). **Expanded trees are not an S-certifiable corpus.**

---

## 4. Production path estimate (A vs B)

| track | goal | spike verdict | path to production |
|---|---|---|---|
| **A** unsafe-block call extract | fix L0/L1 extract holes on **source** | owned elsewhere; not edited here | Required for S quality on real crates; does **not** replace expand for derive/inventory-generated defs. |
| **B** macro-expand indexing | capture derive/proc-macro edges **without sound** | **Feasible as optional operator pipeline**; not default product | See phases below |

### Recommended phases (B)

| phase | work | est. | ship? |
|---|---|---|---|
| **P0 (done this spike)** | shadow expand + separate index + diff scripts + docs | done | docs/scripts only |
| **P1** | Operator runbook: expand **1–2 clean crates** when crates.io/nightly available; keep shadow outside repo; never dual-index without path policy | 0.5–1 d | runbook |
| **P2** (optional product) | Side-index ingest: `confidence=macro_expanded` (or dedicated rule_id) + path map `expanded/…` → `crates/…`; **de-dup** with source Exact edges; CLI flag `--include-macro-expanded` default **off** | **1–2 weeks** eng | only if goldens show L1 gaps expand actually fills |
| **P3** | Selective real expand of derive-heavy clean crates (`model-selection-replay`, `event-engine`, `repository`) on a network-enabled builder; store **edges** not sources | +1 week | builder job, **not** required Rust CI |

**Time-to-production refinement:**

- **Sound expand-graph:** **out of scope / not claimed.** Expanded output injects
  `unsafe` and path drift; full-tree `subset_ok` stays false. Do not market expand as L2.
- **Arbitrary proc-macro completeness:** **non-goal.** Build-script macros, heavy
  `async_trait` generics, and crates that fail to compile on the expand machine remain gaps.
- **Useful production claim (if P2 lands):** “optional expanded-edge *candidates* for
  derive/inventory-shaped symbols on operator-chosen crates”, alongside existing L1
  `rs.di.impl_trait` / `rs.di.inventory_submit` on **source**.
- **Default `src/` product change:** **none** from this spike (no extract.rs edit required).
  Shadow indexing does not panic; no product fix needed for B.

### CI

**Do not** add `cargo expand` / nightly unpretty to required Rust CI (tooling-heavy,
network-sensitive, non-hermetic). Optional nightly job only if a future P3 builder exists.

---

## 5. Non-goals (explicit)

- No soundness claim on expanded indexes  
- No arbitrary proc-macro completeness  
- No committing expanded sources or corpus sources into agentgraph  
- No required-CI expand  
- No claim that expand replaces L1 inventory/impl_trait rules  
- No silent dual-root merge without path mapping / de-dup  

---

## 6. Gates / verification (this spike)

| check | result |
|---|---|
| Product Rust code changed? | **No** (`src/` untouched) |
| `extract.rs` touched? | **No** (Track A owns) |
| Shadow index panics? | **No** (after UTF-8) |
| UTF-16 PowerShell redirect pitfall? | **Yes — fixed in spike.ps1** |
| Real expand indexes? | **Yes** (mini-spike + agentgraph lib) |
| Stock real expand? | **Blocked** on crates.io/nightly this machine |
| Corpus/expanded committed? | **No** |
| Required CI expand added? | **No** |

---

## 7. Operator commands

```powershell
# Full spike (expand attempts + synthetic prepare + dual index + diff)
powershell -File scripts\stock_macro_expand_spike.ps1

# Synthetic prepare + diff only
python scripts\expand_index_diff.py prepare --corpus D:\projects\eval-corpus\stock-trading-app `
  --shadow D:\projects\eval-corpus\stock-trading-app-expanded `
  --crates event-engine,model-selection-replay,repository --extra-crates strategy-plugins
agentgraph --root D:\projects\eval-corpus\stock-trading-app-expanded\source-view index --force
agentgraph --root D:\projects\eval-corpus\stock-trading-app-expanded\expanded-view index --force
python scripts\expand_index_diff.py diff `
  --source-root D:\projects\eval-corpus\stock-trading-app-expanded\source-view `
  --expanded-root D:\projects\eval-corpus\stock-trading-app-expanded\expanded-view

# Real expand when tooling/network allow (UTF-8 write — do NOT use PowerShell '>' alone)
$env:RUSTC_BOOTSTRAP='1'
$raw = cargo rustc -p event-engine --lib -- -Zunpretty=expanded
[System.IO.File]::WriteAllText('path\to\expanded.rs', ($raw -join "`n"), [System.Text.UTF8Encoding]::new($false))
```

Machine note: operator laptop; wall-clock / SQLite counts, not CI.
