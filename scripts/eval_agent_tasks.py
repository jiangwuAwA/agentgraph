#!/usr/bin/env python3
"""P0-1 public Agent code-change task evals harness.

Indexes each public fixture under ``fixtures/eval-agent-tasks/``, runs the
agent-facing recipes (``blast-radius`` / ``who-calls`` / ``subset`` / optional
``graph``), and scores structure facts vs a **name-grep baseline** (symbol
token in source files). No private corpus. No fabricated LLM numbers.

Outputs
-------
- machine-readable ``target/agent_task_eval.json``
- printed score table

Exit codes
----------
0 — harness completed; honesty/recommendation/window gates green
1 — a required tool failed or a honesty/behavior gate failed
2 — usage / IO error
"""
from __future__ import annotations

import argparse
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Sequence, Set, Tuple

if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
        sys.stderr.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:
        sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
        sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")

SOURCE_EXTS = {".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".rs"}
SKIP_DIRS = {".agentgraph", "target", "node_modules", ".git", "__pycache__"}


def repo_root_from_script() -> Path:
    return Path(__file__).resolve().parent.parent


def _bin_has_recipes(path: Path) -> bool:
    try:
        proc = subprocess.run(
            [str(path), "--help"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=20,
        )
    except Exception:
        return False
    text = (proc.stdout or "") + (proc.stderr or "")
    return "blast-radius" in text and "who-calls" in text


def find_bin(cli_arg: Optional[str]) -> Path:
    if cli_arg:
        p = Path(cli_arg)
        if p.is_file():
            return p
        raise SystemExit(f"--bin not found: {p}")
    env = os.environ.get("CARGO_BIN_EXE_agentgraph")
    candidates: List[Path] = []
    if env and Path(env).is_file():
        candidates.append(Path(env))
    root = repo_root_from_script()
    for rel in (
        "target/debug/agentgraph.exe",
        "target/debug/agentgraph",
        "target/release/agentgraph.exe",
        "target/release/agentgraph",
    ):
        p = root / rel
        if p.is_file():
            candidates.append(p)
    which = shutil.which("agentgraph")
    if which:
        candidates.append(Path(which))
    # Prefer a binary that actually exposes agent recipes (P0-4/P3).
    for p in candidates:
        if _bin_has_recipes(p):
            return p
    if candidates:
        return candidates[0]
    raise SystemExit(
        "agentgraph binary not found (build with cargo build, "
        "or pass --bin PATH)"
    )


def norm_path(p: str, prefixes: Sequence[Path]) -> str:
    s = p.replace("\\", "/")
    for pref in prefixes:
        ps = str(pref).replace("\\", "/")
        if s.startswith(ps):
            s = s[len(ps) :].lstrip("/")
            break
        # Windows drive-letter variants
        if s.lower().startswith(ps.lower()):
            s = s[len(ps) :].lstrip("/")
            break
    while s.startswith("./"):
        s = s[2:]
    return s


def collect_source_files(root: Path) -> List[Path]:
    out: List[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for fn in filenames:
            if Path(fn).suffix.lower() in SOURCE_EXTS:
                out.append(Path(dirpath) / fn)
    return out


def name_grep_files(root: Path, symbol: str) -> List[str]:
    """Baseline: files whose source contains the symbol as a token."""
    if not symbol:
        return []
    rx = re.compile(rf"(?<![A-Za-z0-9_]){re.escape(symbol)}(?![A-Za-z0-9_])")
    hits: List[str] = []
    for path in collect_source_files(root):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if rx.search(text):
            hits.append(norm_path(str(path.relative_to(root)), [root]))
    return sorted(set(hits))


def _as_posix(p: Any) -> str:
    return str(p).replace("\\", "/")


def resolve_row_path(row: Dict[str, Any], work: Path, prefixes: Sequence[Path]) -> str:
    """Map a query row to a fixture-relative posix path.

    Workspace stores tag rows with `root_id` + `root_path`; `path` is then
    **root-relative** (e.g. `src/lib.rs` under `packages/engine`).
    """
    raw = row.get("path")
    if not raw and isinstance(row.get("at"), str):
        at = row["at"]
        maybe = at.rsplit(":", 1)[0]
        if Path(maybe).suffix:
            raw = maybe
    if not raw:
        return ""
    path = _as_posix(raw)
    root_path = row.get("root_path") or row.get("rootPath")
    if path and root_path:
        try:
            rp = Path(root_path)
            full = (rp / path)
            try:
                return full.resolve().relative_to(work.resolve()).as_posix()
            except Exception:
                pass
            rp_s = _as_posix(root_path)
            wp_s = _as_posix(work)
            if rp_s.lower().startswith(wp_s.lower()):
                pref = rp_s[len(wp_s) :].lstrip("/")
                return f"{pref}/{path}" if pref else path
        except Exception:
            pass
    # resolved definition path often absolute or root-relative
    return norm_path(path, prefixes)


def files_from_rows(
    rows: Iterable[Any], work: Path, prefixes: Sequence[Path]
) -> Tuple[Set[str], Set[str]]:
    """Return (primary paths, secondary definition/resolved paths)."""
    primary: Set[str] = set()
    secondary: Set[str] = set()
    for n in rows or []:
        if not isinstance(n, dict):
            continue
        p = resolve_row_path(n, work, prefixes)
        if p:
            primary.add(p)
        resolved = n.get("resolved") or n.get("definition_path")
        if resolved:
            rp = resolve_row_path({"path": resolved, "root_path": n.get("root_path")}, work, prefixes)
            if rp:
                secondary.add(rp)
            else:
                rp2 = norm_path(_as_posix(resolved), prefixes)
                if rp2:
                    secondary.add(rp2)
    return primary, secondary


def files_from_nodes(nodes: Iterable[Any]) -> Set[str]:
    out: Set[str] = set()
    for n in nodes or []:
        if isinstance(n, dict):
            p = n.get("path") or n.get("at")
            if isinstance(p, str) and p:
                if ":" in p:
                    maybe = p.rsplit(":", 1)[0]
                    if Path(maybe).suffix:
                        p = maybe
                out.add(p.replace("\\", "/"))
    return out


def recall(expected: Sequence[str], found: Set[str]) -> Tuple[float, List[str], List[str]]:
    exp = [e.replace("\\", "/") for e in expected]
    hit = [e for e in exp if e in found]
    miss = [e for e in exp if e not in found]
    if not exp:
        return 1.0, [], []
    return len(hit) / len(exp), hit, miss


def extra_noise(noise: Sequence[str], found: Set[str]) -> List[str]:
    nset = {n.replace("\\", "/") for n in noise}
    return sorted(nset & found)


def run_cli(
    bin_path: Path,
    cwd: Path,
    args: Sequence[str],
    timeout: int = 120,
) -> Tuple[int, str, str]:
    cmd = [str(bin_path), *args]
    env = os.environ.copy()
    env.setdefault("PYTHONUTF8", "1")
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            env=env,
        )
        return proc.returncode, proc.stdout or "", proc.stderr or ""
    except FileNotFoundError as e:
        return 127, "", f"binary not found: {e}"
    except subprocess.TimeoutExpired:
        return 124, "", f"timeout after {timeout}s: {' '.join(cmd)}"


def parse_json_stdout(stdout: str) -> Optional[Any]:
    stdout = stdout.strip()
    if not stdout:
        return None
    # CLI may print extra lines; take the last JSON object/array-looking chunk.
    candidates = [stdout]
    # try progressive from first '{' or '['
    for ch in ("{", "["):
        i = stdout.find(ch)
        if i > 0:
            candidates.append(stdout[i:])
    for c in candidates:
        try:
            return json.loads(c)
        except json.JSONDecodeError:
            continue
    # last resort: first complete-looking JSON line
    for line in reversed(stdout.splitlines()):
        line = line.strip()
        if line.startswith("{") or line.startswith("["):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


@dataclass
class TaskResult:
    id: str
    theme: str
    language: str
    symbol: str
    issue: str
    recommended_commands: List[str]
    expected_files: List[str]
    noise_files: List[str]
    agentgraph: Dict[str, Any] = field(default_factory=dict)
    name_grep_baseline: Dict[str, Any] = field(default_factory=dict)
    checks: Dict[str, Any] = field(default_factory=dict)
    errors: List[str] = field(default_factory=list)

    @property
    def pass_fail(self) -> bool:
        if self.errors:
            return False
        c = self.checks
        gates = c.get("gates", {})
        return all(bool(v) for v in gates.values())


def copy_fixture(src: Path, dst: Path) -> None:
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(
        src,
        dst,
        ignore=shutil.ignore_patterns("task.json", ".agentgraph", "target", "__pycache__"),
    )


def score_task(
    bin_path: Path,
    task_dir: Path,
    meta: Dict[str, Any],
    work_root: Path,
    run_graph: bool,
    depth: int,
) -> TaskResult:
    tid = meta.get("id") or task_dir.name
    expected = meta.get("expected") or {}
    symbol = meta.get("symbol") or ""
    result = TaskResult(
        id=tid,
        theme=meta.get("theme") or "",
        language=meta.get("language") or "",
        symbol=symbol,
        issue=meta.get("issue") or "",
        recommended_commands=list(meta.get("recommended_commands") or []),
        expected_files=list(expected.get("files_that_matter") or []),
        noise_files=list(expected.get("noise_files") or []),
    )
    adjacent = list(expected.get("adjacent_callee_files") or [])

    ws = meta.get("workspace")
    work = work_root / tid
    copy_fixture(task_dir, work)
    prefixes = [work, task_dir]

    # --- index ---
    index_args: List[str] = []
    if ws and ws.get("manifest"):
        manifest = work / ws["manifest"]
        if not manifest.is_file():
            result.errors.append(f"workspace manifest missing: {ws['manifest']}")
            return result
        index_args = [
            "index",
            "--workspace",
            str(manifest),
            "--force",
        ]
    elif ws and ws.get("roots"):
        index_args = ["index"]
        for r in ws["roots"]:
            index_args += ["--workspace-root", str(work / r["path"])]
        db = work / "ws.db"
        index_args += ["--workspace-db", str(db), "--force"]
    else:
        index_args = ["--root", str(work), "index", "--force"]

    code, out, err = run_cli(bin_path, work, index_args)
    if code != 0:
        result.errors.append(f"index failed ({code}): {err.strip()[:400]}")
        return result

    def query_args(cmd: str, *extra: str) -> List[str]:
        args: List[str] = []
        if ws and ws.get("manifest"):
            args += [cmd, *extra, "--workspace", str(work / ws["manifest"])]
        elif ws and ws.get("roots"):
            args += [cmd, *extra, "--workspace-db", str(work / "ws.db")]
        else:
            args += ["--root", str(work), cmd, *extra]
        return args

    # --- blast-radius ---
    code, out, err = run_cli(
        bin_path, work, query_args("blast-radius", symbol, "--depth", str(depth))
    )
    if code != 0:
        result.errors.append(f"blast-radius failed ({code}): {err.strip()[:400]}")
        return result
    blast = parse_json_stdout(out)
    if not isinstance(blast, dict):
        result.errors.append("blast-radius: non-JSON payload")
        return result

    nodes = blast.get("nodes") or blast.get("impact") or []
    node_paths, node_resolved = files_from_rows(nodes, work, prefixes)

    # Definition files via `find` — an agent changing a symbol always opens these.
    def_files: Set[str] = set()
    code_f, out_f, _err_f = run_cli(bin_path, work, query_args("find", symbol))
    if code_f == 0:
        found = parse_json_stdout(out_f)
        if isinstance(found, list):
            def_files, _ = files_from_rows(found, work, prefixes)
        elif isinstance(found, dict) and isinstance(found.get("results"), list):
            def_files, _ = files_from_rows(found["results"], work, prefixes)

    # `related` = definition + importers + references (scope retrieval).
    # Blast-radius is dependents; callees/siblings show up here.
    related_files: Set[str] = set()
    code_r, out_r, _err_r = run_cli(
        bin_path, work, query_args("related", symbol, "--limit", "20")
    )
    if code_r == 0:
        rel = parse_json_stdout(out_r)
        if isinstance(rel, list):
            for row in rel:
                if isinstance(row, dict) and row.get("path"):
                    related_files.add(norm_path(_as_posix(row["path"]), prefixes))
                    # workspace: related paths may be root-relative without root_path
                    root_path = row.get("root_path")
                    if root_path:
                        rp = resolve_row_path(row, work, prefixes)
                        if rp:
                            related_files.add(rp)

    # Agent structure-fact file set =
    #   blast dependents ∪ node.resolved ∪ find(definition) ∪ related
    # (agentgraph tools an agent is instructed to call — not a raw grep set)
    blast_paths = set(node_paths) | set(node_resolved) | set(def_files) | set(related_files)
    blast_paths = {norm_path(p, prefixes) for p in blast_paths if p}

    rec, hit, miss = recall(result.expected_files, blast_paths)
    noise = extra_noise(result.noise_files, blast_paths)
    # dependent-only recall (blast nodes without def/resolved union) for honesty
    dep_rec, dep_hit, dep_miss = recall(result.expected_files, {norm_path(p, prefixes) for p in node_paths})
    rec_text = blast.get("recommendation")
    rec_present = isinstance(rec_text, str) and len(rec_text.strip()) >= 8
    note = blast.get("note") or ""
    note_honest = isinstance(note, str) and (
        "not a complete runtime graph" in note or "非完整运行时图" in note
    )
    window = blast.get("window")
    subset_ok = blast.get("subset_ok")
    promise_tier = blast.get("promise_tier")
    sound_candidates = blast.get("sound_candidates")
    example_command = blast.get("example_command")

    # window honesty
    exp_window = (expected.get("window") or "any").lower()
    window_honest = True
    window_detail = f"window={window!r} subset_ok={subset_ok!r}"
    if exp_window == "sound":
        window_honest = window == "sound" and subset_ok is True
    elif exp_window in {"default", "not_sound", "disabled"}:
        window_honest = window != "sound"
    elif exp_window == "any":
        window_honest = window in {"sound", "default", "disabled"}
    # never claim sound when subset_ok is false
    if subset_ok is False and window == "sound":
        window_honest = False
        window_detail += " (invalid: sound with subset_ok=false)"

    rec_contains_any = list(expected.get("recommendation_contains_any") or [])
    rec_contains_ok = True
    if rec_contains_any and isinstance(rec_text, str):
        rec_contains_ok = any(s.lower() in rec_text.lower() for s in rec_contains_any)
    elif rec_contains_any:
        rec_contains_ok = False

    # --- who-calls ---
    who_payload: Optional[Dict[str, Any]] = None
    who_checks: Dict[str, Any] = {}
    who_spec = expected.get("who_calls")
    if who_spec:
        code, out, err = run_cli(bin_path, work, query_args("who-calls", symbol, "--limit", "50"))
        if code != 0:
            result.errors.append(f"who-calls failed ({code}): {err.strip()[:400]}")
        else:
            wj = parse_json_stdout(out)
            if isinstance(wj, dict):
                who_payload = wj
                high = wj.get("high_freq_name")
                imp_count = wj.get("implementor_count") or 0
                callers = wj.get("callers") or []
                implementors = wj.get("implementors") or []
                if isinstance(callers, dict):
                    callers = callers.get("callers") or []
                rec_w = wj.get("recommendation") or ""
                who_checks["high_freq_name"] = (
                    bool(high) if who_spec.get("high_freq_name") else True
                )
                if who_spec.get("high_freq_name") is True and not high:
                    who_checks["high_freq_name"] = False
                if who_spec.get("high_freq_name") is False:
                    who_checks["high_freq_name"] = high is False or high is None
                min_imp = int(who_spec.get("min_implementor_count") or 0)
                who_checks["implementor_count"] = imp_count >= min_imp
                if who_spec.get("expect_implementors_separated"):
                    caller_files = files_from_nodes(callers)
                    # default separate mode: callers[] must not be flooded with
                    # implementor role rows
                    has_impl_in_callers = any(
                        isinstance(r, dict) and r.get("edge_role") == "implementor"
                        for r in (callers if isinstance(callers, list) else [])
                    )
                    who_checks["implementors_separated"] = not has_impl_in_callers
                    who_checks["recommendation_mentions_impl"] = (
                        "implementor" in str(rec_w).lower() or "separat" in str(rec_w).lower()
                    )
                    who_checks["caller_files"] = sorted(
                        norm_path(p, prefixes) for p in caller_files
                    )
                    who_checks["implementor_count_value"] = imp_count
                    who_checks["callers_len"] = len(callers) if isinstance(callers, list) else 0
                    who_checks["implementors_len"] = (
                        len(implementors) if isinstance(implementors, list) else 0
                    )
                    who_checks["high_freq_flag"] = high
                    who_checks["recommendation"] = rec_w

    # --- subset (optional honesty companion) ---
    subset_payload: Optional[Dict[str, Any]] = None
    subset_code = None
    code, out, err = run_cli(bin_path, work, query_args("subset"))
    subset_code = code
    # subset may exit 2 on violations — still parse payload
    sj = parse_json_stdout(out)
    if isinstance(sj, dict):
        subset_payload = {
            "in_subset": sj.get("in_subset"),
            "violation_count": sj.get("violation_count"),
            "promise_tier": sj.get("promise_tier"),
            "recommendation_present": isinstance(sj.get("recommendation"), str)
            and len(str(sj.get("recommendation"))) >= 8,
            "sound_candidates_count": len(sj.get("sound_candidates") or []),
            "exit_code": subset_code,
        }

    # --- optional graph ---
    graph_payload: Optional[Dict[str, Any]] = None
    if run_graph:
        code, out, err = run_cli(
            bin_path,
            work,
            query_args("graph", symbol, "--depth", "2"),
        )
        if code == 0:
            gj = parse_json_stdout(out)
            if isinstance(gj, dict):
                graph_payload = {
                    "window": gj.get("window"),
                    "subset_ok": gj.get("subset_ok"),
                    "promise_tier": gj.get("promise_tier"),
                    "note_present": isinstance(gj.get("note"), str)
                    and "complete runtime graph" in str(gj.get("note")),
                    "node_count": gj.get("node_count"),
                    "edge_count": gj.get("edge_count"),
                }
        else:
            # graph write may be string-only / fail on some configs — non-fatal
            graph_payload = {"error": err.strip()[:200], "exit_code": code}

    # --- scoped guidance ---
    scoped_checks: Dict[str, Any] = {}
    scoped_spec = expected.get("scoped_guidance")
    if scoped_spec:
        cands = sound_candidates if isinstance(sound_candidates, list) else []
        scoped_checks["sound_candidates_present"] = len(cands) > 0
        if scoped_spec.get("expect_sound_candidates"):
            scoped_checks["sound_candidates_present"] = len(cands) > 0
        if scoped_spec.get("expect_window_not_sound"):
            scoped_checks["window_not_sound"] = window != "sound"
        elig_root = scoped_spec.get("expect_eligible_root")
        if elig_root:
            found_elig = False
            keys = []
            for c in cands:
                if not isinstance(c, dict):
                    continue
                key = str(c.get("root_id") or c.get("key") or c.get("path") or "")
                keys.append(key)
                if c.get("sound_eligible") and key == elig_root:
                    found_elig = True
                elif key == elig_root and c.get("sound_eligible") is True:
                    found_elig = True
            scoped_checks["eligible_root"] = found_elig
            scoped_checks["candidate_keys"] = keys
        need = list(scoped_spec.get("recommendation_contains_any") or [])
        if need:
            rec_l = (rec_text or "").lower()
            scoped_checks["recommendation_mentions_scoped"] = any(
                s.lower() in rec_l for s in need
            )
            # also accept example_command / by_root fields
            if not scoped_checks["recommendation_mentions_scoped"]:
                blob = json.dumps(
                    {
                        "recommendation": rec_text,
                        "example_command": example_command,
                        "sound_candidates": sound_candidates,
                    },
                    ensure_ascii=False,
                ).lower()
                scoped_checks["recommendation_mentions_scoped"] = any(
                    s.lower() in blob for s in need
                )
        if scoped_spec.get("companion_subset_sound_candidates"):
            sub_cands = ((subset_payload or {}).get("sound_candidates_count") or 0)
            scoped_checks["companion_subset_sound_candidates"] = sub_cands > 0

    # --- name-grep baseline ---
    baseline_files = name_grep_files(work, symbol)
    base_set = set(baseline_files)
    b_rec, b_hit, b_miss = recall(result.expected_files, base_set)
    b_noise = extra_noise(result.noise_files, base_set)

    result.agentgraph = {
        "blast_files": sorted(blast_paths),
        "blast_file_count": len(blast_paths),
        "blast_node_files": sorted(norm_path(p, prefixes) for p in node_paths),
        "definition_files": sorted(norm_path(p, prefixes) for p in def_files),
        "related_files": sorted(related_files),
        "expected_file_recall": round(rec, 4),
        "dependent_only_recall": round(dep_rec, 4),
        "expected_hit": hit,
        "expected_miss": miss,
        "extra_noise_files": noise,
        "extra_noise_count": len(noise),
        "window": window,
        "subset_ok": subset_ok,
        "promise_tier": promise_tier,
        "recommendation_present": rec_present,
        "recommendation_contains_any_ok": rec_contains_ok,
        "recommendation": rec_text if isinstance(rec_text, str) else None,
        "note_honest": note_honest,
        "sound_candidates": sound_candidates if sound_candidates is not None else [],
        "example_command": example_command,
        "nodes_count": len(nodes) if isinstance(nodes, list) else None,
        "subset": subset_payload,
        "graph": graph_payload,
        "who_calls": who_checks or None,
        "scoped_guidance": scoped_checks or None,
        "payload_keys": sorted(blast.keys()) if isinstance(blast, dict) else [],
        "file_set_method": (
            "blast_nodes ∪ node.resolved ∪ find(definition) ∪ related "
            "(workspace paths resolved via root_path)"
        ),
    }
    result.name_grep_baseline = {
        "method": "name-grep (symbol token in source files)",
        "files": baseline_files,
        "file_count": len(baseline_files),
        "expected_file_recall": round(b_rec, 4),
        "expected_hit": b_hit,
        "expected_miss": b_miss,
        "extra_noise_files": b_noise,
        "extra_noise_count": len(b_noise),
    }

    # --- gates ---
    gates: Dict[str, bool] = {
        "index_ok": True,
        "blast_payload_ok": isinstance(blast, dict) and blast.get("tool") in {None, "blast_radius"},
        "recommendation_present": bool(rec_present) if expected.get("recommendation_present", True) else True,
        "recommendation_contains": rec_contains_ok,
        "window_honest": window_honest,
        "note_honest": note_honest,
        "expected_recall_ge": rec >= 0.5,
        "structure_noise_free": len(noise) == 0,
    }
    if who_spec:
        skip_who = {
            "caller_files",
            "recommendation",
            "high_freq_flag",
            "implementor_count_value",
            "callers_len",
            "implementors_len",
        }
        for k, v in (who_checks or {}).items():
            if k in skip_who:
                continue
            if isinstance(v, bool):
                gates[f"who_{k}"] = v
    if scoped_spec:
        if scoped_spec.get("expect_sound_candidates"):
            gates["scoped_sound_candidates"] = bool(
                (scoped_checks or {}).get("sound_candidates_present")
            )
        if scoped_spec.get("expect_window_not_sound"):
            gates["scoped_window_not_sound"] = bool(
                (scoped_checks or {}).get("window_not_sound")
            )
        if scoped_spec.get("expect_eligible_root"):
            gates["scoped_eligible_root"] = bool((scoped_checks or {}).get("eligible_root"))
        if scoped_spec.get("recommendation_contains_any"):
            gates["scoped_recommendation"] = bool(
                (scoped_checks or {}).get("recommendation_mentions_scoped")
            )
        if scoped_spec.get("companion_subset_sound_candidates"):
            gates["scoped_companion_subset"] = bool(
                (scoped_checks or {}).get("companion_subset_sound_candidates")
            )

    result.checks = {
        "gates": gates,
        "window_detail": window_detail,
        "exp_window": exp_window,
        "delta_vs_name_grep": {
            "recall_agentgraph": round(rec, 4),
            "recall_name_grep": round(b_rec, 4),
            "noise_agentgraph": len(noise),
            "noise_name_grep": len(b_noise),
            "file_count_agentgraph": len(blast_paths),
            "file_count_name_grep": len(baseline_files),
        },
    }
    return result


def fmt_pct(x: float) -> str:
    return f"{x * 100:.0f}%"


def print_table(results: Sequence[TaskResult]) -> None:
    headers = [
        "task",
        "symbol",
        "ag_recall",
        "ag_noise",
        "grep_recall",
        "grep_noise",
        "window",
        "rec",
        "pass",
    ]
    rows: List[List[str]] = []
    for r in results:
        ag = r.agentgraph
        gg = r.name_grep_baseline
        rows.append(
            [
                r.id,
                r.symbol,
                fmt_pct(float(ag.get("expected_file_recall") or 0)),
                str(ag.get("extra_noise_count", "-")),
                fmt_pct(float(gg.get("expected_file_recall") or 0)),
                str(gg.get("extra_noise_count", "-")),
                str(ag.get("window") or "-"),
                "Y" if ag.get("recommendation_present") else "N",
                "PASS" if r.pass_fail else "FAIL",
            ]
        )
    widths = [len(h) for h in headers]
    for row in rows:
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(cell))

    def line(cols: Sequence[str]) -> str:
        return " | ".join(c.ljust(widths[i]) for i, c in enumerate(cols))

    print(line(headers))
    print("-+-".join("-" * w for w in widths))
    for row in rows:
        print(line(row))
    print()
    n_pass = sum(1 for r in results if r.pass_fail)
    print(f"tasks: {len(results)}  pass: {n_pass}  fail: {len(results) - n_pass}")
    # honest baseline summary
    ag_rec = [float(r.agentgraph.get("expected_file_recall") or 0) for r in results]
    gg_rec = [float(r.name_grep_baseline.get("expected_file_recall") or 0) for r in results]
    ag_nz = [int(r.agentgraph.get("extra_noise_count") or 0) for r in results]
    gg_nz = [int(r.name_grep_baseline.get("extra_noise_count") or 0) for r in results]
    if results:
        print(
            "mean expected-file recall — agentgraph blast: "
            f"{sum(ag_rec)/len(ag_rec):.3f}  |  name-grep baseline: "
            f"{sum(gg_rec)/len(gg_rec):.3f}"
        )
        print(
            "mean extra-noise files in set — agentgraph blast: "
            f"{sum(ag_nz)/len(ag_nz):.2f}  |  name-grep baseline: "
            f"{sum(gg_nz)/len(gg_nz):.2f}"
        )
        print(
            "baseline method: name-grep (symbol token in files) — "
            "not an LLM baseline; structure-fact fixture scores only."
        )


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="P0-1 agent code-change task evals")
    parser.add_argument("--repo", type=Path, default=None)
    parser.add_argument("--fixtures", type=Path, default=None)
    parser.add_argument("--bin", type=str, default=None)
    parser.add_argument("--out", type=Path, default=None)
    parser.add_argument("--depth", type=int, default=3)
    parser.add_argument("--graph", action="store_true", help="also run optional graph query")
    parser.add_argument(
        "--keep-work",
        action="store_true",
        help="keep temp work dirs (debug)",
    )
    parser.add_argument(
        "--task",
        action="append",
        default=None,
        help="only run these task ids (repeatable)",
    )
    parser.add_argument(
        "--strict-recall",
        type=float,
        default=0.5,
        help="minimum expected-file recall gate (default 0.5)",
    )
    args = parser.parse_args(argv)

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
        task_dirs = [p for p in task_dirs if p.name in wanted or p.name in wanted]
        # also allow meta id filter
        filtered = []
        for p in task_dirs:
            try:
                meta = json.loads((p / "task.json").read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                continue
            if p.name in wanted or meta.get("id") in wanted:
                filtered.append(p)
        task_dirs = filtered

    if len(task_dirs) < 8:
        print(
            f"error: need >=8 public tasks, found {len(task_dirs)} under {fixtures}",
            file=sys.stderr,
        )
        return 2

    out_path = args.out or (repo / "target" / "agent_task_eval.json")
    out_path.parent.mkdir(parents=True, exist_ok=True)

    work_root = Path(tempfile.mkdtemp(prefix="agentgraph-eval-agent-tasks-"))
    results: List[TaskResult] = []
    print(f"bin: {bin_path}")
    print(f"fixtures: {fixtures}")
    print(f"work: {work_root}")
    print(f"tasks: {len(task_dirs)}")
    print()

    try:
        for td in task_dirs:
            try:
                meta = json.loads((td / "task.json").read_text(encoding="utf-8"))
            except json.JSONDecodeError as e:
                print(f"FAIL {td.name}: bad task.json: {e}", file=sys.stderr)
                results.append(
                    TaskResult(
                        id=td.name,
                        theme="",
                        language="",
                        symbol="",
                        issue="",
                        recommended_commands=[],
                        expected_files=[],
                        noise_files=[],
                        errors=[f"bad task.json: {e}"],
                    )
                )
                continue
            print(f"→ {meta.get('id') or td.name}  symbol={meta.get('symbol')!r}")
            r = score_task(bin_path, td, meta, work_root, args.graph, args.depth)
            # apply strict recall gate override
            if r.agentgraph:
                rec = float(r.agentgraph.get("expected_file_recall") or 0)
                r.checks.setdefault("gates", {})["expected_recall_ge"] = rec >= args.strict_recall
            results.append(r)
            if r.errors:
                for e in r.errors:
                    print(f"  ERROR: {e}")
            else:
                print(
                    f"  recall={r.agentgraph.get('expected_file_recall')} "
                    f"noise={r.agentgraph.get('extra_noise_count')} "
                    f"window={r.agentgraph.get('window')} "
                    f"pass={r.pass_fail}"
                )
        print()
        print_table(results)

        payload = {
            "schema": "agentgraph.eval_agent_tasks.v1",
            "description": (
                "Public Agent code-change task evals (P0-1). Structure-fact "
                "scores on public fixtures. Baseline = name-grep file set "
                "(symbol token in files), not an LLM baseline. Not production "
                "monorepo precision; not a complete runtime graph claim."
            ),
            "non_claims": [
                "public synthetic fixtures only — no private stock corpus",
                "scores are structure facts on these fixtures",
                "window=sound is ast_modeled engineering S gate, not ecosystem sound",
                "name-grep baseline is deterministic token search, not LLM",
            ],
            "binary": str(bin_path),
            "fixtures": str(fixtures),
            "task_count": len(results),
            "pass_count": sum(1 for r in results if r.pass_fail),
            "summary": {
                "mean_agentgraph_expected_recall": round(
                    sum(float(r.agentgraph.get("expected_file_recall") or 0) for r in results)
                    / max(len(results), 1),
                    4,
                ),
                "mean_name_grep_expected_recall": round(
                    sum(float(r.name_grep_baseline.get("expected_file_recall") or 0) for r in results)
                    / max(len(results), 1),
                    4,
                ),
                "mean_agentgraph_extra_noise": round(
                    sum(int(r.agentgraph.get("extra_noise_count") or 0) for r in results)
                    / max(len(results), 1),
                    4,
                ),
                "mean_name_grep_extra_noise": round(
                    sum(int(r.name_grep_baseline.get("extra_noise_count") or 0) for r in results)
                    / max(len(results), 1),
                    4,
                ),
                "baseline_method": "name-grep symbol token in source files",
            },
            "tasks": [
                {
                    "id": r.id,
                    "theme": r.theme,
                    "language": r.language,
                    "symbol": r.symbol,
                    "issue": r.issue,
                    "recommended_commands": r.recommended_commands,
                    "expected_files": r.expected_files,
                    "noise_files": r.noise_files,
                    "pass": r.pass_fail,
                    "errors": r.errors,
                    "checks": r.checks,
                    "agentgraph": r.agentgraph,
                    "name_grep_baseline": r.name_grep_baseline,
                }
                for r in results
            ],
        }
        out_path.write_text(
            json.dumps(payload, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        print(f"wrote {out_path}")

        failed = [r for r in results if not r.pass_fail]
        if failed:
            print("\nfailed gates:", file=sys.stderr)
            for r in failed:
                gates = (r.checks or {}).get("gates") or {}
                bad = [k for k, v in gates.items() if not v]
                print(f"  - {r.id}: errors={r.errors} bad_gates={bad}", file=sys.stderr)
            return 1
        return 0
    finally:
        if not args.keep_work:
            shutil.rmtree(work_root, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
