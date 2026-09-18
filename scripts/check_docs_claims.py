#!/usr/bin/env python3
"""M5 credibility gate — docs claims checker (Track M5).

Parses product docs (README / AGENTS / PLAN / key docs/) for banned oversell
phrases and verifies that CLI flags/commands mentioned in those docs exist in
clap (`src/cli.rs`).

Banned product guarantees (honesty / non-claim lines must NOT trip the checker):
  - 零漏报 / zero-miss oversell
  - 生态 sound / ecosystem sound as a product guarantee
  - 宏完整 / macro-complete as a product guarantee
  - production sound as a product guarantee
  - complete runtime graph as a *product* guarantee
    (allowed honesty: "not a complete runtime graph", "非完整运行时图", …)

Exit codes:
  0 — all checked docs green
  1 — oversell phrase and/or missing CLI flag/command
  2 — usage / IO error

Examples:
  python scripts/check_docs_claims.py
  python scripts/check_docs_claims.py --repo D:/projects/agentgraph
  python scripts/check_docs_claims.py --doc README.md --cli src/cli.rs
  python scripts/check_docs_claims.py --doc fixture.md --no-flag-check
  python scripts/check_docs_claims.py --doc fixture.md --known-flags with-macro,sound
  python scripts/check_docs_claims.py --doc fixture.md \\
      --known-flags sound --known-commands callers,impact
"""

from __future__ import annotations

import argparse
import io
import re
import sys
from pathlib import Path
from typing import List, Optional, Sequence, Set, Tuple

# Force UTF-8 stdio so Chinese oversell messages survive Windows GBK consoles.
if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
        sys.stderr.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:
        sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
        sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")

# ---------------------------------------------------------------------------
# Banned oversell phrases
# ---------------------------------------------------------------------------

# Each entry: (check_id, compiled_regex, human label)
BANNED_PHRASES: List[Tuple[str, re.Pattern[str], str]] = [
    (
        "zero_miss",
        re.compile(r"零漏报|动态零漏报|zero[- ]miss(?:ed)?\s+(?:edges?|rate|guarantee)?|zero\s+missed\s+edges", re.I),
        "zero-miss product guarantee (零漏报)",
    ),
    (
        "ecosystem_sound",
        re.compile(r"生态\s*sound|ecosystem[- ]sound|全生态\s*sound", re.I),
        "ecosystem-sound product guarantee (生态 sound)",
    ),
    (
        "macro_complete",
        re.compile(r"宏完整|macro[- ]complete(?:ness)?", re.I),
        "macro-complete product guarantee (宏完整)",
    ),
    (
        "production_sound",
        re.compile(r"production[- ]sound", re.I),
        "production-sound product guarantee",
    ),
    (
        "complete_runtime_graph",
        re.compile(r"complete\s+runtime\s+graph|完整运行时图", re.I),
        "complete runtime graph claimed as product guarantee",
    ),
]

# Markers that, when they govern the banned phrase in the same clause/line,
# turn the occurrence into an honesty / prohibition line (allowed).
_NEGATION_BEFORE = [
    # English
    r"\bnot\s+(?:a\s+|an\s+|the\s+|claimed\s+|a\s+blanket\s+)?",
    r"\bnon[-\s]",
    r"\bno\s+(?:zero[- ]miss|claim|sound|complete)?",
    r"\bnever\s+",
    r"\bwithout\s+(?:claiming\s+|a\s+)?",
    r"\bcannot\s+",
    r"\bcan't\s+",
    r"\b(?:do|does|did)\s+not\s+",
    r"\b(?:is|are|was|were)\s+not\s+",
    r"\b(?:must|should|shall|may)\s+not\s+",
    r"\b(?:claim|claims|claimed)\s+not\s+",
    r"\bnot\s+claimed",
    r"\bforbid(?:s|den)?\s+",
    r"\bprohibit(?:s|ed)?\s+",
    r"\bavoid(?:s|ing|ed)?\s+(?:claiming\s+|writing\s+|saying\s+)?",
    r"\bexclude(?:s|d|ing)?\s+",
    r"\bdisclaim(?:s|ed|ers?)?\s+",
    r"\bfalse\s+(?:claims?|advertising|marketing)\b.*",
    # Chinese
    r"非",
    r"不是",
    r"并不",
    r"绝不",
    r"不是完整",
    r"不(?:是|声称|得|应|要|可|能|会|做|把|将|为|证明|出现|写|等于|承诺|保证|宣称|宣传)?",
    r"未",
    r"无",
    r"没",
    r"禁止",
    r"严禁",
    r"不得",
    r"不应",
    r"不要",
    r"不可",
    r"别写",
    r"禁止写",
    r"禁止出现",
    r"禁止超售",
    r"删除",
    r"不引入",
    r"不新造",
    r"不把",
    r"不算",
    r"不做",
    r"不声称",
    r"不保证",
    r"不承诺",
    r"不宣称",
    r"保留诚实",
    r"诚实",
    r"honesty",
    r"Honesty",
    r"known\s+over-?flag",
    r"Known\s+over-?flag",
    r"over-?sell",
    r"超售",
    r"虚假",
    r"禁止超售",
]

_NEGATION_AFTER = [
    r"禁止",
    r"不得",
    r"不应",
    r"不要",
    r"不可",
    r"严禁",
    r"超售",
    r"虚假",
    r"\*\*禁止\*\*",
    r"\*\*不得\*\*",
    r"\*\*not\*\*",
    r"\bnot\s+claimed\b",
    r"\bforbidden\b",
    r"\bprohibited\b",
    r"\bmust\s+not\b",
    r"\bdo\s+not\b",
    r"\bnon-?claim",
    r"\bnon-?goal",
    r"\bout of scope\b",
    r"\*\*Non-claim\*\*",
    r"\*\*非声称\*\*",
    r"\*\*non-goal\*\*",
]

# Combined clause-level "this line is a disclaimer" heuristics.
_LINE_DISCLAIMER = [
    re.compile(r"禁止超售"),
    re.compile(r"禁止写"),
    re.compile(r"不得出现"),
    re.compile(r"禁止出现"),
    re.compile(r"无「[^」]*零漏报"),
    re.compile(r"无「[^」]*生态"),
    re.compile(r"无「[^」]*宏完整"),
    re.compile(r"\*\*Non-claim\*\*", re.I),
    re.compile(r"\*\*非声称\*\*"),
    re.compile(r"\*\*Honesty\*\*", re.I),
    re.compile(r"\*\*Explicitly outside the claim\*\*", re.I),
    re.compile(r"honesty line", re.I),
    re.compile(r"诚实"),
    re.compile(r"禁止动态"),
    re.compile(r"不.*虚假宣传"),
    re.compile(r"non-?goal", re.I),
    re.compile(r"out of scope", re.I),
    re.compile(r"not claimed", re.I),
    re.compile(r"will\s+\*{0,2}not\*{0,2}\s+claim", re.I),
    re.compile(r"do\s+\*{0,2}not\*{0,2}\s+(?:market|claim)", re.I),
    re.compile(r"非声称"),
    re.compile(r"非目标"),
    re.compile(r"不声称"),
]

# Section headers that flip the rest of the section into non-claim mode.
_SECTION_NEGATION = re.compile(
    r"(?i)("
    r"will\s+\*{0,2}not\*{0,2}\s+claim"
    r"|non-?claims?"
    r"|non-?goals?"
    r"|what we do not claim"
    r"|explicit non-claims?"
    r"|outside the claim"
    r"|非声称"
    r"|非目标"
    r"|非承诺"
    r"|不做"
    r"|禁止超售"
    r"|honest limits?"
    r"|honesty"
    r"|诚实"
    r")"
)

_BEFORE_RES = [re.compile(p, re.I) for p in _NEGATION_BEFORE]
_AFTER_RES = [re.compile(p, re.I) for p in _NEGATION_AFTER]

# Default docs scanned when --doc is omitted.
DEFAULT_DOCS = [
    "README.md",
    "README.zh-CN.md",
    "AGENTS.md",
    "PLAN.md",
    "docs/sound-subset.md",
    "docs/macro-sidecar.md",
    "docs/graph-html.md",
    "docs/graph-diff.md",
    "docs/eval-l1.md",
    "docs/eval-l2.md",
    "docs/eval-large-repo.md",
    "docs/eval-stock-boundary.md",
    "docs/eval-macro-expand.md",
    "docs/eval-query-p95.md",
    "docs/eval-stock-s-map.md",
    "docs/product-boundary-migration.md",
    "formal/TODO.md",
]

# Docs that mention CLI surface (flags / commands) when checking against clap.
DEFAULT_FLAG_DOCS = [
    "README.md",
    "README.zh-CN.md",
    "AGENTS.md",
    "docs/graph-html.md",
    "docs/graph-diff.md",
    "docs/macro-sidecar.md",
    "docs/sound-subset.md",
]


# ---------------------------------------------------------------------------
# Negation-aware claim detection
# ---------------------------------------------------------------------------


def _strip_md(s: str) -> str:
    """Remove markdown emphasis so `**not**` still counts as negation."""
    return re.sub(r"[*_`]", "", s)


def _line_is_disclaimer(line: str) -> bool:
    stripped = _strip_md(line)
    return any(rx.search(line) or rx.search(stripped) for rx in _LINE_DISCLAIMER)


def _is_negated_occurrence(
    line: str,
    start: int,
    end: int,
    phrase: str,
    section_negated: bool = False,
) -> bool:
    """Return True when the banned phrase is an honesty/prohibition mention."""
    if section_negated:
        return True
    if _line_is_disclaimer(line):
        return True

    prefix = line[:start]
    suffix = line[end:]
    prefix_s = _strip_md(prefix)
    suffix_s = _strip_md(suffix)

    # Quoted banned phrase after a prohibition verb: 禁止写「宏完整」 / "not X"
    quoted = re.search(
        r"(?:禁止写|禁止出现|不得出现|禁止|不得|不应|不要|不可|严禁|删除|无|不)"
        r"[^。\n]{0,24}[「『\"'“]?\s*$",
        prefix_s,
    )
    if quoted:
        return True

    # English quoted honesty: not a "complete runtime graph" / NOT sound
    if re.search(
        r"(?:not|non|never|without|no|market)\s+(?:a\s+|an\s+|the\s+|blanket\s+|false\s+|as\s+)*[\"'“']?\s*$",
        prefix_s,
        re.I,
    ):
        return True

    # "Do not market … “zero missed …”" / "not ecosystem sound"
    if re.search(r"(?:do|does|did|shall|should|must|may)\s+not\b", prefix_s, re.I):
        return True
    if re.search(r"\bnot\s+(?:market|claim|equate|label|prove|assert|guarantee)\b", prefix_s, re.I):
        return True
    if re.search(r"\bnot\s+(?:a\s+|an\s+)?(?:blanket\s+)?(?:complete|ecosystem|production|sound|zero)", prefix_s, re.I):
        return True
    if re.search(r"\bnot\s+\S{0,40}$", prefix_s, re.I):
        # `**not** ecosystem sound` / `— **not** ecosystem sound`
        return True

    # Prefix window (same clause-ish), markdown-stripped
    window = prefix_s[-96:] if len(prefix_s) > 96 else prefix_s
    for rx in _BEFORE_RES:
        if rx.search(window):
            return True

    # Also scan a bit further back on very short prefixes
    wider = prefix_s[-160:] if len(prefix_s) > 160 else prefix_s
    for token in (
        "禁止",
        "不得",
        "非",
        "不是",
        "不声称",
        "不承诺",
        "不保证",
        "无「",
        "Honesty",
        "Non-claim",
        "non-goal",
        "non-goal.",
        "not claimed",
        "out of scope",
        "not market",
        "Do not",
    ):
        if token in wider:
            return True

    # Suffix window — table cells often put 禁止 after the phrase
    swindow = suffix_s[:80]
    for rx in _AFTER_RES:
        if rx.search(swindow):
            return True

    # Multi-clause line: "声称「宏完整」「动态零漏报」 | **禁止**"
    if re.search(r"[|｜].{0,20}(?:\*\*)?(?:禁止|不得|forbidden|prohibited)", suffix_s, re.I):
        return True
    if re.search(
        r"(?:禁止|不得|not claimed|forbidden|prohibited|non-goal|out of scope)",
        suffix_s[:120],
        re.I,
    ):
        # Only treat as negation when the line does not also *assert* the claim
        # with positive verbs immediately before the phrase.
        if not re.search(
            r"(?:保证|承诺|宣称|guarantee|ensures?|provides?|delivers?)\s*$",
            prefix_s,
            re.I,
        ):
            return True

    return False


def scan_banned_phrases(path: Path, text: str) -> List[str]:
    """Return human-readable violation lines for banned oversell phrases."""
    violations: List[str] = []
    lines = text.splitlines()
    section_negated = False
    for lineno, line in enumerate(lines, start=1):
        # Heading resets (or sets) non-claim section context.
        if re.match(r"^\s{0,3}#{1,6}\s+\S", line):
            section_negated = bool(_SECTION_NEGATION.search(_strip_md(line)))
        elif re.search(r"(?i)will\s+\*{0,2}not\*{0,2}\s+claim", line):
            section_negated = True

        for check_id, rx, label in BANNED_PHRASES:
            for m in rx.finditer(line):
                if _is_negated_occurrence(
                    line, m.start(), m.end(), m.group(0), section_negated=section_negated
                ):
                    continue
                violations.append(
                    f"{path}:{lineno}: banned oversell [{check_id}] {label}: "
                    f"…{line[max(0, m.start() - 24): m.end() + 24].strip()}…"
                )
    return violations


# ---------------------------------------------------------------------------
# CLI flag / command extraction from clap (src/cli.rs) and docs
# ---------------------------------------------------------------------------


def _kebab(name: str) -> str:
    """CamelCase / snake_case → clap kebab-case (BenchQuery → bench-query)."""
    s1 = re.sub(r"(.)([A-Z][a-z]+)", r"\1-\2", name)
    s2 = re.sub(r"([a-z0-9])([A-Z])", r"\1-\2", s1)
    return s2.replace("_", "-").lower()


def parse_clap_cli(cli_src: str) -> Tuple[Set[str], Set[str]]:
    """Extract (long_flags, subcommands) from clap-derive source text."""
    flags: Set[str] = set()
    commands: Set[str] = set()

    # #[arg(...)] field: Type  → long flag when `long` is present
    for m in re.finditer(
        r"#\[arg\(([^)]*)\)\]\s*(?:pub\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:",
        cli_src,
        re.S,
    ):
        arg_body, field = m.group(1), m.group(2)
        if re.search(r"\blong\b", arg_body):
            flags.add(_kebab(field))

    # Subcommands: first `pub enum Commands { ... }`
    def enum_body(src: str, enum_name: str) -> Optional[str]:
        m = re.search(rf"pub enum {enum_name}\s*\{{", src)
        if not m:
            return None
        i = m.end()
        depth = 1
        while i < len(src) and depth:
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
            i += 1
        return src[m.end() : i - 1]

    commands_body = enum_body(cli_src, "Commands")
    if commands_body:
        for m in re.finditer(
            r"(?m)^[ \t]{2,8}([A-Z][A-Za-z0-9]*)\s*(?:\{|,|$)",
            commands_body,
        ):
            commands.add(_kebab(m.group(1)))

    macro_body = enum_body(cli_src, "MacroCmd")
    if macro_body:
        for m in re.finditer(
            r"(?m)^[ \t]{2,8}([A-Z][A-Za-z0-9]*)\s*(?:\{|,|$)",
            macro_body,
        ):
            sub = _kebab(m.group(1))
            commands.add(sub)
            if "macro" in commands or True:
                commands.add(f"macro {sub}")

    # Ensure compound forms commonly documented
    commands.add("macro")
    commands.add("macro status")
    return flags, commands


_FLAG_RE = re.compile(r"(?<![\w-])--([a-z][a-z0-9-]*)\b", re.I)
_AGENTGRAPH_CMD_RE = re.compile(
    r"(?m)^\s*(?:cargo run(?:\s+--quiet)?\s+--\s+)?agentgraph\b((?:\s+(?:--[\w-]+(?:[=\s]\S+)?|[\w./\\:-]+))*)"
)
_TABLE_CMD_RE = re.compile(r"(?m)^\|\s*`([a-z][a-z0-9-]*(?:\s+[a-z][a-z0-9-]*)?)`\s*\|")
_BACKTICK_CMD_RE = re.compile(
    r"`(macro status|subset|importers|enrich|watch|related|callers|impact|find|index|stats|graph|export|bench-query|mcp|diff)`"
)


def extract_doc_cli_mentions(text: str) -> Tuple[Set[str], Set[str]]:
    """Return (flags, commands) mentioned in markdown text."""
    flags: Set[str] = set()
    commands: Set[str] = set()

    # Strip fenced code for flag scan? No — flags appear in fences legitimately.
    for m in _FLAG_RE.finditer(text):
        flags.add(m.group(1).lower())

    # agentgraph <args> lines
    for m in _AGENTGRAPH_CMD_RE.finditer(text):
        rest = m.group(1) or ""
        tokens = rest.split()
        # skip global options like --root PATH
        i = 0
        while i < len(tokens):
            tok = tokens[i]
            if tok.startswith("--"):
                # flag or --flag=value; skip optional value
                if "=" not in tok and i + 1 < len(tokens) and not tokens[i + 1].startswith("-"):
                    # might be value for --root / --out / --depth
                    flag = tok[2:].lower()
                    flags.add(flag)
                    if flag in {"root", "out", "depth", "limit", "interval", "format", "prefix", "samples", "direction"}:
                        i += 2
                        continue
                else:
                    flags.add(tok.split("=", 1)[0][2:].lower())
                i += 1
                continue
            # first non-flag token is the subcommand
            cmd = tok.lower()
            # multi-word: macro status
            if cmd == "macro" and i + 1 < len(tokens) and not tokens[i + 1].startswith("-"):
                commands.add("macro")
                commands.add(f"macro {tokens[i + 1].lower()}")
                i += 2
                continue
            if cmd in {
                "index", "stats", "find", "callers", "impact", "related",
                "importers", "enrich", "watch", "subset", "export", "mcp",
                "bench-query", "macro", "graph", "diff",
            }:
                commands.add(cmd)
            i += 1

    for m in _TABLE_CMD_RE.finditer(text):
        commands.add(m.group(1).lower())

    for m in _BACKTICK_CMD_RE.finditer(text):
        commands.add(m.group(1).lower())

    # `--foo` inside Chinese/English prose already captured by _FLAG_RE
    return flags, commands


def check_flags_in_docs(
    doc_paths: Sequence[Path],
    known_flags: Set[str],
    known_commands: Set[str],
) -> List[str]:
    """Verify every CLI flag/command mentioned in docs exists in known sets."""
    violations: List[str] = []
    # Flags that are env-var-like or markdown noise — skip.
    skip_flags = {
        # not clap flags
        "root-path",
        "nocapture",
        "quiet",
        "all-targets",
        "d",
        "warnings",
        "check",
        "force",  # still real, keep — removed from skip
    }
    # `cargo` flags that leak from test/docs instructions
    cargo_flags = {
        "test", "build", "run", "install", "fmt", "clippy", "quiet",
        "all-targets", "release", "nocapture", "check", "path",
        "manifest-path", "version", "bin",
    }
    # scip / third-party
    other_skip = {"out", "depth", "limit"}  # real clap flags — do not skip
    # Re-evaluate: only skip non-agentgraph noise.
    skip_flags = cargo_flags | {
        "nocapture",
        "all-targets",
        "d",
        "warnings",
        "check",
        "manifest-path",
        "release",
        "quiet",
        "path",
        "version",
        "bin",
        "install",
        "fmt",
        "clippy",
        "test",
        "build",
        "run",
        "use",
        "force-reindex",  # not a product flag name
    }

    for path in doc_paths:
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as e:
            violations.append(f"{path}: cannot read doc for flag check: {e}")
            continue
        flags, commands = extract_doc_cli_mentions(text)
        for flag in sorted(flags):
            if flag in skip_flags:
                continue
            # Skip obvious non-clap tokens (scip subcommands used as --out values etc.)
            if flag.startswith("agentgraph"):
                continue
            if flag not in known_flags:
                violations.append(
                    f"{path}: doc mentions `--{flag}` but it is not present in clap CLI "
                    f"(src/cli.rs / --known-flags)"
                )
        for cmd in sorted(commands):
            if cmd in known_commands:
                continue
            # ignore table false-positives that are pure prose nouns
            if cmd in {"confidence", "evidence", "promise", "subset_ok"}:
                continue
            violations.append(
                f"{path}: doc mentions command `{cmd}` but it is not present in clap CLI "
                f"(src/cli.rs / --known-commands)"
            )
    return violations


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def _repo_root_from_script() -> Path:
    return Path(__file__).resolve().parent.parent


def run_checks(
    repo: Path,
    docs: Sequence[Path],
    flag_docs: Sequence[Path],
    cli_path: Optional[Path],
    known_flags: Optional[Set[str]],
    known_commands: Optional[Set[str]],
    do_flag_check: bool,
) -> List[str]:
    violations: List[str] = []

    for doc in docs:
        if not doc.is_file():
            # Missing optional docs are not violations; missing required README is.
            if doc.name.upper().startswith("README") or doc.name in {"AGENTS.md", "PLAN.md"}:
                violations.append(f"{doc}: required product doc missing")
            continue
        text = doc.read_text(encoding="utf-8")
        violations.extend(scan_banned_phrases(doc, text))

    if not do_flag_check:
        return violations

    if known_flags is None or known_commands is None:
        if cli_path is None or not cli_path.is_file():
            violations.append(
                "flag check enabled but src/cli.rs not found and no --known-flags/--known-commands given"
            )
            return violations
        cli_src = cli_path.read_text(encoding="utf-8")
        parsed_flags, parsed_cmds = parse_clap_cli(cli_src)
        known_flags = known_flags if known_flags is not None else parsed_flags
        known_commands = known_commands if known_commands is not None else parsed_cmds

    violations.extend(check_flags_in_docs(flag_docs, known_flags or set(), known_commands or set()))
    return violations


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="M5 docs claims checker")
    parser.add_argument("--repo", type=Path, default=None, help="repository root (default: parent of scripts/)")
    parser.add_argument(
        "--doc",
        action="append",
        type=Path,
        default=None,
        help="explicit doc file to scan (repeatable). Relative paths resolve against --repo.",
    )
    parser.add_argument(
        "--cli",
        type=Path,
        default=None,
        help="clap CLI source (default: <repo>/src/cli.rs)",
    )
    parser.add_argument(
        "--flag-doc",
        action="append",
        type=Path,
        default=None,
        help="docs to scan for CLI flag mentions (repeatable)",
    )
    parser.add_argument(
        "--known-flags",
        type=str,
        default=None,
        help="comma-separated clap long flags override (tests / stubs)",
    )
    parser.add_argument(
        "--known-commands",
        type=str,
        default=None,
        help="comma-separated clap subcommands override (tests / stubs)",
    )
    parser.add_argument(
        "--no-flag-check",
        action="store_true",
        help="only scan banned oversell phrases (skip clap flag verification)",
    )
    parser.add_argument(
        "--print-known",
        action="store_true",
        help="print flags/commands parsed from clap and exit 0",
    )
    args = parser.parse_args(argv)

    repo = args.repo.resolve() if args.repo else _repo_root_from_script()
    if not repo.is_dir():
        print(f"error: repo root not a directory: {repo}", file=sys.stderr)
        return 2

    def resolve(p: Path) -> Path:
        return p if p.is_absolute() else (repo / p)

    if args.doc:
        docs = [resolve(p) for p in args.doc]
    else:
        docs = [repo / rel for rel in DEFAULT_DOCS]

    if args.flag_doc:
        flag_docs = [resolve(p) for p in args.flag_doc]
    elif args.doc:
        # explicit --doc also participates in flag scan when flag check is on
        flag_docs = docs
    else:
        flag_docs = [repo / rel for rel in DEFAULT_FLAG_DOCS if (repo / rel).is_file()]

    cli_path = resolve(args.cli) if args.cli else (repo / "src" / "cli.rs")

    known_flags: Optional[Set[str]] = None
    known_commands: Optional[Set[str]] = None
    if args.known_flags is not None:
        known_flags = {x.strip().lstrip("-") for x in args.known_flags.split(",") if x.strip()}
    if args.known_commands is not None:
        known_commands = {x.strip().lower() for x in args.known_commands.split(",") if x.strip()}

    if args.print_known:
        if known_flags is None or known_commands is None:
            src = cli_path.read_text(encoding="utf-8")
            f, c = parse_clap_cli(src)
            known_flags = known_flags or f
            known_commands = known_commands or c
        print("flags:", ",".join(sorted(known_flags or [])))
        print("commands:", ",".join(sorted(known_commands or [])))
        return 0

    violations = run_checks(
        repo=repo,
        docs=docs,
        flag_docs=flag_docs,
        cli_path=cli_path,
        known_flags=known_flags,
        known_commands=known_commands,
        do_flag_check=not args.no_flag_check,
    )

    if violations:
        print("docs claims check FAILED", file=sys.stderr)
        for v in violations:
            print(f"  - {v}", file=sys.stderr)
        print(f"\n{len(violations)} violation(s). Fix oversell wording or align flags with src/cli.rs.", file=sys.stderr)
        return 1

    print(f"docs claims check OK ({len(docs)} doc(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
