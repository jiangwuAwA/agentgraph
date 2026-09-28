#!/usr/bin/env python3
"""Batch-launch H4 live cells via detached `mimo run` (avoids shell kill)."""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TRAJ = ROOT / "evals" / "agent-ab-h4"
MIMO = r"C:\Users\Administrator\.mimocode\bin\mimo.exe"
AG_BIN = ROOT / "target" / "debug" / "agentgraph.exe"
if not AG_BIN.is_file():
    AG_BIN = ROOT / "target" / "release" / "agentgraph.exe"

TASKS = [
    "ts-dense-alias-noise",
    "ts-multi-root-client",
    "rust-real-noise-dense",
    "rust-cross-crate-blast",
    "rust-sound-scoped-clean",
    "ts-nest-user-repo",
    "rust-trait-handler",
    "py-plugin-registry",
    "go-store-api",
]
RUNNERS = {
    "external_live_runner_1": "xiaomi/mimo-v2.6-pro",
    "external_live_runner_2": "xiaomi/mimo-v2.6-flash",
}
ARMS = ("A", "B")


def cells(seeds):
    for seed in seeds:
        for runner in RUNNERS:
            for arm in ARMS:
                for task in TASKS:
                    yield runner, arm, seed, task


def brief_dir(runner, arm, seed, task):
    return TRAJ / "_briefs" / runner / arm / str(seed) / task


def has_outputs(bdir: Path) -> bool:
    return (bdir / "file_set.json").is_file() and (bdir / "meta.json").is_file()


def promote(bdir: Path):
    wd = bdir / "workdir"
    for name in ("file_set.json", "meta.json"):
        src = wd / name
        dst = bdir / name
        if src.is_file() and not dst.is_file():
            shutil.copy2(src, dst)
        elif src.is_file() and dst.is_file():
            # prefer newer workdir copy if brief empty-ish
            if dst.stat().st_size < 10:
                shutil.copy2(src, dst)


def launch_cell(runner, arm, seed, task, model):
    bdir = brief_dir(runner, arm, seed, task)
    issue_path = bdir / "ISSUE.md"
    workdir = bdir / "workdir"
    if not issue_path.is_file() or not workdir.is_dir():
        return None
    if has_outputs(bdir):
        promote(bdir)
        return "skip"
    issue = issue_path.read_text(encoding="utf-8")
    policy = (
        "ARM A: you may use the agentgraph CLI (on PATH as agentgraph) plus reads. "
        if arm == "A"
        else "ARM B: do NOT use agentgraph. Use only listing/reading/grep. "
    )
    prompt = (
        f"You are an independent code-understanding agent (lab cell). {policy}"
        f"Issue:\n{issue}\n"
        "Write file_set.json and meta.json into the current workdir. "
        "file_set schema agentgraph.eval_agent_ab_d.file_set.v1 "
        "(task_id, runner_id, arm, seed, file_set, tool_calls, chose_correct_workspace_root, approx_tokens). "
        "meta schema agentgraph.eval_agent_ab_d.meta.v1 kind=live_llm_agent "
        f"model_note={model} independent_session=true saw_labels_before_commit=false "
        "arm_isolated=true lab_ready_claim=false. "
        "approx_tokens null unless truly measured. "
        "Do not invent labels. Stop after writing both files."
    )
    out = TRAJ / "_logs" / f"{runner}-{arm}-s{seed}-{task}.jsonl"
    err = TRAJ / "_logs" / f"{runner}-{arm}-s{seed}-{task}.err"
    out.parent.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["PATH"] = str(AG_BIN.parent) + os.pathsep + env.get("PATH", "")
    fo = open(out, "w", encoding="utf-8", errors="replace")
    fe = open(err, "w", encoding="utf-8", errors="replace")
    proc = subprocess.Popen(
        [MIMO, "run", "--model", model, "--dir", str(workdir), "--format", "json", prompt],
        stdout=fo,
        stderr=fe,
        cwd=str(workdir),
        env=env,
    )
    return proc


def main():
    seeds = [int(x) for x in (sys.argv[1].split(",") if len(sys.argv) > 1 else ["0", "1", "2", "3", "4"])]
    max_parallel = int(sys.argv[2]) if len(sys.argv) > 2 else 4
    pending = []
    for runner, arm, seed, task in cells(seeds):
        model = RUNNERS[runner]
        bdir = brief_dir(runner, arm, seed, task)
        if has_outputs(bdir):
            promote(bdir)
            continue
        pending.append((runner, arm, seed, task, model))
    print(f"pending {len(pending)} cells", flush=True)
    running = []
    done = ok = 0
    i = 0
    while i < len(pending) or running:
        while len(running) < max_parallel and i < len(pending):
            runner, arm, seed, task, model = pending[i]
            i += 1
            print(f"LAUNCH {runner} {arm} s{seed} {task}", flush=True)
            proc = launch_cell(runner, arm, seed, task, model)
            if proc is None:
                print("  missing brief", flush=True)
                done += 1
                continue
            if proc == "skip":
                print("  skip existing", flush=True)
                done += 1
                ok += 1
                continue
            running.append((runner, arm, seed, task, proc))
        time.sleep(8)
        still = []
        for runner, arm, seed, task, proc in running:
            bdir = brief_dir(runner, arm, seed, task)
            promote(bdir)
            if has_outputs(bdir):
                print(f"  DONE {runner} {arm} s{seed} {task}", flush=True)
                done += 1
                ok += 1
                try:
                    proc.terminate()
                except Exception:
                    pass
            elif proc.poll() is not None:
                promote(bdir)
                status = "ok" if has_outputs(bdir) else f"exited_{proc.returncode}"
                print(f"  END {runner} {arm} s{seed} {task} {status}", flush=True)
                done += 1
                if has_outputs(bdir):
                    ok += 1
            else:
                still.append((runner, arm, seed, task, proc))
        running = still
        print(f"progress done={done} ok={ok} running={len(running)}", flush=True)
    print(f"finished done={done} ok={ok}", flush=True)
    return 0 if ok == done else 1


if __name__ == "__main__":
    raise SystemExit(main())
