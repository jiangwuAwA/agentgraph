#!/usr/bin/env python3
"""Dump all refs + symbols for nestjs-starter (small tree)."""
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
    print("=== symbols ===")
    for row in conn.execute(
        "SELECT name, qualified_name, kind, path, start_line FROM symbols ORDER BY path, start_line"
    ):
        print(json.dumps(dict(row), ensure_ascii=False))
    print("=== refs by confidence ===")
    for row in conn.execute(
        "SELECT confidence, COALESCE(rule_id,'(null)') rid, kind, COUNT(*) c "
        "FROM refs GROUP BY confidence, rid, kind ORDER BY c DESC"
    ):
        print(f"{row['confidence']}\t{row['rid']}\t{row['kind']}\t{row['c']}")
    print("=== all refs ===")
    for row in conn.execute(
        "SELECT name, kind, path, line, enclosing, confidence, rule_id, evidence, qualifier "
        "FROM refs ORDER BY path, line"
    ):
        print(json.dumps(dict(row), ensure_ascii=False))
    conn.close()


if __name__ == "__main__":
    main()
