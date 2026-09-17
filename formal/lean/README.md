# MiniLang (Lean 4) — L3 I4 containment theorem

Research track artifact. **Not** a proof about JavaScript/Python/Go, and **not**
an upgrade of L2 `--sound`.

## What is proven

For the mini-language `S_L` (direct calls + string-literal table dispatch +
`if` with both arms explored; no reflection / non-literal keys):

```text
∀ P a b,  RuntimeEdge P a b  →  Reaches P a b
```

i.e. every runtime call edge recorded from `main` is contained in the static
transitive call-closure (`runtime_subset_static`).

## Build

Requires [elan](https://github.com/leanprover/elan). Toolchain is pinned in
[`lean-toolchain`](lean-toolchain) (`v4.34.0`). **Stdlib only — no mathlib.**

```powershell
# Windows (PowerShell)
$env:Path = "$env:USERPROFILE\.elan\bin;$env:Path"
cd formal\lean
lake build
```

Expected: `Build completed successfully` (exit 0).

## Correspondence table

| Lean (`MiniLang/Basic.lean`) | Rust (`src/formal/mini_lang.rs`) |
|---|---|
| `Stmt.call` / `dispatchLit` / `ite` / `nop` | `Stmt::Call` / `DispatchLit` / `If` / `Nop` |
| `Program` = `List (Name × List Stmt)` | `Program.funcs : BTreeMap<String, Vec<Stmt>>` |
| `Program.body` | `funcs.get(name)` (missing → `[]`) |
| `directEdgesOf` | `collect_edges` |
| `directEdges` | `Program::direct_edges` |
| `Reaches` (inductive transitive closure) | `Program::static_closure` (Floyd expansion) |
| `BodyCalls` / `StmtCalls` | edges taken by `interpret` / `run_body` |
| `Enters` | functions reachable from `main` under fuel/depth |
| `RuntimeEdge` | `Runtime::edges` |
| `runtime_subset_static` | `runtime_subset_of_static` (checked by `tests/l4_mini_lang.rs`) |

Differences (documented, not hidden):

- Lean `Reaches` is an inductive closure relation; Rust `static_closure` is a
  materialized set via Floyd-style iteration. They agree on membership.
- Lean does not model fuel/depth cutoffs; the inductive `Enters`/`BodyCalls`
  relation is the unbounded “enough fuel” limit of the Rust interpreter.
  Bounded-fuel soundness is still covered by the Rust exhaustive tests.

## Non-goals

- Not JS / TS / Python / Go / Rust ecosystem soundness
- Not L2 `--sound` production guarantee
- Not agentgraph’s full extract/DI/qualifier graph
- Not required on the main GitHub Actions Rust CI

## Layout

| File | Role |
|---|---|
| `MiniLang/Basic.lean` | syntax, static analysis, runtime relation, I4 theorem |
| `MiniLang.lean` | library root import |
| `lakefile.toml` | Lake package (lib `MiniLang`) |
| `lean-toolchain` | pinned Lean version |
