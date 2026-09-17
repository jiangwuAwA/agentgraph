#!/usr/bin/env python3
"""Golden-edge L0 vs L1 measurement on stock-trading-app clean/mixed crates.

Labels are hand-written from operator source reading (private corpus; not committed).
Measurement reads `.agentgraph/index.db` refs table only (no private source committed).

Usage:
  python scripts/stock_golden_eval.py [corpus_root]
"""
from __future__ import annotations

import sqlite3
import sys
from pathlib import Path
from dataclasses import dataclass
from typing import Optional

CORPUS = Path(sys.argv[1] if len(sys.argv) > 1 else r"D:\projects\eval-corpus\stock-trading-app")
DB = CORPUS / ".agentgraph" / "index.db"


@dataclass
class Golden:
    id: str
    crate: str  # clean-repo | clean-event | clean-auth | mixed-plugins | noise
    kind: str  # direct_call | trait_impl | registry | import | noise_proxy
    name: str
    # For call/import: expected ref path substring + line (exact match on line when >0)
    path_substr: str
    line: int  # 0 = any line in path
    # For trait_impl: enclosing/type qualifier
    qualifier: Optional[str]
    # For registry: required rule_id
    rule_id: Optional[str]
    notes: str


GOLDENS: list[Golden] = [
    # ── clean crate: repository (trait + direct + registry type) ──
    Golden("R1", "clean-repo", "direct_call", "normalize_freq",
           "crates/repository/src/pg/kline.rs", 64, None, None,
           "query_range normalizes freq"),
    Golden("R2", "clean-repo", "direct_call", "normalize_freq",
           "crates/repository/src/pg/kline.rs", 74, None, None,
           "latest_close normalizes freq"),
    Golden("R3", "clean-repo", "direct_call", "normalize_freq",
           "crates/repository/src/pg/kline.rs", 125, None, None,
           "find_recent normalizes freq"),
    Golden("R4", "clean-repo", "trait_impl", "insert",
           "crates/repository/src/pg/kline.rs", 31, "PgKlineRepo", "rs.di.impl_trait",
           "impl KlineRepository for PgKlineRepo::insert"),
    Golden("R5", "clean-repo", "trait_impl", "query_range",
           "crates/repository/src/pg/kline.rs", 57, "PgKlineRepo", "rs.di.impl_trait",
           "impl KlineRepository::query_range"),
    Golden("R6", "clean-repo", "trait_impl", "latest_close",
           "crates/repository/src/pg/kline.rs", 73, "PgKlineRepo", "rs.di.impl_trait",
           "impl KlineRepository::latest_close"),
    Golden("R7", "clean-repo", "direct_call", "latest_close",
           "crates/service/src/impls/trading.rs", 41, None, None,
           "TradingServiceImpl uses repos.klines.latest_close"),
    Golden("R8", "clean-repo", "direct_call", "query_range",
           "crates/service/src/impls/market.rs", 43, None, None,
           "MarketDataServiceImpl::get_klines_range"),
    Golden("R9", "clean-repo", "direct_call", "find_by_code",
           "crates/service/src/impls/market.rs", 64, None, None,
           "get_realtime stocks.find_by_code"),
    Golden("R10", "clean-repo", "registry", "RepoRegistry",
           "crates/service/src/impls/trading.rs", 0, None, None,
           "import RepoRegistry into TradingServiceImpl (DI field Arc<RepoRegistry>)"),
    Golden("R11", "clean-repo", "import", "KlineRepository",
           "crates/repository/src/pg/kline.rs", 0, None, None,
           "trait imported by PgKlineRepo impl"),
    # ── clean crate: event-engine ──
    Golden("E1", "clean-event", "direct_call", "decide",
           "crates/event-engine/tests/order_submit.rs", 36, None, None,
           "submitted_state calls decide"),
    Golden("E2", "clean-event", "direct_call", "decide",
           "crates/event-engine/tests/order_submit.rs", 46, None, None,
           "valid_submit_emits_one_submitted_event"),
    Golden("E3", "clean-event", "direct_call", "evolve",
           "crates/event-engine/tests/order_submit.rs", 40, None, None,
           "qualified event_engine::evolve"),
    Golden("E4", "clean-event", "direct_call", "submit",
           "crates/event-engine/tests/order_submit.rs", 36, None, None,
           "local helper submit() inside decide call"),
    Golden("E5", "clean-event", "import", "decide",
           "crates/event-engine/tests/order_submit.rs", 0, None, None,
           "use event_engine::{decide, ...}"),
    # ── clean crate: auth ──
    Golden("A1", "clean-auth", "direct_call", "expose_secret",
           "crates/auth/src/token.rs", 46, None, None,
           "LocalTokenVerifier::generate calls secret.expose_secret"),
    Golden("A2", "clean-auth", "direct_call", "constant_time_eq",
           "crates/auth/src/token.rs", 52, None, None,
           "validate uses constant_time_eq"),
    Golden("A3", "clean-auth", "direct_call", "digest",
           "crates/auth/src/token.rs", 46, None, None,
           "generate calls digest(secret)"),
    # ── mixed crate: strategy-plugins (parse_error in ai/debate/ml; inventory registry) ──
    Golden("P1", "mixed-plugins", "trait_impl", "generate",
           "crates/strategy-plugins/src/strategies/momentum_rotation.rs", 24,
           "MomentumRotationStrategy", "rs.di.impl_trait",
           "impl SignalGenerator for MomentumRotationStrategy (method line)"),
    Golden("P2", "mixed-plugins", "trait_impl", "generate",
           "crates/strategy-plugins/src/strategies/vwap.rs", 21,
           "VwapStrategy", "rs.di.impl_trait",
           "impl SignalGenerator for VwapStrategy (method line)"),
    Golden("P3", "mixed-plugins", "trait_impl", "generate",
           "crates/strategy-plugins/src/strategies/trend_following.rs", 23,
           "TrendFollowingStrategy", "rs.di.impl_trait",
           "impl SignalGenerator for TrendFollowingStrategy (method line)"),
    Golden("P4", "mixed-plugins", "direct_call", "last_return",
           "crates/strategy-plugins/src/strategies/event_driven.rs", 35, None, None,
           "EventDrivenStrategy::generate uses last_return"),
    Golden("P5", "mixed-plugins", "direct_call", "last_return",
           "crates/strategy-plugins/src/strategies/multi_factor.rs", 55, None, None,
           "MultiFactorStrategy::generate uses last_return"),
    Golden("P6", "mixed-plugins", "direct_call", "registered_strategies",
           "crates/strategy-plugins/src/lib.rs", 28, None, None,
           "available_strategies() calls registered_strategies()"),
    Golden("P7", "mixed-plugins", "direct_call", "run_registered_fusion",
           "crates/strategy-plugins/tests/canonical_strategy_pipeline.rs", 76, None, None,
           "test calls run_registered_fusion"),
    Golden("P8", "mixed-plugins", "registry", "AiSignalStrategy",
           "crates/strategy-plugins/src/lib.rs", 47, None, "rs.di.inventory_submit",
           "inventory::submit factory Box::new(AiSignalStrategy::new)"),
    Golden("P9", "mixed-plugins", "registry", "MomentumRotationStrategy",
           "crates/strategy-plugins/src/lib.rs", 58, None, "rs.di.inventory_submit",
           "inventory::submit factory MomentumRotationStrategy::new"),
    Golden("P10", "mixed-plugins", "registry", "VwapStrategy",
           "crates/strategy-plugins/src/lib.rs", 64, None, "rs.di.inventory_submit",
           "inventory::submit factory VwapStrategy::new"),
    Golden("P11", "mixed-plugins", "registry", "StrategyRegistration",
           "crates/strategy-plugins/src/lib.rs", 0, None, "rs.di.inventory_submit",
           "registration type in inventory::submit body"),
    Golden("P12", "mixed-plugins", "direct_call", "build_default_pipeline",
           "crates/pipeline/src/lib.rs", 62, None, None,
           "pipeline unit test (clean neighbor crate)"),
    # ── noise proxy: L1 invents call edges for common std names ──
    Golden("N1", "noise", "noise_proxy", "fmt",
           "", 0, None, "rs.di.impl_trait",
           "Display/Debug impls stored as kind=call — flood callers(fmt)"),
    Golden("N2", "noise", "noise_proxy", "drop",
           "", 0, None, "rs.di.impl_trait",
           "Drop impls as callers"),
    Golden("N3", "noise", "noise_proxy", "default",
           "", 0, None, None,
           "Default impls / common name collision"),
]


def match_golden(conn: sqlite3.Connection, g: Golden) -> tuple[bool, bool, str]:
    """Return (l0_found, l1_found, detail)."""
    cur = conn.cursor()

    if g.kind == "noise_proxy":
        # Measure flood size, not "found"
        n_exact = cur.execute(
            "SELECT COUNT(*) FROM refs WHERE name=? AND confidence='exact' AND kind='call'",
            (g.name,),
        ).fetchone()[0]
        n_heur = cur.execute(
            "SELECT COUNT(*) FROM refs WHERE name=? AND confidence='heuristic'",
            (g.name,),
        ).fetchone()[0]
        return False, False, f"exact_calls={n_exact} heuristic={n_heur} (noise volume)"

    sql = "SELECT confidence, path, line, enclosing, qualifier, rule_id, kind FROM refs WHERE name=?"
    rows = cur.execute(sql, (g.name,)).fetchall()

    def path_ok(path: str) -> bool:
        if not g.path_substr:
            return True
        return g.path_substr.replace("\\", "/") in (path or "").replace("\\", "/")

    def line_ok(line: int) -> bool:
        return g.line == 0 or line == g.line

    l0 = False
    l1 = False
    details = []

    for conf, path, line, enclosing, qualifier, rule_id, kind in rows:
        if not path_ok(path) or not line_ok(line):
            continue
        if g.qualifier and g.qualifier not in (qualifier or "") and g.qualifier not in (enclosing or ""):
            # trait_impl heuristic carries qualifier=type; also accept evidence via qualifier
            if not (qualifier and g.qualifier in qualifier):
                continue
        if g.rule_id and rule_id != g.rule_id:
            # exact edges have rule_id None; for registry goldens we require the rule
            if g.kind in ("registry", "trait_impl") and conf != "heuristic":
                continue
            if g.kind == "registry":
                continue
            # trait_impl: exact does not count as impl edge; L1 heuristic does
            if g.kind == "trait_impl" and conf != "heuristic":
                continue
        if g.kind == "trait_impl":
            if conf == "heuristic" and (rule_id == g.rule_id or rule_id == "rs.di.impl_trait"):
                l1 = True
                details.append(f"L1 {conf} {path}:{line} q={qualifier} rule={rule_id}")
            elif conf == "exact" and path_ok(path) and line_ok(line) and (not g.qualifier or g.qualifier in (enclosing or "") or g.qualifier in (qualifier or "")):
                # Exact definition/call at same site does not equal "trait impl edge",
                # but a call to the method name at the impl site may still appear exact.
                details.append(f"note-exact {path}:{line} enc={enclosing}")
            continue
        if g.kind == "registry":
            if conf == "heuristic" and (g.rule_id is None or rule_id == g.rule_id):
                l1 = True
                details.append(f"L1 {conf} {path}:{line} rule={rule_id}")
            elif conf == "exact" and (g.rule_id is None or rule_id == g.rule_id or rule_id is None):
                # import of RepoRegistry counts as L0 registry-type edge
                if g.name == "RepoRegistry" and kind in ("import", "call"):
                    l0 = True
                    l1 = True
                    details.append(f"L0 {conf} {path}:{line} kind={kind}")
                elif g.rule_id is None:
                    l0 = True
                    l1 = True
                    details.append(f"L0 {conf} {path}:{line} kind={kind}")
            continue
        # direct_call / import
        if conf == "exact":
            l0 = True
            l1 = True
            details.append(f"L0 {conf} {path}:{line} kind={kind} enc={enclosing}")
        elif conf == "heuristic":
            l1 = True
            details.append(f"L1-only {conf} {path}:{line} rule={rule_id}")

    # For registry goldens that only fire L1 after product change:
    if g.kind == "registry" and g.rule_id == "rs.di.inventory_submit":
        n = cur.execute(
            "SELECT COUNT(*) FROM refs WHERE name=? AND confidence='heuristic' AND rule_id='rs.di.inventory_submit' AND path LIKE ?",
            (g.name, f"%{g.path_substr}%" if g.path_substr else "%"),
        ).fetchone()[0]
        if n > 0:
            l1 = True
            if not details:
                details.append(f"L1 inventory n={n}")

    if not details:
        details.append("MISS (no matching ref)")
    return l0, l1, "; ".join(details)[:300]


def main() -> None:
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    rows_out = []
    l0_n = l1_n = 0
    per_crate: dict[str, dict[str, int]] = {}

    print("=== GOLDEN EDGE L0 vs L1 (stock-trading-app) ===\n")
    print(f"{'id':4} {'crate':14} {'kind':12} {'name':22} {'L0':3} {'L1':3} notes")
    for g in GOLDENS:
        if g.kind == "noise_proxy":
            l0, l1, detail = match_golden(conn, g)
            print(f"{g.id:4} {g.crate:14} {g.kind:12} {g.name:22} {'—':3} {'—':3} {detail}")
            continue
        l0, l1, detail = match_golden(conn, g)
        l0_n += int(l0)
        l1_n += int(l1)
        pc = per_crate.setdefault(g.crate, {"n": 0, "l0": 0, "l1": 0})
        pc["n"] += 1
        pc["l0"] += int(l0)
        pc["l1"] += int(l1)
        print(f"{g.id:4} {g.crate:14} {g.kind:12} {g.name:22} {str(l0):3} {str(l1):3} {detail}")
        rows_out.append((g, l0, l1, detail))

    labeled = sum(1 for g in GOLDENS if g.kind != "noise_proxy")
    print(f"\n=== TOTAL labeled={labeled}  L0_found={l0_n} ({100*l0_n/labeled:.0f}%)  "
          f"L1_found={l1_n} ({100*l1_n/labeled:.0f}%) ===")
    print("\n=== Per-crate ===")
    for crate, pc in sorted(per_crate.items()):
        print(f"  {crate:14} n={pc['n']:2}  L0={pc['l0']:2}  L1={pc['l1']:2}")

    # Rehearsal table for docs
    print("\n=== Docs table rows ===")
    print("| id | crate | kind | name | L0 | L1 | notes |")
    print("|---|---|---|---|---:|---:|---|")
    for g, l0, l1, detail in rows_out:
        mark0 = "✅" if l0 else "❌"
        mark1 = "✅" if l1 else "❌"
        notes = g.notes
        if not l0 and not l1:
            notes += " | MISS both"
        elif l0 and not l1:
            notes += " | L1 miss (odd)"
        elif l1 and not l0:
            notes += " | L1-only lift"
        print(f"| {g.id} | {g.crate} | {g.kind} | `{g.name}` | {mark0} | {mark1} | {notes} |")

    conn.close()


if __name__ == "__main__":
    main()
