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

## M4 diff / S-recert budgets (Track M4-P)

**Scope:** product paths that are **not** the callers/impact query SLO above —
CLI `agentgraph diff` wall-clock, and dirty-file S re-certification after
watch / path-scoped reindex. These are **SLO-style soft budgets** on a named
fixture + machine, **not** production guarantees.

**Fixture:** 200 synthetic TypeScript files (same shape as
`scripts/gen_fixture.ps1 -N 200`: `pkg*/f*.ts` with `helperN` / `mainN`).
**Machine for the numbers below:** Windows 11 10.0.26200, x86_64,
Intel64 Family 6 Model 158, 8 logical CPUs, release build (LTO). Debug CI
runners are slower; the test asserts **loose** ceilings only.

### Budgets + this machine

| Path | Loose CI budget | This machine (release, n=200) |
|---|---:|---|
| CLI `diff` cold (1st process after index; **process spawn included**) | **< 2s** | 201 ms |
| CLI `diff` warm (20 samples, process spawn included) | **p95 < 2s** | p50 194 ms / **p95 214 ms** / max 226 ms |
| In-process `run_diff` warm (no CLI spawn) | record only | p50 **6.3 ms** / p95 **9.7 ms** |
| S re-cert `refresh_subset_for_paths` (dirty paths) | **< 500 ms** for n≤10 | n=1 **0.95 ms** / n=5 **4.4 ms** / n=10 **7.3 ms** |
| `index_paths` dirty end-to-end (extract + subset meta + recert hook) | **< 2s** for n≤10 | n=1 **22 ms** / n=5 **26 ms** / n=10 **27 ms** |
| Full-corpus `scan_subset` (what full `index` pays for every file) | smoke only | n=200 **75 ms** |

**Reading the table honestly:**

- CLI `diff` wall-clock is dominated by **process start** (image load, AV,
  open store + read snapshot). In-process `run_diff` is the algorithm cost
  (~6–10 ms warm on this fixture). Do **not** present CLI wall-clock as
  “graph-diff latency” without saying spawn is included.
- Dirty S re-cert is **per dirty path**, not a full-corpus rescan. On this
  fixture, `refresh_subset_for_paths` on 10 files is ~10× cheaper than a full
  200-file `scan_subset`. Full `index` still runs the corpus-wide scan.
- These numbers are **fixture + machine local**. They are not “p95 always
  &lt; X ms on production.” Synthetic files are small and uniform; real repos
  have larger ASTs, more languages, and cold antivirus paths.

### Reproduce

```bash
# soft gate (loose ceilings + prints p50/p95; writes target/perf_m4_diff_bench.md)
cargo test --release --test perf_m4_diff -- --nocapture
cargo test --test perf_m4_diff -- --nocapture   # debug CI profile

# manual CLI path
powershell -File scripts/gen_fixture.ps1 -N 200
agentgraph --root <fixture> index --force
agentgraph --root <fixture> diff                 # wall-clock includes process spawn
# dirty a few files, then path-scoped reindex (watch / programmatic index_paths)
agentgraph --root <fixture> subset               # reads stored violations (not a rescan)
```

CI test: `tests/perf_m4_diff.rs`. Budgets asserted there: CLI `diff` cold/warm
p95 &lt; 2s; `refresh_subset_for_paths` max &lt; 500 ms; `index_paths` dirty max
&lt; 2s. Full-corpus scan is smoke-only (&lt; 30s).

## Workspace multi-root smoke budget (M4-W polish)

**Scope:** `index --workspace-root` ×2 + `workspace status` + a few root-filtered
queries on a shared store. Soft SLO-style ceiling on a named fixture — **not** a
production guarantee.

**Fixture:** 2 synthetic TypeScript roots × 100 files each (same shape as
`scripts/gen_fixture.ps1 -N 100` under two roots).

| Path | Loose CI budget | Notes |
|---|---|---|
| Workspace full index (2×100 files, release) | **&lt; 30s** (CI soft); smoke only | One shared SQLite + `root_id` |
| `workspace status` (warm) | **&lt; 2s** CLI wall-clock (process spawn included) | Per-root counts + promise_tier + index_seq |
| Root-filtered `find` / `callers` (warm, in-process) | **&lt; 50ms** p95 (same as classic query SLO) | SQL `root_id = ?` filter |

**Reproduce (operator smoke):**

```bash
# two-root workspace smoke (temp dirs; no private stock required)
powershell -File scripts/gen_fixture.ps1 -N 100 -Out tmp/ws-smoke/api
powershell -File scripts/gen_fixture.ps1 -N 100 -Out tmp/ws-smoke/web
agentgraph index --workspace-root tmp/ws-smoke/api --workspace-root tmp/ws-smoke/web \
  --workspace-db tmp/ws-smoke/ws.db --force
agentgraph workspace status --workspace-db tmp/ws-smoke/ws.db
agentgraph find helper --workspace-root tmp/ws-smoke/api --workspace-db tmp/ws-smoke/ws.db
```

Honesty: numbers are fixture + machine local. Multi-root index cost is roughly
the sum of per-root classic indexes plus one store open — not a free lunch, and
not a claim about arbitrary monorepos.

CI soft gate: extend `tests/workspace_index.rs` (status fields + partial reindex);
full 2×100 timing stays operator smoke.

## Related

- Index incremental SLO: [perf-plan.md](perf-plan.md)
- L2 S-sound: [sound-subset.md](sound-subset.md)
- Indexed-edge diff semantics: [graph-diff.md](graph-diff.md)
- Multi-root workspace product surface: [workspace.md](workspace.md)
- mtime escape hatch: `AGENTGRAPH_TRUST_MTIME=0` forces content-hash every file.

