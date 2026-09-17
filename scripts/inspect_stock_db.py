#!/usr/bin/env python3
"""Inspect agentgraph index.db schema and basic counts."""
from __future__ import annotations
import sqlite3
import sys
from pathlib import Path

root = Path(sys.argv[1] if len(sys.argv) > 1 else r"D:\projects\eval-corpus\stock-trading-app")
db = root / ".agentgraph" / "index.db"
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
conn.row_factory = sqlite3.Row

print("=== tables ===")
for row in conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
    print(row[0])

print("\n=== schema (interesting) ===")
for t in ("files", "symbols", "refs", "subset_violations", "edges"):
    try:
        info = list(conn.execute(f"PRAGMA table_info({t})"))
        if info:
            print(f"\n-- {t} --")
            for c in info:
                print(f"  {c['name']}: {c['type']}")
    except Exception as e:
        print(t, e)

print("\n=== counts ===")
for t in ("files", "symbols", "refs", "subset_violations"):
    try:
        print(t, conn.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0])
    except Exception as e:
        print(t, "ERR", e)

print("\n=== files sample ===")
try:
    for row in conn.execute("SELECT * FROM files LIMIT 3"):
        print(dict(row))
except Exception as e:
    print(e)

print("\n=== subset_violations sample ===")
try:
    for row in conn.execute("SELECT * FROM subset_violations LIMIT 5"):
        print(dict(row))
except Exception as e:
    print(e)

print("\n=== subset kinds ===")
for row in conn.execute("SELECT kind, COUNT(*) c FROM subset_violations GROUP BY kind ORDER BY c DESC"):
    print(row["kind"], row["c"])

print("\n=== refs confidence ===")
try:
    for row in conn.execute("SELECT confidence, COUNT(*) c FROM refs GROUP BY confidence"):
        print(dict(row))
except Exception as e:
    print(e)

conn.close()
