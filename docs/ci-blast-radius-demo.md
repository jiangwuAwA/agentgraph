# CI blast-radius comment demo (P2-2)

**Status:** demo workflow + docs (not a required product-quality gate).  
**Non-claim / Honesty:**

- Recipe payloads are **indexed L0/L1 candidates** (S-qualified modeled edges
  only when `subset_ok`). They are **not** a complete runtime graph.
- `window=sound` is the **`ast_modeled` engineering S gate** — **not** ecosystem
  sound, **not** production sound. Do **not** claim zero-miss coverage.
- This workflow is **demo only**. Product quality stays on `ci.yml`
  (`cargo test` + docs_claims + e2e). Do **not** add expand / `cargo-expand`
  install here — macro sidecar remains optional and default OFF.
- PR comments **soft-fail** when `github.token` lacks write permission (forks,
  restricted repos). Missing comments never block merge by themselves.

Related: [onboarding.md](onboarding.md) ·
[agent-recipes.md](agent-recipes.md) ·
[eval-agent-tasks.md](eval-agent-tasks.md) ·
[workspace.md](workspace.md) ·
[agent-goldens.md](agent-goldens.md)

| Artifact | Path |
|---|---|
| Live demo workflow (this repo) | [`.github/workflows/blast-radius-demo.yml`](../.github/workflows/blast-radius-demo.yml) |
| Copyable monorepo template | [`examples/ci/blast-radius.yml`](../examples/ci/blast-radius.yml) |
| Markdown helper (workflow-only) | [`scripts/ci_blast_radius_markdown.py`](../scripts/ci_blast_radius_markdown.py) |
| Local smoke script | [`scripts/ci_blast_radius_demo.sh`](../scripts/ci_blast_radius_demo.sh) |
| Public fixtures used | fixtures/eval-agent-goldens/clean-ts, fixtures/eval-agent-tasks/ts-nest-user-repo |

---

## What the demo does

```text
workflow_dispatch (or pull_request on fixture paths)
  → checkout
  → cargo build --release --bin agentgraph   # or prebuilt install.sh
  → agentgraph --root <fixture> index --force
  → agentgraph --root <fixture> blast-radius <symbol> --depth 3
  → agentgraph --root <fixture> who-calls <symbol>
  → markdown → $GITHUB_STEP_SUMMARY
  → optional PR comment (soft-fail without permission)
```

Default public fixture + symbols:

| Workflow input | Default |
|---|---|
| fixture | fixtures/eval-agent-goldens/clean-ts |
| symbol (blast-radius) | createUser |
| who_symbol | validateEmail |

`workflow_dispatch` accepts overrides for fixture / symbols. PR path filters
touch only public fixture + this workflow/template/helper — **not** a required
check on every code change.

Honesty keys the helper expects on every blast-radius payload
(stable keys — 勿改名, see [agent-recipes.md](agent-recipes.md)):

`window`, `subset_ok`, `promise_tier`, `recommendation`, `note`

---

## Adapt to a real monorepo

### 1. Install agentgraph in the monorepo CI

Prefer a **prebuilt release** (no Rust cache needed in the host repo):

```bash
curl -fsSL https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.sh | bash
```

Or pin a tag / build from a vendored checkout:

```bash
cargo install --path tools/agentgraph   # vendored
# or: cargo build --release --bin agentgraph
```

**Do not** install expand tooling for this demo.

### 2. Point `--root` at a package (or workspace) you care about

Single package:

```bash
agentgraph --root packages/api index --force
agentgraph --root packages/api blast-radius createUser --depth 3
agentgraph --root packages/api who-calls validateEmail
```

Multi-root workspace (see [workspace.md](workspace.md)):

```bash
agentgraph index \
  --workspace-root packages/api \
  --workspace-root packages/web \
  --workspace-db ./ws.db
agentgraph blast-radius createUser --workspace workspace.json
```

When the union window is dirty, read `sound_candidates` / `recommendation` and
prefer **scoped** queries on clean roots — never label a dirty union as sound:

```bash
agentgraph impact createUser --sound --workspace-root api --workspace workspace.json
```

### 3. Trigger on the paths that matter

Map `on.pull_request.paths` to the packages you actually change (not the whole
tree). Optionally pass the base-ref symbol set (changed exported names) into
`blast-radius` instead of a fixed demo symbol.

### 4. Cache the index (optional)

```yaml
- uses: actions/cache@v4
  with:
    path: packages/api/.agentgraph
    key: agentgraph-api-${{ hashFiles('packages/api/src/**') }}
```

Cache is an optimization only. Incremental index already skips unchanged files
when mtime+size match; on flaky network volumes set `AGENTGRAPH_TRUST_MTIME=0`.

### 5. Comment policy (soft-fail)

| Situation | Behavior |
|---|---|
| `github.token` + `pull_request` + write permission | post / update comment |
| Fork PR or `pull-requests: read` only | **soft-fail** — skip comment, job stays green |
| No PR context (`workflow_dispatch`) | step summary only |

Suggested comment body: markdown from the workflow helper
(`scripts/ci_blast_radius_markdown.py` — invoked by the workflow, not a
product CLI). Body shows a table of honesty fields plus node paths and a
who-calls summary.

Mark the comment with an HTML marker (`<!-- agentgraph-blast-radius-demo -->`)
so you can update instead of spam a new comment each push.

### 6. Required vs demo

| Gate | Role |
|---|---|
| ci.yml (fmt / clippy / test / docs_claims / e2e) | **Required** product quality |
| blast-radius-demo.yml / your monorepo comment job | **Demo / advisory** |

Only promote a monorepo blast-radius job to required after you have measured
noise on **your** tree. Public-fixture scores live in
[eval-agent-tasks.md](eval-agent-tasks.md); they are **not** a production
monorepo precision claim.

### 7. What not to oversell in the comment

- Do **not** write “complete impact” / complete runtime graph.
- Do **not** write zero-miss / ecosystem sound / production sound.
- Always surface `window`, `subset_ok`, and `note`.
- Say when the window is `default` (Exact+Heuristic) vs `sound` (S-qualified).

---

## Local smoke (optional)

```bash
bash scripts/ci_blast_radius_demo.sh
# optional env: CI_FIXTURE / CI_SYMBOL / CI_WHO_SYMBOL / CI_OUT_DIR
```

The smoke script builds or reuses an agentgraph binary, indexes a **public**
fixture, runs the real agentgraph commands, and writes markdown under
`target/ci-blast-radius-demo/`. Exit 0 on success.

Manual agentgraph-only path (same product commands the workflow runs):

```bash
cargo build --release --bin agentgraph
FIXTURE=fixtures/eval-agent-goldens/clean-ts
target/release/agentgraph --root "$FIXTURE" index --force
target/release/agentgraph --root "$FIXTURE" blast-radius createUser --depth 3
target/release/agentgraph --root "$FIXTURE" who-calls validateEmail
```

---

## Limits (explicit)

- Public synthetic fixtures only in this demo — no private corpus paths.
- Blast-radius answers **dependents** (who is affected if I change this), not
  callees.
- Name/type resolution is pragmatic (not full type inference). Dynamic edges
  appear only when you opt into a wider window (`--recall` / `--include-dynamic`)
  — never as the recipe default and never labeled sound.
- Workflow YAML is a sample; monorepo permissions and path filters are yours to
  set.
