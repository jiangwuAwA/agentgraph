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

### High fan-in hot name (`run`, **~4 000 callers**) — dedicated hot track

`bench-query --hot run` measures **only** the hot name (limit 5000).

| mode | callers p50 | **callers p95** | callers max | impact p95 |
|---|---:|---:|---:|---:|
| warm | 3.69 ms | **5.83 ms** | 6.18 ms | 0.06 ms |
| cold (new Store/sample) | 30.0 ms | **35.0 ms** | 49.4 ms | 15.8 ms |

**PASS p95 &lt; 50 ms** (cold p95 35 ms; cold **max 49 ms** is **tight** — machine-local, includes `open_store`; not a CI 5k gate).

```bash
agentgraph --root <hot-fixture> bench-query --samples 40 --hot run --cold
```

Earlier “p95 4.2ms / 0.21ms” figures mixed helpers or omitted hot — **invalid**.

## Caveats

- Synthetic graph still lacks real import/qualifier density of production monorepos.
- First schema prepare after `open` is amortized on cold path (per-sample open cost is included in cold timing — conservative).
- CLI process-spawn timing is **not** query latency — use `bench-query`.

## Related

- Index incremental SLO: [perf-plan.md](perf-plan.md)
- L2 S-sound: [sound-subset.md](sound-subset.md)
- mtime escape hatch: `AGENTGRAPH_TRUST_MTIME=0` forces content-hash every file.

