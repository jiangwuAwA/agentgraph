#!/usr/bin/env python3
"""Find call sites for golden labeling in stock-trading-app."""
from __future__ import annotations
import sqlite3
from pathlib import Path

db = Path(r"D:\projects\eval-corpus\stock-trading-app\.agentgraph\index.db")
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
conn.row_factory = sqlite3.Row

symbols_of_interest = [
    "normalize_freq",
    "RepoRegistry",
    "PgKlineRepo",
    "latest_close",
    "query_range",
    "find_by_code",
    "decide",
    "evolve",
    "KlineRepository",
    "insert",
    "upsert",
    "submit",
    "cancel",
]

for sym in symbols_of_interest:
    print(f"\n======== SYMBOL {sym} ========")
    print("--- symbols table ---")
    for row in conn.execute(
        "SELECT name, qualified_name, kind, path, start_line FROM symbols WHERE name=? LIMIT 20",
        (sym,),
    ):
        print(f"  DEF {row['kind']:20} {row['path']}:{row['start_line']}  q={row['qualified_name']}")
    print("--- refs (confidence groups) ---")
    for row in conn.execute(
        """SELECT confidence, kind, rule_id, COUNT(*) c
           FROM refs WHERE name=? GROUP BY confidence, kind, rule_id ORDER BY c DESC""",
        (sym,),
    ):
        print(f"  {row['confidence']:20} kind={row['kind']:15} rule={row['rule_id'] or '-':20} n={row['c']}")
    print("--- sample refs ---")
    for row in conn.execute(
        "SELECT name, kind, path, line, enclosing, confidence, rule_id, evidence FROM refs WHERE name=? LIMIT 12",
        (sym,),
    ):
        ev = (row["evidence"] or "")[:60].replace("\n", " ")
        print(f"  {row['confidence']:10} {row['path']}:{row['line']} enc={row['enclosing']} kind={row['kind']} rule={row['rule_id']} | {ev}")

conn.close()
