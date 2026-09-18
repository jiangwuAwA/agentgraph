#!/usr/bin/env python3
"""P0-5d isolated lab harness — prepare / run-runner / stamp / score / lab-ready.

Isolation contract (protocol: docs/eval-agent-baseline.md § P0-5d)
-------------------------------------------------------------------
- Each (runner_id, arm, seed, task) cell is an **independent agent session**.
- Brief packs are **issue-only**: no structure-fact labels, no golden lists,
  no other-arm file sets, no trajectory scores.
- Decision path cannot read fixture ``task.json`` labels of this or other arms.
- The harness **never injects goldens**. Runners write ``file_set.json`` +
  ``meta.json`` into their brief dir; ``stamp`` fills scores **offline after**
  commitment from fixture ``task.json``.
- Metrics align P0-5c: recall / extra-noise / cwr / mcp_or_cli_calls /
  file_budget / read_budget / ``approx_tokens`` null-allowed.

lab_ready (true only when all hold)
-----------------------------------
1. ≥2 **live** runner ids (``kind=live_llm_agent``)
2. every counted live runner has ``independent_session=true``
3. N≥5 seeds per (runner, arm, task) cell over the selected task set
4. non-author models only (``mimo-desktop-host-session`` / fixture-author
   session ⇒ not lab-grade)
5. both arms present per cell; easy≥4 + hard≥4 tasks covered

Otherwise ``lab_ready=false`` and the gap list is printed. **No oversell.**

Runner contract
---------------
After ``prepare``, each external runner writes into its brief dir::

    evals/agent-ab-d/_briefs/<runner>/<arm>/<seed>/<task>/
      ISSUE.md          # issue-only (written by harness)
      RUNNER_CONTRACT.md
      workdir/          # isolated fixture copy (no task.json)
      file_set.json     # runner decision path
      meta.json         # runner honesty / identity fields

``file_set.json`` schema: ``agentgraph.eval_agent_ab_d.file_set.v1``
``meta.json`` schema: ``agentgraph.eval_agent_ab_d.meta.v1``

Honesty
-------
- Scripted isolated runner is **not** a live LLM.
- No fabricated multi-model lab numbers.
- No private corpus paths.
- ``lab_ready`` is never claimed true on an incomplete matrix.

Exit codes: 0 ok · 1 gate/IO/forgery problems · 2 usage.
"""
from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Tuple

_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

from eval_agent_ab import (  # noqa: E402
    _as_rel,
    discover_traj_files,
    score_file_set as _score_file_set,
    score_trajectory,
    task_labels as _task_labels,
    write_trajectory as _write_trajectory,
)
from eval_agent_ab_c import (  # noqa: E402
    _is_grep_tool,
    _is_recipe_tool,
    empty_labels,
    empty_score,
)
from eval_agent_tasks import (  # noqa: E402
    copy_fixture,
    repo_root_from_script,
)

TRAJECTORY_SCHEMA = "agentgraph.eval_agent_ab.trajectory.v1"
D_SCHEMA_ALIAS = "agentgraph.eval_agent_ab.d.v1"
FILE_SET_SCHEMA = "agentgraph.eval_agent_ab_d.file_set.v1"
META_SCHEMA = "agentgraph.eval_agent_ab_d.meta.v1"
SELECTION_SCHEMA = "agentgraph.eval_agent_ab_d.selection.v1"
RANDOMIZATION_SCHEMA = "agentgraph.eval_agent_ab_d.randomization.v1"
REPLAY_SCHEMA = "agentgraph.eval_agent_ab_d.replay.v1"
LAB_READY_SCHEMA = "agentgraph.eval_agent_ab_d.lab_ready.v1"
HARNESS_VERSION = "agentgraph.eval_agent_ab_d.harness.v1"

# Strings that must never appear in brief packs (issue-only / blind decision path).
# Tests lock the first two; remaining label keys are also banned.
BRIEF_BAN_STRINGS = (
    "expected",
    "noise_files",
    "files_that_matter",
    "forbidden_files",
)

# Substrings that mark a model/session as fixture-author (not lab-grade).
AUTHOR_MODEL_MARKERS = (
    "mimo-desktop-host-session",
    "author-session",
    "fixture-author",
    "fixture_author",
    "host-session",
)

# Honest offline runner for protocol parity / tests — NOT a live LLM.
SCRIPTED_RUNNER_ID = "scripted_isolated_runner"
SCRIPTED_MODEL_NOTE = "scripted_deterministic_policy"
SCRIPTED_KIND = "scripted_external_runner"

# Planned live slots for S2 (briefs materialized; trajectories only after runners run).
PLANNED_LIVE_RUNNER_SLOTS = (
    "external_live_runner_1",
    "external_live_runner_2",
)

DEFAULT_SEEDS = (0, 1, 2, 3, 4)  # N=5 lab target
DEFAULT_PREPARE_RUNNERS = (SCRIPTED_RUNNER_ID,) + PLANNED_LIVE_RUNNER_SLOTS

# Easy public tasks (P0-1 / P0-5b set) — language diversity + labeled noise.
EASY_TASKS = (
    "ts-nest-user-repo",
    "rust-trait-handler",
    "py-plugin-registry",
    "go-store-api",
)

# Hard public tasks (all four P0-5c fixtures).
HARD_TASKS = (
    "rust-cross-crate-blast",
    "rust-real-noise-dense",
    "rust-sound-scoped-clean",
    "ts-multi-root-client",
)

TASK_SELECTION_RATIONALE = {
    "ts-nest-user-repo": "easy · TypeScript Nest-like DI; clean vs health/unrelated noise",
    "rust-trait-handler": "easy · Rust trait impl blast; metrics sibling noise",
    "py-plugin-registry": "easy · Python registry; docs_strings noise",
    "go-store-api": "easy · Go store API; package-sibling noise",
    "rust-cross-crate-blast": "hard · multi-root cross-crate blast + name collision",
    "rust-real-noise-dense": "hard · dense implementors + encode name collisions",
    "rust-sound-scoped-clean": "hard · sound-disabled dirty sibling + clean scoped root",
    "ts-multi-root-client": "hard · multi-root TS wrong-root + help/docs noise",
}

ARM_TOOLS = {
    "A": [
        "agentgraph: index",
        "agentgraph: blast-radius",
        "agentgraph: who-calls",
        "agentgraph: find",
        "agentgraph: related",
        "agentgraph: subset",
        "read",
    ],
    "B": ["walk", "grep", "read"],
}

# Contamination ban list — documented in protocol + selection; NOT written into briefs.
CONTAMINATION_BAN_LIST = [
    "fixture task.json structure-fact labels (expected / noise / forbidden / workspace roots)",
    "golden file sets or scored tables from any arm, seed, or runner",
    "shared intermediate file-set files between arms",
    "shared workdirs or .agentgraph index.db between arms",
    "P0-5b / P0-5c trajectory scores visible to the decision path",
    "fixture-author session context that already saw labels (forces lab_ready=false)",
    "private monorepo / stock corpus paths",
    "injected harness goldens into brief packs",
]


def repo_root() -> Path:
    return repo_root_from_script()


def easy_fixtures_dir(root: Path) -> Path:
    return root / "fixtures" / "eval-agent-tasks"


def hard_fixtures_dir(root: Path) -> Path:
    return root / "fixtures" / "eval-agent-tasks-hard"


def selected_tasks() -> List[str]:
    return list(EASY_TASKS) + list(HARD_TASKS)


def task_fixture_dir(root: Path, task_id: str) -> Path:
    if task_id in HARD_TASKS:
        return hard_fixtures_dir(root) / task_id
    return easy_fixtures_dir(root) / task_id


def load_task_meta(root: Path, task_id: str) -> Dict[str, Any]:
    path = task_fixture_dir(root, task_id) / "task.json"
    return json.loads(path.read_text(encoding="utf-8"))


def is_author_model(model_note: str) -> bool:
    s = (model_note or "").lower()
    return any(m in s for m in AUTHOR_MODEL_MARKERS)


def task_randomization_d(
    tasks: Sequence[str],
    seed: int,
    runners: Sequence[str],
) -> Dict[str, Any]:
    """Task-level randomization; arm order alternates by seed parity."""
    rng = random.Random(f"p0-5d-{seed}")
    order = list(tasks)
    rng.shuffle(order)
    arm_order = ["A", "B"] if seed % 2 == 0 else ["B", "A"]
    runner_order = list(runners)
    rng.shuffle(runner_order)
    return {
        "seed": seed,
        "task_order": order,
        "arm_order": arm_order,
        "runner_order": runner_order,
        "method": (
            "task-level randomization; arm order alternates by seed parity; "
            "order recorded before arm execution"
        ),
    }


def brief_dir(
    traj_dir: Path,
    runner_id: str,
    arm: str,
    seed: int,
    task_id: str,
) -> Path:
    return traj_dir / "_briefs" / runner_id / arm / str(seed) / task_id


def issue_only_markdown(
    task_meta: Dict[str, Any],
    *,
    task_id: str,
    runner_id: str,
    arm: str,
    seed: int,
) -> str:
    """Issue-only brief pack body. Must not contain banned label strings."""
    issue = (task_meta.get("issue") or "").strip()
    title = (task_meta.get("title") or task_id).strip()
    symbol = (task_meta.get("symbol") or "").strip()
    language = (task_meta.get("language") or "").strip()
    arm_tools = "agentgraph MCP/CLI recipes + reads" if arm == "A" else "walk / grep / read only (no agentgraph)"
    lines = [
        f"# Issue brief — {task_id}",
        "",
        f"- task: `{task_id}`",
        f"- title: {title}",
        f"- runner_id: `{runner_id}`",
        f"- arm: **{arm}**",
        f"- seed: `{seed}`",
        f"- language: {language or 'n/a'}",
        f"- symbol token (from issue): `{symbol}`" if symbol else "- symbol token: (see issue)",
        f"- tools allowed: {arm_tools}",
        "",
        "## Issue",
        "",
        issue,
        "",
        "## Decision-path constraints (blind)",
        "",
        "- Work only inside this cell's `workdir/` (isolated copy; no structure-fact metadata).",
        "- Identify the set of source files that must be reviewed for the issue.",
        "- Write `file_set.json` and `meta.json` into **this** directory (the brief dir).",
        "- Do **not** read structure-fact labels, scored tables, or other arms'/runners' outputs.",
        "- Do **not** share intermediate file-set files across arms or seeds.",
        "- Arm A may invoke agentgraph recipes; arm B must not.",
        "",
        "## Outputs (contract)",
        "",
        "```",
        "file_set.json  — schema agentgraph.eval_agent_ab_d.file_set.v1",
        "meta.json      — schema agentgraph.eval_agent_ab_d.meta.v1",
        "```",
        "",
        "Harness never injects answer-key lists into brief packs. Scores are stamped offline after commitment.",
        "",
    ]
    body = "\n".join(lines)
    lowered = body.lower()
    for banned in BRIEF_BAN_STRINGS:
        if banned.lower() in lowered:
            raise RuntimeError(
                f"brief pack would contain banned string {banned!r} "
                f"for {task_id}/{runner_id}/{arm}/{seed}"
            )
    return body


def runner_contract_markdown(runner_id: str, arm: str, seed: int, task_id: str) -> str:
    """Contract file for external runners. Avoids banned brief strings."""
    return f"""# Runner contract — P0-5d isolated cell

- cell: `{runner_id}` / arm `{arm}` / seed `{seed}` / task `{task_id}`
- harness: `{HARNESS_VERSION}`
- independent session required: **true** for lab-grade live runners
- decision path must not see structure-fact labels or other cells' outputs

## You must write

### `file_set.json`

```json
{{
  "schema": "{FILE_SET_SCHEMA}",
  "task_id": "{task_id}",
  "runner_id": "{runner_id}",
  "arm": "{arm}",
  "seed": {seed},
  "file_set": ["path/relative/to/workdir.ts"],
  "tool_calls": [
    {{"tool": "read", "args": ["src/x.ts"], "ok": true, "summary": {{}}, "note": ""}}
  ],
  "chose_correct_workspace_root": null,
  "approx_tokens": null
}}
```

### `meta.json`

```json
{{
  "schema": "{META_SCHEMA}",
  "runner_id": "{runner_id}",
  "kind": "live_llm_agent",
  "model_note": "<public-model-id-or-operator-note>",
  "independent_session": true,
  "harness_version": "{HARNESS_VERSION}",
  "saw_labels_before_commit": false,
  "arm_isolated": true,
  "read_budget": null,
  "approx_tokens": null,
  "lab_ready_claim": false
}}
```

## Rules

- `approx_tokens`: real runner value or `null` — never invented.
- `saw_labels_before_commit`: must be `false` on a clean decision path.
- `lab_ready_claim`: leave `false`; the harness computes lab-ready from the matrix.
- Scripted offline fills must set `kind=scripted_external_runner` and a non-live `model_note`.
- Harness `stamp` scores offline from fixture metadata after you commit `file_set.json`.
"""


def clean_args_d(args: Sequence[Any]) -> List[str]:
    out: List[str] = []
    for a in args:
        s = str(a)
        if s.startswith("\\\\") or (len(s) >= 3 and s[1] == ":" and s[2] in "\\/"):
            out.append("<fixture-root>")
        elif s.startswith("/"):
            out.append("<fixture-root>")
        else:
            out.append(s.replace("\\", "/"))
    return out


def compute_ext_metrics_d(
    file_set: Sequence[str],
    tool_calls: Sequence[Dict[str, Any]],
    runner_id: str,
    model_note: str,
    independent_session: bool,
    chose_correct_workspace_root: Optional[bool],
    task_meta: Optional[Dict[str, Any]] = None,
    approx_tokens: Optional[int] = None,
    read_budget: Optional[int] = None,
) -> Dict[str, Any]:
    recipe_tools: List[str] = []
    grep_count = 0
    read_files: List[str] = []
    mcp_count = 0
    for c in tool_calls:
        tool = str(c.get("tool") or "")
        if _is_recipe_tool(tool):
            mcp_count += 1
            short = tool.split(".")[-1].replace("_", "-")
            if short not in recipe_tools:
                recipe_tools.append(short)
        if _is_grep_tool(tool):
            grep_count += 1
        if tool.lower().startswith("read"):
            for a in c.get("args") or []:
                s = str(a)
                if "/" in s or s.endswith((".rs", ".ts", ".py", ".go", ".js")):
                    if s not in read_files:
                        read_files.append(s)
    multi = False
    if task_meta:
        ws = task_meta.get("workspace") or {}
        expected = task_meta.get("expected") or {}
        multi = bool(ws.get("roots")) or bool(expected.get("multi_root"))
        if expected.get("correct_workspace_roots"):
            multi = True
    if not multi:
        cwr: Optional[bool] = None
    else:
        cwr = chose_correct_workspace_root
    if read_budget is None:
        read_budget = len(read_files) if read_files else None
    return {
        "mcp_or_cli_calls": {
            "count": mcp_count,
            "recipe_tools": recipe_tools,
            "grep_count": grep_count,
        },
        "chose_correct_workspace_root": cwr,
        "file_budget": len(sorted({_as_rel(p) for p in file_set if p})),
        "read_budget": read_budget,
        "read_files": read_files,
        "approx_tokens": approx_tokens,
        "runner_id": runner_id,
        "model_note": model_note,
        "independent_session": independent_session,
        "saw_labels_before_commit": False,
    }


def build_d_trajectory(
    *,
    task_id: str,
    arm: str,
    seed: int,
    runner_id: str,
    kind: str,
    model_note: str,
    independent_session: bool,
    files: Sequence[str],
    calls: Sequence[Dict[str, Any]],
    rationale: str,
    task_meta: Dict[str, Any],
    fixture_rel: str,
    rand: Dict[str, Any],
    chose_correct_workspace_root: Optional[bool] = None,
    approx_tokens: Optional[int] = None,
    read_budget: Optional[int] = None,
    extra_honesty: Optional[Dict[str, Any]] = None,
    brief_rel: str = "",
) -> Dict[str, Any]:
    cleaned_calls = []
    for i, c in enumerate(calls, start=1):
        cc = dict(c)
        cc["seq"] = i
        cc["args"] = clean_args_d(cc.get("args") or [])
        cleaned_calls.append(cc)
    ext = compute_ext_metrics_d(
        files,
        cleaned_calls,
        runner_id=runner_id,
        model_note=model_note,
        independent_session=independent_session,
        chose_correct_workspace_root=chose_correct_workspace_root,
        task_meta=task_meta,
        approx_tokens=approx_tokens,
        read_budget=read_budget,
    )
    honesty = {
        "live_llm_agent": kind == "live_llm_agent",
        "scripted_tool_policy_agent": False,
        "scripted_external_runner": kind == SCRIPTED_KIND,
        "private_corpus": False,
        "not_public_benchmark_model": kind == "live_llm_agent",
        "standardized_lab_harness": False,
        "independent_session": independent_session,
        "saw_labels_before_commit": False,
        "isolated_session": True,
        "issue_only_brief": True,
        "no_fabricated_llm_numbers": True,
        "no_oversell_live_a_beats_b": True,
        "lab_ready_claim": False,
        "contamination_ban_list": list(CONTAMINATION_BAN_LIST),
        "note": (
            "P0-5d isolated cell — decision path from issue-only brief + workdir; "
            "structure-fact scores stamped offline after commitment. "
            "Not a product-superiority claim."
        ),
    }
    if extra_honesty:
        honesty.update(extra_honesty)
    if is_author_model(model_note):
        honesty["author_model_session"] = True
        honesty["lab_eligible_runner"] = False
    else:
        honesty["author_model_session"] = False
        honesty["lab_eligible_runner"] = bool(
            kind == "live_llm_agent" and independent_session
        )
    meta = {
        "id": task_id,
        "title": task_meta.get("title"),
        "language": task_meta.get("language"),
        "issue": task_meta.get("issue") or "",
        "symbol": task_meta.get("symbol"),
    }
    labels = empty_labels()
    score = empty_score(files)
    payload = {
        "schema": TRAJECTORY_SCHEMA,
        "schema_alias": D_SCHEMA_ALIAS,
        "protocol": "p0-5d-isolated-lab",
        "harness_version": HARNESS_VERSION,
        "policy": arm,
        "policy_label": (
            "P0-5d arm A — agentgraph recipes (+reads) in isolated session"
            if arm == "A"
            else "P0-5d arm B — walk/grep/read only (no agentgraph) in isolated session"
        ),
        "kind": kind,
        "arm": arm,
        "runner_id": runner_id,
        "task_id": task_id,
        "seed": seed,
        "run_id": f"{runner_id}-{arm.lower()}-{seed}",
        "fixture": fixture_rel,
        "brief": brief_rel,
        "symbol": meta.get("symbol") or "",
        "model_note": model_note,
        "independent_session": independent_session,
        "saw_labels_before_commit": False,
        "tools_allowed": list(ARM_TOOLS.get(arm, [])),
        "mcp_or_cli_calls": ext["mcp_or_cli_calls"],
        "chose_correct_workspace_root": ext["chose_correct_workspace_root"],
        "file_budget": ext["file_budget"],
        "read_budget": ext["read_budget"],
        "approx_tokens": ext["approx_tokens"],
        "metrics_ext": ext,
        "task_randomization": rand,
        "honesty": honesty,
        "task": {
            "id": task_id,
            "title": meta.get("title"),
            "language": meta.get("language"),
            "issue": meta.get("issue") or "",
            "symbol": meta.get("symbol"),
            **labels,
        },
        "tool_calls": cleaned_calls,
        "file_set": sorted({_as_rel(p) for p in files if p}),
        "score": score,
        "extras": {
            "runner_id": runner_id,
            "model_note": model_note,
            "arm": arm,
            "rationale": rationale,
            "protocol": (
                "P0-5d isolated lab; issue-only briefs; independent session per "
                "(runner, arm, seed, task); labels stamped offline after commitment"
            ),
            "isolated_slice": True,
        },
        "notes": [
            honesty["note"],
            f"Arm {arm}: {rationale}",
        ],
        "errors": [],
    }
    return payload


# ---------------------------------------------------------------------------
# prepare
# ---------------------------------------------------------------------------


def cmd_prepare(args: argparse.Namespace) -> int:
    root = repo_root()
    traj_dir = Path(args.traj_dir) if args.traj_dir else (root / "evals" / "agent-ab-d")
    seeds = list(args.seeds) if args.seeds else list(DEFAULT_SEEDS)
    runners = list(args.runner) if args.runner else list(DEFAULT_PREPARE_RUNNERS)
    tasks = selected_tasks()
    if args.task:
        tasks = [t for t in tasks if t in set(args.task)]
    if not tasks:
        print("error: no tasks selected", file=sys.stderr)
        return 2
    for tid in tasks:
        tdir = task_fixture_dir(root, tid)
        if not (tdir / "task.json").is_file():
            print(f"error: missing fixture task.json for {tid} under {tdir}", file=sys.stderr)
            return 2

    traj_dir.mkdir(parents=True, exist_ok=True)
    work_root = Path(args.work_root) if args.work_root else (root / "target" / "agent-ab-d-work")

    selection = {
        "schema": SELECTION_SCHEMA,
        "protocol": "p0-5d-isolated-lab",
        "harness_version": HARNESS_VERSION,
        "easy_tasks": [t for t in tasks if t in EASY_TASKS],
        "hard_tasks": [t for t in tasks if t in HARD_TASKS],
        "rationale": {t: TASK_SELECTION_RATIONALE.get(t, "") for t in tasks},
        "selection_note": (
            "easy≥4 + hard≥4 from public fixtures only; "
            "hard set = all four P0-5c fixtures; easy set = P0-5b language-diverse clean/noise tasks"
        ),
        "seeds_target": seeds,
        "n_per_cell_target": len(seeds),
        "prepare_runners": runners,
        "planned_live_runner_slots": [
            {
                "runner_id": rid,
                "status": "not_run",
                "independent_session_required": True,
                "non_author_model_required": True,
                "kind_required": "live_llm_agent",
            }
            for rid in PLANNED_LIVE_RUNNER_SLOTS
        ],
        "scripted_offline_runner": {
            "runner_id": SCRIPTED_RUNNER_ID,
            "kind": SCRIPTED_KIND,
            "model_note": SCRIPTED_MODEL_NOTE,
            "independent_session": True,
            "live_llm": False,
            "note": "protocol parity / offline fill only — not a live lab model",
        },
        "contamination_ban_list": list(CONTAMINATION_BAN_LIST),
        "lab_ready_definition": {
            "min_live_runners": 2,
            "min_seeds_per_cell": 5,
            "require_independent_session": True,
            "require_non_author_models": True,
            "require_both_arms": True,
            "require_easy_and_hard": True,
        },
        "task_order_base": tasks,
    }
    (traj_dir / "task_selection.json").write_text(
        json.dumps(selection, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )

    order_log: List[Dict[str, Any]] = []
    for seed in seeds:
        rand = task_randomization_d(tasks, seed, runners)
        order_log.append(rand)
        for runner_id in rand["runner_order"]:
            for tid in rand["task_order"]:
                if tid not in tasks:
                    continue
                task_meta = load_task_meta(root, tid)
                for arm in rand["arm_order"]:
                    bdir = brief_dir(traj_dir, runner_id, arm, seed, tid)
                    bdir.mkdir(parents=True, exist_ok=True)
                    issue_md = issue_only_markdown(
                        task_meta,
                        task_id=tid,
                        runner_id=runner_id,
                        arm=arm,
                        seed=seed,
                    )
                    (bdir / "ISSUE.md").write_text(issue_md, encoding="utf-8")
                    (bdir / "RUNNER_CONTRACT.md").write_text(
                        runner_contract_markdown(runner_id, arm, seed, tid),
                        encoding="utf-8",
                    )
                    # Isolated workdir: fixture copy without task.json / indexes.
                    if not args.skip_workdirs:
                        workdir = bdir / "workdir"
                        src = task_fixture_dir(root, tid)
                        copy_fixture(src, workdir)
                        # Belt-and-suspenders: never leave task.json in workdir.
                        leaked = workdir / "task.json"
                        if leaked.is_file():
                            leaked.unlink()
                        # Also materialize under target/ for operator runners if requested.
                        if args.also_work_root:
                            alt = work_root / runner_id / arm / str(seed) / tid
                            alt.mkdir(parents=True, exist_ok=True)
                            copy_fixture(src, alt)
                            alt_task = alt / "task.json"
                            if alt_task.is_file():
                                alt_task.unlink()

    log_path = traj_dir / "task_randomization.json"
    log_path.write_text(
        json.dumps(
            {
                "schema": RANDOMIZATION_SCHEMA,
                "protocol": "p0-5d-isolated-lab",
                "harness_version": HARNESS_VERSION,
                "method": (
                    "task-level randomization; arm order alternates by seed parity; "
                    "order recorded before arm execution"
                ),
                "seeds": order_log,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )

    # Top-level honesty README for the trajectory tree.
    readme = traj_dir / "README.md"
    readme.write_text(
        _evals_readme(tasks=tasks, seeds=seeds, runners=runners),
        encoding="utf-8",
    )

    n_briefs = 0
    briefs_root = traj_dir / "_briefs"
    if briefs_root.is_dir():
        n_briefs = sum(1 for p in briefs_root.rglob("ISSUE.md") if p.is_file())
    print(f"prepared {n_briefs} issue-only briefs under {traj_dir / '_briefs'}")
    print(f"task_selection: {traj_dir / 'task_selection.json'}")
    print(f"randomization: {log_path}")
    print("lab_ready remains false until live runners fill the matrix (see lab-ready)")
    return 0


def _evals_readme(*, tasks: Sequence[str], seeds: Sequence[int], runners: Sequence[str]) -> str:
    return f"""# Public P0-5d isolated-lab artifacts

Replay + isolation harness for
[docs/eval-agent-baseline.md](../../docs/eval-agent-baseline.md) § **P0-5d**.

## Status (S1)

- **Harness + protocol shipped.** `lab_ready=**false**` until ≥2 **non-author**
  live runners with `independent_session=true` fill **N≥5** seeds per cell.
- This tree currently holds **issue-only brief packs**, task selection, and
  randomization logs — **not** a complete live lab table.
- **No product-superiority claim.** Do not cite incomplete cells as lab proof.

## Task set (easy≥4 + hard≥4)

| tier | task ids |
|---|---|
| easy | {", ".join(f"`{t}`" for t in tasks if t in EASY_TASKS)} |
| hard | {", ".join(f"`{t}`" for t in tasks if t in HARD_TASKS)} |

Selection rationale: [`task_selection.json`](task_selection.json).
Randomization order: [`task_randomization.json`](task_randomization.json).

## Isolation layout

```text
evals/agent-ab-d/
  task_selection.json
  task_randomization.json
  _briefs/<runner_id>/<arm>/<seed>/<task_id>/
    ISSUE.md              # issue text only — no structure-fact labels
    RUNNER_CONTRACT.md
    workdir/              # isolated fixture copy (no task.json); gitignored
    file_set.json         # runner writes (after live/scripted run)
    meta.json             # runner honesty fields
  <task_id>/
    run-<runner_id>-<arm>-<seed>.json   # harness stamp output (after run)
```

Brief packs must **not** contain structure-fact label keys or golden lists.
The decision path cannot read other arms' file sets or scores.

## Runner contract (summary)

1. `python scripts/eval_agent_ab_d.py prepare`
2. External runner executes one **independent session** per cell using
   `ISSUE.md` + `workdir/` only.
3. Runner writes `file_set.json` + `meta.json` into the brief dir.
4. `python scripts/eval_agent_ab_d.py stamp --traj-dir evals/agent-ab-d`
5. `python scripts/eval_agent_ab_d.py score --traj-dir evals/agent-ab-d`
6. `python scripts/eval_agent_ab_d.py lab-ready --traj-dir evals/agent-ab-d`

Harness **never injects goldens**. Offline stamp uses fixture `task.json`.

## Metrics (aligned P0-5c)

`recall` · `extra-noise` · `cwr` (`chose_correct_workspace_root`) ·
`mcp_or_cli_calls` · `file_budget` · `read_budget` · `approx_tokens` (null-allowed)

## Planned runners (not yet live)

| runner_id | status | requirement |
|---|---|---|
| `scripted_isolated_runner` | offline protocol parity only | **not** live LLM |
| `external_live_runner_1` | not_run (S2) | live, independent_session=true, non-author model |
| `external_live_runner_2` | not_run (S2) | live, independent_session=true, non-author model |

Seeds target: {", ".join(str(s) for s in seeds)} (N={len(seeds)}).
Prepare runner slots: {", ".join(f"`{r}`" for r in runners)}.

## Offline replay

```bash
python scripts/eval_agent_ab_d.py score --traj-dir evals/agent-ab-d
python scripts/eval_agent_ab_d.py lab-ready --traj-dir evals/agent-ab-d
cargo test --test agent_ab_d_eval
```

Schema: `{TRAJECTORY_SCHEMA}` (alias `{D_SCHEMA_ALIAS}`).

## Honesty (mandatory)

- Scripted offline runner is **not** a live LLM / multi-model lab.
- Incomplete matrix ⇒ `lab_ready=false` + gap list. Never invent cells.
- Author-session models (e.g. host-session fixture author) force `lab_ready=false`.
- No private corpus paths.
- README product link for this slice is **withheld** until `lab_ready=true`.
"""


# ---------------------------------------------------------------------------
# run-runner (contract + optional scripted fill)
# ---------------------------------------------------------------------------


def read_brief_outputs(bdir: Path) -> Tuple[Optional[Dict[str, Any]], Optional[Dict[str, Any]], List[str]]:
    errors: List[str] = []
    fs_path = bdir / "file_set.json"
    meta_path = bdir / "meta.json"
    fs_obj: Optional[Dict[str, Any]] = None
    meta_obj: Optional[Dict[str, Any]] = None
    if not fs_path.is_file():
        errors.append(f"missing file_set.json in {bdir}")
    else:
        try:
            fs_obj = json.loads(fs_path.read_text(encoding="utf-8"))
        except Exception as e:  # noqa: BLE001
            errors.append(f"invalid file_set.json in {bdir}: {e}")
    if not meta_path.is_file():
        errors.append(f"missing meta.json in {bdir}")
    else:
        try:
            meta_obj = json.loads(meta_path.read_text(encoding="utf-8"))
        except Exception as e:  # noqa: BLE001
            errors.append(f"invalid meta.json in {bdir}: {e}")
    return fs_obj, meta_obj, errors


def validate_runner_outputs(
    fs_obj: Dict[str, Any],
    meta_obj: Dict[str, Any],
    *,
    runner_id: str,
    arm: str,
    seed: int,
    task_id: str,
) -> List[str]:
    errors: List[str] = []
    if fs_obj.get("schema") not in {FILE_SET_SCHEMA, None}:
        # allow missing schema on early drafts but require when present to match if set
        if fs_obj.get("schema") and fs_obj.get("schema") != FILE_SET_SCHEMA:
            errors.append(f"file_set schema must be {FILE_SET_SCHEMA}, got {fs_obj.get('schema')!r}")
    if meta_obj.get("schema") not in {META_SCHEMA, None}:
        if meta_obj.get("schema") and meta_obj.get("schema") != META_SCHEMA:
            errors.append(f"meta schema must be {META_SCHEMA}, got {meta_obj.get('schema')!r}")
    for key, want in (("runner_id", runner_id), ("arm", arm), ("seed", seed), ("task_id", task_id)):
        if key in fs_obj and fs_obj.get(key) != want:
            errors.append(f"file_set.{key} mismatch: {fs_obj.get(key)!r} != {want!r}")
    if "file_set" not in fs_obj or not isinstance(fs_obj.get("file_set"), list):
        errors.append("file_set.json must contain file_set: list")
    if meta_obj.get("saw_labels_before_commit") not in {False, None}:
        errors.append("meta.saw_labels_before_commit must be false")
    if meta_obj.get("lab_ready_claim") not in {False, None}:
        errors.append("meta.lab_ready_claim must be false (harness computes lab-ready)")
    if not meta_obj.get("model_note"):
        errors.append("meta.model_note required")
    if meta_obj.get("independent_session") is None:
        errors.append("meta.independent_session required")
    if meta_obj.get("kind") == "live_llm_agent" and meta_obj.get("independent_session") is not True:
        errors.append("live_llm_agent meta must set independent_session=true for lab-eligible cells")
    return errors


def fill_scripted_cell(
    root: Path,
    bdir: Path,
    *,
    runner_id: str,
    arm: str,
    seed: int,
    task_id: str,
    task_meta: Dict[str, Any],
) -> List[str]:
    """Offline deterministic fill for protocol parity — not a live LLM."""
    errors: List[str] = []
    workdir = bdir / "workdir"
    if not workdir.is_dir():
        copy_fixture(task_fixture_dir(root, task_id), workdir)
        leaked = workdir / "task.json"
        if leaked.is_file():
            leaked.unlink()
    symbol = task_meta.get("symbol") or ""
    # Decision path: walk sources + token search only (no labels).
    sources: List[Path] = []
    skip = {".agentgraph", "target", "node_modules", ".git", "__pycache__"}
    for p in sorted(workdir.rglob("*")):
        if not p.is_file():
            continue
        if any(part in skip for part in p.parts):
            continue
        if p.suffix.lower() not in {".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".rs"}:
            continue
        sources.append(p)

    def rel(p: Path) -> str:
        try:
            return _as_rel(str(p.relative_to(workdir)))
        except ValueError:
            return _as_rel(str(p))

    files: List[str] = []
    calls: List[Dict[str, Any]] = []
    if arm == "A":
        # Without forcing a live binary in S1 tests: token hits + definition-like
        # lines stand in for recipe-bounded review set. Still labeled scripted.
        calls.append(
            {
                "tool": "agentgraph.find",
                "args": ["find", symbol] if symbol else ["find"],
                "ok": True,
                "summary": {"mode": "scripted_offline_placeholder"},
                "note": "scripted isolated A — not live; recipe binary optional",
            }
        )
        calls.append(
            {
                "tool": "walk",
                "args": ["walk_sources"],
                "ok": True,
                "summary": {"source_count": len(sources)},
                "note": "enumerate sources in isolated workdir",
            }
        )
        for p in sources:
            try:
                text = p.read_text(encoding="utf-8", errors="replace")
            except Exception:  # noqa: BLE001
                continue
            if symbol and symbol in text:
                files.append(rel(p))
        calls.append(
            {
                "tool": "read",
                "args": files[:12],
                "ok": True,
                "summary": {},
                "note": "bounded reads of token-hit files",
            }
        )
        rationale = "scripted isolated A offline fill (token + walk; not live LLM)"
        kind = SCRIPTED_KIND
        model_note = SCRIPTED_MODEL_NOTE
    else:
        calls.append(
            {
                "tool": "walk",
                "args": ["walk_sources"],
                "ok": True,
                "summary": {"source_count": len(sources)},
                "note": "arm B walk only",
            }
        )
        for p in sources:
            try:
                text = p.read_text(encoding="utf-8", errors="replace")
            except Exception:  # noqa: BLE001
                continue
            if symbol and symbol in text:
                files.append(rel(p))
                calls.append(
                    {
                        "tool": "grep",
                        "args": ["grep", symbol, rel(p)],
                        "ok": True,
                        "summary": {},
                        "note": "token hit",
                    }
                )
        rationale = "scripted isolated B offline fill (walk/grep; not live LLM)"
        kind = SCRIPTED_KIND
        model_note = SCRIPTED_MODEL_NOTE

    files = sorted({_as_rel(f) for f in files if f})
    fs_obj = {
        "schema": FILE_SET_SCHEMA,
        "task_id": task_id,
        "runner_id": runner_id,
        "arm": arm,
        "seed": seed,
        "file_set": files,
        "tool_calls": calls,
        "chose_correct_workspace_root": None,
        "approx_tokens": None,
    }
    meta_obj = {
        "schema": META_SCHEMA,
        "runner_id": runner_id,
        "kind": kind,
        "model_note": model_note,
        "independent_session": True,
        "harness_version": HARNESS_VERSION,
        "saw_labels_before_commit": False,
        "arm_isolated": True,
        "read_budget": len(files),
        "approx_tokens": None,
        "lab_ready_claim": False,
        "rationale": rationale,
    }
    (bdir / "file_set.json").write_text(
        json.dumps(fs_obj, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    (bdir / "meta.json").write_text(
        json.dumps(meta_obj, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    return errors


def cmd_run_runner(args: argparse.Namespace) -> int:
    root = repo_root()
    traj_dir = Path(args.traj_dir) if args.traj_dir else (root / "evals" / "agent-ab-d")
    if not traj_dir.is_dir():
        print(f"error: missing traj dir {traj_dir}; run prepare first", file=sys.stderr)
        return 2
    runners = list(args.runner) if args.runner else [SCRIPTED_RUNNER_ID]
    briefs_root = traj_dir / "_briefs"
    if not briefs_root.is_dir():
        print("error: missing _briefs/; run prepare first", file=sys.stderr)
        return 2

    filled = 0
    validated = 0
    problems = 0

    for runner_id in runners:
        rroot = briefs_root / runner_id
        if not rroot.is_dir():
            print(f"warn: no briefs for runner {runner_id}", file=sys.stderr)
            continue
        for arm_dir in sorted(rroot.iterdir()):
            if not arm_dir.is_dir():
                continue
            arm = arm_dir.name
            for seed_dir in sorted(arm_dir.iterdir(), key=lambda p: int(p.name) if p.name.isdigit() else 0):
                if not seed_dir.is_dir():
                    continue
                seed_s = seed_dir.name
                try:
                    seed = int(seed_s)
                except ValueError:
                    continue
                for task_dir in sorted(seed_dir.iterdir()):
                    if not task_dir.is_dir():
                        continue
                    task_id = task_dir.name
                    if args.fill_scripted and runner_id == SCRIPTED_RUNNER_ID:
                        task_meta = load_task_meta(root, task_id)
                        fill_scripted_cell(
                            root,
                            task_dir,
                            runner_id=runner_id,
                            arm=arm,
                            seed=seed,
                            task_id=task_id,
                            task_meta=task_meta,
                        )
                        filled += 1
                    fs_obj, meta_obj, errs = read_brief_outputs(task_dir)
                    if errs and not (args.fill_scripted and runner_id == SCRIPTED_RUNNER_ID):
                        if args.require_outputs:
                            problems += len(errs)
                            for e in errs:
                                print(f"FAIL {e}", file=sys.stderr)
                        continue
                    if fs_obj is None or meta_obj is None:
                        continue
                    v_errs = validate_runner_outputs(
                        fs_obj,
                        meta_obj,
                        runner_id=runner_id,
                        arm=arm,
                        seed=seed,
                        task_id=task_id,
                    )
                    # Ban: decision-path file_set must not embed structure-fact label lists.
                    blob = json.dumps(fs_obj, ensure_ascii=False).lower()
                    for banned in ("noise_files", "files_that_matter", "forbidden_files"):
                        if banned in blob:
                            v_errs.append(f"file_set.json must not contain {banned!r}")
                    if v_errs:
                        problems += len(v_errs)
                        for e in v_errs:
                            print(f"FAIL {task_dir}: {e}", file=sys.stderr)
                    else:
                        validated += 1
                        print(f"OK contract {runner_id}/{arm}/{seed}/{task_id}")

    print(
        json.dumps(
            {
                "schema": "agentgraph.eval_agent_ab_d.run_runner.v1",
                "harness_version": HARNESS_VERSION,
                "runners": runners,
                "filled_scripted": filled,
                "validated": validated,
                "problems": problems,
                "live_agents_executed": 0,
                "note": (
                    "S1 harness does not execute live external agents; "
                    "parent/S2 orchestrates live runners. Scripted fill is offline only."
                ),
            },
            indent=2,
            ensure_ascii=False,
        )
    )
    return 1 if problems else 0


# ---------------------------------------------------------------------------
# stamp
# ---------------------------------------------------------------------------


def stamp_from_brief(
    root: Path,
    traj_dir: Path,
    bdir: Path,
    *,
    runner_id: str,
    arm: str,
    seed: int,
    task_id: str,
    rand: Dict[str, Any],
) -> Dict[str, Any]:
    fs_obj, meta_obj, errs = read_brief_outputs(bdir)
    if fs_obj is None or meta_obj is None:
        return {"path": str(bdir), "skipped": True, "reason": "; ".join(errs) or "missing outputs"}
    v_errs = validate_runner_outputs(
        fs_obj,
        meta_obj,
        runner_id=runner_id,
        arm=arm,
        seed=seed,
        task_id=task_id,
    )
    if v_errs:
        return {"path": str(bdir), "skipped": True, "reason": "; ".join(v_errs)}

    tdir = task_fixture_dir(root, task_id)
    if not (tdir / "task.json").is_file():
        return {"path": str(bdir), "skipped": True, "reason": f"missing fixture for {task_id}"}
    task_meta = json.loads((tdir / "task.json").read_text(encoding="utf-8"))
    labels = _task_labels(task_meta)
    files = list(fs_obj.get("file_set") or [])
    calls = list(fs_obj.get("tool_calls") or [])
    kind = str(meta_obj.get("kind") or SCRIPTED_KIND)
    model_note = str(meta_obj.get("model_note") or "")
    independent = bool(meta_obj.get("independent_session"))
    cwr = fs_obj.get("chose_correct_workspace_root", meta_obj.get("chose_correct_workspace_root"))
    approx = fs_obj.get("approx_tokens", meta_obj.get("approx_tokens"))
    if approx is not None and not isinstance(approx, (int, float)):
        approx = None
    read_budget = meta_obj.get("read_budget")
    if read_budget is not None and not isinstance(read_budget, int):
        read_budget = None

    fixture_rel = (
        f"fixtures/eval-agent-tasks-hard/{task_id}"
        if task_id in HARD_TASKS
        else f"fixtures/eval-agent-tasks/{task_id}"
    )
    brief_rel = f"evals/agent-ab-d/_briefs/{runner_id}/{arm}/{seed}/{task_id}"
    rationale = str(
        meta_obj.get("rationale")
        or (fs_obj.get("extras") or {}).get("rationale")
        or "isolated runner decision path"
    )
    payload = build_d_trajectory(
        task_id=task_id,
        arm=arm,
        seed=seed,
        runner_id=runner_id,
        kind=kind,
        model_note=model_note,
        independent_session=independent,
        files=files,
        calls=calls,
        rationale=rationale,
        task_meta=task_meta,
        fixture_rel=fixture_rel,
        rand=rand,
        chose_correct_workspace_root=cwr,
        approx_tokens=approx if isinstance(approx, (int, float)) else None,
        read_budget=read_budget,
        brief_rel=brief_rel,
        extra_honesty={
            "contamination": (
                "Issue-only brief; workdir has no task.json; decision path did not "
                "read structure-fact labels or other arms' outputs."
            )
        },
    )
    # Offline stamp from fixture task.json (harness does not inject goldens into briefs).
    score = _score_file_set(
        files,
        labels["expected_files"],
        labels["noise_files"],
        labels["forbidden_files"],
    )
    score["stamped"] = True
    score["stamp_source"] = f"{fixture_rel}/task.json"
    score["stamp_offline"] = True
    task_block = dict(payload.get("task") or {})
    task_block.update(labels)
    payload["task"] = task_block
    payload["score"] = score
    payload["file_budget"] = score["file_set_size"]
    if isinstance(payload.get("metrics_ext"), dict):
        payload["metrics_ext"]["file_budget"] = score["file_set_size"]

    out_path = traj_dir / task_id / f"run-{runner_id}-{arm.lower()}-{seed}.json"
    _write_trajectory(out_path, payload)
    return {
        "path": str(out_path),
        "task_id": task_id,
        "runner_id": runner_id,
        "arm": arm,
        "seed": seed,
        "kind": kind,
        "model_note": model_note,
        "independent_session": independent,
        "recall": score.get("expected_file_recall"),
        "noise": score.get("extra_noise_count"),
        "size": score.get("file_set_size"),
        "stamped": True,
        "brief": brief_rel,
    }


def cmd_stamp(args: argparse.Namespace) -> int:
    root = repo_root()
    traj_dir = Path(args.traj_dir) if args.traj_dir else (root / "evals" / "agent-ab-d")
    if not traj_dir.is_dir():
        print(f"error: missing traj dir {traj_dir}", file=sys.stderr)
        return 2
    rand_path = traj_dir / "task_randomization.json"
    rand_by_seed: Dict[int, Dict[str, Any]] = {}
    if rand_path.is_file():
        blob = json.loads(rand_path.read_text(encoding="utf-8"))
        for row in blob.get("seeds") or []:
            if isinstance(row, dict) and row.get("seed") is not None:
                rand_by_seed[int(row["seed"])] = row
    default_rand = {
        "seed": None,
        "task_order": [],
        "arm_order": ["A", "B"],
        "runner_order": [],
        "method": "missing randomization log",
    }

    results: List[Dict[str, Any]] = []
    briefs_root = traj_dir / "_briefs"
    if briefs_root.is_dir():
        for issue in sorted(briefs_root.rglob("ISSUE.md")):
            bdir = issue.parent
            # path: _briefs/<runner>/<arm>/<seed>/<task>/ISSUE.md
            try:
                task_id = bdir.name
                seed = int(bdir.parent.name)
                arm = bdir.parent.parent.name
                runner_id = bdir.parent.parent.parent.name
            except Exception:  # noqa: BLE001
                results.append({"path": str(bdir), "skipped": True, "reason": "bad brief path"})
                continue
            if args.runner and runner_id not in set(args.runner):
                continue
            if args.task and task_id not in set(args.task):
                continue
            if args.arm and arm not in set(args.arm):
                continue
            rand = rand_by_seed.get(seed, default_rand)
            res = stamp_from_brief(
                root,
                traj_dir,
                bdir,
                runner_id=runner_id,
                arm=arm,
                seed=seed,
                task_id=task_id,
                rand=rand,
            )
            results.append(res)
            status = "stamped" if res.get("stamped") else f"skip ({res.get('reason')})"
            print(f"{status} {runner_id}/{arm}/{seed}/{task_id}")
    else:
        print("warn: no _briefs directory; nothing to stamp", file=sys.stderr)

    # Also re-stamp existing unstamped trajectory files if present.
    if args.also_trajectories:
        for p in discover_traj_files(traj_dir):
            if "_briefs" in p.parts:
                continue
            try:
                payload = json.loads(p.read_text(encoding="utf-8"))
            except Exception:  # noqa: BLE001
                continue
            if (payload.get("score") or {}).get("stamped"):
                continue
            tid = payload.get("task_id") or ""
            tdir = task_fixture_dir(root, tid)
            if not (tdir / "task.json").is_file():
                continue
            meta = json.loads((tdir / "task.json").read_text(encoding="utf-8"))
            labels = _task_labels(meta)
            file_set = list(payload.get("file_set") or [])
            score = _score_file_set(
                file_set,
                labels["expected_files"],
                labels["noise_files"],
                labels["forbidden_files"],
            )
            score["stamped"] = True
            score["stamp_source"] = f"{tdir}/task.json"
            score["stamp_offline"] = True
            task = dict(payload.get("task") or {})
            task.update(labels)
            payload["task"] = task
            payload["score"] = score
            payload["file_budget"] = score["file_set_size"]
            if isinstance(payload.get("metrics_ext"), dict):
                payload["metrics_ext"]["file_budget"] = score["file_set_size"]
            _write_trajectory(p, payload)
            results.append({"path": str(p), "stamped": True, "restamped": True})

    out = Path(args.out) if args.out else (root / "target" / "agent_ab_d_stamp.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "schema": "agentgraph.eval_agent_ab_d.stamp.v1",
                "offline": True,
                "network_required": False,
                "harness_version": HARNESS_VERSION,
                "results": results,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    n_ok = sum(1 for r in results if r.get("stamped"))
    print(f"stamped {n_ok} trajectory(ies); wrote {out}")
    return 0


# ---------------------------------------------------------------------------
# score + forgery refusal
# ---------------------------------------------------------------------------


def _mean(xs: List[float]) -> float:
    return round(sum(xs) / len(xs), 4) if xs else 0.0


def detect_forgeries(payload: Dict[str, Any], root: Path) -> List[str]:
    """Return human-readable forgery / honesty violations for one trajectory."""
    issues: List[str] = []
    runner_id = payload.get("runner_id") or ""
    model_note = payload.get("model_note") or ""
    kind = payload.get("kind") or ""
    honesty = payload.get("honesty") or {}

    if payload.get("schema") != TRAJECTORY_SCHEMA:
        issues.append(f"bad schema {payload.get('schema')!r}")

    if payload.get("saw_labels_before_commit") not in {False, None}:
        issues.append("saw_labels_before_commit must be false")
    if honesty.get("saw_labels_before_commit") not in {False, None}:
        issues.append("honesty.saw_labels_before_commit must be false")
    if honesty.get("lab_ready_claim") is True:
        issues.append("honesty.lab_ready_claim must not be true on individual trajectories")
    if honesty.get("private_corpus") is True:
        issues.append("private_corpus must be false")

    # Forged independence for known author-session runners.
    if runner_id in {"host_session_llm"} or is_author_model(model_note):
        if payload.get("independent_session") is True and kind == "live_llm_agent":
            issues.append(
                f"forged independent_session=true for author-session runner/model "
                f"({runner_id}/{model_note})"
            )
        if honesty.get("lab_eligible_runner") is True:
            issues.append("forged lab_eligible_runner=true for author-session model")

    # Live kind requires non-empty model_note; scripted must not claim live.
    if kind == "live_llm_agent":
        if not model_note:
            issues.append("live_llm_agent requires model_note")
        if "scripted" in model_note.lower():
            issues.append("forged live_llm_agent kind with scripted model_note")
    if kind == SCRIPTED_KIND or runner_id == SCRIPTED_RUNNER_ID:
        if honesty.get("live_llm_agent") is True:
            issues.append("forged live_llm_agent=true on scripted isolated runner")
        if is_author_model(model_note):
            issues.append("scripted runner must not claim author/host-session model_note")

    # approx_tokens: null or number only — never invented strings.
    approx = payload.get("approx_tokens")
    if approx is not None and not isinstance(approx, (int, float)):
        issues.append(f"approx_tokens must be null or number, got {type(approx).__name__}")

    # Stamped score must match fixture task.json when available (anti-forge).
    tid = payload.get("task_id") or ""
    score = payload.get("score") or {}
    if score.get("stamped") and tid:
        tdir = task_fixture_dir(root, tid)
        if (tdir / "task.json").is_file():
            meta = json.loads((tdir / "task.json").read_text(encoding="utf-8"))
            labels = _task_labels(meta)
            recomputed = _score_file_set(
                payload.get("file_set") or [],
                labels["expected_files"],
                labels["noise_files"],
                labels["forbidden_files"],
            )
            for key in (
                "expected_file_recall",
                "extra_noise_count",
                "forbidden_hit_count",
                "file_set_size",
            ):
                if score.get(key) != recomputed.get(key):
                    issues.append(
                        f"forged stamped score {key}: trajectory={score.get(key)!r} "
                        f"fixture_recompute={recomputed.get(key)!r}"
                    )
            task_block = payload.get("task") or {}
            for lk in ("expected_files", "noise_files", "forbidden_files"):
                got = list(task_block.get(lk) or [])
                want = list(labels.get(lk) or [])
                if got != want:
                    issues.append(
                        f"forged task.{lk} does not match fixture task.json "
                        f"({len(got)} vs {len(want)})"
                    )

    # N / seed honesty: seed must be int; claimed N in extras must not exceed reality
    # (checked at matrix level in score).
    seed = payload.get("seed")
    if seed is not None and not isinstance(seed, int):
        issues.append(f"seed must be int, got {seed!r}")

    # Private path scan
    for p in payload.get("file_set") or []:
        s = str(p)
        if "stock-trading" in s or "private-stock" in s or "C:\\Users\\" in s or "D:\\Users\\" in s:
            issues.append(f"private path in file_set: {s}")

    return issues


def evaluate_lab_ready(
    traj_dir: Path,
    results_meta: Sequence[Dict[str, Any]],
    selection: Optional[Dict[str, Any]],
) -> Dict[str, Any]:
    """Compute lab_ready from recorded trajectories only (never invent cells)."""
    min_live = 2
    min_n = 5
    if selection and isinstance(selection.get("lab_ready_definition"), dict):
        d = selection["lab_ready_definition"]
        min_live = int(d.get("min_live_runners", 2))
        min_n = int(d.get("min_seeds_per_cell", 5))

    easy = list(EASY_TASKS)
    hard = list(HARD_TASKS)
    if selection:
        if selection.get("easy_tasks"):
            easy = list(selection["easy_tasks"])
        if selection.get("hard_tasks"):
            hard = list(selection["hard_tasks"])

    # rows: task_id, runner_id, arm, seed, kind, model_note, independent_session
    live_rows = [
        r
        for r in results_meta
        if r.get("kind") == "live_llm_agent" and r.get("recorded_matches_replay") is not False
    ]
    live_runners = sorted({r.get("runner_id") for r in live_rows if r.get("runner_id")})
    lab_eligible_runners = sorted(
        {
            r.get("runner_id")
            for r in live_rows
            if r.get("independent_session") is True
            and r.get("model_note")
            and not is_author_model(str(r.get("model_note") or ""))
        }
    )
    author_live = sorted(
        {
            r.get("runner_id")
            for r in live_rows
            if is_author_model(str(r.get("model_note") or "")) or r.get("independent_session") is False
        }
    )

    cells: Dict[Tuple[str, str, str], set] = {}
    for r in live_rows:
        key = (str(r.get("task_id")), str(r.get("runner_id")), str(r.get("arm")))
        cells.setdefault(key, set()).add(r.get("seed"))

    gaps: List[str] = []
    if len(lab_eligible_runners) < min_live:
        gaps.append(
            f"live runners with independent_session=true and non-author model_note: "
            f"{len(lab_eligible_runners)}/{min_live} (found {lab_eligible_runners or 'none'})"
        )
    if author_live:
        gaps.append(
            f"author-session / non-independent live runners present (force lab_ready=false): {author_live}"
        )

    tasks_needed = easy + hard
    easy_present = {t for t in easy if any(k[0] == t for k in cells)}
    hard_present = {t for t in hard if any(k[0] == t for k in cells)}
    if len(easy_present) < min(4, len(easy)):
        gaps.append(f"easy tasks with live cells: {len(easy_present)}/{min(4, len(easy))}")
    if len(hard_present) < min(4, len(hard)):
        gaps.append(f"hard tasks with live cells: {len(hard_present)}/{min(4, len(hard))}")

    incomplete: List[Dict[str, Any]] = []
    if lab_eligible_runners:
        for task_id in tasks_needed:
            for rid in lab_eligible_runners:
                for arm in ("A", "B"):
                    key = (task_id, rid, arm)
                    n = len(cells.get(key, set()))
                    if n < min_n:
                        incomplete.append(
                            {
                                "task_id": task_id,
                                "runner_id": rid,
                                "arm": arm,
                                "n": n,
                                "n_required": min_n,
                            }
                        )
    else:
        # Matrix empty — still report expected cell count for honesty.
        for task_id in tasks_needed:
            for rid in (PLANNED_LIVE_RUNNER_SLOTS if not live_runners else live_runners):
                for arm in ("A", "B"):
                    incomplete.append(
                        {
                            "task_id": task_id,
                            "runner_id": rid,
                            "arm": arm,
                            "n": 0,
                            "n_required": min_n,
                        }
                    )

    if incomplete:
        gaps.append(
            f"incomplete (runner×arm×task) cells vs N≥{min_n}: {len(incomplete)} "
            f"(sample: {incomplete[:3]})"
        )

    lab_ready = len(gaps) == 0 and len(lab_eligible_runners) >= min_live and not incomplete
    # Never true on empty/harness-only trees.
    if not live_rows:
        lab_ready = False
        if "no recorded live trajectories" not in " ".join(gaps):
            gaps.insert(0, "no recorded live trajectories")

    return {
        "schema": LAB_READY_SCHEMA,
        "offline": True,
        "harness_version": HARNESS_VERSION,
        "lab_ready": lab_ready,
        "criteria": {
            "min_live_runners": min_live,
            "min_seeds_per_cell": min_n,
            "require_independent_session": True,
            "require_non_author_models": True,
            "require_both_arms": True,
            "require_easy_and_hard": True,
        },
        "live_runners_seen": live_runners,
        "lab_eligible_live_runners": lab_eligible_runners,
        "author_or_dependent_live_runners": author_live,
        "easy_tasks_with_cells": sorted(easy_present),
        "hard_tasks_with_cells": sorted(hard_present),
        "incomplete_cells": incomplete[:50],
        "incomplete_cell_count": len(incomplete),
        "gaps": gaps,
        "note": (
            "lab_ready=true requires a complete isolated live matrix. "
            "Harness-only / scripted fills never satisfy lab_ready. "
            "No oversell: do not cite this tree as lab product proof while false."
        ),
    }


def cmd_score(args: argparse.Namespace) -> int:
    root = repo_root()
    traj_dir = Path(args.traj_dir) if args.traj_dir else (root / "evals" / "agent-ab-d")
    paths: List[Path] = []
    if args.trajectory:
        paths.append(Path(args.trajectory))
    if args.traj_dir or not args.trajectory:
        base = Path(args.traj_dir) if args.traj_dir else traj_dir
        paths.extend([p for p in discover_traj_files(base) if "_briefs" not in p.parts])
    if not paths:
        print("error: no trajectories to score (prepare + run-runner + stamp first)", file=sys.stderr)
        # Still emit lab_ready=false artifact for gates.
        selection = _load_selection(traj_dir)
        lab = evaluate_lab_ready(traj_dir, [], selection)
        out = Path(args.out) if args.out else (root / "target" / "agent_ab_d_replay.json")
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            json.dumps(
                {
                    "schema": REPLAY_SCHEMA,
                    "offline": True,
                    "results": [],
                    "summary_by_runner_arm": {},
                    "lab_ready_eval": lab,
                    "forgery_violations": [],
                },
                indent=2,
                ensure_ascii=False,
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"lab_ready={lab['lab_ready']} gaps={lab['gaps']}")
        print(f"wrote {out}")
        return 0 if args.allow_empty else 1

    selection = _load_selection(traj_dir)
    results: List[Dict[str, Any]] = []
    results_meta: List[Dict[str, Any]] = []
    by_runner_arm: Dict[str, List[Dict[str, Any]]] = {}
    forgery_violations: List[Dict[str, Any]] = []
    bad = 0

    for p in paths:
        try:
            payload = json.loads(p.read_text(encoding="utf-8"))
        except Exception as e:  # noqa: BLE001
            bad += 1
            results.append({"path": str(p), "error": str(e)})
            print(f"FAIL {p}: {e}", file=sys.stderr)
            continue

        forgeries = detect_forgeries(payload, root)
        if forgeries:
            bad += 1
            forgery_violations.append({"path": str(p), "issues": forgeries})
            for msg in forgeries:
                print(f"FORGERY {p}: {msg}", file=sys.stderr)

        try:
            sc = score_trajectory(payload)
        except Exception as e:  # noqa: BLE001
            bad += 1
            results.append({"path": str(p), "error": str(e), "forgeries": forgeries})
            print(f"FAIL {p}: {e}", file=sys.stderr)
            continue

        if not (payload.get("score") or {}).get("stamped"):
            sc["warn"] = "score not stamped offline from fixture task.json"
        if not sc.get("recorded_matches_replay"):
            bad += 1
            sc["forgeries"] = forgeries or ["recorded score does not match replay"]
            forgery_violations.append(
                {
                    "path": str(p),
                    "issues": sc["forgeries"],
                    "mismatches": sc.get("mismatches"),
                }
            )

        sc["path"] = str(p)
        sc["runner_id"] = payload.get("runner_id")
        sc["model_note"] = payload.get("model_note")
        sc["kind"] = payload.get("kind")
        sc["arm"] = payload.get("arm") or payload.get("policy")
        sc["independent_session"] = payload.get("independent_session")
        sc["saw_labels_before_commit"] = payload.get("saw_labels_before_commit")
        sc["mcp_or_cli_calls"] = payload.get("mcp_or_cli_calls")
        sc["chose_correct_workspace_root"] = payload.get("chose_correct_workspace_root")
        sc["file_budget"] = payload.get("file_budget")
        sc["read_budget"] = payload.get("read_budget")
        sc["approx_tokens"] = payload.get("approx_tokens")
        sc["forgeries"] = forgeries
        results.append(sc)

        results_meta.append(
            {
                "task_id": sc.get("task_id"),
                "runner_id": sc.get("runner_id"),
                "arm": sc.get("arm"),
                "seed": sc.get("seed"),
                "kind": sc.get("kind"),
                "model_note": sc.get("model_note"),
                "independent_session": sc.get("independent_session"),
                "recorded_matches_replay": sc.get("recorded_matches_replay"),
            }
        )

        rec = sc["recomputed"]
        key = f"{sc.get('runner_id')}:{sc.get('arm')}"
        by_runner_arm.setdefault(key, []).append(
            {
                **rec,
                **{
                    k: sc.get(k)
                    for k in (
                        "chose_correct_workspace_root",
                        "file_budget",
                        "read_budget",
                        "mcp_or_cli_calls",
                        "approx_tokens",
                        "kind",
                        "model_note",
                        "independent_session",
                    )
                },
            }
        )
        status = "OK" if sc.get("recorded_matches_replay") and not forgeries else "FAIL"
        print(
            f"{status} {sc.get('task_id')}/{sc.get('runner_id')}/{sc.get('arm')}/{sc.get('seed')} "
            f"recall={rec['expected_file_recall']} noise={rec['extra_noise_count']} "
            f"size={rec['file_set_size']} cwr={sc.get('chose_correct_workspace_root')} "
            f"indep={sc.get('independent_session')} kind={sc.get('kind')}"
        )

    summary = {}
    for key, rows in by_runner_arm.items():
        runner, arm = key.split(":", 1)
        cwr_vals = [
            r["chose_correct_workspace_root"]
            for r in rows
            if r.get("chose_correct_workspace_root") is not None
        ]
        mcp_counts = []
        for r in rows:
            m = r.get("mcp_or_cli_calls") or {}
            if isinstance(m, dict) and m.get("count") is not None:
                mcp_counts.append(float(m["count"]))
        summary[key] = {
            "runner_id": runner,
            "arm": arm,
            "kind": rows[0].get("kind") if rows else None,
            "model_note": rows[0].get("model_note") if rows else None,
            "independent_session": rows[0].get("independent_session") if rows else None,
            "run_count": len(rows),
            "mean_expected_file_recall": _mean(
                [float(r.get("expected_file_recall") or 0) for r in rows]
            ),
            "mean_extra_noise_files": _mean(
                [float(r.get("extra_noise_count") or 0) for r in rows]
            ),
            "mean_file_set_size": _mean([float(r.get("file_set_size") or 0) for r in rows]),
            "mean_file_budget": _mean(
                [float(r.get("file_budget") or r.get("file_set_size") or 0) for r in rows]
            ),
            "mean_mcp_or_cli_calls": _mean(mcp_counts),
            "mean_read_budget": _mean(
                [float(r.get("read_budget") or 0) for r in rows if r.get("read_budget") is not None]
            ),
            "chose_correct_workspace_root_true": sum(1 for v in cwr_vals if v is True),
            "chose_correct_workspace_root_false": sum(1 for v in cwr_vals if v is False),
            "chose_correct_workspace_root_na": sum(
                1 for r in rows if r.get("chose_correct_workspace_root") is None
            ),
            "approx_tokens_null": all(r.get("approx_tokens") is None for r in rows),
        }

    lab = evaluate_lab_ready(traj_dir, results_meta, selection)
    # Refuse forged lab_ready=true embedded in any trajectory honesty block.
    for r in results:
        forgeries = r.get("forgeries") or []
        if any("lab_ready" in str(x) for x in forgeries):
            lab["lab_ready"] = False
    # Hard gate: if forgeries exist, lab_ready cannot be true.
    if forgery_violations:
        lab["lab_ready"] = False
        if "forgery violations present" not in lab["gaps"]:
            lab["gaps"].append("forgery violations present")

    out = Path(args.out) if args.out else (root / "target" / "agent_ab_d_replay.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "schema": REPLAY_SCHEMA,
                "offline": True,
                "network_required": False,
                "harness_version": HARNESS_VERSION,
                "traj_dir": str(traj_dir),
                "summary_by_runner_arm": summary,
                "lab_ready_eval": lab,
                "forgery_violations": forgery_violations,
                "results": results,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {out}")
    print(f"scored {len(results)} P0-5d trajectory(ies); problems={bad}")
    print("summary_by_runner_arm:", json.dumps(summary, ensure_ascii=False))
    print(f"lab_ready={lab['lab_ready']}")
    if lab["gaps"]:
        print("lab_ready_gaps:", json.dumps(lab["gaps"], ensure_ascii=False))

    if forgery_violations and not args.allow_forgery:
        print("gate: refuse forged fields", file=sys.stderr)
        return 1
    if bad and not args.allow_partial:
        print("gate: score problems present", file=sys.stderr)
        return 1
    return 0


def _load_selection(traj_dir: Path) -> Optional[Dict[str, Any]]:
    path = traj_dir / "task_selection.json"
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:  # noqa: BLE001
        return None


def cmd_lab_ready(args: argparse.Namespace) -> int:
    root = repo_root()
    traj_dir = Path(args.traj_dir) if args.traj_dir else (root / "evals" / "agent-ab-d")
    selection = _load_selection(traj_dir)
    paths = [p for p in discover_traj_files(traj_dir) if "_briefs" not in p.parts]
    results_meta: List[Dict[str, Any]] = []
    forgeries: List[Dict[str, Any]] = []
    for p in paths:
        try:
            payload = json.loads(p.read_text(encoding="utf-8"))
        except Exception:  # noqa: BLE001
            continue
        issues = detect_forgeries(payload, root)
        if issues:
            forgeries.append({"path": str(p), "issues": issues})
        results_meta.append(
            {
                "task_id": payload.get("task_id"),
                "runner_id": payload.get("runner_id"),
                "arm": payload.get("arm") or payload.get("policy"),
                "seed": payload.get("seed"),
                "kind": payload.get("kind"),
                "model_note": payload.get("model_note"),
                "independent_session": payload.get("independent_session"),
                "recorded_matches_replay": True,
            }
        )
    lab = evaluate_lab_ready(traj_dir, results_meta, selection)
    if forgeries:
        lab["lab_ready"] = False
        lab["gaps"] = list(lab.get("gaps") or []) + ["forgery violations present"]
        lab["forgery_violations"] = forgeries

    out = Path(args.out) if args.out else (root / "target" / "agent_ab_d_lab_ready.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(lab, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(json.dumps(lab, indent=2, ensure_ascii=False))
    print(f"wrote {out}")
    if lab.get("lab_ready"):
        print("LAB_READY=true — complete isolated live matrix meets P0-5d acceptance")
        return 0
    print("LAB_READY=false — harness/incomplete matrix; do not oversell as lab proof")
    return 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="P0-5d isolated lab harness")
    sub = p.add_subparsers(dest="command")

    pr = sub.add_parser("prepare", help="materialize issue-only brief packs + isolated workdirs")
    pr.add_argument("--traj-dir", type=Path, default=None)
    pr.add_argument("--runner", action="append", default=None, help="runner id slot (repeatable)")
    pr.add_argument(
        "--seeds",
        type=lambda s: [int(x) for x in s.split(",") if x.strip() != ""],
        default=None,
    )
    pr.add_argument("--task", action="append", default=None)
    pr.add_argument("--work-root", type=Path, default=None)
    pr.add_argument("--skip-workdirs", action="store_true", help="briefs only (no workdir copies)")
    pr.add_argument("--also-work-root", action="store_true", help="also copy workdirs under target/")
    pr.set_defaults(func=cmd_prepare)

    rr = sub.add_parser("run-runner", help="validate runner contract; optional scripted offline fill")
    rr.add_argument("--traj-dir", type=Path, default=None)
    rr.add_argument("--runner", action="append", default=None)
    rr.add_argument(
        "--fill-scripted",
        action="store_true",
        help="offline fill for scripted_isolated_runner only (not live LLM)",
    )
    rr.add_argument(
        "--require-outputs",
        action="store_true",
        help="treat missing file_set/meta as gate failures",
    )
    rr.add_argument("--validate-only", action="store_true", help="alias: no fill")
    rr.set_defaults(func=cmd_run_runner)

    st = sub.add_parser("stamp", help="offline score from fixture task.json after commitment")
    st.add_argument("--traj-dir", type=Path, default=None)
    st.add_argument("--runner", action="append", default=None)
    st.add_argument("--task", action="append", default=None)
    st.add_argument("--arm", action="append", default=None)
    st.add_argument("--out", type=Path, default=None)
    st.add_argument("--also-trajectories", action="store_true")
    st.set_defaults(func=cmd_stamp)

    sc = sub.add_parser("score", help="aggregate table; refuse forged fields")
    sc.add_argument("--traj-dir", type=Path, default=None)
    sc.add_argument("--trajectory", type=Path, default=None)
    sc.add_argument("--out", type=Path, default=None)
    sc.add_argument("--allow-empty", action="store_true")
    sc.add_argument("--allow-partial", action="store_true")
    sc.add_argument("--allow-forgery", action="store_true", help="dev only — do not use for gates")
    sc.set_defaults(func=cmd_score)

    lr = sub.add_parser("lab-ready", help="print whether matrix meets P0-5d lab acceptance")
    lr.add_argument("--traj-dir", type=Path, default=None)
    lr.add_argument("--out", type=Path, default=None)
    lr.set_defaults(func=cmd_lab_ready)

    return p


def main(argv: Optional[List[str]] = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    known = {"prepare", "run-runner", "stamp", "score", "lab-ready", "-h", "--help"}
    if not argv or argv[0] not in known:
        # default to prepare for bare invocation after flags
        if argv and argv[0].startswith("-"):
            argv = ["prepare", *argv]
        else:
            print("usage: eval_agent_ab_d.py {prepare|run-runner|stamp|score|lab-ready}", file=sys.stderr)
            return 2
    parser = build_parser()
    args = parser.parse_args(argv)
    if not getattr(args, "func", None):
        parser.print_help()
        return 2
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
