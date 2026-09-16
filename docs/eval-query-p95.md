# Query p95 hard acceptance (PLAN §6)

**SLO:** `callers` / `impact` **p95 &lt; 50ms** on a **5 000-file** tree.

## Method

In-process measurement (avoids CLI process-startup bias):

```bash
# generate + index
powershell -File scripts/gen_fixture.ps1 -N 5000
agentgraph --root <fixture> index --force
agentgraph --root <fixture> bench-query --samples 200 --prefix helper
```

Or: `powershell -File scripts/bench_query_p95.ps1 -N 5000`

CI soft gate: `tests/query_p95.rs` (400-file debug DB, same 50ms budget).

## Results (release, this machine)

### Low fan-in (5 000 files, warm cache, `helperN`)

| query | p50 | **p95** | p99 | max |
|---|---:|---:|---:|---:|
| `callers` (limit 20) | 0.04 ms | **0.08 ms** | 0.15 ms | 0.21 ms |
| `impact` (depth 2, limit 50) | 0.12 ms | **0.21 ms** | 0.31 ms | 0.36 ms |

### High fan-in hot name (`run`, **~4 000 callers**)

| mode | callers p50 | **callers p95** | impact p50 | **impact p95** | max (callers) |
|---|---:|---:|---:|---:|---:|
| **cold** (new Store/sample, limit 5000) | 2.19 ms | **4.21 ms** | 2.71 ms | **4.24 ms** | 34.7 ms |
| warm (cache, limit 5000) | 0.11 ms | **0.65 ms** | 0.30 ms | **0.78 ms** | 30.1 ms |

**PASS** — hot cold p95 ≪ 50 ms (honest numbers after hot-first sampling fix).
Earlier 0.21/0.39 ms figures were **invalid** (hot never sampled).

Reproduce:

```bash
powershell -File scripts/gen_fixture.ps1 -N 1000 -HotName 4000
agentgraph --root <fixture> index --force
agentgraph --root <fixture> bench-query --samples 40 --hot run --cold
```

`--cold` includes **fresh `open_store`** per sample (no in-process cache).
OS/SQLite page cache may still be warm.

## Caveats

- Synthetic graph still lacks real import/qualifier density of production monorepos.
- First schema prepare after `open` is amortized on cold path (per-sample open cost is included in cold timing — conservative).
- CLI process-spawn timing is **not** query latency — use `bench-query`.

## Related

- Index incremental SLO: [perf-plan.md](perf-plan.md)
- L2 S-sound: [sound-subset.md](sound-subset.md)
- mtime escape hatch: `AGENTGRAPH_TRUST_MTIME=0` forces content-hash every file.

