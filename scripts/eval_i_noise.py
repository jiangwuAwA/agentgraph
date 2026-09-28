#!/usr/bin/env python3
"""I-track budgeted noise scorer (additive; never rewrites H4/P0 scores).

Noise definition (I1):
- extra-noise = every file in file_set that is **not** in expected.files_that_matter
  (whether or not it appears in the fixture noise list).
- file_budget K from task.json (default 8); over-budget files still count as noise.
- precision@K = |file_set ∩ expected| / max(1, min(K, |file_set|))

Usage:
  python scripts/eval_i_noise.py score --traj-dir evals/agent-ab-noise
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any, Dict, List


def _norm(p: str) -> str:
    return p.replace("\\", "/").lstrip("./")


def score_one(file_set: List[str], expected: List[str], budget: int) -> Dict[str, Any]:
    found = [_norm(x) for x in file_set if x]
    exp = {_norm(x) for x in expected}
    hit = [f for f in found if f in exp]
    extra = [f for f in found if f not in exp]
    k = max(1, budget)
    prec = len(hit) / max(1, min(k, len(found))) if found else 0.0
    rec = len(hit) / max(1, len(exp))
    return {
        "expected_file_recall": round(rec, 4),
        "extra_noise_count": len(extra),
        "extra_noise_files": extra[:20],
        "file_set_size": len(found),
        "file_budget": budget,
        "budget_exceeded": len(found) > budget,
        "precision_at_k": round(prec, 4),
    }


def load_task(task_id: str) -> Dict[str, Any]:
    for base in (
        Path("fixtures/eval-i-noise") / task_id,
        Path("fixtures/eval-agent-tasks-hard") / task_id,
        Path("fixtures/eval-agent-tasks") / task_id,
    ):
        p = base / "task.json"
        if p.is_file():
            return json.loads(p.read_text(encoding="utf-8"))
    raise SystemExit(f"missing task.json for {task_id}")


def cmd_score(args: argparse.Namespace) -> int:
    traj = Path(args.traj_dir)
    if not traj.is_dir():
        print(f"missing traj dir {traj}", file=sys.stderr)
        return 2
    rows = []
    by = defaultdict(list)
    for fp in sorted(traj.rglob("file_set.json")):
        obj = json.loads(fp.read_text(encoding="utf-8-sig"))
        task_id = obj.get("task_id") or fp.parent.name
        task = load_task(task_id)
        exp = (task.get("expected") or {}).get("files_that_matter") or []
        budget = int((task.get("expected") or {}).get("file_budget") or 8)
        sc = score_one(list(obj.get("file_set") or []), exp, budget)
        rec = {
            "task_id": task_id,
            "runner_id": obj.get("runner_id"),
            "arm": obj.get("arm"),
            "seed": obj.get("seed"),
            "score": sc,
            "path": str(fp),
        }
        rows.append(rec)
        key = (obj.get("runner_id"), obj.get("arm"))
        by[key].append(sc)

    summary = {}
    for (runner, arm), scs in sorted(by.items()):
        summary[f"{runner}:{arm}"] = {
            "runner_id": runner,
            "arm": arm,
            "n": len(scs),
            "mean_expected_file_recall": round(
                sum(s["expected_file_recall"] for s in scs) / max(1, len(scs)), 4
            ),
            "mean_extra_noise_count": round(
                sum(s["extra_noise_count"] for s in scs) / max(1, len(scs)), 4
            ),
            "mean_file_set_size": round(sum(s["file_set_size"] for s in scs) / max(1, len(scs)), 4),
            "mean_precision_at_k": round(sum(s["precision_at_k"] for s in scs) / max(1, len(scs)), 4),
            "budget_exceeded_runs": sum(1 for s in scs if s["budget_exceeded"]),
        }

    # A vs B noise delta if both present
    deltas = []
    runners = sorted({k[0] for k in by})
    for runner in runners:
        a = summary.get(f"{runner}:A")
        b = summary.get(f"{runner}:B")
        if a and b:
            deltas.append(
                {
                    "runner_id": runner,
                    "noise_A": a["mean_extra_noise_count"],
                    "noise_B": b["mean_extra_noise_count"],
                    "noise_delta_B_minus_A": round(
                        b["mean_extra_noise_count"] - a["mean_extra_noise_count"], 4
                    ),
                    "recall_A": a["mean_expected_file_recall"],
                    "recall_B": b["mean_expected_file_recall"],
                    "noise_advantage_A": a["mean_extra_noise_count"] < b["mean_extra_noise_count"],
                }
            )

    out = {
        "schema": "agentgraph.eval_i_noise.score.v1",
        "traj_dir": str(traj),
        "n_rows": len(rows),
        "summary_by_runner_arm": summary,
        "a_vs_b": deltas,
        "honesty": {
            "noise_definition": "file_set minus expected.files_that_matter (all non-expected)",
            "no_score_forgery": True,
            "historical_scores_not_rewritten": True,
        },
    }
    dest = Path(args.out or (Path("target") / "eval_i_noise_score.json"))
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(out, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"summary_by_runner_arm": summary, "a_vs_b": deltas}, indent=2, ensure_ascii=False))
    print(f"wrote {dest}")
    return 0


def main(argv: List[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="I-track budgeted noise scorer")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sc = sub.add_parser("score")
    sc.add_argument("--traj-dir", default="evals/agent-ab-noise")
    sc.add_argument("--out", default="")
    sc.set_defaults(func=cmd_score)
    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
