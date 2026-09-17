#!/usr/bin/env python3
from __future__ import annotations
import sqlite3
from pathlib import Path

db = Path(r"D:\projects\eval-corpus\stock-trading-app\.agentgraph\index.db")
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
conn.row_factory = sqlite3.Row

print("=== parse_error paths ===")
for row in conn.execute("SELECT path, kind, line, snippet FROM subset_violations WHERE kind='parse_error' ORDER BY path"):
    print(f"{row['path']}:{row['line']}  {row['snippet'][:80]!r}")

print("\n=== unsafe paths (by crate) ===")
for row in conn.execute(
    """SELECT path, COUNT(*) c FROM subset_violations WHERE kind='unsafe' GROUP BY path ORDER BY c DESC"""
):
    print(f"{row['c']:3}  {row['path']}")

print("\n=== model-selection-storage unsafe vs source ===")
for row in conn.execute(
    "SELECT path, kind, line, snippet FROM subset_violations WHERE path LIKE '%model-selection-storage%'"
):
    print(dict(row))

print("\n=== strategy-library violations ===")
for row in conn.execute(
    "SELECT path, kind, line, snippet FROM subset_violations WHERE path LIKE '%strategy-library%'"
):
    print(dict(row))

print("\n=== scheduler violations ===")
for row in conn.execute(
    "SELECT path, kind, line, snippet FROM subset_violations WHERE path LIKE '%scheduler%'"
):
    print(dict(row))

conn.close()
