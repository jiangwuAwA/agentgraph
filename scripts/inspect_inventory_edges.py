#!/usr/bin/env python3
"""Confirm inventory_submit is sound-eligible per is_sound_eligible docs/code."""
from __future__ import annotations
import sqlite3
from pathlib import Path

db = Path(r"D:\projects\eval-corpus\stock-trading-app\.agentgraph\index.db")
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
conn.row_factory = sqlite3.Row
print("inventory_submit edges:")
for row in conn.execute(
    "SELECT name, path, line FROM refs WHERE rule_id='rs.di.inventory_submit' ORDER BY path, line"
):
    print(f"  {row['name']:30} {row['path']}:{row['line']}")
print("total", conn.execute("SELECT COUNT(*) FROM refs WHERE rule_id='rs.di.inventory_submit'").fetchone()[0])
conn.close()
