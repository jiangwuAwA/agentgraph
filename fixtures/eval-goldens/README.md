# fixtures/eval-goldens — public synthetic goldens (M5)

**Status:** shipped as **minimal public synthetic** corpora for credibility-gate
and future L1/L2/L3 regression (Track M5 / product-boundary-migration).

**Non-claim:**
- Edges here are **candidates / expected sites**, not a soundness proof.
- Not a complete runtime graph; no zero-miss / ecosystem-sound / macro-complete
  product guarantee.
- This directory is **not** a private quant monorepo corpus and must never
  contain private sources or expand artifacts.

**Reproduce (smoke):** files + `golden.json` are checked by
`cargo test --test docs_claims`. Optional local extract smoke:

```bash
cargo test --test docs_claims
# future L1 harness can point at fixtures/eval-goldens/golden.json
```

## Layout

| path | shape | purpose |
|---|---|---|
| `ts-nest-mini/` | Nest-like TS module/controller/service | public TS DI / `@Module` registration shapes |
| `rust-inventory-mini/` | `inventory::submit!` registry + impl Trait | public Rust registry/DI shapes (`rs.di.*`) |
| `golden.json` | expected edge sites + confidence hints | machine-readable goldens for M1–M3 regression |

## Honesty

Goldens lock **extractable structure** (symbol → site snippet). They do **not**
claim:
- expand/sidecar completeness
- L2 `--sound` eligibility of every edge
- production monorepo precision
