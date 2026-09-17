#!/usr/bin/env python3
from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path


def main() -> None:
    root = Path(sys.argv[1])
    db = root / ".agentgraph" / "index.db"
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    print("=== subset violation kinds ===")
    for row in conn.execute(
        "SELECT kind, COUNT(*) AS c FROM subset_violations GROUP BY kind ORDER BY c DESC"
    ):
        print(f"{row['kind']}\t{row['c']}")
    print(
        "TOTAL",
        conn.execute("SELECT COUNT(*) FROM subset_violations").fetchone()[0],
    )
    print("=== FromRef heuristic ===")
    for row in conn.execute(
        "SELECT name, path, line, enclosing, evidence FROM refs "
        "WHERE confidence='heuristic' AND name='from_ref' LIMIT 12"
    ):
        print(json.dumps(dict(row), ensure_ascii=False))
    print("=== high-name-collision defaults ===")
    for row in conn.execute(
        "SELECT name, path, line, enclosing, evidence FROM refs "
        "WHERE confidence='heuristic' AND name IN ('default','fmt','drop') "
        "ORDER BY RANDOM() LIMIT 8"
    ):
        print(json.dumps(dict(row), ensure_ascii=False))
    conn.close()


if __name__ == "__main__":
    main()
