#!/usr/bin/env python3
"""I-track live runner: budgeted file-set A/B on unlabeled dense fixtures.

Isolation: one `mimo run` per cell (independent_session=true).
Prompts enforce **file_budget K** (default 8). No labels in fixtures.

Usage:
  python scripts/eval_i_noise_live.py prepare
  python scripts/eval_i_noise_live.py run --seeds 0-4
  python scripts/eval_i_noise_live.py score
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

ROOT = Path(__file__).resolve().parents[1]
TRAJ = ROOT / "evals" / "agent-ab-noise"
MIMO = r"C:\Users\Administrator\.mimocode\bin\mimo.exe"
AG = ROOT / "target" / "debug" / "agentgraph.exe"
if not AG.is_file():
    AG = ROOT / "target" / "release" / "agentgraph.exe"

TASKS = ["i-order-pipeline", "i-cache-registry"]
RUNNERS = {
    "i_live_runner_1": "xiaomi/mimo-v2.6-pro",
    "i_live_runner_2": "xiaomi/mimo-v2.6-flash",
}
ARMS = ("A", "B")
FILE_BUDGET = 8


def brief_dir(runner: str, arm: str, seed: int, task: str) -> Path:
    return TRAJ / "_briefs" / runner / arm / str(seed) / task


def cmd_prepare(args: argparse.Namespace) -> int:
    if "-" in args.seeds:
        a, b = args.seeds.split("-", 1)
        seeds = list(range(int(a), int(b) + 1))
    else:
        seeds = [int(x) for x in args.seeds.split(",") if x]
    runners = args.runner or list(RUNNERS)
    for seed in seeds:
        for runner in runners:
            for arm in ARMS:
                for task in TASKS:
                    bdir = brief_dir(runner, arm, seed, task)
                    bdir.mkdir(parents=True, exist_ok=True)
                    src = ROOT / "fixtures" / "eval-i-noise" / task
                    if not src.is_dir():
                        print("missing fixture", src, file=sys.stderr)
                        return 1
                    workdir = bdir / "workdir"
                    if workdir.exists():
                        shutil.rmtree(workdir)
                    shutil.copytree(src, workdir)
                    # never leave labels / task.json in workdir
                    (workdir / "task.json").unlink(missing_ok=True)
                    issue = (
                        f"# Issue brief — {task}\n\n"
                        f"- task: `{task}`\n"
                        f"- runner_id: `{runner}`\n- arm: **{arm}**\n- seed: `{seed}`\n\n"
                        f"## Issue\n\n{(src / 'task.json').read_text(encoding='utf-8')}\n"
                    )
                    # strip expected/labels from issue: use only issue text
                    tj = json.loads((src / "task.json").read_text(encoding="utf-8"))
                    issue = (
                        f"# Issue brief — {task}\n\n"
                        f"- task: `{task}`\n- runner_id: `{runner}`\n"
                        f"- arm: **{arm}**\n- seed: `{seed}`\n\n"
                        f"## Issue (no golden list)\n\n{tj.get('issue','')}\n\n"
                        f"## Constraints\n\n"
                        f"- Return **at most {FILE_BUDGET} files** in `file_set`.\n"
                        f"- Prefer precision over coverage.\n"
                    )
                    (bdir / "ISSUE.md").write_text(issue, encoding="utf-8")
                    (bdir / "RUNNER_CONTRACT.md").write_text(
                        f"cell {runner}/{arm}/{seed}/{task}; write file_set.json + meta.json in workdir; "
                        f"schema agentgraph.eval_agent_ab_d.file_set.v1 / meta.v1; "
                        f"file_budget={FILE_BUDGET}; independent_session=true.\n",
                        encoding="utf-8",
                    )
    print("prepared", TRAJ)
    return 0


def build_prompt(task: str, arm: str, seed: int, runner: str, model: str, issue: str) -> str:
    policy = (
        "ARM A: use agentgraph CLI (on PATH) plus reads. "
        "Prefer blast_radius / who-calls `selected[]` (file_budget K) and avoid `pruned[]` files. "
        if arm == "A"
        else "ARM B: do NOT use agentgraph. Use listing / reading / grep only. "
    )
    return f"""You are an independent code-understanding agent (lab cell).
{policy}

{issue}

Workdir (cwd) is an isolated copy. Decide which files matter for the issue.

**Hard budget: at most {FILE_BUDGET} files** in file_set (precision over coverage).

Write `file_set.json` and `meta.json` into the current workdir.

file_set.json (agentgraph.eval_agent_ab_d.file_set.v1):
{{
  "schema": "agentgraph.eval_agent_ab_d.file_set.v1",
  "task_id": "{task}",
  "runner_id": "{runner}",
  "arm": "{arm}",
  "seed": {seed},
  "file_set": ["src/..."],
  "tool_calls": [{{"tool": "read", "args": ["src/x.ts"], "ok": true, "summary": {{}}, "note": ""}}],
  "chose_correct_workspace_root": null,
  "approx_tokens": null
}}

meta.json (agentgraph.eval_agent_ab_d.meta.v1) kind=live_llm_agent model_note={model}
independent_session=true saw_labels_before_commit=false arm_isolated=true lab_ready_claim=false.

Stop after writing both files.
"""


def run_cell(runner: str, arm: str, seed: int, task: str, timeout: int) -> Dict[str, Any]:
    bdir = brief_dir(runner, arm, seed, task)
    workdir = bdir / "workdir"
    issue_path = bdir / "ISSUE.md"
    fs_path = bdir / "file_set.json"
    meta_path = bdir / "meta.json"
    if fs_path.is_file() and meta_path.is_file():
        return {"status": "skip", "task": task, "runner": runner, "arm": arm, "seed": seed}
    if not issue_path.is_file() or not workdir.is_dir():
        return {"status": "missing_brief", "task": task, "runner": runner, "arm": arm, "seed": seed}

    issue = issue_path.read_text(encoding="utf-8")
    model = RUNNERS[runner]
    prompt = build_prompt(task, arm, seed, runner, model, issue)
    env = os.environ.copy()
    env["PATH"] = str(AG.parent) + os.pathsep + env.get("PATH", "")
    out_log = TRAJ / "_logs" / f"{runner}-{arm}-s{seed}-{task}.jsonl"
    err_log = TRAJ / "_logs" / f"{runner}-{arm}-s{seed}-{task}.err"
    out_log.parent.mkdir(parents=True, exist_ok=True)
    proc = subprocess.Popen(
        [MIMO, "run", "--model", model, "--dir", str(workdir), "--format", "json", prompt],
        cwd=str(workdir),
        env=env,
        stdout=open(out_log, "w", encoding="utf-8", errors="replace"),
        stderr=open(err_log, "w", encoding="utf-8", errors="replace"),
    )
    t0 = time.time()
    while time.time() - t0 < timeout:
        for name in ("file_set.json", "meta.json"):
            src = workdir / name
            dst = bdir / name
            if src.is_file() and not dst.is_file():
                shutil.copy2(src, dst)
        if fs_path.is_file() and meta_path.is_file():
            # backfill meta identity fields
            try:
                meta = json.loads(meta_path.read_text(encoding="utf-8-sig"))
                meta.setdefault("task_id", task)
                meta.setdefault("runner_id", runner)
                meta.setdefault("kind", "live_llm_agent")
                meta.setdefault("model_note", model)
                meta.setdefault("independent_session", True)
                meta.setdefault("arm", arm)
                meta.setdefault("seed", seed)
                meta.setdefault("saw_labels_before_commit", False)
                meta.setdefault("arm_isolated", True)
                meta.setdefault("lab_ready_claim", False)
                meta_path.write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
            except Exception:
                pass
            try:
                proc.terminate()
            except Exception:
                pass
            return {
                "status": "ok",
                "task": task,
                "runner": runner,
                "arm": arm,
                "seed": seed,
                "seconds": round(time.time() - t0, 1),
            }
        if proc.poll() is not None:
            break
        time.sleep(5)
    # promote leftovers
    for name in ("file_set.json", "meta.json"):
        src = workdir / name
        dst = bdir / name
        if src.is_file() and not dst.is_file():
            shutil.copy2(src, dst)
    ok = fs_path.is_file() and meta_path.is_file()
    return {
        "status": "ok" if ok else "timeout_or_missing",
        "task": task,
        "runner": runner,
        "arm": arm,
        "seed": seed,
        "seconds": round(time.time() - t0, 1),
    }


def cmd_run(args: argparse.Namespace) -> int:
    if "-" in args.seeds:
        a, b = args.seeds.split("-", 1)
        seeds = list(range(int(a), int(b) + 1))
    else:
        seeds = [int(x) for x in args.seeds.split(",") if x]
    runners = args.runner or [args.only_runner or "i_live_runner_1"]
    log: List[Dict[str, Any]] = []
    for seed in seeds:
        for runner in runners:
            for arm in ARMS:
                for task in args.task or TASKS:
                    print(f"CELL {runner} {arm} s{seed} {task}", flush=True)
                    rec = run_cell(runner, arm, seed, task, args.timeout)
                    print("  ->", rec.get("status"), rec.get("seconds", ""), flush=True)
                    log.append(rec)
                    TRAJ.mkdir(parents=True, exist_ok=True)
                    (TRAJ / "i_run_log.json").write_text(
                        json.dumps(log, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
                    )
    ok = sum(1 for x in log if x.get("status") in ("ok", "skip"))
    print(f"done ok={ok}/{len(log)}")
    return 0 if ok == len(log) else 1


def cmd_score(_args: argparse.Namespace) -> int:
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "eval_i_noise.py"), "score", "--traj-dir", str(TRAJ)],
        cwd=str(ROOT),
    ).returncode


def main(argv: Optional[List[str]] = None) -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("--seeds", default="0-4")
    p.add_argument("--runner", action="append")
    p.set_defaults(func=cmd_prepare)
    r = sub.add_parser("run")
    r.add_argument("--seeds", default="0-4")
    r.add_argument("--runner", action="append")
    r.add_argument("--only-runner", default="")
    r.add_argument("--task", action="append")
    r.add_argument("--timeout", type=int, default=240)
    r.set_defaults(func=cmd_run)
    sc = sub.add_parser("score")
    sc.set_defaults(func=cmd_score)
    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
