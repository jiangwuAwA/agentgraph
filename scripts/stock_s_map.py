#!/usr/bin/env python3
"""Per-crate S-violation map for stock-trading-app corpus.

Produces:
  - console table (ranked by dirtiness)
  - docs/eval-stock-s-map.md (caller may also merge into eval-large-repo.md)

Does NOT commit corpus source. Reads index.db + optional source scans for
unsafe/async_trait stats that the DB may not fully capture.
"""
from __future__ import annotations

import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

CORPUS = Path(r"D:\projects\eval-corpus\stock-trading-app")
OUT_MD = Path(r"D:\projects\agentgraph\docs\eval-stock-s-map.md")


def crate_of(path: str) -> str:
    """Map indexed path to crate / top-level bucket."""
    p = path.replace("\\", "/")
    if p.startswith("crates/"):
        parts = p.split("/")
        return parts[1] if len(parts) > 1 else "crates/?"
    if p.startswith("src-tauri/"):
        return "src-tauri"
    if p.startswith("src/"):
        return "src (app root rust/other)"
    if p.startswith("proxy/"):
        return "proxy"
    if p.startswith("analysis/"):
        return "analysis"
    if p.startswith("ops/"):
        return "ops"
    if p.startswith("scripts/"):
        return "scripts"
    if p.startswith("docs/"):
        return "docs"
    if p.startswith("data/"):
        return "data"
    if p.startswith("fixtures/"):
        return "fixtures"
    if p.startswith("test_data/") or p.startswith("test-results/"):
        return "test-data"
    if p.startswith(".claude/") or p.startswith(".github/") or p.startswith(".superpowers/"):
        return p.split("/")[0]
    if p.startswith(".workbuddy/") or p.startswith(".playwright-mcp/") or p.startswith(".cargo/"):
        return p.split("/")[0]
    # first path segment
    return p.split("/")[0] if "/" in p else "(repo root files)"


def scan_source_unsafe(crate_rs: dict[str, list[Path]]) -> dict[str, dict[str, int]]:
    """Source-level scans for patterns S cares about (AST-ish via regex on clean-ish code).

    These are *operator corpus stats*, not agentgraph claims. Regex can overcount
    comments/strings; we note that in the doc.
    """
    out: dict[str, dict[str, int]] = {}
    pat_unsafe = re.compile(r"\bunsafe\b")
    pat_transmute = re.compile(r"\b(transmute|std::ptr|core::ptr|std::arch|global_asm!)\b")
    pat_async_trait = re.compile(r"#\s*\[\s*async_trait\s*\]")
    pat_async_fn = re.compile(r"\basync\s+fn\b")
    pat_tokio = re.compile(r"#\s*\[\s*tokio::")
    pat_derive = re.compile(r"#\s*\[\s*derive\s*\(")
    pat_dyn_dispatch = re.compile(r"\bBox\s*<\s*dyn\b|&\s*dyn\b|dyn\s+\w+")
    for crate, files in crate_rs.items():
        c = Counter()
        for f in files:
            try:
                text = f.read_text(encoding="utf-8", errors="replace")
            except OSError:
                c["read_fail"] += 1
                continue
            c["unsafe_tok"] += len(pat_unsafe.findall(text))
            c["transmute_ptr_tok"] += len(pat_transmute.findall(text))
            c["async_trait"] += len(pat_async_trait.findall(text))
            c["async_fn"] += len(pat_async_fn.findall(text))
            c["tokio_attr"] += len(pat_tokio.findall(text))
            c["derive"] += len(pat_derive.findall(text))
            c["dyn_dispatch"] += len(pat_dyn_dispatch.findall(text))
        out[crate] = dict(c)
    return out


def main() -> None:
    import sqlite3

    db = CORPUS / ".agentgraph" / "index.db"
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row

    # Files from index
    crate_files: dict[str, list[str]] = defaultdict(list)
    crate_lang: dict[str, Counter] = defaultdict(Counter)
    crate_size: dict[str, int] = defaultdict(int)
    for row in conn.execute("SELECT path, language, size FROM files"):
        c = crate_of(row["path"])
        crate_files[c].append(row["path"])
        crate_lang[c][row["language"] or "?"] += 1
        crate_size[c] += row["size"] or 0

    # Violations
    crate_viol: dict[str, Counter] = defaultdict(Counter)
    crate_viol_files: dict[str, set[str]] = defaultdict(set)
    for row in conn.execute("SELECT path, kind FROM subset_violations"):
        c = crate_of(row["path"])
        crate_viol[c][row["kind"]] += 1
        crate_viol_files[c].add(row["path"])

    # Source scan for Rust crates + a few non-crate dirs
    rust_files_by_crate: dict[str, list[Path]] = defaultdict(list)
    for crate, paths in crate_files.items():
        for p in paths:
            if p.endswith(".rs"):
                full = CORPUS / p
                if full.is_file():
                    rust_files_by_crate[crate].append(full)
    src_stats = scan_source_unsafe(rust_files_by_crate)

    # Build rows
    rows = []
    all_crates = set(crate_files) | set(crate_viol) | set(src_stats)
    for crate in all_crates:
        paths = crate_files.get(crate, [])
        rs_files = sum(1 for p in paths if p.endswith(".rs"))
        lang = crate_lang.get(crate, Counter())
        viol = crate_viol.get(crate, Counter())
        src = src_stats.get(crate, {})
        unsafe_n = viol.get("unsafe", 0)
        parse_err = viol.get("parse_error", 0)
        transmute = viol.get("transmute", 0)
        std_ptr = viol.get("std_ptr", 0)
        other = sum(viol.values()) - unsafe_n - parse_err - transmute - std_ptr
        # subset_ok-ish: no violations at all in indexed files for this crate
        subset_okish = "yes" if sum(viol.values()) == 0 else ("no" if unsafe_n or parse_err or transmute else "partial")
        rows.append(
            {
                "crate": crate,
                "indexed_files": len(paths),
                "rs_files": rs_files,
                "langs": ",".join(f"{k}:{v}" for k, v in sorted(lang.items())),
                "bytes": crate_size.get(crate, 0),
                "unsafe_db": unsafe_n,
                "unsafe_src": src.get("unsafe_tok", 0),
                "transmute_ptr_db": transmute + std_ptr,
                "transmute_ptr_src": src.get("transmute_ptr_tok", 0),
                "parse_error": parse_err,
                "other_s_viol": other,
                "viol_total": sum(viol.values()),
                "subset_okish": subset_okish,
                "async_trait": src.get("async_trait", 0),
                "async_fn": src.get("async_fn", 0),
                "tokio_attr": src.get("tokio_attr", 0),
                "derive": src.get("derive", 0),
                "dyn_dispatch": src.get("dyn_dispatch", 0),
            }
        )

    # Rank: dirtiest first by viol_total, then unsafe_src
    rows.sort(key=lambda r: (-r["viol_total"], -r["unsafe_src"], -r["unsafe_db"], r["crate"]))
    rust_rows = [r for r in rows if r["rs_files"] > 0]
    rust_clean = sorted(
        [r for r in rust_rows if r["unsafe_db"] == 0 and r["unsafe_src"] == 0 and r["parse_error"] == 0],
        key=lambda r: (r["viol_total"], -r["rs_files"], r["crate"]),
    )
    rust_dirty = sorted(
        [r for r in rust_rows if r["viol_total"] > 0 or r["unsafe_src"] > 0],
        key=lambda r: (-r["viol_total"], -r["unsafe_db"], -r["unsafe_src"], r["crate"]),
    )

    top5_clean = rust_clean[:5]
    top5_dirty = rust_dirty[:5]

    # Console
    print("=== ALL buckets (dirtiest first) ===")
    hdr = f"{'crate':32} {'files':>5} {'rs':>4} {'uns_db':>6} {'uns_src':>7} {'ptr':>4} {'parse':>5} {'other':>5} {'total':>5} {'ok?':>7} {'async_fn':>8} {'async_tr':>8} {'tokio':>6} {'derive':>6}"
    print(hdr)
    for r in rows:
        if r["indexed_files"] == 0 and r["viol_total"] == 0:
            continue
        print(
            f"{r['crate'][:32]:32} {r['indexed_files']:5} {r['rs_files']:4} "
            f"{r['unsafe_db']:6} {r['unsafe_src']:7} {r['transmute_ptr_db']:4} "
            f"{r['parse_error']:5} {r['other_s_viol']:5} {r['viol_total']:5} "
            f"{r['subset_okish']:>7} {r['async_fn']:8} {r['async_trait']:8} "
            f"{r['tokio_attr']:6} {r['derive']:6}"
        )

    print("\n=== TOP-5 CLEANEST Rust crates (0 unsafe, 0 parse_error, sorted by size) ===")
    for r in top5_clean:
        print(f"  {r['crate']}: rs={r['rs_files']} viol={r['viol_total']} async_fn={r['async_fn']} derive={r['derive']}")

    print("\n=== TOP-5 DIRTIEST Rust crates ===")
    for r in top5_dirty:
        print(
            f"  {r['crate']}: rs={r['rs_files']} uns_db={r['unsafe_db']} uns_src={r['unsafe_src']} "
            f"ptr={r['transmute_ptr_db']} parse={r['parse_error']} total={r['viol_total']}"
        )

    # Write markdown
    lines = [
        "# Per-crate S-violation map — stock-trading-app",
        "",
        "**Corpus:** private `D:\\projects\\eval-corpus\\stock-trading-app` (do not commit source).",
        "**Index:** `.agentgraph/index.db` (operator-run; not CI).",
        "**Generated by:** `scripts/stock_s_map.py`",
        "",
        "## Method",
        "",
        "- Crate = `crates/<name>` path segment; non-crate trees bucketed by top-level dir.",
        "- `unsafe` / `parse_error` / `std_ptr` / `transmute` counts come from agentgraph `subset_violations` (AST scanner).",
        "- `unsafe_src` / `async_*` / `tokio` / `derive` / `dyn` are **regex operator stats** on `.rs` files (can overcount comments/strings; directionally useful, not golden).",
        "- `subset_ok-ish`:",
        "  - `yes` = zero subset_violations in any indexed file under this crate",
        "  - `partial` = violations present but **no** unsafe/parse_error/transmute (e.g. only `std_ptr` or py dynamic)",
        "  - `no` = at least one unsafe / parse_error / transmute",
        "- `--sound` eligibility for a *query* is still global `subset_ok` over the walked set — per-crate `yes` means **trial candidate when scoped**, not that full-repo sound is enabled.",
        "",
        "## Totals (index.db)",
        "",
    ]
    total_files = sum(r["indexed_files"] for r in rows)
    total_rs = sum(r["rs_files"] for r in rows)
    total_viol = sum(r["viol_total"] for r in rows)
    total_unsafe = sum(r["unsafe_db"] for r in rows)
    total_parse = sum(r["parse_error"] for r in rows)
    lines += [
        f"- Indexed files: **{total_files}**",
        f"- Rust files (indexed): **{total_rs}**",
        f"- subset_violations rows: **{total_viol}**",
        f"- of which `unsafe`: **{total_unsafe}**, `parse_error`: **{total_parse}**",
        "",
        "## Full table (dirtiest first)",
        "",
        "| crate | files | rs | unsafe_db | unsafe_src | transmute/ptr_db | parse_error | other_s | viol_total | subset_ok-ish | async_fn | async_trait | tokio_attr | derive |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|",
    ]
    for r in rows:
        if r["indexed_files"] == 0 and r["viol_total"] == 0:
            continue
        lines.append(
            f"| `{r['crate']}` | {r['indexed_files']} | {r['rs_files']} | {r['unsafe_db']} | {r['unsafe_src']} "
            f"| {r['transmute_ptr_db']} | {r['parse_error']} | {r['other_s_viol']} | {r['viol_total']} "
            f"| {r['subset_okish']} | {r['async_fn']} | {r['async_trait']} | {r['tokio_attr']} | {r['derive']} |"
        )

    lines += [
        "",
        "## Top-5 cleanest Rust crates (0 unsafe_db, 0 unsafe_src, 0 parse_error)",
        "",
        "Preferred for `--sound` **scoped trials** (index a subset / single crate root if product supports it; full-repo `subset_ok` stays false).",
        "",
        "| rank | crate | rs files | viol_total | async_fn | async_trait | derive | notes |",
        "|---:|---|---:|---:|---:|---:|---:|---|",
    ]
    for i, r in enumerate(top5_clean, 1):
        note = "trait-heavy" if r["derive"] > 20 else ("async-heavy" if r["async_fn"] > 10 else "clean/simple")
        lines.append(
            f"| {i} | `{r['crate']}` | {r['rs_files']} | {r['viol_total']} | {r['async_fn']} | {r['async_trait']} | {r['derive']} | {note} |"
        )

    lines += [
        "",
        "## Top-5 dirtiest Rust crates",
        "",
        "Do **not** claim `--sound` on these. Useful for honesty demos / parse_error recovery.",
        "",
        "| rank | crate | rs files | unsafe_db | unsafe_src | ptr | parse_error | viol_total |",
        "|---:|---|---:|---:|---:|---:|---:|---:|",
    ]
    for i, r in enumerate(top5_dirty, 1):
        lines.append(
            f"| {i} | `{r['crate']}` | {r['rs_files']} | {r['unsafe_db']} | {r['unsafe_src']} "
            f"| {r['transmute_ptr_db']} | {r['parse_error']} | {r['viol_total']} |"
        )

    # High-frequency patterns this repo actually shows
    async_heavy = sorted(rust_rows, key=lambda r: -r["async_fn"])[:8]
    trait_heavy = sorted(rust_rows, key=lambda r: -r["derive"])[:8]
    tokio_heavy = sorted(rust_rows, key=lambda r: -r["tokio_attr"])[:8]

    lines += [
        "",
        "## High-frequency Rust patterns in THIS repo (product targeting)",
        "",
        "### async_fn density (top crates)",
        "",
        "| crate | rs | async_fn | async_trait | tokio_attr | derive |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for r in async_heavy:
        lines.append(
            f"| `{r['crate']}` | {r['rs_files']} | {r['async_fn']} | {r['async_trait']} | {r['tokio_attr']} | {r['derive']} |"
        )
    lines += [
        "",
        "### derive / trait density (top crates)",
        "",
        "| crate | rs | derive | async_fn | dyn_dispatch (src regex) |",
        "|---|---:|---:|---:|---:|",
    ]
    for r in trait_heavy:
        lines.append(
            f"| `{r['crate']}` | {r['rs_files']} | {r['derive']} | {r['async_fn']} | {r['dyn_dispatch']} |"
        )
    lines += [
        "",
        "### tokio attribute density",
        "",
        "| crate | rs | tokio_attr | async_fn |",
        "|---|---:|---:|---:|",
    ]
    for r in tokio_heavy:
        lines.append(f"| `{r['crate']}` | {r['rs_files']} | {r['tokio_attr']} | {r['async_fn']} |")

    lines += [
        "",
        "## Honesty limits",
        "",
        "- Full-repo `subset_ok` remains **false** while any violation remains in the walked set (current index: 114 rows).",
        "- Per-crate `subset_ok-ish=yes` is a **trial candidate**, not a product claim.",
        "- `unsafe_src` is regex and may count comments; DB `unsafe_db` is AST and is the S-map authority.",
        "- Do not restore `--sound` marketing on unsafe crates; do not expand proc-macros.",
        "",
        "## Reproduce",
        "",
        "```powershell",
        "python scripts\\stock_s_map.py",
        "python scripts\\eval_subset_kinds.py D:\\projects\\eval-corpus\\stock-trading-app",
        "```",
        "",
    ]

    OUT_MD.parent.mkdir(parents=True, exist_ok=True)
    OUT_MD.write_text("\n".join(lines), encoding="utf-8")
    print(f"\nWrote {OUT_MD}")

    # Also dump a compact TSV for golden-script use
    tsv = OUT_MD.with_suffix(".tsv")
    tsv_lines = [
        "crate\tindexed_files\trs_files\tunsafe_db\tunsafe_src\ttransmute_ptr_db\tparse_error\tother_s\tviol_total\tsubset_okish\tasync_fn\tasync_trait\ttokio_attr\tderive\tdyn_dispatch"
    ]
    for r in rows:
        tsv_lines.append(
            f"{r['crate']}\t{r['indexed_files']}\t{r['rs_files']}\t{r['unsafe_db']}\t{r['unsafe_src']}\t"
            f"{r['transmute_ptr_db']}\t{r['parse_error']}\t{r['other_s_viol']}\t{r['viol_total']}\t"
            f"{r['subset_okish']}\t{r['async_fn']}\t{r['async_trait']}\t{r['tokio_attr']}\t{r['derive']}\t{r['dyn_dispatch']}"
        )
    tsv.write_text("\n".join(tsv_lines) + "\n", encoding="utf-8")
    print(f"Wrote {tsv}")

    conn.close()


if __name__ == "__main__":
    main()
