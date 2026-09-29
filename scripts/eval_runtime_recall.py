#!/usr/bin/env python3
"""Runtime recall gate (R-track): trace × static graph − gap ledger.

miss = trace edges not in static graph and not in gap ledger.
recall@covered = 1 - miss / |trace| (0 miss => 1.0).

Usage:
  python scripts/eval_runtime_recall.py --trace target/rr_trace.jsonl \\
    --db fixtures/eval-runtime-recall/.agentgraph/index.db \\
    --gap fixtures/eval-runtime-recall/gap_ledger.json \\
    --out target/runtime_recall.json

Honesty: covered paths only — not production absolute zero-miss.
"""
from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple


def norm_sym(s: str) -> str:
    return s.replace("\\", "/").strip()


def edge_key(frm: str, to: str) -> Tuple[str, str]:
    return (norm_sym(frm), norm_sym(to))


def load_trace(path: Path) -> List[Dict[str, Any]]:
    edges = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if obj.get("from") and obj.get("to"):
            edges.append(obj)
    return edges


def load_gap(path: Optional[Path]) -> List[Dict[str, Any]]:
    if not path or not path.is_file():
        return []
    obj = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(obj, list):
        return obj
    return list(obj.get("edges") or [])


def static_edge_names(db: Path) -> Set[Tuple[str, str]]:
    """Best-effort static edge set from refs: (enclosing_or_path, name) plus (module, name)."""
    if not db.is_file():
        return set()
    con = sqlite3.connect(str(db))
    edges: Set[Tuple[str, str]] = set()
    try:
        rows = con.execute(
            "SELECT name, enclosing, module, path, kind FROM refs"
        ).fetchall()
        for name, enclosing, module, path, kind in rows:
            n = norm_sym(name or "")
            if not n:
                continue
            for left in (enclosing, module, path):
                if left:
                    edges.add((norm_sym(str(left)), n))
            # also bare last-segment matches
            if enclosing:
                seg = norm_sym(str(enclosing)).split("/")[-1]
                edges.add((seg, n))
    finally:
        con.close()
    return edges


def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser(description="Runtime recall gate")
    ap.add_argument("--trace", required=True)
    ap.add_argument("--db", required=True)
    ap.add_argument("--gap", default="")
    ap.add_argument("--out", default="target/runtime_recall.json")
    args = ap.parse_args(argv)

    trace_path = Path(args.trace)
    db = Path(args.db)
    gap_path = Path(args.gap) if args.gap else None
    if not trace_path.is_file():
        print(f"missing trace: {trace_path}", file=sys.stderr)
        return 2

    trace = load_trace(trace_path)
    static = static_edge_names(db)
    gap = load_gap(gap_path)
    gap_keys = {edge_key(e.get("from", ""), e.get("to", "")) for e in gap}

    missed: List[Dict[str, Any]] = []
    gap_used: List[Dict[str, Any]] = []
    static_hits = 0
    for e in trace:
        k = edge_key(e.get("from", ""), e.get("to", ""))
        if k in gap_keys:
            gap_used.append(e)
            continue
        # static match: exact pair, or callee name appears with any recorded left
        # (caller enclosing / module / path may not equal runtime from-path).
        to_last = k[1].split("::")[-1]
        from_last = k[0].split("::")[-1]
        hit = False
        for s_from, s_to in static:
            s_to_last = s_to.split("::")[-1]
            s_from_last = s_from.split("::")[-1]
            if s_to_last == to_last and (s_from_last == from_last or s_from == k[0]):
                hit = True
                break
            if s_to == k[1] and (s_from == k[0] or s_from_last == from_last):
                hit = True
                break
        if hit:
            static_hits += 1
            continue
        missed.append(e)

    n_trace = len(trace)
    n_miss = len(missed)
    recall = 1.0 if n_trace == 0 else round(1.0 - (n_miss / n_trace), 6)

    out = {
        "schema": "agentgraph.eval_runtime_recall.report.v1",
        "trace_edges": n_trace,
        "static_hits": static_hits,
        "gap_used": len(gap_used),
        "missed_count": n_miss,
        "missed": missed,
        "recall_at_covered": recall,
        "zero_miss": n_miss == 0,
        "honesty": {
            "claim": "covered paths zero-miss / measured recall",
            "non_claim": "not production absolute zero-miss / not ecosystem sound",
        },
    }
    dest = Path(args.out)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(out, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({k: out[k] for k in ("trace_edges", "static_hits", "gap_used", "missed_count", "recall_at_covered", "zero_miss")}, indent=2))
    print(f"wrote {dest}")
    return 0 if n_miss == 0 or all(any(edge_key(e.get('from',''), e.get('to','')) == g for g in gap_keys) for e in missed) else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
