#!/usr/bin/env python3
from __future__ import annotations
import sqlite3
from pathlib import Path

db = Path(r"D:\projects\eval-corpus\stock-trading-app\.agentgraph\index.db")
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
conn.row_factory = sqlite3.Row

print("=== generate heuristic by path ===")
for row in conn.execute(
    "SELECT name, path, line, enclosing, qualifier, rule_id, confidence FROM refs WHERE name='generate' AND confidence='heuristic' ORDER BY path"
):
    print(dict(row))

print("\n=== all refs on momentum_rotation ===")
for row in conn.execute(
    "SELECT name, path, line, enclosing, qualifier, confidence, rule_id, kind FROM refs WHERE path LIKE '%momentum_rotation%'"
):
    print(dict(row))

print("\n=== symbols on momentum_rotation ===")
for row in conn.execute(
    "SELECT name, kind, start_line, qualified_name FROM symbols WHERE path LIKE '%momentum_rotation%'"
):
    print(dict(row))

print("\n=== refs name like Momentum* ===")
for row in conn.execute(
    "SELECT name, path, line, enclosing, qualifier, confidence, rule_id FROM refs WHERE name LIKE 'Momentum%' OR name LIKE '%Rotation%'"
):
    print(dict(row))

print("\n=== impl_trait heuristic count by path prefix strategy-plugins ===")
for row in conn.execute(
    "SELECT path, COUNT(*) c FROM refs WHERE rule_id='rs.di.impl_trait' AND path LIKE '%strategy-plugins%' GROUP BY path ORDER BY path"
):
    print(row["c"], row["path"])

conn.close()
