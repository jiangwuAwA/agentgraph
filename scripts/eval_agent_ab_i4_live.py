#!/usr/bin/env python3
"""I4 full-matrix live runner (evals/agent-ab-i4)."""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval_i_noise_live as base  # noqa: E402

base.TRAJ = Path(__file__).resolve().parents[1] / "evals" / "agent-ab-i4"
base.TASKS = [
    "i-order-pipeline",
    "i-cache-registry",
    "ts-dense-alias-noise",
    "ts-multi-root-client",
]
base.RUNNERS = {"i4_live_runner_1": "xiaomi/mimo-v2.6-pro"}
base.FILE_BUDGET = 5


def _prepare(args):
    _tasks = base.TASKS
    base.TASKS = ["i-order-pipeline", "i-cache-registry"]
    rc = base.cmd_prepare(args)
    base.TASKS = _tasks
    hard = Path(__file__).resolve().parents[1] / "fixtures" / "eval-agent-tasks-hard"
    for seed in range(5):
        for arm in base.ARMS:
            for task in ["ts-dense-alias-noise", "ts-multi-root-client"]:
                bdir = base.TRAJ / "_briefs" / "i4_live_runner_1" / arm / str(seed) / task
                bdir.mkdir(parents=True, exist_ok=True)
                src = hard / task
                wd = bdir / "workdir"
                if wd.exists():
                    shutil.rmtree(wd, ignore_errors=True)
                shutil.copytree(src, wd)
                (wd / "task.json").unlink(missing_ok=True)
                tj = json.loads((src / "task.json").read_text(encoding="utf-8"))
                issue = (
                    f"# Issue brief — {task}\n\n- task: `{task}`\n"
                    f"- runner_id: i4_live_runner_1\n- arm: **{arm}**\n- seed: `{seed}`\n\n"
                    f"## Issue (no golden list)\n\n{tj.get('issue', '')}\n\n"
                    f"## Constraints\n\n- Return **at most 5 files**.\n- Prefer precision.\n"
                )
                (bdir / "ISSUE.md").write_text(issue, encoding="utf-8")
    return rc


def _score(args):
    return subprocess.run(
        [
            sys.executable,
            str(Path(__file__).resolve().parent / "eval_i_noise.py"),
            "score",
            "--traj-dir",
            str(base.TRAJ),
        ],
        cwd=str(Path(__file__).resolve().parents[1]),
    ).returncode


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("--seeds", default="0-4")
    p.add_argument("--runner", action="append")
    p.set_defaults(func=_prepare)
    r = sub.add_parser("run")
    r.add_argument("--seeds", default="0-4")
    r.add_argument("--runner", action="append")
    r.add_argument("--only-runner", default="")
    r.add_argument("--task", action="append")
    r.add_argument("--timeout", type=int, default=280)
    r.add_argument("--force", action="store_true")
    r.add_argument("--allow-partial", action="store_true")
    r.set_defaults(func=base.cmd_run)
    sc = sub.add_parser("score")
    sc.set_defaults(func=_score)
    args = ap.parse_args()
    raise SystemExit(args.func(args))
