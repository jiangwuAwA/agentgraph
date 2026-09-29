#!/usr/bin/env python3
"""Rust call tracer MVP (R-track) — run a fixture bin and collect JSONL edges.

The fixture records edges via `rr_edge` probes into `RR_TRACE`.
This script builds/runs the fixture (or accepts an existing trace) and prints
a summary. Same claim class as `scripts/py_trace.py` / `diff_trace.cjs`:
**direct call edges on the covered path only**.

Usage:
  python scripts/rs_trace.py fixtures/eval-runtime-recall --out target/rr_trace.jsonl
  python scripts/rs_trace.py --from-trace target/rr_trace.jsonl
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import List


def run_fixture(root: Path, out: Path) -> int:
    out.parent.mkdir(parents=True, exist_ok=True)
    if out.exists():
        out.unlink()
    env = os.environ.copy()
    env["RR_TRACE"] = str(out.resolve())
    # Build + run the fixture binary (cargo in fixture dir).
    build = subprocess.run(
        ["cargo", "build", "--quiet"],
        cwd=str(root),
        env=env,
        capture_output=True,
        text=True,
    )
    if build.returncode != 0:
        print(build.stderr, file=sys.stderr)
        print("cargo build failed", file=sys.stderr)
        return 1
    # Prefer explicit bin name from Cargo.toml (rr-main).
    exe = None
    for cand in (
        root / "target" / "debug" / "rr-main",
        root / "target" / "debug" / "rr-main.exe",
        root / "target" / "debug" / "runtime-recall-fixture",
    ):
        if cand.is_file():
            exe = cand
            break
    if exe is None:
        print("fixture binary not found", file=sys.stderr)
        return 1
    run = subprocess.run([str(exe)], cwd=str(root), env=env, capture_output=True, text=True)
    if run.returncode != 0:
        print(run.stderr, file=sys.stderr)
        print("fixture run failed", file=sys.stderr)
        return 1
    if not out.is_file():
        print(f"no trace written: {out}", file=sys.stderr)
        return 1
    n = sum(1 for line in out.read_text(encoding="utf-8").splitlines() if line.strip())
    print(f"traced {n} edge(s) -> {out}")
    return 0


def summarize(path: Path) -> int:
    edges = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            edges.append(json.loads(line))
        except json.JSONDecodeError as e:
            print(f"skip bad line: {e}", file=sys.stderr)
    print(json.dumps({"entry": "fixture", "edges": edges}, indent=2, ensure_ascii=False))
    return 0


def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser(description="Rust call tracer MVP (RR_TRACE probe fixture)")
    ap.add_argument("fixture", nargs="?", default="fixtures/eval-runtime-recall")
    ap.add_argument("--out", default="target/rr_trace.jsonl")
    ap.add_argument("--from-trace", dest="from_trace", default="")
    args = ap.parse_args(argv)
    if args.from_trace:
        return summarize(Path(args.from_trace))
    return run_fixture(Path(args.fixture), Path(args.out))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
