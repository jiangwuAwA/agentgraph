#!/usr/bin/env python3
"""Read-only SQLite stats for L1 sampling eval (operator-run; not committed as product code)."""
from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path


def open_ro(db: Path) -> sqlite3.Connection:
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def main() -> None:
    root = Path(sys.argv[1])
    db = root / ".agentgraph" / "index.db"
    conn = open_ro(db)
    print(f"DB={db}")
    print("=== confidence x rule_id ===")
    for row in conn.execute(
        "SELECT confidence, COALESCE(rule_id,'(null)') AS rid, COUNT(*) AS c "
        "FROM refs GROUP BY confidence, rid ORDER BY c DESC"
    ):
        print(f"{row['confidence']}\t{row['rid']}\t{row['c']}")

    print("=== heuristic edges (full sample for noise review) ===")
    rows = list(
        conn.execute(
            "SELECT id, name, path, line, enclosing, rule_id, evidence, qualifier "
            "FROM refs WHERE confidence='heuristic' ORDER BY id"
        )
    )
    print(f"heuristic_total={len(rows)}")
    # dump first N as JSON lines for classification
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 40
    for row in rows[:n]:
        print(json.dumps(dict(row), ensure_ascii=False))

    print("=== dynamic_candidate edges ===")
    for row in conn.execute(
        "SELECT id, name, path, line, enclosing, rule_id, evidence "
        "FROM refs WHERE confidence='dynamic_candidate' ORDER BY id"
    ):
        print(json.dumps(dict(row), ensure_ascii=False))

    print("=== top heuristic names ===")
    for row in conn.execute(
        "SELECT name, rule_id, COUNT(*) AS c FROM refs WHERE confidence='heuristic' "
        "GROUP BY name, rule_id ORDER BY c DESC LIMIT 25"
    ):
        print(f"{row['name']}\t{row['rule_id']}\t{row['c']}")

    print("=== symbols with most heuristic inbound refs ===")
    for row in conn.execute(
        "SELECT r.name, COUNT(*) AS c FROM refs r WHERE r.confidence='heuristic' "
        "GROUP BY r.name ORDER BY c DESC LIMIT 20"
    ):
        print(f"{row['name']}\t{row['c']}")

    conn.close()


if __name__ == "__main__":
    main()
