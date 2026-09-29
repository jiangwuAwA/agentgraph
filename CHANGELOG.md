# Changelog

All notable product cuts. Honesty: package map stays **partial** (not full TypeScript
resolution / not complete npm graph / not ecosystem sound).

## v0.5.13

### Added
- **I3 precision policy:** default K=5; hard-decoy dir prune; dir max2; pure-decoy cap1;
  definition from import resolved (drops same-name decoy defs).

### Eval (I-track, additive)
- I3 live: A extra-noise **1.43** vs B **2.18** (delta ~0.75; peak 1.2); precision **0.71 vs 0.56**; recall A >= B.
- **Budgeted** noise advantage only. Not H4 / not 生态 sound. Historical scores unchanged.

## v0.5.12

### Added
- **I2 precision file policy** on blast_radius / who_calls:
  file_budget / selected / pruned / pruned_count / selection_reason (stable keys).
  Default **少而准** (K=8); demote legacy/admin/common-name; same-dir diversity.

### Eval (I-track, additive)
- I2 live: A extra-noise **4.86** vs B **5.43** (delta ~0.57), recall equal — **budgeted** noise advantage, not ecosystem-wide.
- **No** live noise advantage claim beyond this budgeted slice. Not 生态 sound.

## v0.5.11

### Added
- H4-live matrix **closed**: 36/36 cells at **N=5** (180 scored).
  `lab_ready=true` (isolated live matrix completeness **only**).

### H4-live limits
- Live A/B **noise not separated** (extra-noise 0.00 all arms) — **recall + cwr** only.
- **No** live noise advantage / 生态 sound / full TS claims.
- Historical P0-5c/d scores **unchanged**.

## v0.5.10

### Added
- H4-live matrix fill: easy+hard x A/B x 2 live runners, **N=5** on 31/36 cells
  (5 residual cells <5 — listed by lab-ready; **not invented**).

### H4-live limits
- Live A/B **noise not separated** (extra-noise 0.00 all arms) — **recall + cwr** only.
- `lab_ready=false` until residual cells reach N>=5.
- Historical P0-5c/d scores **unchanged**.

## v0.5.9

### Added
- **H4-live** hard-slice live A/B via isolated `mimo run` sessions
  (`xiaomi/mimo-v2.6-pro` / `xiaomi/mimo-v2.6-flash`, `independent_session=true`).
- Trajectories `evals/agent-ab-h4/**` + stamp/score replay.

### H4-live limits (honest)
- Live A/B **noise not separated** (extra-noise ≈ 0 both arms) — primary signals
  **recall + cwr**. **Do not** claim live noise advantage.
- Per-cell real N is labeled; matrix may be partial (`lab_ready=false` until complete).
- Historical P0-5c/d scores are **not** rewritten.

### Citable
- structure-fact / recall + cwr / hard scripted noise / package map partial

### Not citable
- live noise advantage · 生态 sound · full TS resolution · complete npm graph

## v0.5.8 (H1–H6 close-out)

### Added
- **H1** `package.json` `exports` **subset**: `"."` / `"./sub"` keys; conditions
  `import` > `default` > `require` > `types`. Priority: exports key → package root
  → tsconfig wildcard. **Not** full exports graphs / `imports` / custom conditions.
- **H2** `external_dependency=true` on importers rows for non-workspace npm packages
  (module-only; no invented file edges).
- **H3** public synthetic dirty fixture `fixtures/eval-package-map-dirty/` +
  coverage matrix tests.
- **H4** denser hard fixture `ts-dense-alias-noise` (same-name decoys, cross-root
  alias, legacy collision). Live N≥8 **blocked** (no host LLM runner); historical
  scores **not rewritten**.
- **H5** P2-3 eval gate: exports/import goldens already covered; extra Go iface
  rule has no lift → **eval 不支持扩规则** (no new L1 rules shipped).
- **H6** this changelog.

### Citable
- structure-fact / recall + cwr / hard scripted noise comparison
- package map **partial** (wildcard resolved + exports subset + external honest)

### Not citable
- live noise advantage (live noise **not separated**)
- 生态 sound / complete module resolution / full npm graph

## v0.5.7

- tsconfig wildcard `@/*` resolved via **limited extension table**
  (`.ts/.tsx/.d.ts/.js/.jsx` + `index.*`); miss → `resolved=null` (**never invent**).
- Package map remains **partial**.

## v0.5.6

- Duplicate `package.json` names **fail-loud** unless `--workspace-alias` override.
- Subpath package resolve + `docs/eval-package-map.md`.

## v0.5.5 / v0.5.4

- TS `export { X } from` extract; package-alias fixture lock; release hygiene.

## v0.5.0 – v0.5.3

- Agent path recipes, onboarding, P0–P2 evals, workspace multi-root, noise
  governance, macro_default, isolated lab (`lab_ready=true`).
