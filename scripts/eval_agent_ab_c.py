#!/usr/bin/env python3
"""P0-5c multi-runner live A/B on hard public tasks (extended metrics).

Runner kinds (at least two; incomplete cells labeled honestly):

- ``host_session_llm`` — live host-session agent decisions on hard fixtures.
  ``model_note=mimo-desktop-host-session``. Not a public benchmark model.
  Same-session fixture authorship ⇒ ``independent_session=false`` (disclosed).
  Decision path: file_set from tool/grep evidence; ``saw_labels_before_commit=false``.
- ``scripted_external_runner`` — P0-5 policy A/B as an external deterministic
  runner invoked on hard fixtures. Script does not read ``expected`` when
  assembling file sets ⇒ ``independent_session=true`` for the decision path.

Arms:
- **A** agentgraph MCP/CLI recipes (+ reads) — never grep-only.
- **B** read/grep only — never agentgraph.

Extended metrics (per run JSON; offline score echoes when present):
- structure-fact recall / extra-noise (existing)
- ``mcp_or_cli_calls``: {count, recipe_tools[], grep_count}
- ``chose_correct_workspace_root``: bool | null (n/a when single-root task)
- ``file_budget`` = |file_set|; ``read_budget`` = files opened; ``approx_tokens`` null unless runner reports
- ``runner_id``, ``model_note``, ``independent_session``, ``saw_labels_before_commit=false``
- ``task_randomization``: recorded task/arm order per seed

Honesty (required)
-------------------
- No fabricated multi-model lab numbers.
- No oversell that live A beats B.
- No private corpus.
- Incomplete runner×task×seed cells are labeled in docs and score summary.

Exit codes: 0 ok · 1 gate/IO problems · 2 usage.
"""
from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence

_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

from eval_agent_ab import (  # noqa: E402
    PolicyRun,
    _as_rel,
    discover_traj_files,
    run_policy_a,
    run_policy_b,
    score_file_set as _score_file_set,
    score_trajectory,
    task_labels as _task_labels,
    write_trajectory as _write_trajectory,
)
from eval_agent_tasks import (  # noqa: E402
    copy_fixture,
    find_bin,
    repo_root_from_script,
)

C_SCHEMA_ALIAS = "agentgraph.eval_agent_ab.c.v1"
TRAJECTORY_SCHEMA = "agentgraph.eval_agent_ab.trajectory.v1"
HOST_MODEL_NOTE = "mimo-desktop-host-session"
SCRIPTED_MODEL_NOTE = "scripted_deterministic_policy"

RUNNER_META = {
    "host_session_llm": {
        "kind": "live_llm_agent",
        "model_note": HOST_MODEL_NOTE,
        "independent_session": False,
        "honesty_note": (
            "P0-5c host_session_llm on hard fixtures — not a public benchmark "
            "model, not a multi-model lab. Same session authored hard fixtures "
            "⇒ independent_session=false (contamination disclosed). File sets "
            "derived from recorded tool/grep evidence, not by copying expected "
            "lists (saw_labels_before_commit=false on the decision path)."
        ),
    },
    "scripted_external_runner": {
        "kind": "scripted_external_runner",
        "model_note": SCRIPTED_MODEL_NOTE,
        "independent_session": True,
        "honesty_note": (
            "P0-5c scripted_external_runner — deterministic P0-5 policy A/B "
            "invoked as an external runner on hard fixtures. Not a live LLM. "
            "Decision path does not read task.json expected labels."
        ),
    },
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

# Host-session live decisions on hard tasks (P0-5c).
# file_set committed from tool/grep evidence; labels stamped offline after.
HOST_DECISIONS: Dict[str, Dict[str, Any]] = {
    "rust-cross-crate-blast": {
        "symbol": "normalize_id",
        "issue_digest": (
            "core::normalize_id contract change; multi-root cross-crate blast; "
            "exclude tools name-collision and web comment noise."
        ),
        "A": {
            "files": [
                "packages/core/src/id.rs",
                "packages/core/src/lib.rs",
                "packages/api/src/routes.rs",
                "packages/api/src/lib.rs",
            ],
            "chose_correct_workspace_root": True,
            "calls": [
                {
                    "tool": "agentgraph.index",
                    "args": ["index", "--workspace", "workspace.json", "--force"],
                    "ok": True,
                    "summary": {"exit_code": 0},
                    "note": "multi-root workspace index",
                },
                {
                    "tool": "agentgraph.find",
                    "args": [
                        "find",
                        "normalize_id",
                        "--workspace",
                        "workspace.json",
                        "--workspace-root",
                        "packages/core",
                    ],
                    "ok": True,
                    "summary": {"definition_files": ["packages/core/src/id.rs"]},
                    "note": "scoped find on definition root core",
                },
                {
                    "tool": "agentgraph.blast-radius",
                    "args": [
                        "blast-radius",
                        "normalize_id",
                        "--depth",
                        "3",
                        "--workspace",
                        "workspace.json",
                    ],
                    "ok": True,
                    "summary": {"window": "sound"},
                    "note": "union blast; true dependents core+api",
                },
                {
                    "tool": "agentgraph.who-calls",
                    "args": [
                        "who-calls",
                        "normalize_id",
                        "--limit",
                        "50",
                        "--workspace",
                        "workspace.json",
                    ],
                    "ok": True,
                    "summary": {
                        "callers_files": [
                            "packages/core/src/lib.rs",
                            "packages/api/src/routes.rs",
                            "packages/tools/src/lib.rs",
                        ]
                    },
                    "note": "tools local collision is not a core consumer — omit after evidence",
                },
                {
                    "tool": "read",
                    "args": [
                        "packages/api/src/routes.rs",
                        "packages/tools/src/lib.rs",
                        "packages/web/src/lib.rs",
                        "packages/core/src/lib.rs",
                    ],
                    "ok": True,
                    "summary": {},
                    "note": "confirm cross-crate call vs name collision / comment-only",
                },
                {
                    "tool": "agentgraph.subset",
                    "args": ["subset", "--workspace", "workspace.json"],
                    "ok": True,
                    "summary": {"in_subset": True},
                    "note": "honesty companion",
                },
            ],
            "rationale": (
                "Scoped find on core + workspace blast/who-calls: definition "
                "id.rs, core re-export lib.rs, api routes call site, api lib "
                "re-export. tools::normalize_id is a local collision after read "
                "— omit. web/* comment-only — omit. Correct roots: core+api."
            ),
        },
        "B": {
            "files": [
                "packages/core/src/id.rs",
                "packages/core/src/lib.rs",
                "packages/api/src/routes.rs",
                "packages/api/src/lib.rs",
                "packages/tools/src/lib.rs",
            ],
            "chose_correct_workspace_root": None,
            "calls": [
                {
                    "tool": "walk",
                    "args": ["packages"],
                    "ok": True,
                    "summary": {"source_files": 10},
                    "note": "enumerate multi-root sources",
                },
                {
                    "tool": "grep",
                    "args": ["normalize_id"],
                    "ok": True,
                    "summary": {
                        "hits": [
                            "packages/core/src/id.rs",
                            "packages/core/src/lib.rs",
                            "packages/api/src/routes.rs",
                            "packages/tools/src/lib.rs",
                            "packages/web/src/lib.rs",
                            "packages/web/src/pages.rs",
                        ]
                    },
                    "note": "token hits across roots",
                },
                {
                    "tool": "read",
                    "args": [
                        "packages/core/src/id.rs",
                        "packages/api/src/routes.rs",
                        "packages/tools/src/lib.rs",
                        "packages/web/src/lib.rs",
                    ],
                    "ok": True,
                    "summary": {},
                    "note": "read hits; web comment-only omit; tools local fn looks like def to greps",
                },
            ],
            "rationale": (
                "grep normalize_id; read shows api uses core::normalize_id and "
                "web is comments. tools local normalize_id still looks like a "
                "definition to token heuristics — may remain in set (noise). "
                "No agentgraph; no workspace-root metric (B is path-grep)."
            ),
        },
    },
    "rust-real-noise-dense": {
        "symbol": "encode",
        "issue_digest": (
            "Encode::encode format change; dense implementors + metrics/legacy/"
            "clone name collisions; separate real call sites."
        ),
        "A": {
            "files": ["src/packet.rs", "src/wire.rs", "src/lib.rs"],
            "chose_correct_workspace_root": None,
            "calls": [
                {
                    "tool": "agentgraph.index",
                    "args": ["index", "--force"],
                    "ok": True,
                    "summary": {"exit_code": 0},
                    "note": "single-crate index",
                },
                {
                    "tool": "agentgraph.who-calls",
                    "args": ["who-calls", "encode", "--limit", "50"],
                    "ok": True,
                    "summary": {
                        "high_freq_name": True,
                        "implementor_count": 12,
                        "callers_files": ["src/wire.rs"],
                    },
                    "note": "high-freq: callers in wire.rs; implementors in packet.rs",
                },
                {
                    "tool": "agentgraph.find",
                    "args": ["find", "Encode"],
                    "ok": True,
                    "summary": {"definition_files": ["src/packet.rs"]},
                    "note": "trait definition",
                },
                {
                    "tool": "agentgraph.blast-radius",
                    "args": ["blast-radius", "encode", "--depth", "3"],
                    "ok": True,
                    "summary": {"window": "sound"},
                    "note": "structure edges live in packet/wire/lib",
                },
                {
                    "tool": "read",
                    "args": ["src/metrics.rs", "src/legacy.rs", "src/clone_heavy.rs"],
                    "ok": True,
                    "summary": {},
                    "note": "collision modules are not Encode consumers — omit",
                },
            ],
            "rationale": (
                "who_calls high-freq separates implementors (packet.rs) from "
                "callers (wire.rs). lib re-exports Encode. metrics/legacy/"
                "clone_heavy collide on encode names after read — omit."
            ),
        },
        "B": {
            "files": [
                "src/packet.rs",
                "src/wire.rs",
                "src/lib.rs",
                "src/metrics.rs",
                "src/legacy.rs",
                "src/clone_heavy.rs",
            ],
            "chose_correct_workspace_root": None,
            "calls": [
                {
                    "tool": "grep",
                    "args": ["encode"],
                    "ok": True,
                    "summary": {
                        "hits": [
                            "src/packet.rs",
                            "src/wire.rs",
                            "src/lib.rs",
                            "src/metrics.rs",
                            "src/legacy.rs",
                            "src/clone_heavy.rs",
                        ]
                    },
                    "note": "token hits flood noise modules",
                },
                {
                    "tool": "read",
                    "args": [
                        "src/packet.rs",
                        "src/wire.rs",
                        "src/metrics.rs",
                        "src/legacy.rs",
                        "src/clone_heavy.rs",
                    ],
                    "ok": True,
                    "summary": {},
                    "note": "careful read may drop some noise; token flood remains attractive",
                },
            ],
            "rationale": (
                "grep encode hits all six modules. Dense collisions make "
                "exclusion harder without structure tools; some noise often "
                "remains. No agentgraph."
            ),
        },
    },
    "rust-sound-scoped-clean": {
        "symbol": "batch_write",
        "issue_digest": (
            "clean::batch_write change; dirty unsafe sibling disables union "
            "sound; scoped clean root; dirty name collisions are noise."
        ),
        "A": {
            "files": [
                "packages/clean/src/write.rs",
                "packages/clean/src/lib.rs",
            ],
            "chose_correct_workspace_root": True,
            "calls": [
                {
                    "tool": "agentgraph.index",
                    "args": ["index", "--workspace", "workspace.json", "--force"],
                    "ok": True,
                    "summary": {"exit_code": 0},
                    "note": "workspace index",
                },
                {
                    "tool": "agentgraph.blast-radius",
                    "args": [
                        "blast-radius",
                        "batch_write",
                        "--depth",
                        "3",
                        "--workspace",
                        "workspace.json",
                    ],
                    "ok": True,
                    "summary": {"window": "default", "subset_ok": False},
                    "note": "union not sound — do not claim sound",
                },
                {
                    "tool": "agentgraph.find",
                    "args": [
                        "find",
                        "batch_write",
                        "--workspace",
                        "workspace.json",
                        "--workspace-root",
                        "packages/clean",
                    ],
                    "ok": True,
                    "summary": {"definition_files": ["packages/clean/src/write.rs"]},
                    "note": "scoped find on clean root",
                },
                {
                    "tool": "agentgraph.who-calls",
                    "args": [
                        "who-calls",
                        "batch_write",
                        "--workspace",
                        "workspace.json",
                    ],
                    "ok": True,
                    "summary": {"callers_files": ["packages/clean/src/lib.rs"]},
                    "note": "clean caller only; dirty shadow is not batch_write",
                },
                {
                    "tool": "agentgraph.subset",
                    "args": ["subset", "--workspace", "workspace.json"],
                    "ok": True,
                    "summary": {"in_subset": False},
                    "note": "legacy unsafe disables union S; scoped candidates on clean",
                },
                {
                    "tool": "read",
                    "args": [
                        "packages/dirty/src/lib.rs",
                        "packages/dirty/src/shadow.rs",
                    ],
                    "ok": True,
                    "summary": {},
                    "note": "dirty is unsafe sibling + name noise — not symbol review files",
                },
            ],
            "rationale": (
                "Scoped find/who-calls on clean: write.rs definition + lib.rs "
                "caller. window=default / subset_ok=false — recommend scoped "
                "sound on clean; do not claim union sound. dirty/* name "
                "collisions omitted from review set."
            ),
        },
        "B": {
            "files": [
                "packages/clean/src/write.rs",
                "packages/clean/src/lib.rs",
                "packages/dirty/src/lib.rs",
            ],
            "chose_correct_workspace_root": None,
            "calls": [
                {
                    "tool": "grep",
                    "args": ["batch_write"],
                    "ok": True,
                    "summary": {
                        "hits": [
                            "packages/clean/src/write.rs",
                            "packages/clean/src/lib.rs",
                            "packages/dirty/src/lib.rs",
                            "packages/dirty/src/shadow.rs",
                        ]
                    },
                    "note": "dirty comments/helpers collide on the name",
                },
                {
                    "tool": "read",
                    "args": [
                        "packages/clean/src/write.rs",
                        "packages/dirty/src/lib.rs",
                        "packages/dirty/src/shadow.rs",
                    ],
                    "ok": True,
                    "summary": {},
                    "note": "dirty is unsafe; name collisions may still leak into set",
                },
            ],
            "rationale": (
                "grep batch_write hits clean def/caller + dirty collisions. "
                "Without structure scoping, dirty noise often remains. No "
                "agentgraph; no workspace-root metric."
            ),
        },
    },
    "ts-multi-root-client": {
        "symbol": "RegistryClient",
        "issue_digest": (
            "RegistryClient.fetch contract change; multi-root TS; service "
            "consumes; cli/docs name mentions are noise."
        ),
        "A": {
            "files": [
                "packages/registry/src/client.ts",
                "packages/registry/src/index.ts",
                "packages/service/src/order.service.ts",
                "packages/service/src/order.controller.ts",
            ],
            "chose_correct_workspace_root": True,
            "calls": [
                {
                    "tool": "agentgraph.index",
                    "args": ["index", "--workspace", "workspace.json", "--force"],
                    "ok": True,
                    "summary": {"exit_code": 0},
                    "note": "multi-root index",
                },
                {
                    "tool": "agentgraph.find",
                    "args": [
                        "find",
                        "RegistryClient",
                        "--workspace",
                        "workspace.json",
                        "--workspace-root",
                        "packages/registry",
                    ],
                    "ok": True,
                    "summary": {
                        "definition_files": [
                            "packages/registry/src/client.ts",
                            "packages/registry/src/index.ts",
                        ]
                    },
                    "note": "scoped definition root registry",
                },
                {
                    "tool": "agentgraph.blast-radius",
                    "args": [
                        "blast-radius",
                        "RegistryClient",
                        "--depth",
                        "3",
                        "--workspace",
                        "workspace.json",
                    ],
                    "ok": True,
                    "summary": {"window": "sound"},
                    "note": "dependents: service package",
                },
                {
                    "tool": "agentgraph.who-calls",
                    "args": [
                        "who-calls",
                        "RegistryClient",
                        "--workspace",
                        "workspace.json",
                    ],
                    "ok": True,
                    "summary": {
                        "callers_files": [
                            "packages/service/src/order.service.ts",
                            "packages/service/src/order.controller.ts",
                        ]
                    },
                    "note": "service construct/use; cli/docs are string mentions",
                },
                {
                    "tool": "read",
                    "args": [
                        "packages/cli/src/main.ts",
                        "packages/docs/src/overview.ts",
                    ],
                    "ok": True,
                    "summary": {},
                    "note": "help/prose name mentions — omit",
                },
            ],
            "rationale": (
                "Scoped find on registry + who-calls/blast: registry def/reexport "
                "+ service consumers. cli/docs string mentions are not dependents "
                "— omit. Correct roots: registry+service."
            ),
        },
        "B": {
            "files": [
                "packages/registry/src/client.ts",
                "packages/registry/src/index.ts",
                "packages/service/src/order.service.ts",
                "packages/service/src/order.controller.ts",
                "packages/cli/src/main.ts",
            ],
            "chose_correct_workspace_root": None,
            "calls": [
                {
                    "tool": "grep",
                    "args": ["RegistryClient"],
                    "ok": True,
                    "summary": {
                        "hits": [
                            "packages/registry/src/client.ts",
                            "packages/registry/src/index.ts",
                            "packages/service/src/order.service.ts",
                            "packages/service/src/order.controller.ts",
                            "packages/cli/src/main.ts",
                            "packages/cli/src/help.ts",
                            "packages/docs/src/overview.ts",
                            "packages/docs/src/changelog.ts",
                        ]
                    },
                    "note": "token hits include help/docs noise",
                },
                {
                    "tool": "read",
                    "args": [
                        "packages/cli/src/main.ts",
                        "packages/docs/src/overview.ts",
                        "packages/service/src/order.service.ts",
                    ],
                    "ok": True,
                    "summary": {},
                    "note": "docs prose drop; cli HELP string may remain attractive",
                },
            ],
            "rationale": (
                "grep RegistryClient floods cli/docs. Careful reads drop some "
                "docs noise; help-string noise may remain. No agentgraph."
            ),
        },
    },
}


def repo_root() -> Path:
    return repo_root_from_script()


def hard_fixtures_dir(root: Path) -> Path:
    return root / "fixtures" / "eval-agent-tasks-hard"


def empty_labels() -> Dict[str, Any]:
    return {
        "expected_files": [],
        "noise_files": [],
        "forbidden_files": [],
        "forbidden_source": "",
    }


def empty_score(file_set: Sequence[str]) -> Dict[str, Any]:
    found = sorted({_as_rel(p) for p in file_set if p})
    return {
        "expected_file_recall": None,
        "expected_hit": [],
        "expected_miss": [],
        "extra_noise_files": [],
        "extra_noise_count": None,
        "forbidden_files": [],
        "forbidden_hit": [],
        "forbidden_hit_count": None,
        "file_set": found,
        "file_set_size": len(found),
        "stamped": False,
    }


def _clean_args(args: Sequence[str]) -> List[str]:
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


def _is_recipe_tool(tool: str) -> bool:
    t = str(tool).lower().replace("_", "-")
    return t.startswith("agentgraph.") or t in {
        "blast-radius",
        "who-calls",
        "subset",
        "index",
        "find",
        "related",
        "impact",
    }


def _is_grep_tool(tool: str) -> bool:
    t = str(tool).lower()
    return t in {"grep", "grep_symbol", "grep_token", "name_grep", "walk", "walk_sources"} or t.startswith(
        "grep"
    )


def compute_ext_metrics(
    file_set: Sequence[str],
    tool_calls: Sequence[Dict[str, Any]],
    runner_id: str,
    model_note: str,
    independent_session: bool,
    chose_correct_workspace_root: Optional[bool],
    task_meta: Optional[Dict[str, Any]] = None,
    approx_tokens: Optional[int] = None,
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
        if tool.lower().startswith("read") or tool.lower() == "read":
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
    # chose_correct_workspace_root: null when task is not multi-root
    if not multi:
        cwr: Optional[bool] = None
    else:
        cwr = chose_correct_workspace_root
    return {
        "mcp_or_cli_calls": {
            "count": mcp_count,
            "recipe_tools": recipe_tools,
            "grep_count": grep_count,
        },
        "chose_correct_workspace_root": cwr,
        "file_budget": len(sorted({_as_rel(p) for p in file_set if p})),
        "read_budget": len(read_files) if read_files else None,
        "read_files": read_files,
        "approx_tokens": approx_tokens,
        "runner_id": runner_id,
        "model_note": model_note,
        "independent_session": independent_session,
        "saw_labels_before_commit": False,
    }


def task_randomization(tasks: Sequence[str], seed: int, arm_order: Sequence[str]) -> Dict[str, Any]:
    rng = random.Random(f"p0-5c-{seed}")
    order = list(tasks)
    rng.shuffle(order)
    return {
        "seed": seed,
        "task_order": order,
        "arm_order": list(arm_order),
        "method": "task-level randomization; order recorded before arm execution",
    }


def build_c_trajectory(
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
    meta: Dict[str, Any],
    labels: Dict[str, Any],
    score: Dict[str, Any],
    fixture_rel: str,
    chose_correct_workspace_root: Optional[bool],
    task_meta: Optional[Dict[str, Any]],
    rand: Dict[str, Any],
    extra_honesty: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    cleaned_calls = []
    for i, c in enumerate(calls, start=1):
        cc = dict(c)
        cc["seq"] = i
        cc["args"] = _clean_args(cc.get("args") or [])
        cleaned_calls.append(cc)
    ext = compute_ext_metrics(
        files,
        cleaned_calls,
        runner_id=runner_id,
        model_note=model_note,
        independent_session=independent_session,
        chose_correct_workspace_root=chose_correct_workspace_root,
        task_meta=task_meta,
        approx_tokens=None,
    )
    honesty = {
        "live_llm_agent": kind == "live_llm_agent",
        "scripted_tool_policy_agent": False,
        "scripted_external_runner": runner_id == "scripted_external_runner",
        "private_corpus": False,
        "not_public_benchmark_model": kind == "live_llm_agent",
        "standardized_lab_harness": False,
        "independent_session": independent_session,
        "saw_labels_before_commit": False,
        "note": RUNNER_META[runner_id]["honesty_note"],
        "no_fabricated_llm_numbers": True,
        "no_oversell_live_a_beats_b": True,
    }
    if extra_honesty:
        honesty.update(extra_honesty)
    payload = {
        "schema": TRAJECTORY_SCHEMA,
        "schema_alias": C_SCHEMA_ALIAS,
        "policy": arm,
        "policy_label": (
            "P0-5c arm A — agentgraph recipes (+reads)"
            if arm == "A"
            else "P0-5c arm B — read/grep only (no agentgraph)"
        ),
        "kind": kind,
        "arm": arm,
        "runner_id": runner_id,
        "task_id": task_id,
        "seed": seed,
        "run_id": f"{runner_id}-{arm.lower()}-{seed}",
        "fixture": fixture_rel,
        "symbol": meta.get("symbol") or "",
        "model_note": model_note,
        "independent_session": independent_session,
        "saw_labels_before_commit": False,
        "tools_allowed": list(ARM_TOOLS[arm]),
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
                "P0-5c hard tasks; file_set from tool/grep evidence; "
                "expected labels stamped offline after commitment"
            ),
            "hard_slice": True,
        },
        "notes": [
            RUNNER_META[runner_id]["honesty_note"],
            f"Arm {arm}: {rationale}",
        ],
        "errors": [],
    }
    return payload


def policy_run_to_fields(run: PolicyRun, task_meta: Dict[str, Any], arm: str) -> Dict[str, Any]:
    """Map scripted PolicyRun → P0-5c extended fields."""
    recipe_tools: List[str] = []
    grep_count = 0
    read_n = 0
    for c in run.tool_calls:
        tool = str(c.get("tool") or "")
        if _is_recipe_tool(tool):
            short = tool.split(".")[-1].replace("_", "-")
            if short not in recipe_tools:
                recipe_tools.append(short)
        if _is_grep_tool(tool):
            grep_count += 1
        if tool.lower().startswith("read"):
            read_n += int((c.get("summary") or {}).get("read_count") or len(c.get("args") or []) or 0)
            if read_n == 0:
                read_n = len([a for a in c.get("args") or [] if "/" in str(a) or "." in str(a)])
    # Scripted A may use --workspace-root; B does not.
    multi = bool((task_meta.get("workspace") or {}).get("roots")) or bool(
        (task_meta.get("expected") or {}).get("multi_root")
    )
    if not multi:
        cwr: Optional[bool] = None
    else:
        if arm == "A":
            # true if any query used --workspace / --workspace-root on a multi-root task
            used_ws = any(
                any(
                    str(a) in {"--workspace", "--workspace-root", "--workspace-db"}
                    for a in c.get("args") or []
                )
                for c in run.tool_calls
            )
            cwr = bool(used_ws)
        else:
            # B is path-grep; workspace-root selection is n/a for tool args.
            # Record false when file_set includes known noise under wrong roots.
            expected = task_meta.get("expected") or {}
            noise = set(expected.get("noise_files") or [])
            cwr = not any(p in noise for p in run.file_set)
    ext = {
        "mcp_or_cli_calls": {
            "count": sum(1 for c in run.tool_calls if _is_recipe_tool(str(c.get("tool") or ""))),
            "recipe_tools": recipe_tools,
            "grep_count": grep_count,
        },
        "chose_correct_workspace_root": cwr,
        "file_budget": len(run.file_set),
        "read_budget": read_n if read_n else None,
        "approx_tokens": None,
    }
    return ext


def load_hard_task_meta(root: Path, task_id: str) -> Dict[str, Any]:
    path = hard_fixtures_dir(root) / task_id / "task.json"
    return json.loads(path.read_text(encoding="utf-8"))


def list_hard_tasks(root: Path) -> List[str]:
    base = hard_fixtures_dir(root)
    if not base.is_dir():
        return []
    out = []
    for p in sorted(base.iterdir()):
        if p.is_dir() and (p / "task.json").is_file():
            out.append(p.name)
    return out


def cmd_write(args: argparse.Namespace) -> int:
    root = repo_root()
    out_root = args.out_dir or (root / "evals" / "agent-ab-c")
    seeds = args.seeds or [0, 1, 2]
    runners = args.runner or ["host_session_llm", "scripted_external_runner"]
    tasks = list_hard_tasks(root)
    if args.task:
        tasks = [t for t in tasks if t in set(args.task)]
    if not tasks:
        print("error: no hard fixtures under fixtures/eval-agent-tasks-hard/", file=sys.stderr)
        return 2
    bin_path: Optional[Path] = None
    if "scripted_external_runner" in runners and not args.no_exec_scripted:
        try:
            bin_path = find_bin(args.bin)
        except SystemExit as e:
            print(f"warn: agentgraph binary unavailable ({e}); scripted A may fail", file=sys.stderr)

    written = 0
    order_log: List[Dict[str, Any]] = []
    for seed in seeds:
        # Arm order alternates by seed for isolation record (A→B vs B→A).
        arm_order = ["A", "B"] if seed % 2 == 0 else ["B", "A"]
        rand = task_randomization(tasks, seed, arm_order)
        order_log.append(rand)
        for runner_id in runners:
            meta_r = RUNNER_META[runner_id]
            for tid in rand["task_order"]:
                if args.task and tid not in set(args.task):
                    continue
                task_meta = load_hard_task_meta(root, tid)
                meta = {
                    "id": task_meta.get("id") or tid,
                    "title": task_meta.get("title"),
                    "language": task_meta.get("language"),
                    "issue": task_meta.get("issue"),
                    "symbol": task_meta.get("symbol"),
                }
                fixture_rel = f"fixtures/eval-agent-tasks-hard/{tid}"
                for arm in arm_order:
                    if runner_id == "host_session_llm":
                        if tid not in HOST_DECISIONS or arm not in HOST_DECISIONS[tid]:
                            print(f"incomplete cell: host_session_llm {tid} arm {arm}", file=sys.stderr)
                            continue
                        dec = HOST_DECISIONS[tid][arm]
                        files = dec["files"]
                        calls = dec["calls"]
                        cwr = dec.get("chose_correct_workspace_root")
                        payload = build_c_trajectory(
                            task_id=tid,
                            arm=arm,
                            seed=seed,
                            runner_id=runner_id,
                            kind=meta_r["kind"],
                            model_note=meta_r["model_note"],
                            independent_session=meta_r["independent_session"],
                            files=files,
                            calls=calls,
                            rationale=dec["rationale"],
                            meta=meta,
                            labels=empty_labels(),
                            score=empty_score(files),
                            fixture_rel=fixture_rel,
                            chose_correct_workspace_root=cwr,
                            task_meta=task_meta,
                            rand=rand,
                            extra_honesty={
                                "contamination": (
                                    "Host session authored hard fixtures in this slice; "
                                    "independent_session=false. Decision path uses tool/"
                                    "grep evidence only (saw_labels_before_commit=false)."
                                )
                            },
                        )
                    else:
                        # scripted_external_runner — execute policies on hard fixtures
                        tdir = hard_fixtures_dir(root) / tid
                        work = root / "target" / "agent_ab_c_work" / tid / f"{runner_id}-{arm}-{seed}"
                        copy_fixture(tdir, work)
                        if arm == "A":
                            if bin_path is None:
                                print(f"incomplete cell: scripted A needs binary for {tid}", file=sys.stderr)
                                continue
                            run = run_policy_a(bin_path, tdir, task_meta, work, seed)
                            run.policy = "A"
                        else:
                            run = run_policy_b(tdir, task_meta, work, seed)
                            run.policy = "B"
                        if run.errors:
                            print(f"warn: scripted {arm} {tid} seed {seed} errors={run.errors}", file=sys.stderr)
                        calls = list(run.tool_calls)
                        files = list(run.file_set)
                        ext = policy_run_to_fields(run, task_meta, arm)
                        payload = build_c_trajectory(
                            task_id=tid,
                            arm=arm,
                            seed=seed,
                            runner_id=runner_id,
                            kind=meta_r["kind"],
                            model_note=meta_r["model_note"],
                            independent_session=meta_r["independent_session"],
                            files=files,
                            calls=calls,
                            rationale=(
                                run.extras.get("file_set_method")
                                or run.extras.get("policy")
                                or "scripted external policy"
                            ),
                            meta=meta,
                            labels=empty_labels(),
                            score=empty_score(files),
                            fixture_rel=fixture_rel,
                            chose_correct_workspace_root=ext["chose_correct_workspace_root"],
                            task_meta=task_meta,
                            rand=rand,
                        )
                        # overwrite ext fields from actual policy run
                        payload["mcp_or_cli_calls"] = ext["mcp_or_cli_calls"]
                        payload["file_budget"] = ext["file_budget"]
                        payload["read_budget"] = ext["read_budget"]
                        payload["chose_correct_workspace_root"] = ext["chose_correct_workspace_root"]
                        payload["metrics_ext"].update(ext)
                        if run.errors:
                            payload["errors"] = list(run.errors)
                    path = out_root / tid / f"run-{runner_id}-{arm.lower()}-{seed}.json"
                    _write_trajectory(path, payload)
                    written += 1
                    print(f"wrote {path}")
    # record randomization log
    log_path = out_root / "task_randomization.json"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log_path.write_text(
        json.dumps(
            {
                "schema": "agentgraph.eval_agent_ab.c.randomization.v1",
                "seeds": order_log,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {written} P0-5c trajectory(ies) under {out_root}; order log {log_path}")
    return 0


def stamp_one(path: Path, fixtures: Path) -> Dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    tid = payload.get("task_id") or ""
    tdir = fixtures / tid
    if not (tdir / "task.json").is_file():
        return {"path": str(path), "skipped": True, "reason": f"missing hard fixture {tid}"}
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
    score["stamp_source"] = f"fixtures/eval-agent-tasks-hard/{tid}/task.json"
    task = dict(payload.get("task") or {})
    task.update(labels)
    payload["task"] = task
    payload["score"] = score
    # recompute file_budget from stamped set
    payload["file_budget"] = score["file_set_size"]
    if isinstance(payload.get("metrics_ext"), dict):
        payload["metrics_ext"]["file_budget"] = score["file_set_size"]
    _write_trajectory(path, payload)
    return {
        "path": str(path),
        "task_id": tid,
        "policy": payload.get("policy"),
        "runner_id": payload.get("runner_id"),
        "run_id": payload.get("run_id"),
        "recall": score.get("expected_file_recall"),
        "noise": score.get("extra_noise_count"),
        "size": score.get("file_set_size"),
        "chose_correct_workspace_root": payload.get("chose_correct_workspace_root"),
        "mcp_or_cli_calls": payload.get("mcp_or_cli_calls"),
        "stamped": True,
    }


def cmd_stamp(args: argparse.Namespace) -> int:
    root = repo_root()
    fixtures = hard_fixtures_dir(root)
    traj_dir = args.traj_dir or (root / "evals" / "agent-ab-c")
    paths = discover_traj_files(traj_dir)
    if args.trajectory:
        paths = [Path(args.trajectory)]
    if not paths:
        print(f"error: no trajectories under {traj_dir}", file=sys.stderr)
        return 2
    results = [stamp_one(p, fixtures) for p in paths]
    out = args.out or (root / "target" / "agent_ab_c_stamp.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "schema": "agentgraph.eval_agent_ab.c.stamp.v1",
                "offline": True,
                "results": results,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"stamped {sum(1 for r in results if r.get('stamped'))} trajectory(ies); wrote {out}")
    return 0


def _mean(xs: List[float]) -> float:
    return round(sum(xs) / len(xs), 4) if xs else 0.0


def cmd_score(args: argparse.Namespace) -> int:
    root = repo_root()
    traj_dir = args.traj_dir or (root / "evals" / "agent-ab-c")
    paths: List[Path] = []
    if args.trajectory:
        paths.append(Path(args.trajectory))
    if args.traj_dir or not args.trajectory:
        paths.extend(discover_traj_files(Path(args.traj_dir) if args.traj_dir else traj_dir))
    if not paths:
        print("error: no trajectories to score", file=sys.stderr)
        return 2
    results = []
    bad = 0
    by_runner_arm: Dict[str, List[Dict[str, Any]]] = {}
    completeness: Dict[str, Dict[str, int]] = {}
    for p in paths:
        try:
            payload = json.loads(p.read_text(encoding="utf-8"))
            sc = score_trajectory(payload)
            if not (payload.get("score") or {}).get("stamped"):
                sc["warn"] = "score not stamped from hard fixtures after commitment"
            sc["path"] = str(p)
            sc["runner_id"] = payload.get("runner_id")
            sc["model_note"] = payload.get("model_note")
            sc["arm"] = payload.get("arm") or payload.get("policy")
            sc["independent_session"] = payload.get("independent_session")
            sc["saw_labels_before_commit"] = payload.get("saw_labels_before_commit")
            sc["mcp_or_cli_calls"] = payload.get("mcp_or_cli_calls")
            sc["chose_correct_workspace_root"] = payload.get("chose_correct_workspace_root")
            sc["file_budget"] = payload.get("file_budget")
            sc["read_budget"] = payload.get("read_budget")
            sc["approx_tokens"] = payload.get("approx_tokens")
        except Exception as e:  # noqa: BLE001
            bad += 1
            results.append({"path": str(p), "error": str(e)})
            print(f"FAIL {p}: {e}", file=sys.stderr)
            continue
        results.append(sc)
        status = "OK" if sc.get("recorded_matches_replay") else "MISMATCH"
        if not sc.get("recorded_matches_replay") or sc.get("errors"):
            bad += 1
        rec = sc["recomputed"]
        key = f"{sc.get('runner_id')}:{sc.get('arm')}"
        by_runner_arm.setdefault(key, []).append({**rec, **{k: sc.get(k) for k in (
            "chose_correct_workspace_root", "file_budget", "read_budget",
            "mcp_or_cli_calls", "approx_tokens",
        )}})
        tcomp = completeness.setdefault(sc.get("task_id") or "?", {})
        tcomp[key] = tcomp.get(key, 0) + 1
        print(
            f"{status} {sc.get('task_id')}/{sc.get('runner_id')}/{sc.get('arm')}/{sc.get('run_id')} "
            f"recall={rec['expected_file_recall']} noise={rec['extra_noise_count']} "
            f"size={rec['file_set_size']} cwr={sc.get('chose_correct_workspace_root')} "
            f"mcp={ (sc.get('mcp_or_cli_calls') or {}).get('count') } "
            f"indep={sc.get('independent_session')}"
        )

    summary = {}
    for key, rows in by_runner_arm.items():
        runner, arm = key.split(":", 1)
        cwr_vals = [r["chose_correct_workspace_root"] for r in rows if r.get("chose_correct_workspace_root") is not None]
        mcp_counts = []
        for r in rows:
            m = r.get("mcp_or_cli_calls") or {}
            if isinstance(m, dict) and m.get("count") is not None:
                mcp_counts.append(float(m["count"]))
        summary[key] = {
            "runner_id": runner,
            "arm": arm,
            "run_count": len(rows),
            "mean_expected_file_recall": _mean([float(r.get("expected_file_recall") or 0) for r in rows]),
            "mean_extra_noise_files": _mean([float(r.get("extra_noise_count") or 0) for r in rows]),
            "mean_file_set_size": _mean([float(r.get("file_set_size") or 0) for r in rows]),
            "mean_file_budget": _mean([float(r.get("file_budget") or r.get("file_set_size") or 0) for r in rows]),
            "mean_mcp_or_cli_calls": _mean(mcp_counts),
            "chose_correct_workspace_root_true": sum(1 for v in cwr_vals if v is True),
            "chose_correct_workspace_root_false": sum(1 for v in cwr_vals if v is False),
            "chose_correct_workspace_root_na": sum(1 for r in rows if r.get("chose_correct_workspace_root") is None),
            "approx_tokens_reported": any(r.get("approx_tokens") is not None for r in rows),
        }

    expected_runners = {"host_session_llm", "scripted_external_runner"}
    seen_runners = {k.split(":", 1)[0] for k in by_runner_arm}
    runner_kinds_present = sorted(seen_runners)
    incomplete_cells = []
    for tid, arms in completeness.items():
        for rk in sorted(expected_runners):
            for arm in ("A", "B"):
                key = f"{rk}:{arm}"
                n = arms.get(key, 0)
                if n < len(seeds_from_results(results, tid, rk, arm)) or n == 0:
                    if n == 0:
                        incomplete_cells.append({"task_id": tid, "runner_id": rk, "arm": arm, "runs": 0})

    out = args.out or (root / "target" / "agent_ab_c_replay.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "schema": "agentgraph.eval_agent_ab.c.replay.v1",
                "offline": True,
                "network_required": False,
                "runner_kinds_present": runner_kinds_present,
                "hard_tasks": sorted({r.get("task_id") for r in results if r.get("task_id")}),
                "summary_by_runner_arm": summary,
                "completeness": completeness,
                "incomplete_cells": incomplete_cells,
                "results": results,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {out}")
    print(f"scored {len(results)} P0-5c trajectory(ies); problems={bad}")
    print("runner_kinds_present:", runner_kinds_present)
    print("summary_by_runner_arm:", json.dumps(summary, ensure_ascii=False))
    if incomplete_cells:
        print("incomplete_cells:", json.dumps(incomplete_cells, ensure_ascii=False))
    # Gates: ≥2 runner kinds, ≥4 hard tasks mentioned in results when full set present
    if len(seen_runners) < 2 and not args.allow_partial_runners:
        print("gate: need ≥2 runner kinds in scored set", file=sys.stderr)
        return 1
    return 1 if bad else 0


def seeds_from_results(results: List[Dict[str, Any]], task_id: str, runner: str, arm: str) -> List[int]:
    seeds = []
    for r in results:
        if r.get("task_id") == task_id and r.get("runner_id") == runner and r.get("arm") == arm:
            seeds.append(r.get("seed"))
    return seeds


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="P0-5c multi-runner hard-task A/B harness")
    sub = p.add_subparsers(dest="command")
    w = sub.add_parser("write", help="write P0-5c trajectories (unstamped)")
    w.add_argument("--out-dir", type=Path, default=None)
    w.add_argument("--task", action="append", default=None)
    w.add_argument(
        "--runner",
        action="append",
        choices=["host_session_llm", "scripted_external_runner"],
        default=None,
        help="runner kind (repeatable); default both",
    )
    w.add_argument(
        "--seeds",
        type=lambda s: [int(x) for x in s.split(",") if x.strip() != ""],
        default=None,
    )
    w.add_argument("--bin", type=str, default=None, help="agentgraph binary for scripted runner")
    w.add_argument(
        "--no-exec-scripted",
        action="store_true",
        help="skip scripted CLI execution (incomplete cells only)",
    )
    w.set_defaults(func=cmd_write)
    st = sub.add_parser("stamp", help="stamp structure-fact scores after commitment")
    st.add_argument("--traj-dir", type=Path, default=None)
    st.add_argument("--trajectory", type=Path, default=None)
    st.add_argument("--out", type=Path, default=None)
    st.set_defaults(func=cmd_stamp)
    sc = sub.add_parser("score", help="offline replay scoring with extended metrics")
    sc.add_argument("--traj-dir", type=Path, default=None)
    sc.add_argument("--trajectory", type=Path, default=None)
    sc.add_argument("--out", type=Path, default=None)
    sc.add_argument(
        "--allow-partial-runners",
        action="store_true",
        help="do not fail when <2 runner kinds present (dev only)",
    )
    sc.set_defaults(func=cmd_score)
    return p


def main(argv: Optional[List[str]] = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv or argv[0] not in {"write", "stamp", "score", "-h", "--help"}:
        argv = ["write", *argv]
    parser = build_parser()
    args = parser.parse_args(argv)
    if not getattr(args, "func", None):
        parser.print_help()
        return 2
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
