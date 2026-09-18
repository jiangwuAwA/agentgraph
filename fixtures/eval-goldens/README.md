# fixtures/eval-goldens — public synthetic goldens (M5)

**Status:** shipped as **minimal public synthetic** corpora for credibility-gate
and future L1/L2/L3 regression (Track M5 / product-boundary-migration).

**Non-claim:**
- Edges here are **candidates / expected sites**, not a soundness proof.
- Not a complete runtime graph; no zero-miss / ecosystem-sound / macro-complete
  product guarantee.
- `expected_in_s: true` marks a **clean modeled slice** that should not mint S
  violations (e.g. type-only `typeof Function`, literal `getattr`) — it is
  **not** an ecosystem-sound claim.
- This directory is **not** a private quant monorepo corpus and must never
  contain private sources or expand artifacts.

**Reproduce (smoke):** files + `golden.json` are checked by
`cargo test --test docs_claims`. Optional local extract smoke:

```bash
cargo test --test docs_claims
cargo test --test l1_eval -- --nocapture
cargo test --test l2_sound
```

## Layout

| path | shape | purpose |
|---|---|---|
| `ts-nest-mini/` | Nest-like TS module/controller/service | public TS DI / `@Module` registration shapes |
| `rust-inventory-mini/` | `inventory::submit!` registry + impl Trait | public Rust registry/DI shapes (`rs.di.*`) |
| `rust-dyn-mini/` | `dyn Trait` + same-file `impl Trait for T` | public M3-A dyn-trait candidate shapes (`rs.di.dyn_trait_method`) |
| `go-iface-mini/` | interface assert `var _ I = (*T)(nil)` + method-set | public M3-B Go iface v2 shapes (`go.di.interface_impl_v2`) |
| `ts-router-mini/` | Express-style `router.get/post` + `app.use` | public M3-D router registration (`ts.framework.register`) |
| `ts-typeonly-function/` | type-only `typeof Function` + Exact calls | public M2 over-flag S golden — type position stays in S |
| `py-overflag/` | literal `getattr` / literal `import_module` | public M2 clean-S Python golden — no non-literal dynamics |
| `golden.json` | expected edge sites + confidence hints | machine-readable goldens for M1–M3 + M2 S slices |

## Honesty

Goldens lock **extractable structure** (symbol → site snippet). They do **not**
claim:
- expand/sidecar completeness
- L2 `--sound` eligibility of every edge
- production monorepo precision
- ecosystem soundness for any language

`expected_in_s` corpora exist so M2 over-flag regressions fail closed in review;
runtime ⊆ sound edges is still proven only by the S differential harnesses
(`tests/l2_*.rs`), not by this directory alone.
