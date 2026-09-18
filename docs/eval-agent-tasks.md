# Agent code-change task evals (P0-1)

**Status:** shipped first public slice (≥8 public tasks + harness + name-grep baseline).  
**Non-claim / Honesty:**
- Public synthetic fixtures only — **no private stock / proprietary corpus**.
- Scores are **structure facts** on these fixtures (expected files, noise files,
  honesty fields) — **not** production monorepo precision.
- `window=sound` on fixtures is the **`ast_modeled` engineering S gate** only.
- **Non-claim:** this is **not** an ecosystem-level guarantee; this is **not** a
  production-level guarantee; the recipe payloads are **not** a complete
  runtime graph.
- The baseline is a **name-grep** file set (symbol token in source files) —
  **not** a fabricated LLM baseline. We do **not** report invented “no-tool LLM”
  numbers.
- Blast-radius answers **dependents** (who is affected if I change this).
  **Callees** live in the definition / a sibling symbol query — not claimed as
  blast dependents.

**Reproduce**

```bash
cargo build   # or use CARGO_BIN_EXE_agentgraph from `cargo test`
python scripts/eval_agent_tasks.py
# optional CI-style:
cargo test --test agent_task_eval
```

Machine-readable output: `target/agent_task_eval.json`.

---

## Task set

Fixtures: [`fixtures/eval-agent-tasks/`](../fixtures/eval-agent-tasks/)  
Each task dir holds a mini public-domain repo + `task.json` (issue text, symbol,
expected structure facts, recommended tools/commands).

| id | theme | language | symbol | what it probes |
|---|---|---|---|---|
| ts-nest-user-repo | clean TS Nest-like DI | typescript | `UserRepository` | DI ctor + module providers; noise files stay out |
| ts-nest-order-service | clean TS Nest-like DI | typescript | `OrderService` | controller/module dependents; order-word noise ignored |
| py-fastapi-invoice | clean Python | python | `InvoiceService` | deps factory + API handler |
| py-plugin-registry | clean Python registry | python | `PluginRegistry` | bootstrap construction vs docs-string noise |
| go-store-api | clean Go iface/method | go | `Get` | implementors + `store.Repository` call sites |
| rust-trait-handler | Rust trait impl | rust | `render` | dyn dispatch + implementors; `who_calls` separation |
| rust-workspace-two-roots | two-root workspace | rust | `compute` | stay in `engine`; web noise excluded; subset scoped candidates |
| rust-unsafe-scoped | unsafe → sound disabled | rust | `export_rows` | union `window=default`; scoped sound on clean `core` |
| noisy-name-fmt | noisy high-freq name | rust | `fmt` | implementor flood; `who_calls` separates + high-freq demote |

Shipped count: **9** (≥8 gate).

---

## Scoring protocol

For each task the harness:

1. Copies the mini-repo to a temp dir (excludes `task.json` / `.agentgraph`).
2. Indexes (`index` / `index --workspace …`).
3. Runs agent-facing recipes:
   - `blast-radius <symbol> --depth 3`
   - `who-calls <symbol>` (when the task expects implementor/high-freq behavior)
   - `subset` (honesty companion; exit 2 on violations is OK)
   - `find` + `related` (definition / importers — part of the recommended agent path)
   - optional `graph` (CLI subcommand `agentgraph graph`; harness opt-in flag)
4. Builds an **agent structure-fact file set**:
   `blast nodes ∪ node.resolved ∪ find(definition) ∪ related`
   Workspace rows resolve `path` via `root_path` + `root_id`.
5. Computes:
   - **expected-file recall** = \|set ∩ files_that_matter\| / \|files_that_matter\|
   - **extra-noise files** = \|set ∩ noise_files\|
   - **recommendation present** + optional substring checks
   - **window honesty** (`sound` only when `subset_ok`; dirty union never `sound`)
   - **note honesty** (`not a complete runtime graph`)
   - task-specific `who_calls` / scoped-guidance gates
6. Runs the **name-grep baseline** on the same fixture: any source file whose
   text contains the symbol as a token. Same recall/noise metrics.

Exit `0` when every task’s tool runs succeed and honesty/behavior gates pass.

---

## Results (this commit)

Harness: `python scripts/eval_agent_tasks.py`  
JSON: `target/agent_task_eval.json`  
Binary used for the numbers below: debug build with `blast-radius` / `who-calls`
(P3 recipes). Older release binaries without those subcommands are skipped.

### Score table

| task | symbol | ag recall | ag noise files | name-grep recall | name-grep noise | window | rec | pass |
|---|---|---:|---:|---:|---:|---|---|---|
| ts-nest-user-repo | UserRepository | 100% | 0 | 100% | 1 | sound | Y | PASS |
| ts-nest-order-service | OrderService | 100% | 0 | 100% | 1 | sound | Y | PASS |
| py-fastapi-invoice | InvoiceService | 100% | 0 | 100% | 0 | sound | Y | PASS |
| py-plugin-registry | PluginRegistry | 100% | 0 | 100% | 1 | sound | Y | PASS |
| go-store-api | Get | 100% | 0 | 100% | 0 | sound | Y | PASS |
| rust-trait-handler | render | 100% | 0 | 100% | 1 | sound | Y | PASS |
| rust-workspace-two-roots | compute | 100% | 0 | 100% | 2 | sound | Y | PASS |
| rust-unsafe-scoped | export_rows | 100% | 0 | 100% | 0 | **default** | Y | PASS |
| noisy-name-fmt | fmt | 100% | 0 | 100% | 1 | sound | Y | PASS |

### Aggregate (honest)

| metric | agentgraph recipe file-set | name-grep baseline |
|---|---:|---:|
| mean expected-file recall | **1.00** | **1.00** |
| mean extra-noise files in set | **0.00** | **0.78** |
| mean file-set size (example) | smaller / structure-bounded | token-union, includes comment/string hits |

**What the numbers say**

- On these **public structure-fact fixtures**, both methods can hit the labeled
  “files that matter” when the symbol token appears in those files.
- The product difference is **noise / wrong-file edits**: agentgraph recipe sets
  excluded every labeled noise file (mean extra-noise **0**), while name-grep
  pulled comment/string/name-collision files (docs strings, `fmt` substring,
  workspace `compute` comments in the web package, legacy report class names).
- **Window honesty:** `rust-unsafe-scoped` correctly reports
  `window=default`, `subset_ok=false`, `promise_tier=disabled`, with
  `sound_candidates` eligible=`core` / avoid=`legacy` and
  `example_command=impact export_rows --sound --workspace-root core`.
- **Noise governance:** `noisy-name-fmt` `who_calls` reports
  `high_freq_name=true`, `implementor_count≈72` (capped list), callers section
  **without** implementor rows (`implementors_separated=true`).
- Clean two-root workspace: blast nodes stay in `engine`; subset still lists
  scoped `--sound` candidates per root (blast `sound_candidates` empty on
  `window=sound` is honest — no scoped-command spam when already sound).

### Baseline honesty

- Implemented baseline = **name-grep** (deterministic token search).
- We do **not** claim an LLM / “no-tool agent” baseline in this slice.
- A weaker/noisier name set is exactly the failure mode the tasks label
  (`fmt` substring, docs prose, sibling package comments).

---

## Recommended agent path on these tasks

From each `task.json` → `recommended_commands` (example):

```bash
agentgraph index
agentgraph blast-radius <symbol> --depth 3
agentgraph who-calls <symbol>
agentgraph related <symbol>
agentgraph subset
# dirty workspace / unsafe sibling:
agentgraph blast-radius <symbol> --workspace workspace.json
agentgraph impact <symbol> --sound --workspace-root <clean-root> --workspace workspace.json
```

Always read `window`, `subset_ok`, `recommendation`, and `note` before editing
files. See [agent-recipes.md](agent-recipes.md).

---

## Limits (explicit)

- Fixture-scale public repos; not a production monorepo claim
  (private operator trials stay out of CI — see
  [eval-stock-boundary.md](eval-stock-boundary.md) / [eval-large-repo.md](eval-large-repo.md)).
- Expected files are **hand-labeled structure facts** for these tasks, not a
  complete runtime impact proof.
- `blast-radius` = dependents; open the definition (and sibling queries) for
  callees.
- High-freq name demotion is **query-time presentation**; the store keeps edges
  ([noise-governance.md](noise-governance.md)).

---

## Layout / code

| path | role |
|---|---|
| `fixtures/eval-agent-tasks/<id>/` | public mini-repo + `task.json` |
| `scripts/eval_agent_tasks.py` | harness (index + recipes + score + JSON) |
| `target/agent_task_eval.json` | machine-readable scores |
| `tests/agent_task_eval.rs` | fixture schema + harness invocation gate |

Related: [agent-recipes.md](agent-recipes.md), [sound-subset.md](sound-subset.md),
[noise-governance.md](noise-governance.md), [workspace.md](workspace.md).
