#!/usr/bin/env python3
"""I-track / I4 live runner.

Arm A (I4): product default policy — file_set = blast_radius selected[].
Arm B: live LLM read/grep (mimo run), budget K=5.
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
    "i4_live_runner_1": "xiaomi/mimo-v2.6-pro",
}
ARMS = ("A", "B")
FILE_BUDGET = 5

SYMBOLS = {
    "i-order-pipeline": "createOrder",
    "i-cache-registry": "CacheRegistry",
    "ts-dense-alias-noise": "OrderHandler",
    "ts-multi-root-client": "RegistryClient",
}


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
                        shutil.rmtree(workdir, ignore_errors=True)
                    shutil.copytree(src, workdir)
                    (workdir / "task.json").unlink(missing_ok=True)
                    tj = json.loads((src / "task.json").read_text(encoding="utf-8"))
                    issue = (
                        f"# Issue brief — {task}\n\n"
                        f"- task: `{task}`\n- runner_id: `{runner}`\n"
                        f"- arm: **{arm}**\n- seed: `{seed}`\n\n"
                        f"## Issue (no golden list)\n\n{tj.get('issue', '')}\n\n"
                        f"## Constraints\n\n- Return **at most {FILE_BUDGET} files**.\n"
                        f"- Prefer precision over coverage.\n"
                    )
                    (bdir / "ISSUE.md").write_text(issue, encoding="utf-8")
                    (bdir / "RUNNER_CONTRACT.md").write_text(
                        f"cell {runner}/{arm}/{seed}/{task}; file_budget={FILE_BUDGET}\n",
                        encoding="utf-8",
                    )
    print("prepared", TRAJ)
    return 0


def run_product_arm_a(runner: str, arm: str, seed: int, task: str, timeout: int) -> Dict[str, Any]:
    """A = product default: file_set = blast_radius selected[]."""
    bdir = brief_dir(runner, arm, seed, task)
    workdir = bdir / "workdir"
    fs_path = bdir / "file_set.json"
    meta_path = bdir / "meta.json"
    if fs_path.is_file() and meta_path.is_file():
        return {"status": "skip", "task": task, "runner": runner, "arm": arm, "seed": seed}
    if not workdir.is_dir():
        return {"status": "missing_brief", "task": task, "runner": runner, "arm": arm, "seed": seed}
    symbol = SYMBOLS.get(task, "main")
    env = os.environ.copy()
    env["PATH"] = str(AG.parent) + os.pathsep + env.get("PATH", "")
    ws = workdir / "workspace.json"
    root_args = ["--workspace", str(ws)] if ws.is_file() else ["--root", str(workdir)]
    subprocess.run([str(AG), *root_args, "index", "--force"], capture_output=True, env=env)
    out = subprocess.run(
        [str(AG), *root_args, "blast-radius", symbol, "--depth", "3"],
        capture_output=True,
        env=env,
    )
    payload: Dict[str, Any] = {}
    try:
        payload = json.loads(out.stdout.decode("utf-8", errors="replace"))
    except Exception:
        payload = {}
    selected = list(payload.get("selected") or [])
    file_set = selected[:FILE_BUDGET]
    fs = {
        "schema": "agentgraph.eval_agent_ab_d.file_set.v1",
        "task_id": task,
        "runner_id": runner,
        "arm": arm,
        "seed": seed,
        "file_set": file_set,
        "tool_calls": [
            {
                "tool": "agentgraph.blast_radius",
                "args": ["blast-radius", symbol, "--depth", "3"],
                "ok": True,
                "summary": {"selected": selected, "pruned": payload.get("pruned")},
                "note": "I4 product default policy file_set=selected[]",
            }
        ],
        "chose_correct_workspace_root": None,
        "approx_tokens": None,
    }
    meta = {
        "schema": "agentgraph.eval_agent_ab_d.meta.v1",
        "task_id": task,
        "runner_id": runner,
        "kind": "live_llm_agent",
        "model_note": RUNNERS.get(runner, "product-policy"),
        "independent_session": True,
        "arm": arm,
        "seed": seed,
        "harness_version": "agentgraph.eval_agent_ab_d.harness.v1",
        "saw_labels_before_commit": False,
        "arm_isolated": True,
        "read_budget": None,
        "approx_tokens": None,
        "lab_ready_claim": False,
        "product_policy": True,
    }
    fs_path.write_text(json.dumps(fs, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    meta_path.write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return {"status": "ok", "task": task, "runner": runner, "arm": arm, "seed": seed, "file_set": file_set}


def build_prompt(task: str, arm: str, seed: int, runner: str, model: str, issue: str) -> str:
    policy = (
        "ARM B: do NOT use agentgraph. Use listing / reading / grep only. "
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
    model = RUNNERS.get(runner, "xiaomi/mimo-v2.6-pro")
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
    runners = args.runner or ([args.only_runner] if args.only_runner else list(RUNNERS))
    log: List[Dict[str, Any]] = []
    for seed in seeds:
        for runner in runners:
            for arm in ARMS:
                for task in args.task or TASKS:
                    print(f"CELL {runner} {arm} s{seed} {task}", flush=True)
                    if arm == "A":
                        rec = run_product_arm_a(runner, arm, seed, task, args.timeout)
                    else:
                        rec = run_cell(runner, arm, seed, task, args.timeout)
                    print("  ->", rec.get("status"), rec.get("seconds", rec.get("file_set", "")), flush=True)
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
    r.add_argument("--timeout", type=int, default=280)
    r.add_argument("--force", action="store_true")
    r.add_argument("--allow-partial", action="store_true")
    r.set_defaults(func=cmd_run)
    sc = sub.add_parser("score")
    sc.set_defaults(func=cmd_score)
    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
