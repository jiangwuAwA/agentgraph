#!/usr/bin/env python3
"""Sample heuristic edges with source context for manual classification."""
from __future__ import annotations

import json
import random
import sqlite3
import sys
from pathlib import Path


def main() -> None:
    root = Path(sys.argv[1])
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 20
    db = root / ".agentgraph" / "index.db"
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    rows = list(
        conn.execute(
            "SELECT id, name, path, line, enclosing, rule_id, evidence, qualifier "
            "FROM refs WHERE confidence IN ('heuristic','dynamic_candidate')"
        )
    )
    # stratify: prefer non-impl_trait if any, else random across all
    non_trait = [r for r in rows if r["rule_id"] != "rs.di.impl_trait"]
    trait = [r for r in rows if r["rule_id"] == "rs.di.impl_trait"]
    rng = random.Random(42)
    picks = list(non_trait)
    if len(picks) < n:
        need = n - len(picks)
        picks.extend(rng.sample(trait, min(need, len(trait))))
    else:
        picks = rng.sample(picks, n)

    for r in picks:
        path = root / r["path"]
        line_no = r["line"]
        context = ""
        if path.exists():
            try:
                text = path.read_text(encoding="utf-8", errors="replace").splitlines()
                lo = max(0, line_no - 2)
                hi = min(len(text), line_no + 2)
                context = "\n".join(f"{i+1}: {text[i]}" for i in range(lo, hi))
            except Exception as e:
                context = f"<err {e}>"
        ev = r["evidence"]
        print("=" * 72)
        print(json.dumps({k: r[k] for k in ("id", "name", "path", "line", "enclosing", "rule_id", "qualifier")}, ensure_ascii=False))
        print(f"evidence={ev}")
        print(context)
    conn.close()


if __name__ == "__main__":
    main()
