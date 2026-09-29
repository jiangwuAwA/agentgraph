# Runtime recall (R-track) — covered paths zero-miss

**Status:** MVP on public fixture; stock run is **operator-only** (private corpus not committed).

**Honesty (non-claims):** this is **not** “production absolute zero-miss”, **not** ecosystem sound, **not** a claim that unexecuted paths are covered. We only claim:

1. **Covered paths zero-miss** — for paths that are actually executed under a recorded trace, every runtime call edge is in the static graph **or** in the gap ledger with a reason.
2. **Measurably close to no-miss** — recall@covered is measured, gaps are attributed, and a replay gate exists.

---

## Terms

| Term | Meaning |
|---|---|
| **covered path** | An execution path that produced a recorded runtime trace (integration test, command bin, or fixture entry). |
| **trace edge** | A runtime caller→callee edge observed by the tracer (`from` symbol, `to` symbol, `file:line`). |
| **static edge** | An indexed ref edge in agentgraph (`refs` / export) with `kind` in call/import/define, etc. |
| **gap ledger** | JSON allowlist of edges that are known **not** to be static-modeled, each with `reason` (e.g. `function_pointer`, `macro`, `framework`, `uncovered_test`, `parse_error`). |
| **recall@covered** | `1 - missed/trace_edges` where `missed` = trace edges neither in the static graph nor in the gap ledger. |

## Success line (fixture + stock)

- **Covered paths zero-miss:** `missed == 0`, **or** every miss is in the gap ledger with a `reason`.
- **Measured recall** is reported (not “100% of production”).
- Non-goals: unexecuted paths, production absolute zero-miss, ecosystem sound.

## Pipeline

```
runtime trace (JSONL edges)
        ×
agentgraph index / export (static edges)
        ↓
scripts/eval_runtime_recall.py
        ↓
miss = trace − static − gap
recall@covered = 1 − miss / |trace|
```

## Components

| Piece | Path |
|---|---|
| Spec (this doc) | `docs/eval-runtime-recall.md` |
| Rust call tracer (MVP) | `scripts/rs_trace.py` + fixture `fixtures/eval-runtime-recall/` |
| Diff / gate | `scripts/eval_runtime_recall.py` |
| Gap ledger schema | `fixtures/eval-runtime-recall/gap_ledger.json` |
| Fixture gate test | `tests/runtime_recall.rs` |
| Stock operator run | local only — **private source never committed** |

## Gap reasons (ledger `reason` enum)

`function_pointer` · `macro` · `framework` · `uncovered_test` · `parse_error` · `dyn_dispatch` · `external_crate` · `other` (must include note)

## R1 tracer (MVP)

- **Fixture:** small Rust program with explicit `rr_probe::enter/exit` (or a `trace_call!` wrapper) that writes JSONL call edges.
- **Stock (operator):** prefer `cargo test` / command bins under `crates/*`; use the same JSONL schema; do not commit private sources.

JSONL line schema (`agentgraph.eval_runtime_recall.edge.v1`):

```json
{"schema":"agentgraph.eval_runtime_recall.edge.v1","from":"order::create","to":"payment::charge","from_file":"src/orders/create.rs","from_line":12,"to_file":"src/payments/charge.rs","to_line":40}
```

## R2 gate

```bash
python scripts/eval_runtime_recall.py \\
  --trace path/to/trace.jsonl \\
  --db <root>/.agentgraph/index.db \\
  --gap fixtures/eval-runtime-recall/gap_ledger.json \\
  --out target/runtime_recall.json
```

Output keys: `trace_edges`, `static_hits`, `missed`, `recall_at_covered`, `gap_used`, `extra_static` (info only).

## R3 stock (operator)

- At least one full integration/test/replay trace on the stock corpus.
- Report: covered edges / missed / recall / gap distribution.
- `miss>0` → fix product (L1 / expand sidecar / extract) **or** add gap with reason + raise test coverage.

## R4 static fill (parallel)

- Derive-dense crates: `--macro-expanded-root` sidecar (already shipped).
- Clean crates: scoped `--sound` pilot.
- Prefer **source L1** over widening allowlist.

## Reproduce (fixture)

```bash
python scripts/rs_trace.py fixtures/eval-runtime-recall/src/main.rs --out target/rr_trace.jsonl
agentgraph --root fixtures/eval-runtime-recall index --force
python scripts/eval_runtime_recall.py --trace target/rr_trace.jsonl \\
  --db fixtures/eval-runtime-recall/.agentgraph/index.db \\
  --gap fixtures/eval-runtime-recall/gap_ledger.json
cargo test --test runtime_recall
```


## S-FILL (stock operator) — unsafe L0 + expand sidecar

Private stock corpus is **operator-only** (never committed).

### 1. unsafe L0
- Extract: generic call walk records calls inside `unsafe { }` / `unsafe fn` (`tests/rust_unsafe_calls.rs`).
- Stock: **88** unsafe sites / **24** files; call names in index: flock 17, geteuid 20, dup 4, from_raw_fd 11, fcntl 7.
- No systematic extract hole for unsafe blocks.

### 2. expand sidecar
- `cargo expand --lib --offline` after temp-commenting criterion dev-deps (restored).
- Shadow sibling: `stock-trading-app-expanded-sfill` (operator-local).
- event-engine + auth expanded; repository expand failed on storage compile errors.
- PowerShell `>` wrote UTF-16 — converted to UTF-8 (else language=unknown, 0 symbols).
- Sidecar: **442 symbols / 1590 refs**; `impact --with-macro` dedup `kept_sidecar=1`.

### 3. R-track
- No stock runtime trace this cut. Expand raises candidate surface; `--with-macro` not sound; `--sound` ignores sidecar.

### 4. Discipline
- No private source / expand artifacts in agentgraph. Not production zero-miss.
