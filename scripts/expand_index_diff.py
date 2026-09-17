#!/usr/bin/env python3
"""Synthetic macro-expand approximator for agentgraph spike (Track B).

NOT rustc/cargo-expand output. Appends post-expansion-shaped impls derived from
source patterns so we can measure whether agentgraph indexes macro-generated
edges (Debug::fmt, Default::default, Error::fmt, async_trait method shapes).

When `cargo expand` / nightly -Zunpretty works, prefer real expansion; this
script remains the offline fallback and measurement helper.

Usage:
  python scripts/expand_index_diff.py prepare \
      --corpus D:/projects/eval-corpus/stock-trading-app \
      --shadow D:/projects/eval-corpus/stock-trading-app-expanded \
      --crates event-engine,model-selection-replay,repository

  python scripts/expand_index_diff.py diff \
      --source-root <root with index.db> \
      --expanded-root <root with index.db>
"""
from __future__ import annotations

import argparse
import re
import shutil
import sqlite3
import sys
from collections import Counter
from pathlib import Path

DEFAULT_CRATES = ["event-engine", "model-selection-replay", "repository"]
MARK_BEGIN = "// === AGENTGRAPH SYNTHETIC EXPAND BEGIN ==="
MARK_END = "// === AGENTGRAPH SYNTHETIC EXPAND END ==="

RE_DERIVE = re.compile(
    r"#\s*\[\s*derive\s*\((?P<traits>[^)]*)\)\s*\]\s*(?:#\s*\[[^\]]*\]\s*)*"
    r"(?:pub\s+)?(?P<kind>struct|enum)\s+(?P<name>\w+)",
    re.MULTILINE,
)
RE_ASYNC_TRAIT_FN = re.compile(
    r"^\s*(?:pub\s+)?async\s+fn\s+(?P<name>\w+)\s*(?P<rest><[^>]*>)?\s*\(",
    re.MULTILINE,
)
RE_ASYNC_TRAIT_BLOCK = re.compile(
    r"#\s*\[\s*async_trait[^\]]*\]\s*(?P<item>pub\s+)?(?P<kind>trait|impl)\s+(?P<body>.*?)(?=\n(?:pub\s+)?(?:#\[|///|//\s*──|struct|enum|trait|impl|fn|pub fn|mod|pub mod)|\Z)",
    re.DOTALL,
)
RE_INVENTORY_SUBMIT = re.compile(
    r"inventory::submit!\s*\{(?P<body>[^}]{0,2000})\}",
    re.DOTALL,
)
RE_TYPE_IN_SUBMIT = re.compile(r"\b([A-Z][A-Za-z0-9_]*)\b")


def _derives_in(traits_csv: str) -> list[str]:
    out = []
    for t in traits_csv.split(","):
        t = t.strip()
        if not t:
            continue
        if "::" in t:
            t = t.split("::")[-1]
        out.append(t)
    return out


def _debug_impl(name: str, fields_hint: str = "") -> str:
    return (
        f"impl ::core::fmt::Debug for {name} {{\n"
        f"    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{\n"
        f"        f.debug_struct(\"{name}\"){fields_hint}.finish()\n"
        f"    }}\n"
        f"}}\n"
    )


def _clone_impl(name: str) -> str:
    return (
        f"impl ::core::clone::Clone for {name} {{\n"
        f"    fn clone(&self) -> Self {{\n"
        f"        Self {{ /* synthetic expand: field-wise clone */ }}\n"
        f"    }}\n"
        f"}}\n"
    )


def _default_impl(name: str) -> str:
    return (
        f"impl ::core::default::Default for {name} {{\n"
        f"    fn default() -> Self {{\n"
        f"        Self {{ /* synthetic expand: Default::default */ }}\n"
        f"    }}\n"
        f"}}\n"
    )


def _partial_eq_impl(name: str) -> str:
    return (
        f"impl ::core::cmp::PartialEq for {name} {{\n"
        f"    fn eq(&self, other: &Self) -> bool {{\n"
        f"        let _ = other;\n"
        f"        true\n"
        f"    }}\n"
        f"}}\n"
    )


def _eq_impl(name: str) -> str:
    return f"impl ::core::cmp::Eq for {name} {{}}\n"


def _hash_impl(name: str) -> str:
    return (
        f"impl ::core::hash::Hash for {name} {{\n"
        f"    fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {{\n"
        f"        let _ = state;\n"
        f"    }}\n"
        f"}}\n"
    )


def _error_impl(name: str) -> str:
    # thiserror expands to Display + Error; source/description are the edge names we care about.
    return (
        f"impl ::core::fmt::Display for {name} {{\n"
        f"    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{\n"
        f"        write!(f, \"{{}}\", \"synthetic {name}\")\n"
        f"    }}\n"
        f"}}\n"
        f"impl std::error::Error for {name} {{\n"
        f"    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {{\n"
        f"        None\n"
        f"    }}\n"
        f"    fn description(&self) -> &str {{\n"
        f"        \"synthetic error description\"\n"
        f"    }}\n"
        f"}}\n"
    )


def _partial_ord_impl(name: str) -> str:
    return (
        f"impl ::core::cmp::PartialOrd for {name} {{\n"
        f"    fn partial_cmp(&self, other: &Self) -> Option<::core::cmp::Ordering> {{\n"
        f"        let _ = other;\n"
        f"        Some(::core::cmp::Ordering::Equal)\n"
        f"    }}\n"
        f"}}\n"
    )


def _ord_impl(name: str) -> str:
    return (
        f"impl ::core::cmp::Ord for {name} {{\n"
        f"    fn cmp(&self, other: &Self) -> ::core::cmp::Ordering {{\n"
        f"        let _ = other;\n"
        f"        ::core::cmp::Ordering::Equal\n"
        f"    }}\n"
        f"}}\n"
    )


DERIVE_IMPL = {
    "Debug": lambda n: _debug_impl(n),
    "Clone": _clone_impl,
    "Default": _default_impl,
    "PartialEq": _partial_eq_impl,
    "Eq": _eq_impl,
    "Hash": _hash_impl,
    "PartialOrd": _partial_ord_impl,
    "Ord": _ord_impl,
    "Error": _error_impl,
}


def expand_source(text: str, crate: str, rel_path: str) -> tuple[str, Counter]:
    stats: Counter = Counter()
    chunks: list[str] = [MARK_BEGIN, f"// mode=synthetic_expand crate={crate} path={rel_path}", ""]

    for m in RE_DERIVE.finditer(text):
        name = m.group("name")
        traits = _derives_in(m.group("traits"))
        stats["derive_targets"] += 1
        for t in traits:
            if t in DERIVE_IMPL:
                chunks.append(DERIVE_IMPL[t](name))
                stats[f"derive_impl_{t}"] += 1
            elif t == "Copy":
                stats["derive_impl_Copy_skipped"] += 1

    # async_trait method shapes (trait defs + impls). Method names stay; bodies become BoxFuture-like.
    at_hits = list(re.finditer(r"#\s*\[\s*async_trait", text))
    stats["async_trait_attrs"] += len(at_hits)
    # Collect async fn names after each attribute until next non-async item ends roughly
    for attr in at_hits:
        window = text[attr.start() : attr.start() + 8000]
        for fm in RE_ASYNC_TRAIT_FN.finditer(window):
            mname = fm.group("name")
            stats["async_trait_methods"] += 1
            chunks.append(
                f"// async_trait shape for `{mname}` "
                f"(Pin<Box<dyn Future + Send>> stand-in)\n"
                f"fn {mname}_async_trait_boxed_shape(&self) {{ /* synthetic */ }}\n"
            )

    for sm in RE_INVENTORY_SUBMIT.finditer(text):
        body = sm.group("body")
        stats["inventory_submits"] += 1
        names = [n for n in RE_TYPE_IN_SUBMIT.findall(body) if n not in {"Box", "Self", "Ok", "Err"}]
        uniq = []
        for n in names:
            if n not in uniq:
                uniq.append(n)
        chunks.append(
            "// inventory::submit! synthetic registrar\n"
            "pub struct SyntheticInventoryRegistrar {\n"
            "    pub registration: &'static str,\n"
            "    pub factory: fn() -> (),\n"
            "}\n"
        )
        for n in uniq[:8]:
            chunks.append(
                f"pub fn synthetic_inventory_registrar_{n.lower()}() -> SyntheticInventoryRegistrar {{\n"
                f"    SyntheticInventoryRegistrar {{ registration: \"{n}\", factory: || {{ }} }}\n"
                f"}}\n"
            )
            stats["inventory_registrar_types"] += 1

    if len(chunks) <= 2:
        return "", stats

    chunks.append(MARK_END)
    return "\n\n" + "\n".join(chunks) + "\n", stats


def cmd_prepare(args: argparse.Namespace) -> int:
    corpus = Path(args.corpus)
    shadow = Path(args.shadow)
    crates = [c.strip() for c in args.crates.split(",") if c.strip()]
    src_view = shadow / "source-view"
    exp_view = shadow / "expanded-view"
    src_view.mkdir(parents=True, exist_ok=True)
    exp_view.mkdir(parents=True, exist_ok=True)

    total = Counter()
    for crate in crates:
        crate_src = corpus / "crates" / crate / "src"
        if not crate_src.is_dir():
            print(f"WARN missing crate src: {crate_src}", file=sys.stderr)
            continue
        for rs in sorted(crate_src.rglob("*.rs")):
            rel = rs.relative_to(crate_src)
            rel_out = Path("crates") / crate / "src" / rel
            text = rs.read_text(encoding="utf-8", errors="replace")
            out_src = src_view / rel_out
            out_exp = exp_view / rel_out
            out_src.parent.mkdir(parents=True, exist_ok=True)
            out_exp.parent.mkdir(parents=True, exist_ok=True)
            out_src.write_text(text, encoding="utf-8")
            extra, stats = expand_source(text, crate, str(rel_out))
            total.update(stats)
            out_exp.write_text(text + extra, encoding="utf-8")

    # Optional inventory-heavy crate: write BOTH trees so name-set diff stays fair.
    extra_crates = [c.strip() for c in args.extra_crates.split(",") if c.strip()]
    for crate in extra_crates:
        crate_src = corpus / "crates" / crate / "src"
        if not crate_src.is_dir():
            continue
        for rs in sorted(crate_src.rglob("*.rs")):
            rel = rs.relative_to(crate_src)
            rel_out = Path("crates") / crate / "src" / rel
            text = rs.read_text(encoding="utf-8", errors="replace")
            out_src = src_view / rel_out
            out_exp = exp_view / rel_out
            out_src.parent.mkdir(parents=True, exist_ok=True)
            out_exp.parent.mkdir(parents=True, exist_ok=True)
            out_src.write_text(text, encoding="utf-8")
            extra, stats = expand_source(text, crate, str(rel_out))
            out_exp.write_text(text + extra, encoding="utf-8")
            total.update({f"{crate}_{k}" if not k.startswith(crate) else k: v for k, v in stats.items()})

    # Real -Zunpretty artifacts go to a SEPARATE root so they do not skew stock diff.
    real_root = shadow / "real-expand-root"
    real_root.mkdir(parents=True, exist_ok=True)
    toolcheck = shadow / "_toolcheck_agentgraph_expanded.rs"
    if toolcheck.exists():
        dest = real_root / "agentgraph_lib_expanded.rs"
        shutil.copy2(toolcheck, dest)
        print(f"copied real expand artifact -> {dest}")
    mini_exp = shadow / "mini-spike" / "expanded_lib.rs"
    if mini_exp.exists():
        dest = real_root / "mini-spike" / "expanded_lib.rs"
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(mini_exp, dest)
        print(f"copied mini-spike expand -> {dest}")

    metrics = shadow / "metrics" / "synthetic_expand_stats.txt"
    metrics.parent.mkdir(parents=True, exist_ok=True)
    lines = ["synthetic expand stats (not rustc):"] + [f"{k}={v}" for k, v in sorted(total.items())]
    metrics.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    print(f"source-view={src_view}")
    print(f"expanded-view={exp_view}")
    return 0


def _load_counts(root: Path) -> dict:
    db = root / ".agentgraph" / "index.db"
    if not db.exists():
        return {"error": f"no index.db under {root}"}
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    out: dict = {"root": str(root)}
    out["files"] = conn.execute("SELECT COUNT(*) c FROM files").fetchone()["c"]
    out["symbols"] = conn.execute("SELECT COUNT(*) c FROM symbols").fetchone()["c"]
    out["refs"] = conn.execute("SELECT COUNT(*) c FROM refs").fetchone()["c"]
    out["symbol_kinds"] = {
        r["kind"]: r["c"]
        for r in conn.execute("SELECT kind, COUNT(*) c FROM symbols GROUP BY kind ORDER BY c DESC")
    }
    out["ref_conf"] = {
        r["confidence"]: r["c"]
        for r in conn.execute("SELECT confidence, COUNT(*) c FROM refs GROUP BY confidence")
    }
    # interesting macro-ish names
    interesting = [
        "fmt", "clone", "default", "eq", "hash", "source", "description",
        "Debug", "Display", "Error", "Clone", "Default", "Hash",
        "insert", "query_range", "latest_close",
    ]
    like = ["%async_trait_boxed_shape", "%inventory_registrar%", "SyntheticInventoryRegistrar"]
    out["names"] = {}
    for name in interesting:
        out["names"][name] = {
            "symbols": conn.execute("SELECT COUNT(*) c FROM symbols WHERE name=?", (name,)).fetchone()["c"],
            "refs": conn.execute("SELECT COUNT(*) c FROM refs WHERE name=?", (name,)).fetchone()["c"],
        }
    for pat in like:
        out["names"][pat] = {
            "symbols": conn.execute("SELECT COUNT(*) c FROM symbols WHERE name LIKE ?", (pat,)).fetchone()["c"],
            "refs": conn.execute("SELECT COUNT(*) c FROM refs WHERE name LIKE ?", (pat,)).fetchone()["c"],
        }
    out["symbol_names"] = {
        r["name"] for r in conn.execute("SELECT DISTINCT name FROM symbols")
    }
    conn.close()
    return out


def cmd_diff(args: argparse.Namespace) -> int:
    src = _load_counts(Path(args.source_root))
    exp = _load_counts(Path(args.expanded_root))
    print("=== index counts ===")
    print(f"source : files={src.get('files')} symbols={src.get('symbols')} refs={src.get('refs')}")
    print(f"expand : files={exp.get('files')} symbols={exp.get('symbols')} refs={exp.get('refs')}")
    if "error" in src or "error" in exp:
        print("ERROR missing index", src.get("error"), exp.get("error"))
        return 2
    print("\n=== name overlap (symbols/refs) ===")
    print(f"{'name':30} {'src_sym':>8} {'exp_sym':>8} {'src_ref':>8} {'exp_ref':>8} {'d_sym':>8} {'d_ref':>8}")
    for name, s in src["names"].items():
        e = exp["names"].get(name, {"symbols": 0, "refs": 0})
        print(
            f"{name:30} {s['symbols']:8} {e['symbols']:8} {s['refs']:8} {e['refs']:8} "
            f"{e['symbols']-s['symbols']:+8} {e['refs']-s['refs']:+8}"
        )
    s_names = src.get("symbol_names") or set()
    e_names = exp.get("symbol_names") or set()
    only_exp = sorted(e_names - s_names)
    only_src = sorted(s_names - e_names)
    both = len(s_names & e_names)
    print(f"\n=== symbol-name set ===")
    print(f"source names={len(s_names)} expanded names={len(e_names)} intersection={both}")
    print(f"only-in-expanded={len(only_exp)} only-in-source={len(only_src)}")
    print("sample only-in-expanded:", ", ".join(only_exp[:40]))
    print("sample only-in-source:", ", ".join(only_src[:20]))
    print("source conf=", {k: src.get("ref_conf", {}).get(k) for k in sorted(src.get("ref_conf") or {})})
    print("expand conf=", {k: exp.get("ref_conf", {}).get(k) for k in sorted(exp.get("ref_conf") or {})})
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prepare", help="copy source + write synthetic expanded shadow tree")
    p.add_argument("--corpus", default=r"D:\projects\eval-corpus\stock-trading-app")
    p.add_argument("--shadow", default=r"D:\projects\eval-corpus\stock-trading-app-expanded")
    p.add_argument("--crates", default=",".join(DEFAULT_CRATES))
    p.add_argument("--include-real-toolcheck", action="store_true", default=True)
    p.add_argument(
        "--extra-crates",
        default="strategy-plugins",
        help="Crates copied into BOTH trees (inventory/async_trait mix), optional",
    )
    p.set_defaults(func=cmd_prepare)
    d = sub.add_parser("diff", help="compare two agentgraph index.db roots")
    d.add_argument("--source-root", required=True)
    d.add_argument("--expanded-root", required=True)
    d.set_defaults(func=cmd_diff)
    args = ap.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
