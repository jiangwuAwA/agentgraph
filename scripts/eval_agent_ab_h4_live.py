#!/usr/bin/env python3
"""H4-live external LLM runner — drives `mimo run` isolated sessions on hard tasks.

Extends P0-5d protocol (issue-only briefs → live decision → offline stamp/score).

Honesty
-------
- Each cell is a **fresh** `mimo run` process (independent_session=true).
- Models: `xiaomi/mimo-v2.6-pro` / `xiaomi/mimo-v2.6-flash` (non-author).
- Decision path never sees `task.json` labels (workdir copy strips them).
- `approx_tokens` is null unless the runner reports a real value (we leave null).
- Live A/B noise may stay ~0 / unseparated — do **not** claim live noise advantage.
- Historical P0-5c/d scores are never rewritten; H4 writes its own traj dir.

Usage
-----
  python scripts/eval_agent_ab_h4_live.py prepare --seeds 0-4
  python scripts/eval_agent_ab_h4_live.py run --seeds 0-4
  python scripts/eval_agent_ab_h4_live.py stamp
  python scripts/eval_agent_ab_h4_live.py score
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
from typing import Any, Dict, List, Optional, Sequence

_SCRIPT_DIR = Path(__file__).resolve().parent
_ROOT = _SCRIPT_DIR.parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

MIMO = r"C:\Users\Administrator\.mimocode\bin\mimo.exe"
TRAJ = _ROOT / "evals" / "agent-ab-h4"
HARD_TASKS = [
    "ts-dense-alias-noise",
    "ts-multi-root-client",
    "rust-real-noise-dense",
    "rust-cross-crate-blast",
    "rust-sound-scoped-clean",
]
RUNNERS = {
    "external_live_runner_1": {
        "model": "xiaomi/mimo-v2.6-pro",
        "kind": "live_llm_agent",
    },
    "external_live_runner_2": {
        "model": "xiaomi/mimo-v2.6-flash",
        "kind": "live_llm_agent",
    },
}
ARMS = ("A", "B")


def parse_seeds(s: str) -> List[int]:
    if "-" in s:
        a, b = s.split("-", 1)
        return list(range(int(a), int(b) + 1))
    return [int(x) for x in s.split(",") if x.strip() != ""]


def brief_dir(runner: str, arm: str, seed: int, task: str) -> Path:
    return TRAJ / "_briefs" / runner / arm / str(seed) / task


def agentgraph_bin() -> Path:
    for p in (
        _ROOT / "target" / "release" / "agentgraph.exe",
        _ROOT / "target" / "debug" / "agentgraph.exe",
    ):
        if p.is_file():
            return p
    raise SystemExit("agentgraph binary not found; cargo build first")


def cmd_prepare(args: argparse.Namespace) -> int:
    seeds = parse_seeds(args.seeds)
    # Reuse P0-5d prepare for brief packs + workdirs.
    for runner in RUNNERS:
        for seed in seeds:
            cmd = [
                sys.executable,
                str(_SCRIPT_DIR / "eval_agent_ab_d.py"),
                "prepare",
                "--traj-dir",
                str(TRAJ),
                "--runner",
                runner,
                "--seeds",
                str(seed),
            ]
            for t in HARD_TASKS:
                cmd.extend(["--task", t])
            print("prepare", runner, "seed", seed, flush=True)
            r = subprocess.run(cmd, cwd=str(_ROOT))
            if r.returncode != 0:
                return r.returncode
    # Selection note for H4
    sel = {
        "schema": "agentgraph.eval_agent_ab_h4.selection.v1",
        "hard_tasks": HARD_TASKS,
        "runners": {k: v["model"] for k, v in RUNNERS.items()},
        "seeds": seeds,
        "note": "H4-live hard slice including ts-dense-alias-noise; live LLM via mimo run isolated sessions",
    }
    (TRAJ / "h4_selection.json").write_text(
        json.dumps(sel, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print("prepared", TRAJ)
    return 0


def build_prompt(task: str, arm: str, seed: int, runner: str, model: str, issue: str) -> str:
    policy = (
        "ARM A: you may use the agentgraph CLI (on PATH as `agentgraph`) "
        "in addition to reading files. Prefer `agentgraph index --force`, "
        "`blast-radius`, `who-calls`, `find` on the workdir.\n"
        if arm == "A"
        else "ARM B: do NOT use agentgraph. Use only file listing, reading, and grep-like search.\n"
    )
    return f"""You are an independent code-understanding agent session (lab cell).

Task id: {task}
Arm: {arm} (seed {seed})
Runner: {runner} model_note={model}

{policy}

ISSUE (only this; there is no golden list):
{issue}

Workdir (isolated copy, already cwd): current directory.
1. Explore enough to decide which files matter for the issue.
2. Write `file_set.json` and `meta.json` in the brief directory (the parent of workdir).

file_set.json schema agentgraph.eval_agent_ab_d.file_set.v1:
{{
  "schema": "agentgraph.eval_agent_ab_d.file_set.v1",
  "task_id": "{task}",
  "runner_id": "{runner}",
  "arm": "{arm}",
  "seed": {seed},
  "file_set": ["relative/path.ts"],
  "tool_calls": [{{"tool": "read", "args": ["x"], "ok": true, "summary": {{}}, "note": ""}}],
  "chose_correct_workspace_root": null,
  "approx_tokens": null
}}

meta.json schema agentgraph.eval_agent_ab_d.meta.v1:
{{
  "schema": "agentgraph.eval_agent_ab_d.meta.v1",
  "task_id": "{task}",
  "runner_id": "{runner}",
  "kind": "live_llm_agent",
  "model_note": "{model}",
  "independent_session": true,
  "arm": "{arm}",
  "seed": {seed},
  "harness_version": "agentgraph.eval_agent_ab_d.harness.v1",
  "saw_labels_before_commit": false,
  "arm_isolated": true,
  "read_budget": null,
  "approx_tokens": null,
  "lab_ready_claim": false
}}

Rules:
- Do not invent expected labels or scores.
- approx_tokens stays null unless you truly measured tokens.
- saw_labels_before_commit must be false.
- Keep tool_calls a truthful summary of what you actually did.
- Write both JSON files into the current workdir, then stop with a one-line summary.
"""


def run_one_cell(runner: str, arm: str, seed: int, task: str, timeout: int) -> Dict[str, Any]:
    bdir = brief_dir(runner, arm, seed, task)
    issue_path = bdir / "ISSUE.md"
    workdir = bdir / "workdir"
    if not issue_path.is_file() or not workdir.is_dir():
        return {"status": "missing_brief", "task": task, "runner": runner, "arm": arm, "seed": seed}
    fs_path = bdir / "file_set.json"
    meta_path = bdir / "meta.json"
    if fs_path.is_file() and meta_path.is_file() and not args_force:
        return {"status": "skip_existing", "task": task}

    issue = issue_path.read_text(encoding="utf-8")
    model = RUNNERS[runner]["model"]
    prompt = build_prompt(task, arm, seed, runner, model, issue)

    env = os.environ.copy()
    ag = str(agentgraph_bin().parent)
    env["PATH"] = ag + os.pathsep + env.get("PATH", "")

    cmd = [
        MIMO,
        "run",
        "--model",
        model,
        "--dir",
        str(workdir),
        "--format",
        "json",
        prompt,
    ]
    t0 = time.time()
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(workdir),
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        rc = proc.returncode
        tail = (proc.stdout or "")[-2000:]
    except subprocess.TimeoutExpired:
        return {
            "status": "timeout",
            "task": task,
            "runner": runner,
            "arm": arm,
            "seed": seed,
            "seconds": timeout,
        }
    elapsed = time.time() - t0
    approx_tokens = None
    if proc.stdout:
        for line in proc.stdout.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            part = ev.get("part") or {}
            toks = part.get("tokens") or {}
            if isinstance(toks.get("total"), int):
                approx_tokens = toks["total"]

    for name in ("file_set.json", "meta.json"):
        if (workdir / name).is_file() and not (bdir / name).is_file():
            shutil.move(str(workdir / name), str(bdir / name))

    if approx_tokens and meta_path.is_file():
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
            if meta.get("approx_tokens") in (None, 0):
                meta["approx_tokens"] = approx_tokens
                meta_path.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
        except Exception:
            pass

    if fs_path.is_file() and not meta_path.is_file():
        try:
            fs = json.loads(fs_path.read_text(encoding="utf-8"))
        except Exception:
            fs = {}
        meta_path.write_text(
            json.dumps(
                {
                    "schema": "agentgraph.eval_agent_ab_d.meta.v1",
                    "task_id": task,
                    "runner_id": runner,
                    "kind": "live_llm_agent",
                    "model_note": model,
                    "independent_session": True,
                    "arm": arm,
                    "seed": seed,
                    "harness_version": "agentgraph.eval_agent_ab_d.harness.v1",
                    "saw_labels_before_commit": False,
                    "arm_isolated": True,
                    "read_budget": None,
                    "approx_tokens": approx_tokens,
                    "lab_ready_claim": False,
                    "metrics_ext": {
                        "mcp_or_cli_calls": len(fs.get("tool_calls") or []),
                        "approx_tokens": approx_tokens,
                    },
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

    ok = fs_path.is_file() and meta_path.is_file()
    return {
        "status": "ok" if ok else "missing_outputs",
        "rc": rc,
        "seconds": round(elapsed, 1),
        "task": task,
        "runner": runner,
        "arm": arm,
        "seed": seed,
        "has_file_set": fs_path.is_file(),
        "has_meta": meta_path.is_file(),
        "tail": tail[-400:] if not ok else "",
    }


args_force = False


def cmd_run(args: argparse.Namespace) -> int:
    global args_force
    args_force = bool(getattr(args, "force", False))
    seeds = parse_seeds(args.seeds)
    runners = args.runner or list(RUNNERS)
    tasks = args.task or HARD_TASKS
    log: List[Dict[str, Any]] = []
    only = args.only
    for seed in seeds:
        for runner in runners:
            for arm in ARMS:
                for task in tasks:
                    if only and f"{runner}/{arm}/{seed}/{task}" != only and only not in f"{runner}-{arm}-{seed}-{task}":
                        continue
                    print(f"CELL {runner} {arm} seed={seed} {task}", flush=True)
                    rec = run_one_cell(runner, arm, seed, task, args.timeout)
                    print("  ->", rec.get("status"), rec.get("seconds", ""), flush=True)
                    log.append(rec)
                    (TRAJ / "h4_run_log.json").write_text(
                        json.dumps(log, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
                    )
    ok = sum(1 for x in log if x.get("status") == "ok")
    print(f"done ok={ok}/{len(log)}")
    return 0 if ok == len(log) or args.allow_partial else 1


def cmd_stamp(_args: argparse.Namespace) -> int:
    cmd = [
        sys.executable,
        str(_SCRIPT_DIR / "eval_agent_ab_d.py"),
        "stamp",
        "--traj-dir",
        str(TRAJ),
    ]
    return subprocess.run(cmd, cwd=str(_ROOT)).returncode


def cmd_score(_args: argparse.Namespace) -> int:
    cmd = [
        sys.executable,
        str(_SCRIPT_DIR / "eval_agent_ab_d.py"),
        "score",
        "--traj-dir",
        str(TRAJ),
    ]
    return subprocess.run(cmd, cwd=str(_ROOT)).returncode


def main(argv: Optional[Sequence[str]] = None) -> int:
    ap = argparse.ArgumentParser(description="H4-live external LLM runner")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("prepare")
    p.add_argument("--seeds", default="0-4")
    p.set_defaults(func=cmd_prepare)

    r = sub.add_parser("run")
    r.add_argument("--seeds", default="0-4")
    r.add_argument("--runner", action="append")
    r.add_argument("--task", action="append")
    r.add_argument("--timeout", type=int, default=240)
    r.add_argument("--only", default="")
    r.add_argument("--force", action="store_true")
    r.add_argument("--allow-partial", action="store_true")
    r.set_defaults(func=cmd_run)

    s = sub.add_parser("stamp")
    s.set_defaults(func=cmd_stamp)
    sc = sub.add_parser("score")
    sc.set_defaults(func=cmd_score)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
