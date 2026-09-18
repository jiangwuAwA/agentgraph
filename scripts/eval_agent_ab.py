#!/usr/bin/env python3
"""P0-5 scripted tool-policy A/B/C evals (honest: not live LLM agents).

Policies (labels follow the operator brief for this slice):

- **A — Agent-like tool policy + MCP/CLI recipes**
  Deterministic scripted policy that indexes the fixture, then uses agentgraph
  recipes (``blast-radius`` / ``who-calls`` / ``find`` / ``related`` /
  ``subset``) to assemble a file set to touch/review.

- **B — Agent-like read/grep policy**
  Same task instructions; tools limited to reading source + name/token search.
  **No agentgraph MCP / recipes.** Simple documented heuristics (see
  ``docs/eval-agent-baseline.md``).

- **C — name-grep control**
  Deterministic token baseline retained from P0-1 (symbol token in files).

Honesty
-------
These are **scripted tool-policy agents** (deterministic, reproducible), **not**
a live LLM agent experiment. A future P0-5b live run is separate. No private
corpus. No fabricated LLM numbers.

Outputs
-------
- trajectories: ``evals/agent-ab/<task>/run-{a|b|c}-<seed>.json``
- machine-readable: ``target/agent_ab_eval.json``
- markdown table: ``target/agent_ab_eval.md``

Replay
------
Score any trajectory offline (no network, no agentgraph binary):

    python scripts/eval_agent_ab.py score --trajectory evals/agent-ab/<t>/run-a-0.json
    python scripts/eval_agent_ab.py score --traj-dir evals/agent-ab

Exit codes
----------
0 — run/replay completed; gates green
1 — gate / tool failure
2 — usage / IO error
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import random
import re
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Set, Tuple

# Reuse P0-1 helpers (import-safe: no main on import).
_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

from eval_agent_tasks import (  # noqa: E402
    SOURCE_EXTS,
    collect_source_files,
    copy_fixture,
    extra_noise,
    files_from_rows,
    find_bin,
    name_grep_files,
    norm_path,
    parse_json_stdout,
    recall,
    repo_root_from_script,
    run_cli,
)

if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
        sys.stderr.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:
        sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
        sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")

TRAJECTORY_SCHEMA = "agentgraph.eval_agent_ab.trajectory.v1"
RESULT_SCHEMA = "agentgraph.eval_agent_ab.v1"
POLICY_KINDS = {
    "A": "scripted_tool_policy",
    "B": "scripted_read_grep_policy",
    "C": "name_grep_control",
}
HONESTY_NOTE = (
    "scripted deterministic tool-policy agent — not a live LLM agent experiment; "
    "future live P0-5b is separate"
)

# B-policy knobs (documented in docs/eval-agent-baseline.md)
B_MAX_SAME_DIR = 6
B_MAX_SECONDARY = 8
B_MAX_READ = 12
B_MAX_SECONDARY_HITS_PER_TOKEN = 8

_IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]{2,}")
_TOKEN_RE_CACHE: Dict[str, re.Pattern[str]] = {}


def _token_rx(symbol: str) -> re.Pattern[str]:
    rx = _TOKEN_RE_CACHE.get(symbol)
    if rx is None:
        rx = re.compile(rf"(?<![A-Za-z0-9_]){re.escape(symbol)}(?![A-Za-z0-9_])")
        _TOKEN_RE_CACHE[symbol] = rx
    return rx


def looks_like_definition(line: str, symbol: str) -> bool:
    if not _token_rx(symbol).search(line):
        return False
    s = re.escape(symbol)
    pats = [
        rf"\bclass\s+{s}\b",
        rf"\binterface\s+{s}\b",
        rf"\b(?:function|fn|def|func)\s+{s}\b",
        rf"\btype\s+{s}\b",
        rf"\b(?:struct|enum|trait|const|static)\s+{s}\b",
        rf"\bimpl\b[^\n]*\b{s}\b",
        rf"(?:const|let|var)\s+{s}\s*=",
        rf"\bfunc\s+\([^)]*\)\s*{s}\b",
        rf"\bfn\s+{s}\b",
        rf"\bclass\s+\w+[^\n]*\b{s}\b",  # class-level mention near type name
        rf"\bexport\s+(?:default\s+)?(?:class|interface|function|type|const)\s+{s}\b",
    ]
    return any(re.search(p, line) for p in pats)


def seeded_pick(items: Sequence[str], k: int, seed: int) -> List[str]:
    """Deterministic subset: stable order, seed breaks ties when capping."""
    if k <= 0:
        return []
    if len(items) <= k:
        return list(items)
    rng = random.Random(seed)
    shuffled = list(items)
    rng.shuffle(shuffled)
    return sorted(shuffled[:k])


def _as_rel(p: str) -> str:
    return str(p).replace("\\", "/").lstrip("./")


def score_file_set(
    file_set: Sequence[str],
    expected_files: Sequence[str],
    noise_files: Sequence[str],
    forbidden_files: Sequence[str] | None = None,
) -> Dict[str, Any]:
    """Offline structure-fact scoring (replay-safe; no network)."""
    found = {_as_rel(p) for p in file_set if p}
    rec, hit, miss = recall(expected_files, found)
    noise = extra_noise(noise_files, found)
    forb = list(forbidden_files or [])
    forb_hit = extra_noise(forb, found) if forb else []
    return {
        "expected_file_recall": round(rec, 4),
        "expected_hit": hit,
        "expected_miss": miss,
        "extra_noise_files": noise,
        "extra_noise_count": len(noise),
        "forbidden_files": forb,
        "forbidden_hit": forb_hit,
        "forbidden_hit_count": len(forb_hit),
        "file_set": sorted(found),
        "file_set_size": len(found),
    }


def _rel_from_work(path: str, work: Path, prefixes: Sequence[Path]) -> str:
    return norm_path(path, prefixes)


@dataclass
class PolicyRun:
    policy: str
    task_id: str
    seed: int
    run_id: str
    kind: str
    file_set: List[str] = field(default_factory=list)
    tool_calls: List[Dict[str, Any]] = field(default_factory=list)
    notes: List[str] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)
    extras: Dict[str, Any] = field(default_factory=dict)


def _record(
    calls: List[Dict[str, Any]],
    tool: str,
    args: Sequence[str] | None = None,
    ok: bool = True,
    summary: Optional[Dict[str, Any]] = None,
    note: str = "",
) -> None:
    calls.append(
        {
            "seq": len(calls) + 1,
            "tool": tool,
            "args": list(args or []),
            "ok": ok,
            "summary": summary or {},
            "note": note,
        }
    )


# ---------------------------------------------------------------------------
# Policy A — agentgraph tool policy
# ---------------------------------------------------------------------------


def run_policy_a(
    bin_path: Path,
    task_dir: Path,
    meta: Dict[str, Any],
    work: Path,
    seed: int,
    depth: int = 3,
) -> PolicyRun:
    tid = meta.get("id") or task_dir.name
    symbol = meta.get("symbol") or ""
    run = PolicyRun(
        policy="A",
        task_id=tid,
        seed=seed,
        run_id=f"a-{seed}",
        kind=POLICY_KINDS["A"],
    )
    prefixes = [work, task_dir]
    ws = meta.get("workspace")
    calls = run.tool_calls

    # index
    if ws and ws.get("manifest"):
        manifest = work / ws["manifest"]
        if not manifest.is_file():
            run.errors.append(f"workspace manifest missing: {ws['manifest']}")
            return run
        index_args = ["index", "--workspace", str(manifest), "--force"]
    elif ws and ws.get("roots"):
        index_args = ["index"]
        for r in ws["roots"]:
            index_args += ["--workspace-root", str(work / r["path"])]
        index_args += ["--workspace-db", str(work / "ws.db"), "--force"]
    else:
        index_args = ["--root", str(work), "index", "--force"]

    code, out, err = run_cli(bin_path, work, index_args)
    _record(
        calls,
        "index",
        index_args,
        ok=code == 0,
        summary={"exit_code": code},
        note="fixture index (required before recipes)",
    )
    if code != 0:
        run.errors.append(f"index failed ({code}): {err.strip()[:300]}")
        return run

    def query_args(cmd: str, *extra: str) -> List[str]:
        if ws and ws.get("manifest"):
            return [cmd, *extra, "--workspace", str(work / ws["manifest"])]
        if ws and ws.get("roots"):
            return [cmd, *extra, "--workspace-db", str(work / "ws.db")]
        return ["--root", str(work), cmd, *extra]

    # blast-radius
    blast_args = query_args("blast-radius", symbol, "--depth", str(depth))
    code, out, err = run_cli(bin_path, work, blast_args)
    blast = parse_json_stdout(out) if code == 0 else None
    _record(
        calls,
        "blast-radius",
        blast_args,
        ok=code == 0 and isinstance(blast, dict),
        summary={
            "exit_code": code,
            "window": (blast or {}).get("window") if isinstance(blast, dict) else None,
            "subset_ok": (blast or {}).get("subset_ok") if isinstance(blast, dict) else None,
            "promise_tier": (blast or {}).get("promise_tier") if isinstance(blast, dict) else None,
        },
        note="dependents recipe (agent-facing window + honesty fields)",
    )
    if not isinstance(blast, dict):
        run.errors.append(f"blast-radius failed ({code}): {err.strip()[:300]}")
        return run

    nodes = blast.get("nodes") or blast.get("impact") or []
    node_paths, node_resolved = files_from_rows(nodes, work, prefixes)
    blast_files = {
        norm_path(p, prefixes)
        for p in (set(node_paths) | set(node_resolved))
        if p
    }

    # find (definition)
    find_args = query_args("find", symbol)
    code_f, out_f, _ = run_cli(bin_path, work, find_args)
    def_files: Set[str] = set()
    if code_f == 0:
        found = parse_json_stdout(out_f)
        rows = found if isinstance(found, list) else (
            found.get("results") if isinstance(found, dict) else None
        )
        if isinstance(rows, list):
            def_files, _ = files_from_rows(rows, work, prefixes)
    _record(
        calls,
        "find",
        find_args,
        ok=code_f == 0,
        summary={"exit_code": code_f, "definition_files": sorted(def_files)},
        note="open definition file(s)",
    )

    # related
    related_args = query_args("related", symbol, "--limit", "20")
    code_r, out_r, _ = run_cli(bin_path, work, related_args)
    related_files: Set[str] = set()
    if code_r == 0:
        rel = parse_json_stdout(out_r)
        if isinstance(rel, list):
            for row in rel:
                if isinstance(row, dict) and row.get("path"):
                    related_files.add(norm_path(str(row["path"]), prefixes))
                    rp = row.get("root_path")
                    if rp:
                        full = str(Path(rp) / str(row["path"]))
                        related_files.add(norm_path(full, prefixes))
    _record(
        calls,
        "related",
        related_args,
        ok=code_r == 0,
        summary={"exit_code": code_r, "related_file_count": len(related_files)},
        note="importers/references (scope retrieval; callees/siblings)",
    )

    # who-calls
    who_args = query_args("who-calls", symbol, "--limit", "50")
    code_w, out_w, _ = run_cli(bin_path, work, who_args)
    who_files: Set[str] = set()
    high_freq = False
    who_summary: Dict[str, Any] = {"exit_code": code_w}
    if code_w == 0:
        wj = parse_json_stdout(out_w)
        if isinstance(wj, dict):
            high_freq = bool(wj.get("high_freq_name"))
            callers = wj.get("callers") or []
            implementors = wj.get("implementors") or []
            if isinstance(callers, dict):
                callers = callers.get("callers") or []
            for group in (callers, implementors if not high_freq else []):
                if not isinstance(group, list):
                    continue
                for n in group:
                    if isinstance(n, dict):
                        p = n.get("path") or n.get("at")
                        if isinstance(p, str) and p:
                            if ":" in p and Path(p.rsplit(":", 1)[0]).suffix:
                                p = p.rsplit(":", 1)[0]
                            who_files.add(norm_path(p, prefixes))
            who_summary.update(
                {
                    "high_freq_name": high_freq,
                    "implementor_count": wj.get("implementor_count"),
                    "callers_files": sorted(who_files),
                }
            )
    _record(
        calls,
        "who-calls",
        who_args,
        ok=code_w == 0,
        summary=who_summary,
        note=(
            "callers vs implementors; high-freq demotion — "
            "implementor paths excluded from file set when high_freq_name"
        ),
    )

    # subset (honesty companion — does not expand the edit set by itself)
    subset_args = query_args("subset")
    code_s, out_s, _ = run_cli(bin_path, work, subset_args)
    sj = parse_json_stdout(out_s) if code_s in {0, 2} else None
    _record(
        calls,
        "subset",
        subset_args,
        ok=isinstance(sj, dict),
        summary={
            "exit_code": code_s,
            "subset_ok": (sj or {}).get("subset_ok") if isinstance(sj, dict) else None,
            "in_subset": (sj or {}).get("in_subset") if isinstance(sj, dict) else None,
        },
        note="honesty companion (window / scoped sound candidates)",
    )

    # Assemble file set (policy A):
    #   blast nodes ∪ resolved ∪ find(definition) ∪ related
    #   ∪ who-calls caller files (skip implementor flood when high_freq)
    file_set = set(blast_files) | set(def_files) | set(related_files) | set(who_files)
    file_set = {norm_path(p, prefixes) for p in file_set if p}
    run.file_set = sorted(file_set)
    run.extras = {
        "window": blast.get("window"),
        "subset_ok": blast.get("subset_ok"),
        "promise_tier": blast.get("promise_tier"),
        "note_honest": isinstance(blast.get("note"), str)
        and (
            "not a complete runtime graph" in blast.get("note")
            or "非完整运行时图" in blast.get("note")
        ),
        "recommendation_present": isinstance(blast.get("recommendation"), str)
        and len(blast.get("recommendation") or "") >= 8,
        "high_freq_name": high_freq,
        "blast_files": sorted(blast_files),
        "definition_files": sorted(def_files),
        "related_files": sorted(related_files),
        "who_files": sorted(who_files),
        "file_set_method": (
            "blast_nodes ∪ node.resolved ∪ find(definition) ∪ related "
            "∪ who-calls non-flood paths"
        ),
        "policy": (
            "index → blast-radius → who-calls → find → related → subset → "
            "assemble structure-bounded file set"
        ),
    }
    run.notes.append(
        "A is a scripted recipe policy using agentgraph CLI (same payload "
        "surface as MCP blast_radius/who_calls); not a live LLM agent."
    )
    return run


# ---------------------------------------------------------------------------
# Policy B — agent-like read/grep (no agentgraph)
# ---------------------------------------------------------------------------


def run_policy_b(
    task_dir: Path,
    meta: Dict[str, Any],
    work: Path,
    seed: int,
) -> PolicyRun:
    tid = meta.get("id") or task_dir.name
    symbol = meta.get("symbol") or ""
    run = PolicyRun(
        policy="B",
        task_id=tid,
        seed=seed,
        run_id=f"b-{seed}",
        kind=POLICY_KINDS["B"],
    )
    calls = run.tool_calls
    prefixes = [work, task_dir]

    # 1) walk sources
    sources = collect_source_files(work)
    rel_sources = sorted({norm_path(str(p.relative_to(work)), prefixes) for p in sources})
    _record(
        calls,
        "walk_sources",
        [str(work)],
        ok=True,
        summary={"source_file_count": len(rel_sources)},
        note="enumerate source files under the mini-repo (no agentgraph)",
    )

    # 2) grep symbol tokens
    hits: List[str] = []
    hit_lines: Dict[str, List[str]] = {}
    rx = _token_rx(symbol)
    for path in sources:
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if rx.search(text):
            rel = norm_path(str(path.relative_to(work)), prefixes)
            hits.append(rel)
            lines = [ln for ln in text.splitlines() if rx.search(ln)]
            hit_lines[rel] = lines[:8]
    hits = sorted(set(hits))
    _record(
        calls,
        "grep_symbol",
        [symbol],
        ok=True,
        summary={"hit_count": len(hits), "hits": hits},
        note="token search for the issue symbol across source files",
    )

    # 3) bounded reads + definition detection
    def_files: List[str] = []
    secondary_pool: List[str] = []
    for rel in hits:
        fp = work / rel
        try:
            text = fp.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for ln in text.splitlines():
            if looks_like_definition(ln, symbol):
                def_files.append(rel)
                for tok in _IDENT_RE.findall(ln):
                    if tok != symbol and tok not in secondary_pool:
                        secondary_pool.append(tok)
                break

    def_files = sorted(set(def_files))
    reads = list(def_files)
    # also "read" a few hit files that look like callers (lines with symbol)
    extra_reads = [h for h in hits if h not in reads][: max(0, B_MAX_READ - len(reads))]
    reads = (reads + extra_reads)[: B_MAX_READ]
    _record(
        calls,
        "read_files",
        reads,
        ok=True,
        summary={"definition_files": def_files, "read_count": len(reads)},
        note=f"bounded reads (cap {B_MAX_READ}); detect definition-like lines",
    )

    # 4) same-dir expansion
    same_dir: List[str] = []
    for rel in def_files:
        parent = Path(rel).parent
        siblings = []
        for p in rel_sources:
            if Path(p).parent == parent and p not in def_files:
                siblings.append(p)
        siblings = sorted(siblings)
        picked = seeded_pick(siblings, B_MAX_SAME_DIR, seed)
        same_dir.extend(picked)
        _record(
            calls,
        "expand_same_dir",
            [str(parent) if str(parent) != "." else ".", *picked],
            ok=True,
            summary={
                "dir": str(parent).replace("\\", "/") if str(parent) != "." else ".",
                "siblings_total": len(siblings),
                "included": picked,
                "cap": B_MAX_SAME_DIR,
                "seed": seed,
            },
            note="include sibling source files in the definition directory "
            f"(cap {B_MAX_SAME_DIR}; seed breaks ties)",
        )
    same_dir = sorted(set(same_dir))

    # 5) secondary token greps
    secondary_tokens = seeded_pick(sorted(secondary_pool), B_MAX_SECONDARY, seed)
    secondary_hits: List[str] = []
    for tok in secondary_tokens:
        tok_rx = _token_rx(tok)
        tok_hits: List[str] = []
        for path in sources:
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if tok_rx.search(text):
                rel = norm_path(str(path.relative_to(work)), prefixes)
                if rel not in tok_hits:
                    tok_hits.append(rel)
        tok_hits = sorted(tok_hits)[:B_MAX_SECONDARY_HITS_PER_TOKEN]
        secondary_hits.extend(tok_hits)
        _record(
            calls,
            "grep_token",
            [tok],
            ok=True,
            summary={"token": tok, "hits": tok_hits},
            note="secondary identifier search from definition lines",
        )
    secondary_hits = sorted(set(secondary_hits))

    file_set = sorted(set(hits) | set(def_files) | set(same_dir) | set(secondary_hits))
    run.file_set = file_set
    run.extras = {
        "policy": (
            "walk → grep symbol token → bounded definition reads → "
            "same-dir sibling expansion → secondary identifier greps"
        ),
        "heuristics": {
            "include": "files containing the symbol token",
            "plus": "source files in the same directory as definition hits",
            "caps": {
                "same_dir": B_MAX_SAME_DIR,
                "secondary_tokens": B_MAX_SECONDARY,
                "reads": B_MAX_READ,
                "secondary_hits_per_token": B_MAX_SECONDARY_HITS_PER_TOKEN,
            },
            "seed_role": "tie-break only when capping same-dir siblings / secondary tokens",
        },
        "grep_hits": hits,
        "definition_files": def_files,
        "same_dir_files": same_dir,
        "secondary_tokens": secondary_tokens,
        "secondary_files": secondary_hits,
        "file_set_method": (
            "symbol-token hits ∪ definition files ∪ same-dir siblings "
            "∪ secondary-token hits (no agentgraph tools)"
        ),
    }
    run.notes.append(HONESTY_NOTE)
    return run


# ---------------------------------------------------------------------------
# Policy C — name-grep control (P0-1)
# ---------------------------------------------------------------------------


def run_policy_c(
    task_dir: Path,
    meta: Dict[str, Any],
    work: Path,
    seed: int,
) -> PolicyRun:
    tid = meta.get("id") or task_dir.name
    symbol = meta.get("symbol") or ""
    run = PolicyRun(
        policy="C",
        task_id=tid,
        seed=seed,
        run_id=f"c-{seed}",
        kind=POLICY_KINDS["C"],
    )
    files = name_grep_files(work, symbol)
    _record(
        run.tool_calls,
        "name_grep",
        [symbol],
        ok=True,
        summary={"file_count": len(files), "files": files},
        note="P0-1 deterministic name-grep control (symbol token in source files)",
    )
    run.file_set = files
    run.extras = {
        "policy": "name-grep (symbol token in source files)",
        "method": "name-grep (symbol token in source files)",
        "file_set_method": "name-grep symbol token",
        "seed_role": "metadata only — C is seed-invariant",
    }
    run.notes.append("C retained from P0-1; deterministic token control, not an LLM.")
    return run


# ---------------------------------------------------------------------------
# Trajectory build / replay
# ---------------------------------------------------------------------------


def load_task_meta(task_dir: Path) -> Dict[str, Any]:
    return json.loads((task_dir / "task.json").read_text(encoding="utf-8"))


def task_labels(meta: Dict[str, Any]) -> Dict[str, Any]:
    expected = meta.get("expected") or {}
    forbidden = list(expected.get("forbidden_files") or [])
    # noise_files double as wrong-file set when no explicit forbidden list
    if not forbidden:
        forbidden = list(expected.get("noise_files") or [])
    return {
        "expected_files": list(expected.get("files_that_matter") or []),
        "noise_files": list(expected.get("noise_files") or []),
        "forbidden_files": forbidden,
        "forbidden_source": (
            "expected.forbidden_files"
            if expected.get("forbidden_files")
            else "expected.noise_files (no explicit forbidden list)"
        ),
    }


def build_trajectory(
    run: PolicyRun,
    meta: Dict[str, Any],
    labels: Dict[str, Any],
    fixture_rel: str,
) -> Dict[str, Any]:
    score = score_file_set(
        run.file_set,
        labels["expected_files"],
        labels["noise_files"],
        labels["forbidden_files"],
    )
    # Avoid committing absolute host paths in public replay fixtures.
    calls = []
    for c in run.tool_calls:
        cc = dict(c)
        args = []
        for a in cc.get("args") or []:
            s = str(a)
            # drop absolute paths that point outside the fixture namespace
            if re.match(r"^[A-Za-z]:[\\/]", s) or s.startswith("\\\\") or s.startswith("/"):
                args.append("<fixture-root>")
            else:
                args.append(s.replace("\\", "/"))
        cc["args"] = args
        calls.append(cc)
    return {
        "schema": TRAJECTORY_SCHEMA,
        "policy": run.policy,
        "policy_label": {
            "A": "Agent-like tool policy + agentgraph MCP/CLI recipes (scripted)",
            "B": "Agent-like read/grep policy — no agentgraph (scripted)",
            "C": "name-grep control (P0-1 deterministic baseline)",
        }[run.policy],
        "kind": run.kind,
        "task_id": run.task_id,
        "seed": run.seed,
        "run_id": run.run_id,
        "fixture": fixture_rel,
        "symbol": meta.get("symbol") or "",
        "honesty": {
            "live_llm_agent": False,
            "scripted_tool_policy_agent": True,
            "note": HONESTY_NOTE,
            "private_corpus": False,
        },
        "task": {
            "id": meta.get("id"),
            "title": meta.get("title"),
            "language": meta.get("language"),
            "issue": meta.get("issue") or "",
            **labels,
        },
        "tool_calls": calls,
        "file_set": sorted({_as_rel(p) for p in run.file_set if p}),
        "score": score,
        "extras": run.extras,
        "notes": run.notes,
        "errors": run.errors,
    }


def score_trajectory(payload: Dict[str, Any]) -> Dict[str, Any]:
    """Replay scoring: recompute metrics from trajectory contents only."""
    if payload.get("schema") != TRAJECTORY_SCHEMA:
        raise ValueError(f"unexpected schema: {payload.get('schema')!r}")
    task = payload.get("task") or {}
    labels = {
        "expected_files": list(task.get("expected_files") or []),
        "noise_files": list(task.get("noise_files") or []),
        "forbidden_files": list(task.get("forbidden_files") or []),
    }
    recomputed = score_file_set(
        payload.get("file_set") or [],
        labels["expected_files"],
        labels["noise_files"],
        labels["forbidden_files"],
    )
    recorded = payload.get("score") or {}
    mismatches = []
    for key in (
        "expected_file_recall",
        "extra_noise_count",
        "forbidden_hit_count",
        "file_set_size",
    ):
        if recorded.get(key) != recomputed.get(key):
            mismatches.append(
                f"{key}: recorded={recorded.get(key)!r} recomputed={recomputed.get(key)!r}"
            )
    return {
        "schema": TRAJECTORY_SCHEMA,
        "task_id": payload.get("task_id"),
        "policy": payload.get("policy"),
        "run_id": payload.get("run_id"),
        "seed": payload.get("seed"),
        "kind": payload.get("kind"),
        "live_llm_agent": bool(
            (payload.get("honesty") or {}).get("live_llm_agent", False)
        ),
        "recomputed": recomputed,
        "recorded_matches_replay": not mismatches,
        "mismatches": mismatches,
        "errors": list(payload.get("errors") or []),
    }


def trajectory_fingerprint(payload: Dict[str, Any]) -> str:
    """Stable hash of policy file set (for seed-invariance reporting)."""
    blob = json.dumps(
        {
            "policy": payload.get("policy"),
            "task_id": payload.get("task_id"),
            "file_set": payload.get("file_set"),
        },
        sort_keys=True,
        ensure_ascii=False,
    )
    return hashlib.sha256(blob.encode("utf-8")).hexdigest()[:16]


def write_trajectory(path: Path, payload: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


def discover_traj_files(traj_dir: Path) -> List[Path]:
    if not traj_dir.is_dir():
        return []
    out: List[Path] = []
    for p in sorted(traj_dir.rglob("run-*.json")):
        if p.is_file():
            out.append(p)
    return out


# ---------------------------------------------------------------------------
# Aggregation / markdown
# ---------------------------------------------------------------------------


def aggregate_results(runs: Sequence[Dict[str, Any]]) -> Dict[str, Any]:
    """runs = list of {task_id, policy, seed, score, errors, kind, fingerprint}"""
    by_policy: Dict[str, List[Dict[str, Any]]] = {"A": [], "B": [], "C": []}
    for r in runs:
        by_policy.setdefault(r["policy"], []).append(r)

    def mean(xs: List[float]) -> float:
        return round(sum(xs) / len(xs), 4) if xs else 0.0

    summary: Dict[str, Any] = {}
    for pol, rows in by_policy.items():
        recs = [float(r["score"].get("expected_file_recall") or 0) for r in rows]
        noises = [int(r["score"].get("extra_noise_count") or 0) for r in rows]
        sizes = [int(r["score"].get("file_set_size") or 0) for r in rows]
        forb = [int(r["score"].get("forbidden_hit_count") or 0) for r in rows]
        summary[pol] = {
            "kind": POLICY_KINDS.get(pol, ""),
            "run_count": len(rows),
            "mean_expected_file_recall": mean(recs),
            "mean_extra_noise_files": mean(noises),
            "mean_file_set_size": mean(sizes),
            "mean_forbidden_hit": mean(forb),
            "min_expected_file_recall": min(recs) if recs else 0.0,
            "max_extra_noise_files": max(noises) if noises else 0,
        }

    # per-task × policy means
    tasks: Dict[str, Dict[str, Any]] = {}
    for r in runs:
        t = tasks.setdefault(
            r["task_id"],
            {
                "id": r["task_id"],
                "symbol": r.get("symbol") or "",
                "policies": {},
                "seed_invariance": {},
            },
        )
        pol = r["policy"]
        bucket = t["policies"].setdefault(
            pol,
            {
                "runs": [],
                "kind": POLICY_KINDS.get(pol, ""),
                "mean_recall": 0.0,
                "mean_noise": 0.0,
                "mean_size": 0.0,
                "mean_forbidden_hit": 0.0,
            },
        )
        bucket["runs"].append(
            {
                "seed": r["seed"],
                "run_id": r.get("run_id"),
                "recall": r["score"].get("expected_file_recall"),
                "noise": r["score"].get("extra_noise_count"),
                "size": r["score"].get("file_set_size"),
                "forbidden_hit": r["score"].get("forbidden_hit_count"),
                "fingerprint": r.get("fingerprint"),
                "errors": r.get("errors") or [],
            }
        )

    for tid, t in tasks.items():
        for pol, bucket in t["policies"].items():
            runs_b = bucket["runs"]
            bucket["mean_recall"] = round(
                sum(float(x["recall"] or 0) for x in runs_b) / max(len(runs_b), 1), 4
            )
            bucket["mean_noise"] = round(
                sum(int(x["noise"] or 0) for x in runs_b) / max(len(runs_b), 1), 4
            )
            bucket["mean_size"] = round(
                sum(int(x["size"] or 0) for x in runs_b) / max(len(runs_b), 1), 4
            )
            bucket["mean_forbidden_hit"] = round(
                sum(int(x["forbidden_hit"] or 0) for x in runs_b) / max(len(runs_b), 1), 4
            )
            fps = {x["fingerprint"] for x in runs_b if x.get("fingerprint")}
            t["seed_invariance"][pol] = len(fps) <= 1
            bucket["seed_invariant"] = len(fps) <= 1
            bucket["distinct_file_sets"] = len(fps)

    return {
        "summary_by_policy": summary,
        "tasks": tasks,
        "product_delta_B_minus_A": {
            "note": (
                "Backlog card framed B−A as the product metric when A=grep-only "
                "and B=MCP. This slice uses operator labels A=MCP recipes, "
                "B=read/grep — product signal is A noise/recall vs B (and C)."
            ),
            "mean_recall_A": summary.get("A", {}).get("mean_expected_file_recall"),
            "mean_recall_B": summary.get("B", {}).get("mean_expected_file_recall"),
            "mean_recall_C": summary.get("C", {}).get("mean_expected_file_recall"),
            "mean_noise_A": summary.get("A", {}).get("mean_extra_noise_files"),
            "mean_noise_B": summary.get("B", {}).get("mean_extra_noise_files"),
            "mean_noise_C": summary.get("C", {}).get("mean_extra_noise_files"),
            "noise_delta_B_minus_A": round(
                float(summary.get("B", {}).get("mean_extra_noise_files") or 0)
                - float(summary.get("A", {}).get("mean_extra_noise_files") or 0),
                4,
            ),
        },
    }


def render_markdown(payload: Dict[str, Any]) -> str:
    """Render human table from a result payload that embeds aggregate results."""
    agg = payload.get("summary") or {}
    if "summary_by_policy" not in agg and "tasks" in payload:
        # already-aggregate-shaped
        agg = payload
    summary_by_pol = agg.get("summary_by_policy") or payload.get("summary_by_policy") or {}
    tasks = agg.get("tasks") or payload.get("tasks") or {}
    delta = payload.get("delta") or agg.get("product_delta_B_minus_A") or {}
    lines: List[str] = []
    lines.append("## Scripted tool-policy A/B/C score table")
    lines.append("")
    lines.append(
        "**Honesty:** scripted deterministic tool-policy agents — "
        "**not** live LLM agents. Public fixtures only. "
        "See `docs/eval-agent-baseline.md`."
    )
    lines.append("")
    lines.append(
        "| task | symbol | A recall | A noise | B recall | B noise | C recall | C noise | A seed-invariant |"
    )
    lines.append("|---|---|---:|---:|---:|---:|---:|---:|---|")
    for tid in sorted(tasks.keys()):
        t = tasks[tid]
        pol = t.get("policies") or {}

        def cell(p: str, key: str, _pol=pol) -> str:
            b = _pol.get(p) or {}
            val = b.get(key)
            if val is None:
                return "—"
            if key == "mean_recall":
                return f"{float(val) * 100:.0f}%"
            return f"{val}"

        inv = (t.get("seed_invariance") or {}).get("A")
        inv_s = "Y" if inv else ("N" if inv is False else "—")
        lines.append(
            f"| {tid} | {t.get('symbol') or ''} "
            f"| {cell('A', 'mean_recall')} | {cell('A', 'mean_noise')} "
            f"| {cell('B', 'mean_recall')} | {cell('B', 'mean_noise')} "
            f"| {cell('C', 'mean_recall')} | {cell('C', 'mean_noise')} "
            f"| {inv_s} |"
        )
    lines.append("")
    lines.append("### Aggregate (mean over tasks × runs)")
    lines.append("")
    lines.append("| policy | kind | runs | mean recall | mean noise files | mean set size |")
    lines.append("|---|---|---:|---:|---:|---:|")
    for pol in ("A", "B", "C"):
        s = summary_by_pol.get(pol) or {}
        lines.append(
            f"| {pol} | {s.get('kind') or ''} | {s.get('run_count') or 0} "
            f"| {float(s.get('mean_expected_file_recall') or 0) * 100:.0f}% "
            f"| {s.get('mean_extra_noise_files') or 0} "
            f"| {s.get('mean_file_set_size') or 0} |"
        )
    nd = delta.get("noise_delta_B_minus_A", "—")
    lines.append("")
    lines.append(
        f"Product signal (noise): B − A = **{nd}** "
        f"extra-noise files on average (positive → recipes cleaner than read/grep)."
    )
    lines.append("")
    lines.append(
        "Primary product read on these public structure-fact fixtures: "
        "**A (recipe policy) keeps noise at 0** while **B/C pull token/comment "
        "collisions**. Recall alone is not the differentiator when symbols "
        "appear in expected files. This is **not** a live LLM product claim."
    )
    lines.append("")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def cmd_run(args: argparse.Namespace) -> int:
    repo = args.repo.resolve() if args.repo else repo_root_from_script()
    fixtures = (
        args.fixtures.resolve()
        if args.fixtures
        else repo / "fixtures" / "eval-agent-tasks"
    )
    if not fixtures.is_dir():
        print(f"error: fixtures dir missing: {fixtures}", file=sys.stderr)
        return 2
    try:
        bin_path = find_bin(args.bin)
    except SystemExit as e:
        print(f"error: {e}", file=sys.stderr)
        return 2

    task_dirs = sorted(
        p for p in fixtures.iterdir() if p.is_dir() and (p / "task.json").is_file()
    )
    if args.task:
        wanted = set(args.task)
        filtered = []
        for p in task_dirs:
            try:
                meta = json.loads((p / "task.json").read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                continue
            if p.name in wanted or meta.get("id") in wanted:
                filtered.append(p)
        task_dirs = filtered

    if not task_dirs:
        print("error: no tasks selected", file=sys.stderr)
        return 2
    if not args.task and len(task_dirs) < 6:
        print(
            f"error: P0-5 public slice needs >=6 tasks, found {len(task_dirs)}",
            file=sys.stderr,
        )
        return 2

    seeds = list(args.seeds) if args.seeds is not None else list(range(args.runs))
    if len(seeds) < args.min_runs:
        print(
            f"error: need >= {args.min_runs} seeds/runs per policy, got {seeds}",
            file=sys.stderr,
        )
        return 2

    out_path = args.out or (repo / "target" / "agent_ab_eval.json")
    md_path = args.markdown or out_path.with_suffix(".md")
    traj_dir = (args.traj_dir or (repo / "evals" / "agent-ab")).resolve()
    out_path.parent.mkdir(parents=True, exist_ok=True)

    import tempfile

    work_root = Path(tempfile.mkdtemp(prefix="agentgraph-eval-agent-ab-"))
    print(f"bin: {bin_path}")
    print(f"fixtures: {fixtures}")
    print(f"work: {work_root}")
    print(f"seeds: {seeds}")
    print(f"traj_dir: {traj_dir}")
    print(f"tasks: {len(task_dirs)}")
    print()

    all_runs: List[Dict[str, Any]] = []
    trajectory_paths: List[str] = []
    gate_failures: List[str] = []

    try:
        for td in task_dirs:
            try:
                meta = load_task_meta(td)
            except json.JSONDecodeError as e:
                gate_failures.append(f"{td.name}: bad task.json: {e}")
                continue
            tid = meta.get("id") or td.name
            labels = task_labels(meta)
            fixture_rel = f"fixtures/eval-agent-tasks/{td.name}"
            print(f"→ {tid}  symbol={meta.get('symbol')!r}")

            work = work_root / tid
            copy_fixture(td, work)

            # Policy A: copy once, index once per seed is wasteful — still
            # record N runs (seed metadata). Reuse same workdir after first index.
            for seed in seeds:
                run_a = run_policy_a(bin_path, td, meta, work, seed, depth=args.depth)
                traj_a = build_trajectory(run_a, meta, labels, fixture_rel)
                replay_a = score_trajectory(traj_a)
                if not replay_a["recorded_matches_replay"]:
                    run_a.errors.append(
                        "replay mismatch: " + "; ".join(replay_a["mismatches"])
                    )
                    traj_a["errors"] = run_a.errors
                if args.write_traj:
                    p = traj_dir / tid / f"run-a-{seed}.json"
                    write_trajectory(p, traj_a)
                    trajectory_paths.append(str(p))
                all_runs.append(
                    {
                        "task_id": tid,
                        "symbol": meta.get("symbol") or "",
                        "policy": "A",
                        "seed": seed,
                        "run_id": f"a-{seed}",
                        "kind": POLICY_KINDS["A"],
                        "score": traj_a["score"],
                        "errors": run_a.errors,
                        "fingerprint": trajectory_fingerprint(traj_a),
                        "extras_honesty": {
                            "window": run_a.extras.get("window"),
                            "note_honest": run_a.extras.get("note_honest"),
                            "recommendation_present": run_a.extras.get(
                                "recommendation_present"
                            ),
                        },
                    }
                )
                # A gates (structure-fact recipe quality)
                if run_a.errors:
                    gate_failures.append(f"{tid}/A/{seed}: {run_a.errors}")
                else:
                    if traj_a["score"]["expected_file_recall"] < args.strict_recall:
                        gate_failures.append(
                            f"{tid}/A/{seed}: recall "
                            f"{traj_a['score']['expected_file_recall']} "
                            f"< {args.strict_recall}"
                        )
                    if traj_a["score"]["extra_noise_count"] != 0:
                        gate_failures.append(
                            f"{tid}/A/{seed}: structure noise "
                            f"{traj_a['score']['extra_noise_files']}"
                        )
                    if run_a.extras.get("note_honest") is False:
                        gate_failures.append(f"{tid}/A/{seed}: note not honest")

            # Policies B and C need a clean fixture copy (no .agentgraph required)
            work_bc = work_root / f"{tid}-bc"
            copy_fixture(td, work_bc)
            for seed in seeds:
                run_b = run_policy_b(td, meta, work_bc, seed)
                traj_b = build_trajectory(run_b, meta, labels, fixture_rel)
                replay_b = score_trajectory(traj_b)
                if not replay_b["recorded_matches_replay"]:
                    run_b.errors.append(
                        "replay mismatch: " + "; ".join(replay_b["mismatches"])
                    )
                    traj_b["errors"] = run_b.errors
                if args.write_traj:
                    p = traj_dir / tid / f"run-b-{seed}.json"
                    write_trajectory(p, traj_b)
                    trajectory_paths.append(str(p))
                all_runs.append(
                    {
                        "task_id": tid,
                        "symbol": meta.get("symbol") or "",
                        "policy": "B",
                        "seed": seed,
                        "run_id": f"b-{seed}",
                        "kind": POLICY_KINDS["B"],
                        "score": traj_b["score"],
                        "errors": run_b.errors,
                        "fingerprint": trajectory_fingerprint(traj_b),
                    }
                )
                if run_b.errors:
                    gate_failures.append(f"{tid}/B/{seed}: {run_b.errors}")

                run_c = run_policy_c(td, meta, work_bc, seed)
                traj_c = build_trajectory(run_c, meta, labels, fixture_rel)
                if args.write_traj:
                    p = traj_dir / tid / f"run-c-{seed}.json"
                    write_trajectory(p, traj_c)
                    trajectory_paths.append(str(p))
                all_runs.append(
                    {
                        "task_id": tid,
                        "symbol": meta.get("symbol") or "",
                        "policy": "C",
                        "seed": seed,
                        "run_id": f"c-{seed}",
                        "kind": POLICY_KINDS["C"],
                        "score": traj_c["score"],
                        "errors": run_c.errors,
                        "fingerprint": trajectory_fingerprint(traj_c),
                    }
                )

            # brief console line
            def _means(pol: str) -> Tuple[str, str]:
                rows = [
                    r for r in all_runs if r["task_id"] == tid and r["policy"] == pol
                ]
                if not rows:
                    return "—", "—"
                rec = sum(float(r["score"]["expected_file_recall"]) for r in rows) / len(rows)
                nz = sum(int(r["score"]["extra_noise_count"]) for r in rows) / len(rows)
                return f"{rec * 100:.0f}%", f"{nz:.1f}"

            ar, an = _means("A")
            br, bn = _means("B")
            cr, cn = _means("C")
            print(f"  A {ar}/{an} noise  B {br}/{bn}  C {cr}/{cn}")

        print()
        agg = aggregate_results(all_runs)
        payload: Dict[str, Any] = {
            "schema": RESULT_SCHEMA,
            "description": (
                "P0-5 scripted tool-policy A/B/C evals on public "
                "fixtures/eval-agent-tasks. A=agentgraph MCP/CLI recipe policy, "
                "B=read/grep policy (no agentgraph), C=name-grep control. "
                "These are scripted deterministic agents, not live LLM agents."
            ),
            "non_claims": [
                "not a live LLM agent experiment (P0-5b would be)",
                "public synthetic fixtures only — no private stock corpus",
                "scores are structure-fact file-set metrics on these fixtures",
                "window=sound is ast_modeled engineering S gate, not ecosystem sound",
                "name-grep / read-grep baselines are deterministic, not LLM",
            ],
            "honesty": {
                "live_llm_agent": False,
                "kind": "scripted_tool_policy_agents",
                "note": HONESTY_NOTE,
            },
            "binary": str(bin_path),
            "fixtures": str(fixtures),
            "seeds": seeds,
            "runs_per_policy_per_task": len(seeds),
            "task_count": len({r["task_id"] for r in all_runs}),
            "trajectory_count": len(all_runs),
            "trajectories_written": trajectory_paths,
            "traj_dir": str(traj_dir),
            "gates": {
                "strict_recall_A": args.strict_recall,
                "failures": gate_failures,
                "ok": not gate_failures,
            },
            "summary": agg,
            "delta": agg["product_delta_B_minus_A"],
            "runs": all_runs,
            "policy_definitions": {
                "A": (
                    "Scripted agent-like tool policy: index then blast-radius / "
                    "who-calls / find / related / subset; file set = recipe "
                    "structure paths (workspace-resolved)."
                ),
                "B": (
                    "Scripted agent-like read/grep policy: walk sources, grep "
                    "symbol tokens, bounded definition reads, same-dir sibling "
                    "expansion, secondary identifier greps. No agentgraph."
                ),
                "C": "name-grep control from P0-1 (symbol token in source files).",
            },
        }
        payload["summary"] = agg
        payload["delta"] = agg["product_delta_B_minus_A"]
        md = render_markdown(payload)

        out_path.write_text(
            json.dumps(payload, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        md_path.write_text(md + "\n", encoding="utf-8")
        print(md)
        print(f"wrote {out_path}")
        print(f"wrote {md_path}")

        if gate_failures:
            print("\ngate failures:", file=sys.stderr)
            for g in gate_failures:
                print(f"  - {g}", file=sys.stderr)
            return 1
        return 0
    finally:
        if not args.keep_work:
            shutil.rmtree(work_root, ignore_errors=True)


def cmd_score(args: argparse.Namespace) -> int:
    paths: List[Path] = []
    if args.trajectory:
        p = Path(args.trajectory)
        if not p.is_file():
            print(f"error: trajectory not found: {p}", file=sys.stderr)
            return 2
        paths.append(p)
    if args.traj_dir:
        paths.extend(discover_traj_files(Path(args.traj_dir)))
    if not paths:
        print("error: pass --trajectory and/or --traj-dir", file=sys.stderr)
        return 2

    results = []
    bad = 0
    for p in paths:
        try:
            payload = json.loads(p.read_text(encoding="utf-8"))
            sc = score_trajectory(payload)
        except Exception as e:  # noqa: BLE001 — report any replay failure
            bad += 1
            results.append({"path": str(p), "error": str(e)})
            print(f"FAIL {p}: {e}", file=sys.stderr)
            continue
        sc["path"] = str(p)
        results.append(sc)
        status = "OK" if sc["recorded_matches_replay"] else "MISMATCH"
        if not sc["recorded_matches_replay"] or sc["errors"]:
            bad += 1
            status = "FAIL" if sc["errors"] else status
        print(
            f"{status} {sc['task_id']}/{sc['policy']}/{sc['run_id']} "
            f"recall={sc['recomputed']['expected_file_recall']} "
            f"noise={sc['recomputed']['extra_noise_count']} "
            f"forbidden={sc['recomputed']['forbidden_hit_count']}"
        )

    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(
            json.dumps(
                {
                    "schema": "agentgraph.eval_agent_ab.replay.v1",
                    "offline": True,
                    "network_required": False,
                    "results": results,
                },
                indent=2,
                ensure_ascii=False,
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"wrote {args.out}")

    print(f"scored {len(results)} trajectory(ies); problems={bad}")
    return 1 if bad else 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="P0-5 scripted tool-policy A/B/C evals (not live LLM)"
    )
    sub = parser.add_subparsers(dest="command")

    run_p = sub.add_parser("run", help="run A/B/C policies and write trajectories")
    run_p.add_argument("--repo", type=Path, default=None)
    run_p.add_argument("--fixtures", type=Path, default=None)
    run_p.add_argument("--bin", type=str, default=None)
    run_p.add_argument("--out", type=Path, default=None)
    run_p.add_argument("--markdown", type=Path, default=None)
    run_p.add_argument("--traj-dir", type=Path, default=None)
    run_p.add_argument("--task", action="append", default=None)
    run_p.add_argument("--runs", type=int, default=3, help="runs per policy per task")
    run_p.add_argument(
        "--seeds",
        type=lambda s: [int(x) for x in s.split(",") if x.strip() != ""],
        default=None,
        help="comma-separated seeds (default 0..runs-1)",
    )
    run_p.add_argument("--min-runs", type=int, default=3)
    run_p.add_argument("--depth", type=int, default=3)
    run_p.add_argument("--strict-recall", type=float, default=0.5)
    run_p.add_argument("--keep-work", action="store_true")
    run_p.add_argument(
        "--no-write-traj",
        dest="write_traj",
        action="store_false",
        help="do not write evals/agent-ab trajectories",
    )
    run_p.set_defaults(write_traj=True)

    score_p = sub.add_parser("score", help="offline replay scoring")
    score_p.add_argument("--trajectory", type=Path, default=None)
    score_p.add_argument("--traj-dir", type=Path, default=None)
    score_p.add_argument("--out", type=Path, default=None)

    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    # Allow bare `python scripts/eval_agent_ab.py` → run
    known_subs = {"run", "score"}
    if not argv or (argv[0] not in known_subs and argv[0] not in {"-h", "--help"}):
        argv = ["run", *argv]

    parser = build_parser()
    args = parser.parse_args(argv)
    cmd = getattr(args, "command", "run") or "run"
    if cmd == "score":
        return cmd_score(args)
    return cmd_run(args)


if __name__ == "__main__":
    sys.exit(main())
