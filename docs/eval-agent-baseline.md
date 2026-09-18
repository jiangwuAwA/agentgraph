# Agent baseline evals — scripted tool-policy A/B/C (P0-5) + live host-session A/B (P0-5b) + multi-runner hard A/B (P0-5c)

**Status:**

- **P0-5 (scripted):** protocol + scripted policies + replay fixtures + harness
  numbers. **Label (mandatory):** those rows are **scripted tool-policy
  agents** (deterministic, reproducible) — **not** a live LLM product proof.
- **P0-5b (live):** host-session live LLM Agent ± agentgraph on the same public
  tasks — recorded trajectories under [`evals/agent-ab-live/`](../evals/agent-ab-live/).
  **Label (mandatory):** single-host-session live agent
  (`model_note=mimo-desktop-host-session`) — **not** a public benchmark model,
  **not** a standardized lab harness. We do **not** fabricate LLM numbers;
  only scored, recorded trajectories appear below.
- **P0-5c (multi-runner hard):** independent-session protocol + **harder**
  public fixtures + **≥2 runner kinds** + extended metrics (MCP call counts,
  workspace-root choice, file/read budgets) — trajectories under
  [`evals/agent-ab-c/`](../evals/agent-ab-c/). **Label (mandatory):**
  `host_session_llm` is still **not** a public benchmark model / multi-model
  lab; `scripted_external_runner` is **not** a live LLM. **No oversell**
  that live A “beats” B as a product proof.

Related: [eval-agent-tasks.md](eval-agent-tasks.md) (P0-1 structure-fact
scores vs name-grep), [agent-recipes.md](agent-recipes.md),
[product-improvement-backlog.md](product-improvement-backlog.md) (P0-5 / P0-5b / P0-5c card).

---

## Non-goals / Non-claims

- **Scripted P0-5 rows are not** a live LLM / “Agent product” proof.
- **P0-5b live rows are not** a public-benchmark or multi-model lab result —
  one host session, contamination risk disclosed below.
- **No** private stock / proprietary corpus paths in fixtures, trajectories,
  or docs.
- **No** sound oversell: `window=sound` on fixtures is the **`ast_modeled`**
  engineering S gate — **not** ecosystem sound, **not** production sound,
  **not** a complete runtime graph.
- **No** claim that fixture scores transfer to production monorepo precision.
- **No** claim that a single lucky run is a conclusion — we record **N≥3**
  runs/policy/task even when the policy is seed-invariant.
- **No** claim that live A “beats” live B on this slice when the recorded
  noise/recall tables do not separate them.

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
  N=3 is still recorded for protocol parity (live P0-5b reuses the same slots).
- **Honesty fields (A):** clean tasks report `window=sound` +
  `subset_ok=true`; `rust-unsafe-scoped` stays **`window=default`** with
  scoped `sound_candidates` (eligible `core` / avoid `legacy`) — never a
  union sound claim. `note` always carries “not a complete runtime graph”.
- **This is not** “Agent with MCP beats Agent without MCP in an LLM product
  sense.” It is a **scripted tool-policy** comparison on public fixtures.
  Live host-session results are **P0-5b** below (also not a product proof).

### Baseline honesty

- **A** = scripted agentgraph **recipe tool policy** (CLI ≡ MCP payload
  builders) — **not** a live LLM agent with MCP.
- **B** = scripted **read/grep** policy with documented caps — **not** a live
  LLM agent without tools.
- **C** = **name-grep** control from P0-1 — deterministic token search.
- Scores are **structure-fact file sets** on public fixtures — **not**
  production change-quality, **not** ecological soundness, **not** a complete
  runtime graph.
- Live A/B must **not** be summarized from scripted numbers alone — see
  **P0-5b live** below (host-session limits apply).

---

## P0-5b live — host-session LLM Agent ± agentgraph

**Protocol (what we actually ran):**

For each public task in `fixtures/eval-agent-tasks/` (**9/9**), for repeat
`seed = 0..2`, the **same live host-session LLM** (`mimo-desktop-host-session`):

1. **Arm A — live + agentgraph:** issue text + agentgraph CLI recipes
   (`index`, `blast-radius`, `who-calls`, `find`, `related`, `subset`) +
   reads → JSON file-set trajectory.
2. **Arm B — live read/grep only:** same issue text; **forbidden**
   agentgraph binary/MCP; shell `grep`/token search + reads only → JSON
   file-set trajectory.
3. Trajectories written to
   [`evals/agent-ab-live/<task>/run-{a|b}-<seed>.json`](../evals/agent-ab-live/)
   (`agentgraph.eval_agent_ab.trajectory.v1` + live meta; alias
   `agentgraph.eval_agent_ab.live.v1`).
4. File sets **committed first**; structure-fact labels + scores **stamped
   offline after** from fixture `task.json` (`scripts/eval_agent_ab_live.py
   stamp`). Offline replay: `score --traj-dir evals/agent-ab-live`.

**Completeness:** tasks **9** × seeds **3** × arms **A/B** = **54/54**
trajectories recorded (complete). Host-session decisions were
**seed-invariant** on these fixtures given the same tool/grep evidence.

### Score table — live A/B vs scripted A/B/C vs name-grep (C)

Mean over recorded runs only (no invented cells).

| policy / arm | kind | model_note | runs | mean expected-file recall | mean extra-noise files | mean file-set size |
|---|---|---|---:|---:|---:|---:|
| **A live** recipes | `live_llm_agent` | `mimo-desktop-host-session` | 27 | **1.00** | **0.00** | 2.56 |
| **B live** read/grep | `live_llm_agent` | `mimo-desktop-host-session` | 27 | **1.00** | **0.00** | 2.56 |
| **A scripted** recipes | `scripted_tool_policy` | n/a (deterministic) | 27 | 1.00 | 0.00 | 2.78 |
| **B scripted** read/grep | `scripted_read_grep_policy` | n/a (deterministic) | 27 | 1.00 | 1.56 | 4.22 |
| **C** name-grep control | `name_grep_control` | n/a (P0-1) | 27 | 1.00 | 0.78 | 3.00 |

Per-task live means (seeds 0–2; live A/B file sets matched per task in this run):

| task | symbol | A live recall | A live noise | B live recall | B live noise | A/B size | scripted A noise | scripted B noise | C noise |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| go-store-api | Get | 100% | 0.0 | 100% | 0.0 | 2 | 0.0 | 2.0 | 0.0 |
| noisy-name-fmt | fmt | 100% | 0.0 | 100% | 0.0 | 1 | 0.0 | 1.0 | 1.0 |
| py-fastapi-invoice | InvoiceService | 100% | 0.0 | 100% | 0.0 | 4 | 0.0 | 2.0 | 0.0 |
| py-plugin-registry | PluginRegistry | 100% | 0.0 | 100% | 0.0 | 2 | 0.0 | 1.0 | 1.0 |
| rust-trait-handler | render | 100% | 0.0 | 100% | 0.0 | 2 | 0.0 | 1.0 | 1.0 |
| rust-unsafe-scoped | export_rows | 100% | 0.0 | 100% | 0.0 | 2 | 0.0 | 1.0 | 0.0 |
| rust-workspace-two-roots | compute | 100% | 0.0 | 100% | 0.0 | 2 | 0.0 | 2.0 | 2.0 |
| ts-nest-order-service | OrderService | 100% | 0.0 | 100% | 0.0 | 4 | 0.0 | 2.0 | 1.0 |
| ts-nest-user-repo | UserRepository | 100% | 0.0 | 100% | 0.0 | 4 | 0.0 | 2.0 | 1.0 |

### What the live numbers say (honest — no oversell)

- On these **public, tiny, often self-labeled** fixtures, **live A and live B
  did not separate** on structure-fact recall or noise (both 1.00 / 0.00 in
  this host-session run). **We do not claim** “Agent+MCP beats grep/read for
  live LLMs” from this slice.
- **Scripted B** still shows higher mechanical noise (mean **+1.56** vs A)
  because its same-dir expansion does not read comments. Live B in this session
  **read** files after grep and excluded self-labeled noise (`// Noise: …`).
  That is a **fixture + judgment** effect — **not** proof that structure tools
  are unnecessary on real monorepos.
- Live file sets sometimes **differ from scripted A** where the issue text
  demands adjacency (e.g. Go implementors despite `high_freq_name`,
  `invoice_repo.py`, `payment.service.ts`). Scored against the same structure
  facts; extra non-noise files do not count as noise.
- **C (name-grep)** remains the deterministic lower bound (noise 0.78 mean) —
  not replaced by live numbers.

### P0-5b honest limits (required)

- **Single host session LLM** (`mimo-desktop-host-session`) — **not** a public
  benchmark model id; temperature / sampling not a standardized lab config.
- **Not a standardized lab harness:** operator-driven tool traces in one MiMo
  Desktop session; no independent harness isolation, no multi-model panel.
- **Arm contamination:** same session sees both arms; evidence order was
  Arm A tools → Arm B greps per task (not per-seed randomization). Prior P0-5
  work also exposed fixture `expected` labels to this session. Mitigations
  recorded in trajectories (`honesty.contamination`); residual risk remains.
- **N small:** 9 tasks × 3 repeats × 2 arms, all on **public synthetic
  mini-repos**. Seed-invariant host decisions ⇒ repeats are protocol parity,
  **not** independent statistical draws.
- **No oversell:** these live rows do **not** prove production wrong-file
  reduction, ecological soundness, or that recipes always beat a careful
  grepping agent. They **do** document one honest live protocol + replayable
  trajectories.

### Live reproduce

```bash
# trajectories already committed under evals/agent-ab-live/
python scripts/eval_agent_ab_live.py score --traj-dir evals/agent-ab-live
python scripts/eval_agent_ab.py score --traj-dir evals/agent-ab-live
cargo test --test agent_ab_live
```

Harness script: `scripts/eval_agent_ab_live.py` (`write` / `stamp` / `score`).
Machine-readable live replay: `target/agent_ab_live_replay.json` (not committed).

---

## Relationship to P0-1 (`eval-agent-tasks.md`)

| | P0-1 | P0-5 + P0-5b (this doc) |
|---|---|---|
| Question | Do recipe structure-fact sets beat **name-grep** on labeled files/noise? | Do **agent-like tool policies** differ from **agent-like read/grep** and name-grep? How does a **host-session live** A/B compare (honest limits)? |
| Baselines | name-grep only | **B** read/grep policy + **C** name-grep + live A/B |
| Agent behavior | none (recipes vs tokens) | **scripted** tool/read policies + **P0-5b live host-session** (not a public benchmark lab) |
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
| `docs/eval-agent-baseline.md` | this protocol + results (P0-5 scripted + P0-5b live) |
| `scripts/eval_agent_ab.py` | harness: run scripted A/B/C + offline `score` replay |
| `scripts/eval_agent_ab_live.py` | P0-5b live: write / stamp / score live trajectories |
| `evals/agent-ab/<task>/run-*.json` | public **scripted** trajectories (replay fixtures) |
| `evals/agent-ab-live/<task>/run-*.json` | public **live host-session** trajectories (P0-5b) |
| `evals/agent-ab-live/README.md` | live honesty + layout |
| `target/agent_ab_eval.json` | machine-readable scripted aggregate (not committed) |
| `target/agent_ab_live_replay.json` | machine-readable live replay (not committed) |
| `tests/agent_ab_eval.rs` | scripted fixture coverage + replay + harness gate |
| `tests/agent_ab_live.rs` | live trajectory coverage + replay + honesty gates |
| `fixtures/eval-agent-tasks/**` | reused public tasks (read-only) |

Related: [agent-recipes.md](agent-recipes.md), [eval-agent-tasks.md](eval-agent-tasks.md),
[noise-governance.md](noise-governance.md), [workspace.md](workspace.md),
[sound-subset.md](sound-subset.md).

---

## P0-5c — multi-runner live对照 on hard public tasks

**Question:** on **harder** public fixtures, do runner kinds / arms separate on
structure-fact **noise** and **extended process metrics** (MCP calls,
workspace-root choice, budgets) — without fabricating a multi-model lab?

### Independent session requirements (protocol)

| Requirement | How this slice implements it | Residual limit |
|---|---|---|
| Fresh context | Each arm/seed is a **separate trajectory JSON**; no shared intermediate file-set state between arms | `host_session_llm` still shares the MiMo Desktop session that authored fixtures |
| **No access to `task.json` expected/golden before file-set commit** | `write` emits **empty labels + unstamped score**; `stamp` fills structure facts **after** commitment. Trajectories record `saw_labels_before_commit=false` on the decision path | Host session **authored** hard fixtures in this slice ⇒ residual contamination (disclosed) |
| Arm isolation | Arm **A never greps-only** (must invoke agentgraph recipes); Arm **B never agentgraph** (walk/grep/read only). Enforced in tests | Same host process for `host_session_llm` arms; isolation is **tool-policy** isolation, not separate machines |
| Task-level randomization order recorded | `evals/agent-ab-c/task_randomization.json` records per-seed `task_order` + `arm_order` (A→B vs B→A by seed parity) | Not a full counterbalanced lab design |

### Runner kinds (≥2)

| `runner_id` | kind | `model_note` | `independent_session` | what it is |
|---|---|---|---|---|
| `host_session_llm` | `live_llm_agent` | `mimo-desktop-host-session` | **false** | Live host-session decisions from tool/grep evidence on hard fixtures |
| `scripted_external_runner` | `scripted_external_runner` | `scripted_deterministic_policy` | **true** (decision path) | P0-5 policy A/B executed as an external deterministic runner on hard fixtures |

Optional third (isolated subagents without labels) is **not shipped** on this
host — labeled incomplete rather than invented.

**Not multi-model:** only one live host-session model note is recorded. We do
**not** fabricate a second lab model id.

### Larger N protocol

- **Documented target:** N≥5 seeds per runner×task×arm for a lab-grade slice.
- **Honest on this host:** seeds **0,1,2** recorded (N=3) for both runners ×
  4 hard tasks × arms A/B = **48/48** trajectories (complete for N=3).
  Seeds are protocol parity / order randomization; host-session decisions are
  largely seed-invariant given the same tool evidence. **N≥5 remains open.**

### Harder public tasks

Fixtures: [`fixtures/eval-agent-tasks-hard/`](../fixtures/eval-agent-tasks-hard/)

| task_id | stress | multi-root | notes |
|---|---|---|---|
| `rust-cross-crate-blast` | cross-crate symbol blast + name collision | yes | true dependents core+api; tools/web noise |
| `rust-real-noise-dense` | dense implementors + encode name collisions | no | metrics/legacy/clone_heavy noise |
| `rust-sound-scoped-clean` | sound-disabled dirty sibling + clean scoped root | yes | union not sound; scoped `clean` |
| `ts-multi-root-client` | multi-root TS wrong-root + help/docs noise | yes | registry+service true; cli/docs noise |

Each task ships `task.json` with `expected.files_that_matter`, `noise_files`,
`forbidden_files`, multi-root `correct_workspace_roots`, and honesty notes.

### Extended metrics (per run JSON)

| metric | definition |
|---|---|
| structure-fact recall / extra-noise | existing P0-5 offline scorer |
| `mcp_or_cli_calls` | `{count, recipe_tools[], grep_count}` — recipe tools for A; grep counts for B (A typically `grep_count=0`) |
| `chose_correct_workspace_root` | `true`/`false` on multi-root tasks when recorded; **`null` (n/a)** on single-root or when arm has no workspace-root choice |
| `file_budget` | \|file_set\| |
| `read_budget` | files opened (from read tool args) or `null` if not recorded |
| `approx_tokens` | runner-reported or **`null`** — **never invented** |
| `runner_id`, `model_note` | who produced the run |
| `independent_session` | bool (see runner table) |
| `saw_labels_before_commit` | **`false`** — decision path did not read expected labels |
| `task_randomization` | per-seed task/arm order |

### Score table — recorded hard-task runs only

Mean over recorded trajectories (N=3 seeds; **no invented cells**).
Offline: `python scripts/eval_agent_ab_c.py score --traj-dir evals/agent-ab-c`

| runner | arm | runs | recall | extra-noise | file_budget | mean MCP/CLI calls | cwr true/false/na | independent |
|---|---|---:|---:|---:|---:|---:|---|---|
| `host_session_llm` | A | 12 | **1.00** | **0.00** | 3.25 | 4.5 | 9 / 0 / 3 | **false** |
| `host_session_llm` | B | 12 | **1.00** | **1.50** | 4.75 | 0.0 (grep only) | 0 / 0 / 12 | **false** |
| `scripted_external_runner` | A | 12 | **0.9375** | **0.25** | 5.00 | 6.0 | 9 / 0 / 3 | **true** (decision path) |
| `scripted_external_runner` | B | 12 | **1.00** | **3.25** | 6.50 | 0.0 | 0 / 9 / 3 | **true** (decision path) |

`cwr` = `chose_correct_workspace_root`; `na` = null (single-root task or no
workspace-root choice). `approx_tokens` **null** in all recorded runs.

#### What the hard-task numbers say (honest)

- **Scripted external runner separates on noise:** A mean extra-noise **0.25**
  vs B **3.25** on hard fixtures (token/same-dir policies pull collision files
  that structure recipes exclude). This is a **scripted tool-policy** result —
  **not** a live LLM product proof.
- **Host-session slice also separates on these hard fixtures** (A noise 0.00 vs
  B 1.50). **We do not oversell** this as multi-model / independent-lab proof:
  `independent_session=false`, same session authored fixtures, N=3, public
  synthetic mini-repos only.
- **Scripted A recall 0.9375:** on `ts-multi-root-client`, recipe assembly
  missed `packages/registry/src/index.ts` (re-export) and also recorded
  package-relative path aliases (`src/...`) alongside workspace paths — honest
  path-normalization gap on multi-root fixtures, not a claimed product win.
- **Workspace-root metric:** scripted/host A used scoped `--workspace-root` on
  multi-root tasks (cwr true 9/9 multi-root A runs). Scripted B pulled wrong-root
  noise (cwr false 9/9 multi-root B runs). Host B cwr is **n/a** (no workspace
  tool args; path-grep only).
- **Budget metrics:** A keeps smaller `file_budget` than B on host-session means
  (3.25 vs 4.75) and scripted means (5.0 vs 6.5). `read_budget` recorded from
  read calls where present; `approx_tokens` remains null.
- **No claim** that live A beats B on production monorepos, ecological soundness,
  or any private corpus. **No private corpus paths** in fixtures or trajectories.

### P0-5c honest limits (required)

- **Not a multi-model lab:** one live `model_note` only. Scripted runner is
  not a second LLM.
- **Host-session contamination disclosed:** fixture authorship + live decisions
  share a session; `independent_session=false` for `host_session_llm`.
- **N=3 recorded** (target protocol N≥5) — incomplete vs lab target, labeled.
- **Hard fixtures are still public synthetic** mini-repos — scores do not
  transfer to production monorepo precision.
- **Arm isolation is tool-policy isolation** (A recipes vs B grep/read), not
  separate machines/API keys for live arms.
- **No oversell:** these rows document an honest multi-runner protocol +
  replayable hard-task trajectories. They are **not** “Agent+MCP product
  superiority” proof.

### P0-5c reproduce

```bash
# write (host_session decisions + scripted external runner on hard fixtures)
python scripts/eval_agent_ab_c.py write --seeds 0,1,2 --bin target/debug/agentgraph.exe
# stamp offline after file-set commitment
python scripts/eval_agent_ab_c.py stamp --traj-dir evals/agent-ab-c
# offline replay (structure-fact + extended metrics)
python scripts/eval_agent_ab_c.py score --traj-dir evals/agent-ab-c
cargo test --test agent_ab_c_eval
```

Harness: [`scripts/eval_agent_ab_c.py`](../scripts/eval_agent_ab_c.py)
(`write` / `stamp` / `score` / `--runner`).
Trajectories: [`evals/agent-ab-c/`](../evals/agent-ab-c/) (**recorded runs only**).
Machine-readable replay: `target/agent_ab_c_replay.json` (not committed).

### Layout additions (P0-5c)

| path | role |
|---|---|
| `docs/eval-agent-baseline.md` § P0-5c | this protocol + recorded hard-task table |
| `scripts/eval_agent_ab_c.py` | harness: write / stamp / score / `--runner` |
| `fixtures/eval-agent-tasks-hard/**` | ≥4 hard public tasks + `task.json` |
| `evals/agent-ab-c/**` | public hard-task multi-runner trajectories + randomization log |
| `evals/agent-ab-c/README.md` | P0-5c replay honesty + layout |
| `tests/agent_ab_c_eval.rs` | hard fixtures + metrics keys + ≥2 runners + honesty gates |
