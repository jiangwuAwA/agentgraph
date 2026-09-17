#!/usr/bin/env python3
from __future__ import annotations
import sqlite3
from pathlib import Path

db = Path(r"D:\projects\eval-corpus\stock-trading-app\.agentgraph\index.db")
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
conn.row_factory = sqlite3.Row

for sym in [
    "registered_strategies",
    "StrategyRegistration",
    "inventory",
    "MomentumRotationStrategy",
    "generate",
    "SignalGenerator",
    "build_default_pipeline",
    "available_strategies",
    "run_registered_fusion",
    "last_return",
    "expose_secret",
    "validate",
    "digest",
    "constant_time_eq",
    "fmt",
    "drop",
]:
    print(f"\n======== {sym} ========")
    for row in conn.execute(
        "SELECT name, qualified_name, kind, path, start_line FROM symbols WHERE name=? LIMIT 8",
        (sym,),
    ):
        print(f"  DEF {row['kind']:15} {row['path']}:{row['start_line']} q={row['qualified_name']}")
    for row in conn.execute(
        """SELECT confidence, kind, rule_id, COUNT(*) c FROM refs WHERE name=?
           GROUP BY confidence, kind, rule_id ORDER BY c DESC""",
        (sym,),
    ):
        print(f"  REF {row['confidence']:12} kind={row['kind']:12} rule={row['rule_id'] or '-':20} n={row['c']}")
    for row in conn.execute(
        "SELECT path, line, enclosing, confidence, rule_id FROM refs WHERE name=? LIMIT 8",
        (sym,),
    ):
        print(f"      {row['confidence']:10} {row['path']}:{row['line']} enc={row['enclosing']} rule={row['rule_id']}")

conn.close()
