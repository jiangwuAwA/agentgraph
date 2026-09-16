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

| op | result |
|---|---|
| Full index (`--force`) | **~39 s** → 995 files, **20 924** symbols, **167 600** refs, 0 parse failures |
| Incremental (no change) | **~21 s** (hash walk of 995 files; not p95&lt;50ms query — index path) |
| Languages | rust, typescript, tsx, python, javascript |

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
- Incremental index ~21 s on ~1k files is a perf smell (mtime walk + full hash); profile before claiming &lt;50 ms query SLO on this size.
