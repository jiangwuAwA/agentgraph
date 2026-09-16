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

## Results (release, this machine, 5 000 files / 10 000 symbols / 5 000 refs)

| query | p50 | **p95** | p99 | max |
|---|---:|---:|---:|---:|
| `callers` (limit 20) | **0.04 ms** | **0.08 ms** | 0.15 ms | 0.21 ms |
| `impact` (depth 2, limit 50) | **0.12 ms** | **0.21 ms** | 0.31 ms | 0.36 ms |

**PASS** — p95 ≪ 50 ms (≈600× headroom on callers).

## Caveats (adversarial review)

- Synthetic uniform `helperN`/`mainN` graph has **fan-in = 1**; hot names in
  real repos (`run`/`execute` with 10³–10⁴ callers) are slower. This number
  is a **floor**, not a worst-case guarantee.
- Warmup + in-process **query cache** mean repeated names are hits; first-hit
  and cold-open are not isolated here.
- CI gate is 400-file debug (`tests/query_p95.rs`); 5k release is script-only.
- CLI process-spawn timing (~400–600 ms) is **not** query latency — use
  `bench-query`.

## Related

- Index incremental SLO: [perf-plan.md](perf-plan.md) (noop &lt; 2s — **met**)
- L2 S-sound: [sound-subset.md](sound-subset.md)
