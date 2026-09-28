# Changelog

All notable product cuts. Honesty: package map stays **partial** (not full TypeScript
resolution / not complete npm graph / not ecosystem sound).

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
