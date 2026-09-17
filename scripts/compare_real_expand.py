import sqlite3
import os
import sys

base = r"D:\projects\eval-corpus\stock-trading-app-expanded"
pairs = [
    ("source", os.path.join(base, "real-source-view")),
    ("expanded", os.path.join(base, "real-expanded-view")),
]
for label, root in pairs:
    db = os.path.join(root, ".agentgraph", "index.db")
    if not os.path.exists(db):
        print(label, "NO DB at", db)
        continue
    c = sqlite3.connect(db)
    files = c.execute("select count(*) from files").fetchone()[0]
    syms = c.execute("select count(*) from symbols").fetchone()[0]
    refs = c.execute("select count(*) from refs").fetchone()[0]
    conf = dict(c.execute("select confidence, count(*) from refs group by 1").fetchall())
    top = c.execute(
        "select name, count(*) n from refs group by 1 order by n desc limit 20"
    ).fetchall()
    print(f"{label}: files={files} symbols={syms} refs={refs} conf={conf}")
    print("  top refs:")
    for name, n in top:
        print(f"    {n:5d}  {name}")
    # derive-like names
    for pat in ("fmt", "clone", "eq", "source", "description", "hash"):
        n = c.execute(
            "select count(*) from refs where name = ?", (pat,)
        ).fetchone()[0]
        s = c.execute(
            "select count(*) from symbols where name = ?", (pat,)
        ).fetchone()[0]
        print(f"  metric {pat}: symbols={s} refs={n}")
    c.close()
    print()
