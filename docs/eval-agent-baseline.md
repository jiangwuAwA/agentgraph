# Agent baseline evals — scripted tool-policy A/B/C (P0-5)

**Status:** shipped first public slice — protocol + scripted policies + replay
fixtures + harness numbers.  
**Label (mandatory):** these are **scripted tool-policy agents**
(deterministic, reproducible) — **not** a live LLM agent experiment. A future
live run is **P0-5b** (operator/CI track with model, temperature, and
environment recorded). We do **not** fabricate LLM numbers here.

Related: [eval-agent-tasks.md](eval-agent-tasks.md) (P0-1 structure-fact
scores vs name-grep), [agent-recipes.md](agent-recipes.md),
[product-improvement-backlog.md](product-improvement-backlog.md) (P0-5 card).

---

## Non-goals / Non-claims

- **Not** a live LLM / “Agent product” proof — only scripted policies below.
- **No** private stock / proprietary corpus paths in fixtures, trajectories,
  or docs.
- **No** sound oversell: `window=sound` on fixtures is the **`ast_modeled`**
  engineering S gate — **not** ecosystem sound, **not** production sound,
  **not** a complete runtime graph.
- **No** claim that fixture scores transfer to production monorepo precision.
- **No** claim that a single lucky run is a conclusion — we record **N≥3**
  runs/policy/task even when the policy is seed-invariant.

---

## Policy definitions

Same public task set as P0-1: [`fixtures/eval-agent-tasks/`](../fixtures/eval-agent-tasks/)
(issue + mini-repo + `task.json` structure facts). This slice runs **≥6 of 9**
tasks (harness runs all available; gate requires ≥6 with full A/B coverage).

| Policy | Name | Tools allowed | File-set assembly |
|---|---|---|---|
| **A** | Agent-like **tool policy** + MCP/CLI recipes | agentgraph CLI recipes (same payload surface as MCP `blast_radius` / `who_calls` / `subset`) | `index` → `blast-radius` → `who-calls` → `find` → `related` → `subset`; set = blast nodes ∪ node.resolved ∪ definition ∪ related ∪ who-calls non-flood paths (implementor flood skipped when `high_freq_name`) |
| **B** | Agent-like **read/grep** policy | reading source + name/token search only — **no** agentgraph | walk → grep symbol token → bounded definition reads → same-dir sibling expansion → secondary identifier greps |
| **C** | **name-grep** control | token search only | files whose source contains the symbol as a whole token (P0-1 baseline) |

### Policy A (scripted, deterministic)

1. Copy fixture to a temp work dir (exclude `task.json` / `.agentgraph`).
2. `agentgraph index` (or `index --workspace …` when `task.json` has workspace).
3. Run recipes: `blast-radius <symbol> --depth 3`, `who-calls <symbol>`,
   `find <symbol>`, `related <symbol>`, `subset`.
4. Assemble the structure-bounded file set (table above).
5. Record every tool call + honesty fields (`window`, `subset_ok`,
   `recommendation`, `note`) in the trajectory.

Seeds are **metadata** for A: the recipe policy is seed-invariant; we still
record N runs for protocol parity and so live P0-5b can reuse the same slots.

### Policy B (scripted read/grep heuristics)

Documented, simple, deterministic given a seed for caps:

1. **walk_sources** — enumerate source files (`.ts/.tsx/.js/.jsx/.py/.go/.rs`),
   skip `.agentgraph` / `target` / `node_modules` / `.git` / `__pycache__`.
2. **grep_symbol** — whole-token search for `task.json` `symbol`.
3. **read_files** — bounded reads (cap **12**). Flag definition-like lines
   (`class`/`interface`/`def`/`fn`/`func`/`struct`/`trait`/`impl`/`type`/… +
   the symbol).
4. **expand_same_dir** — for each definition file directory, include sibling
   source files (cap **6**/dir). Seed breaks ties when capping.
5. **grep_token** — secondary identifiers extracted from definition lines
   (cap **8** tokens; **8** hits/token). Seed breaks ties when capping.

File set = symbol-token hits ∪ definition files ∪ same-dir siblings ∪
secondary-token hits.

**B does not call** agentgraph MCP/CLI recipes. Same issue text as A/C.

### Policy C (name-grep control)

Retained from P0-1. File set = source files containing the symbol token.
Seed-invariant; recorded as N metadata runs.

---

## Scoring

Offline **structure-fact** file-set metrics (same spirit as P0-1):

| Metric | Definition |
|---|---|
| **expected-file hit rate (recall)** | \|file_set ∩ `expected.files_that_matter`\| / \|files_that_matter\| |
| **extra-noise files** | \|file_set ∩ `expected.noise_files`\| |
| **wrong-file / forbidden hits** | \|file_set ∩ forbidden\| where forbidden = `expected.forbidden_files` if present, else `expected.noise_files` |
| **file-set size** | \|file_set\| (context for noise/recall) |

Repeats: **N ≥ 3** seeds per task per policy A/B (C recorded equally).  
Deterministic policies may be **seed-invariant** — still recorded N times;
`seed_invariance` is reported in the aggregate JSON.

Primary product signal on these fixtures (honest, structure-fact only):

- **A vs B/C noise** — whether recipe-bounded sets exclude labeled noise files
  that token/read heuristics pull in (comments, name collisions, sibling pkgs).
- Recall alone is weak when the symbol token already appears in expected files;
  **wrong-file edits** are the product differentiator the fixtures label.

> Backlog card text framed **B−A** as the product metric with A=grep-only and
> B=MCP. **This slice uses operator labels** (A=MCP/CLI recipes, B=read/grep).
> The comparison content is unchanged: **recipes vs no-structure tools**, plus
> **C** as the deterministic name-grep control. Do not mix label systems when
> citing numbers.

---

## Offline replay

Trajectories are public JSON under [`evals/agent-ab/<task>/run-{a|b|c}-<seed>.json`](../evals/agent-ab/).
Replay scores them **without network** and **without** the agentgraph binary:

```bash
# one trajectory
python scripts/eval_agent_ab.py score \
  --trajectory evals/agent-ab/ts-nest-user-repo/run-a-0.json

# all committed trajectories
python scripts/eval_agent_ab.py score --traj-dir evals/agent-ab

# cargo gate (replay + harness + fixture coverage)
cargo test --test agent_ab_eval
```

Trajectory schema: `agentgraph.eval_agent_ab.trajectory.v1`  
Result schema: `agentgraph.eval_agent_ab.v1`  
Replay schema: `agentgraph.eval_agent_ab.replay.v1`

Each trajectory records: `policy`, `kind` (scripted / name-grep), `seed`,
`honesty.live_llm_agent=false`, `tool_calls[]`, `file_set[]`, `score`, and
task labels (`expected_files`, `noise_files`, `forbidden_files`).

---

## Reproduce (full run)

```bash
cargo build
python scripts/eval_agent_ab.py run \
  --bin target/debug/agentgraph \
  --runs 3 \
  --out target/agent_ab_eval.json \
  --traj-dir evals/agent-ab
# markdown table: target/agent_ab_eval.md
cargo test --test agent_ab_eval
```

Machine-readable scores: `target/agent_ab_eval.json`  
Markdown table: `target/agent_ab_eval.md`

---

## Results (this commit — scripted policies)

Harness: `python scripts/eval_agent_ab.py run`  
Binary: local debug build with `blast-radius` / `who-calls` recipes.  
Runs: **3 seeds** (0,1,2) × policies A/B/C × available public tasks.

### Score table

Mean over **3 seeds** per task per policy (scripted; A/C seed-invariant on
these fixtures — still N=3 recorded).

| task | symbol | A recall | A noise | B recall | B noise | C recall | C noise | A seed-invariant |
|---|---|---:|---:|---:|---:|---:|---:|---|
| go-store-api | Get | 100% | 0.0 | 100% | 2.0 | 100% | 0.0 | Y |
| noisy-name-fmt | fmt | 100% | 0.0 | 100% | 1.0 | 100% | 1.0 | Y |
| py-fastapi-invoice | InvoiceService | 100% | 0.0 | 100% | 2.0 | 100% | 0.0 | Y |
| py-plugin-registry | PluginRegistry | 100% | 0.0 | 100% | 1.0 | 100% | 1.0 | Y |
| rust-trait-handler | render | 100% | 0.0 | 100% | 1.0 | 100% | 1.0 | Y |
| rust-unsafe-scoped | export_rows | 100% | 0.0 | 100% | 1.0 | 100% | 0.0 | Y |
| rust-workspace-two-roots | compute | 100% | 0.0 | 100% | 2.0 | 100% | 2.0 | Y |
| ts-nest-order-service | OrderService | 100% | 0.0 | 100% | 2.0 | 100% | 1.0 | Y |
| ts-nest-user-repo | UserRepository | 100% | 0.0 | 100% | 2.0 | 100% | 1.0 | Y |

Shipped task count with full A/B/C × 3 seeds: **9** (≥6 gate).

### Aggregate (mean over 9 tasks × 3 runs = 27 runs/policy)

| policy | kind | runs | mean expected-file recall | mean extra-noise files | mean file-set size |
|---|---|---:|---:|---:|---:|
| **A** recipes (scripted) | `scripted_tool_policy` | 27 | **1.00** | **0.00** | 2.78 |
| **B** read/grep (scripted) | `scripted_read_grep_policy` | 27 | **1.00** | **1.56** | 4.22 |
| **C** name-grep control | `name_grep_control` | 27 | **1.00** | **0.78** | 3.00 |

Product signal (noise): **B − A = +1.56** extra-noise files on average
(positive → recipe-bounded sets cleaner than the scripted read/grep policy).
**C − A = +0.78** vs the P0-1 name-grep control.

### What the numbers say (honest)

- On these **public structure-fact fixtures**, all three scripted policies
  reach **100% mean expected-file recall** — the labeled files usually contain
  the symbol token, so recall alone does **not** separate the policies.
- The differentiator is **wrong-file / noise inclusion**:
  - **A** (index + `blast-radius` / `who-calls` / `find` / `related` /
    `subset`) excluded **every** labeled noise file (mean extra-noise **0**).
  - **B** (read/grep with same-dir expansion) pulled the most noise: health/
    unrelated Nest files, Go package siblings, workspace `web` comment hits,
    docs-string registry noise, metrics implementors.
  - **C** name-grep sits between: token collisions without B’s directory
    expansion (e.g. `fmt` / `compute` comment hits) but misses B-only noise
    when the token is absent from a sibling file.
- **A is seed-invariant** on all nine tasks (deterministic recipe policy);
  N=3 is still recorded for protocol parity and for a future live P0-5b.
- **Honesty fields (A):** clean tasks report `window=sound` +
  `subset_ok=true`; `rust-unsafe-scoped` stays **`window=default`** with
  scoped `sound_candidates` (eligible `core` / avoid `legacy`) — never a
  union sound claim. `note` always carries “not a complete runtime graph”.
- **This is not** “Agent with MCP beats Agent without MCP in an LLM product
  sense.” It is a **scripted tool-policy** comparison on public fixtures.
  Live model runs remain P0-5b.

### Baseline honesty

- **A** = scripted agentgraph **recipe tool policy** (CLI ≡ MCP payload
  builders) — **not** a live LLM agent with MCP.
- **B** = scripted **read/grep** policy with documented caps — **not** a live
  LLM agent without tools.
- **C** = **name-grep** control from P0-1 — deterministic token search.
- Scores are **structure-fact file sets** on public fixtures — **not**
  production change-quality, **not** ecological soundness, **not** a complete
  runtime graph.
- Live A/B (model, temperature, N repeats, failure modes) remains
  **P0-5b** and must not be summarized from these scripted numbers.

---

## Relationship to P0-1 (`eval-agent-tasks.md`)

| | P0-1 | P0-5 (this doc) |
|---|---|---|
| Question | Do recipe structure-fact sets beat **name-grep** on labeled files/noise? | Do **agent-like tool policies** differ from **agent-like read/grep** and name-grep? |
| Baselines | name-grep only | **B** read/grep policy + **C** name-grep |
| Agent behavior | none (recipes vs tokens) | **scripted** tool/read policies (still not live LLM) |
| Scoring | recall / noise / honesty gates | same structure-fact metrics + forbidden hits + replay |
| Private corpus | forbidden | forbidden |

P0-1 tables stay valid. P0-5 adds the **policy-behavior** frame and offline
trajectory replay so a future live run can drop into the same scorer.

---

## Limits (explicit)

- Public synthetic mini-repos; fixture-scale only.
- Expected/noise files are hand-labeled structure facts, not a full runtime
  impact proof.
- A’s file set uses blast dependents + definition/related/who-calls —
  callees are **not** claimed as blast dependents (open definition / sibling
  queries; see [eval-agent-tasks.md](eval-agent-tasks.md)).
- High-freq names (`fmt`, …): A skips implementor-flood paths when
  `who_calls.high_freq_name` is true; store still keeps edges
  ([noise-governance.md](noise-governance.md)).
- S3 CI `workflow_dispatch` demo for agent-ab is **optional** and not shipped
  in this slice.

---

## Layout / code

| path | role |
|---|---|
| `docs/eval-agent-baseline.md` | this protocol + results |
| `scripts/eval_agent_ab.py` | harness: run A/B/C + offline `score` replay |
| `evals/agent-ab/<task>/run-*.json` | public scripted trajectories (replay fixtures) |
| `target/agent_ab_eval.json` | machine-readable aggregate (not committed) |
| `target/agent_ab_eval.md` | markdown table from harness |
| `tests/agent_ab_eval.rs` | fixture coverage + replay + harness exit-0 gate |
| `fixtures/eval-agent-tasks/**` | reused public tasks (read-only) |

Related: [agent-recipes.md](agent-recipes.md), [eval-agent-tasks.md](eval-agent-tasks.md),
[noise-governance.md](noise-governance.md), [workspace.md](workspace.md),
[sound-subset.md](sound-subset.md).
