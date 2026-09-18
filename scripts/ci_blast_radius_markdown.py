#!/usr/bin/env python3
"""P2-2 helper: turn agentgraph blast-radius / who-calls JSON into markdown.

Used by `.github/workflows/blast-radius-demo.yml` (and `examples/ci/blast-radius.yml`)
to write `$GITHUB_STEP_SUMMARY` and optional PR comments.

Non-claim: output is a **recipe payload digest** (window / honesty fields + node
paths). It is **not** a complete runtime graph and is **not** a soundness proof.

Only real CLI flags are listed in the summary body (the ones the demo ran).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional


def _load_json(path: Path) -> Dict[str, Any]:
    # utf-8-sig: tolerate PowerShell `Out-File -Encoding utf8` BOM on local smoke.
    text = path.read_text(encoding="utf-8-sig")
    data = json.loads(text)
    if not isinstance(data, dict):
        raise SystemExit(f"{path}: expected JSON object payload")
    return data


def _nodes_table(nodes: Any, limit: int) -> List[str]:
    rows: List[str] = []
    if not isinstance(nodes, list) or not nodes:
        rows.append("| — | — | — | — |")
        return rows
    for n in nodes[:limit]:
        if not isinstance(n, dict):
            continue
        name = str(n.get("name") or n.get("enclosing") or "?")
        path = str(n.get("path") or n.get("file") or "")
        depth = n.get("depth", "")
        role = n.get("edge_role") or n.get("confidence") or ""
        rows.append(f"| `{name}` | `{path}` | {depth} | {role} |")
    if len(nodes) > limit:
        rows.append(f"| … | *(+{len(nodes) - limit} more)* |  |  |")
    return rows


def _who_sections(payload: Dict[str, Any]) -> List[str]:
    lines: List[str] = []
    callers = payload.get("callers")
    implementors = payload.get("implementors")
    if isinstance(callers, list):
        lines.append(f"- **callers:** {len(callers)} row(s)")
        for c in callers[:8]:
            if isinstance(c, dict):
                path = c.get("path") or c.get("resolved") or ""
                name = c.get("name") or c.get("enclosing") or "?"
                lines.append(f"  - `{name}` — `{path}`")
    if isinstance(implementors, list):
        lines.append(f"- **implementors:** {len(implementors)} row(s) (separated by default)")
        for c in implementors[:5]:
            if isinstance(c, dict):
                path = c.get("path") or ""
                name = c.get("name") or c.get("enclosing") or "?"
                lines.append(f"  - `{name}` — `{path}`")
    if payload.get("high_freq_name"):
        lines.append(f"- **high_freq_name:** `{payload.get('high_freq_name')}`")
    return lines


def build_markdown(
    *,
    fixture: str,
    symbol: str,
    who_symbol: str,
    blast: Dict[str, Any],
    who_calls: Optional[Dict[str, Any]],
    commands: List[str],
    node_limit: int,
) -> str:
    window = blast.get("window", "")
    subset_ok = blast.get("subset_ok", "")
    promise_tier = blast.get("promise_tier", "")
    recommendation = blast.get("recommendation", "")
    note = blast.get("note", "not a complete runtime graph")
    nodes = blast.get("nodes") or blast.get("impact") or []

    lines: List[str] = []
    lines.append("### agentgraph blast-radius demo (P2-2)")
    lines.append("")
    lines.append(
        "> Demo only — **not** a required product-quality gate. "
        "Payload is indexed L0/L1 candidates (+ S-qualified edges only when "
        "`subset_ok`); **not** a complete runtime graph."
    )
    lines.append("")
    lines.append("| Field | Value |")
    lines.append("|---|---|")
    lines.append(f"| Fixture | `{fixture}` |")
    lines.append(f"| Blast symbol | `{symbol}` |")
    lines.append(f"| `window` | `{window}` |")
    lines.append(f"| `subset_ok` | `{subset_ok}` |")
    lines.append(f"| `promise_tier` | `{promise_tier}` |")
    lines.append(f"| `note` | {note} |")
    lines.append("")
    lines.append("**Commands run**")
    lines.append("")
    lines.append("```bash")
    for c in commands:
        lines.append(c)
    lines.append("```")
    lines.append("")
    if recommendation:
        lines.append(f"**recommendation:** {recommendation}")
        lines.append("")
    lines.append(f"#### Blast-radius nodes (`{symbol}`, up to {node_limit})")
    lines.append("")
    lines.append("| name | path | depth | edge/conf |")
    lines.append("|---|---|---|---|")
    lines.extend(_nodes_table(nodes, node_limit))
    lines.append("")
    if who_calls is not None:
        lines.append(f"#### who-calls `{who_symbol}`")
        lines.append("")
        sec = _who_sections(who_calls)
        if sec:
            lines.extend(sec)
        else:
            lines.append("_empty who-calls payload_")
        lines.append("")
        if who_calls.get("recommendation"):
            lines.append(f"**who-calls recommendation:** {who_calls.get('recommendation')}")
            lines.append("")
    lines.append("---")
    lines.append("")
    lines.append(
        "Adapt for a real monorepo: [docs/ci-blast-radius-demo.md](docs/ci-blast-radius-demo.md). "
        "Eval scores: [docs/eval-agent-tasks.md](docs/eval-agent-tasks.md). "
        "Onboarding: [docs/onboarding.md](docs/onboarding.md)."
    )
    return "\n".join(lines) + "\n"


def main(argv: Optional[List[str]] = None) -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--fixture", required=True, help="Fixture path label")
    p.add_argument("--symbol", required=True, help="Blast-radius symbol")
    p.add_argument("--who-symbol", default="", help="Who-calls symbol (optional)")
    p.add_argument("--blast", required=True, type=Path, help="blast-radius JSON file")
    p.add_argument("--who-calls", type=Path, default=None, help="who-calls JSON file")
    p.add_argument(
        "--command",
        action="append",
        default=[],
        dest="commands",
        help="Command line that was executed (repeatable; for the summary body)",
    )
    p.add_argument("--node-limit", type=int, default=20)
    p.add_argument("--out", type=Path, required=True, help="Markdown output path")
    args = p.parse_args(argv)

    blast = _load_json(args.blast)
    who = _load_json(args.who_calls) if args.who_calls and args.who_calls.is_file() else None
    who_symbol = args.who_symbol or args.symbol

    md = build_markdown(
        fixture=args.fixture,
        symbol=args.symbol,
        who_symbol=who_symbol,
        blast=blast,
        who_calls=who,
        commands=args.commands,
        node_limit=max(1, args.node_limit),
    )
    args.out.write_text(md, encoding="utf-8")
    sys.stdout.write(md)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
