# HTML code-graph visualization (`agentgraph graph`)

**Status:** shipped. Offline, self-contained HTML.  
**Honesty:** the page shows **indexed** L0/L1 (and optional L1 dynamic / macro sidecar) edges — **not** a complete runtime graph and not a soundness proof.

---

## Command

```bash
agentgraph index   # required first (empty index fails loudly)
agentgraph graph <symbol> \
  [--depth N] \
  [--out path.html] \
  [--direction impact|callers|both] \
  [--impact] \
  [--exact-only] [--include-dynamic] [--with-macro]
```

| Flag | Meaning |
|---|---|
| *(default view)* | **Impact-style BFS** — outgoing blast radius from the symbol |
| `--depth N` | Impact BFS depth (default `2`) |
| `--direction callers` | Direct call/reference sites (depth-1 neighborhood) |
| `--direction both` | Union of impact + callers |
| `--impact` | Alias for `--direction impact` |
| `--out path.html` | Output file (default: `<root>/.agentgraph/graph.html`) |
| `--exact-only` | Only L0 Exact edges |
| `--include-dynamic` | Also DynamicCandidate (noisier) |
| `--with-macro` | Union optional macro sidecar rows (badge `origin=macro_expanded`) |

**Default `--out`:** when omitted, writes `<root>/.agentgraph/graph.html` (creates the directory).  
Open the file in any browser. **No network**, no CDN, no build step.

### Exit contract

| Case | Behavior |
|---|---|
| Success with nodes/edges | exit `0`, HTML written, stdout path + counts |
| Indexed project, unknown symbol / no edges | exit `0`, **empty-state** HTML written, stderr note |
| Index missing / empty | exit non-zero (same as `callers` / `impact`) |

Empty neighborhood is **not** a hard failure: the deliverable is the honest page.

---

## What the page contains

1. **Header** — bilingual title, query name, flags, depth.
2. **Honesty line** — always:  
   `L0/L1 candidates, not a complete runtime graph · L0/L1 候选边，非完整运行时图`  
   Plus `subset_ok` / `promise_tier` when sound flags are used in the data model.
3. **Legend** — Exact (green) / Heuristic (amber) / DynamicCandidate (purple) / macro badge.
4. **SVG graph** — radial BFS layout, center = query symbol, rings = depth.
5. **Node labels** — name, `d{depth}`, confidence; `path:line` in tooltip / info panel.
6. **Click** — highlights neighbors, fills the info panel (path, line, origin, edge kinds).
7. **Macro badge** — `MACRO` + distinct stroke when `origin=macro_expanded` (`--with-macro`).
8. **Cap** — at most **300 nodes**; truncation notice if more would have been drawn.
9. **XSS** — all names/paths HTML-escaped; JSON blob uses `\u003c` escapes.

---

## Implementation map

| Piece | Location |
|---|---|
| Pure renderer | `src/viz/mod.rs` → `render_graph_html(&GraphVizData) -> String` |
| Graph builders | `build_impact_graph` / `build_callers_graph` / `merge_graphs` |
| Macro badges | `add_macro_impact_rows` / `add_macro_caller_rows` |
| CLI | `src/cli.rs` → `Commands::Graph` |
| Tests | `tests/graph_html.rs` |

Reuse: same store queries and confidence filters as `impact` / `callers` (`Query::impact_filtered` / `callers_filtered`).  
Read-only against the main index; the only write is the output HTML file.

---

## Sample (fixture)

Against the e2e TS fixture (`helper` ← `createUser` ← `loginHandler`, plus DI heuristic):

```bash
agentgraph graph helper --depth 2 --out graph.html
```

Typical neighborhood: query `helper` + `createUser` + `loginHandler` (and call-site nodes when enclosing is missing) — on the order of **3–8 nodes** for this tiny app, not hundreds.

---

## Non-goals

- No force-directed “complete graph” marketing claim.
- No L2 sound walk in the HTML CLI today (use `impact --sound` JSON for S-qualified rows).
- No remote fetch of assets or telemetry.
