#!/usr/bin/env python3
from __future__ import annotations
import sqlite3
from pathlib import Path

db = Path(r"D:\projects\eval-corpus\stock-trading-app\.agentgraph\index.db")
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
conn.row_factory = sqlite3.Row

print("=== authorize refs in security_core.rs ===")
for row in conn.execute(
    "SELECT name, path, line, enclosing, confidence, kind, evidence FROM refs WHERE name='authorize' AND path LIKE '%security_core%'"
):
    print(dict(row))

print("\n=== authorize all sample ===")
for row in conn.execute(
    "SELECT path, line, enclosing, confidence, kind FROM refs WHERE name='authorize' LIMIT 15"
):
    print(dict(row))

print("\n=== create refs in trading/persist.rs ===")
for row in conn.execute(
    "SELECT name, path, line, enclosing, confidence, kind FROM refs WHERE path LIKE '%trading/src/persist%' AND name IN ('create','upsert','is_transient_db_err','spawn') LIMIT 30"
):
    print(dict(row))

print("\n=== any ref on persist.rs ===")
for row in conn.execute(
    "SELECT name, line, enclosing, confidence, kind FROM refs WHERE path LIKE '%trading/src/persist%' LIMIT 40"
):
    print(f"  {row['confidence']:8} {row['kind']:10} {row['name']}:{row['line']} enc={row['enclosing']}")

print("\n=== symbols defined in persist.rs ===")
for row in conn.execute(
    "SELECT name, kind, start_line FROM symbols WHERE path LIKE '%trading/src/persist%' LIMIT 30"
):
    print(dict(row))

conn.close()
