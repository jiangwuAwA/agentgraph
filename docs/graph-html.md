# HTML code-graph visualization (`agentgraph graph`)

**Status:** shipped. Offline, self-contained HTML.  
**Non-claim / Honesty:** the page shows **indexed** L0/L1 (and optional L1 dynamic / macro sidecar) edges — **not** a complete runtime graph and not a soundness proof. No zero-miss / ecosystem-sound / macro-complete guarantee.  
`--sound` pages show **sound-eligible modeled edges only** when `subset_ok=true` (engineering S gate, still not a complete runtime graph); when `subset_ok=false` the HTML is still written but is **not** labeled a sound graph.
**Reproduce:** `cargo test --test graph_html` or `agentgraph --root fixtures/sample-app index --force && agentgraph --root fixtures/sample-app graph <symbol>`.

---

## Command

```bash
agentgraph index   # required first (empty index fails loudly)
agentgraph graph <symbol> \
  [--depth N] \
  [--out path.html] \
  [--direction impact|callers|both] \
  [--impact] \
  [--exact-only] [--include-dynamic] [--with-macro] \
  [--sound]
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
| `--sound` | L2: only sound-eligible edges; header shows `subset_ok` + `promise_tier`. **Mutually exclusive** with `--with-macro` and with `--exact-only` / `--include-dynamic` |

**Default `--out`:** when omitted, writes `<root>/.agentgraph/graph.html` (creates the directory).  
Open the file in any browser. **No network**, no CDN, no build step.

### Exit contract

| Case | Behavior |
|---|---|
| Success with nodes/edges | exit `0`, HTML written, stdout path + counts |
| Indexed project, unknown symbol / no edges | exit `0`, **empty-state** HTML written, stderr note |
| Index missing / empty | exit non-zero (same as `callers` / `impact`) |
| `--sound` + `subset_ok=true` | exit `0`, HTML with S-qualified header |
| `--sound` + `subset_ok=false` | HTML **still written**, exit **non-zero**; page marked disabled / **not** a sound graph |
| `--sound --with-macro` (or `--sound --exact-only`) | fail-closed (mutually exclusive) |

Empty neighborhood is **not** a hard failure (non-sound path): the deliverable is the honest page.

---

## What the page contains

1. **Header** — bilingual title, query name, flags, depth.
2. **Honesty line** — always. Default:  
   `L0/L1 candidates, not a complete runtime graph · L0/L1 候选边，非完整运行时图`  
   With `--sound` + `subset_ok=true`:  
   `S-qualified sound-eligible edges only (modeled L2) — not a complete runtime graph … subset_ok=true · promise_tier=…`  
   With `--sound` + `subset_ok=false`:  
   `S VIOLATED — this page is NOT a sound graph … promise_tier=disabled`
3. **Sound banner** (when `--sound`) — `subset_ok` + `promise_tier`; red “DISABLED” style when violated.
4. **Legend** — Exact (green) / Heuristic (amber) / DynamicCandidate (purple) / macro badge.
5. **SVG graph** — radial BFS layout, center = query symbol, rings = depth.
6. **Node labels** — name, `d{depth}`, confidence; `path:line` in tooltip / info panel.
7. **Click** — highlights neighbors, fills the info panel (path, line, origin, edge kinds).
8. **Macro badge** — `MACRO` + distinct stroke when `origin=macro_expanded` (`--with-macro`).
9. **Cap** — at most **300 nodes**; truncation notice if more would have been drawn.
10. **XSS** — all names/paths HTML-escaped; JSON blob uses `\u003c` escapes.

---

## `--sound` (L2) visualization — Track M4

Edges on a `--sound` page come from the same eligibility filter as
`impact --sound` / `callers --sound`:

- Exact syntactic calls
- Allowlisted Heuristic DI/route/event rules (see [sound-subset.md](sound-subset.md))
- Finite-domain dynamic rules (string-literal keys)

**When `subset_ok=false`:**

- HTML is still written (deliverable = honest page).
- The page is **not** labeled a sound graph; honesty + banner say DISABLED.
- Exit code is non-zero so agents do not treat it as a certified sound view.
- Best-effort sound-eligible candidates may still render; `promise_tier=disabled`.

**After watch / `index_paths`:** subset violations are re-certified on dirty
files (Track M4 S re-cert). `graph --sound` therefore reflects **current disk**
S state, not a stale last-full-index-only snapshot. See
[sound-subset.md](sound-subset.md) and [graph-diff.md](graph-diff.md).

---

## Implementation map

| Piece | Location |
|---|---|
| Pure renderer | `src/viz/mod.rs` → `render_graph_html(&GraphVizData) -> String` |
| Graph builders | `build_impact_graph` / `build_callers_graph` / `merge_graphs` |
| Sound walk data | `Store::impact_sound` / `Store::callers_sound` |
| Macro badges | `add_macro_impact_rows` / `add_macro_caller_rows` |
| CLI | `src/cli.rs` → `Commands::Graph` (`--sound`) |
| Tests | `tests/graph_html.rs` |

Reuse: same store queries and confidence filters as `impact` / `callers` (`Query::impact_filtered` / `callers_filtered`); `--sound` uses the L2 sound walk.  
Read-only against the main index; the only write is the output HTML file.

---

## Sample (fixture)

Against the e2e TS fixture (`helper` ← `createUser` ← `loginHandler`, plus DI heuristic):

```bash
agentgraph graph helper --depth 2 --out graph.html
agentgraph graph helper --sound --out graph-sound.html
```

Typical neighborhood: query `helper` + `createUser` + `loginHandler` (and call-site nodes when enclosing is missing) — on the order of **3–8 nodes** for this tiny app, not hundreds.

---

## Non-goals

- No force-directed “complete graph” marketing claim.
- No claim that a `--sound` page is ecosystem sound / production sound.
- No remote fetch of assets or telemetry.
- No `--sound --with-macro` combination (macro sidecar is never sound-certified).

