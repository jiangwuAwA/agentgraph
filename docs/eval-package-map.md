# Package map coverage — workspace cross-root imports

**Status:** shipped partial package map (not full TypeScript resolution).  
**Scope:** multi-root `agentgraph index --workspace` / `--workspace-root` +  
`--workspace-alias`, `tsconfig` paths, `package.json` `name`.  
**Non-goals:** `node_modules` external resolution; `exports` condition graphs;
TS project references; monorepo build-system graph.  
This is **not** ecosystem sound and **not** a product guarantee.

Reproduce: `cargo test --test workspace_package_alias --test workspace_package_dup --test tsconfig_wildcard_paths`.

## Scenario coverage

| Scenario | Coverage | Behavior |
|---|---|---|
| `package.json` `name` auto-map | **shipped** | `{"name":"@demo/registry"}` → root; lowest priority |
| CLI `--workspace-alias pkg=root` | **shipped** | highest priority; overrides all |
| `tsconfig` `compilerOptions.paths` | **shipped** | medium priority; first usable target wins |
| **tsconfig wildcard** `@/*` → `src/*` | **partial covered (F6)** | import subpath probed with **limited extension table** below; concrete file → `resolved` set; no match → `resolved` stays null (**never invent**) |
| Subpath `@demo/registry/client` | **shipped** | `split_package_specifier` + `resolve_package_import`; same package root |
| **Two roots same package name** | **shipped (fail-loud)** | `index` **errors** unless `--workspace-alias` picks a root — **no silent first-root** (`workspace_package_dup`) |
| Nested workspace roots | **limited** | roots may nest with warning; package map is per-root `package.json` only |
| `exports` conditions | **gap** | not parsed; record as non-support (do not claim) |
| `node_modules` external packages | **gap / explicit non-goal** | **not resolved** — unknown packages stay unresolved |
| Duplicate `id` in manifest | **shipped** | dedup/reject paths in `finalize_roots` (pre-existing) |

## Honesty

- Partial package map only — **not** full TS module resolution.
- Unmapped package imports remain unlinked (`resolved` absent).
- Fail-loud duplicate names prevent **silent cross-root** aliasing.
- No claim that blast-radius is complete across `node_modules` or `exports`.

### F6 limited extension table (wildcard only)

When a tsconfig `paths` key contains `*` (e.g. `"@/*": ["src/*"]`), the import
subpath is substituted into the target pattern and probed **on disk** with this
finite table only (first hit wins):

| Probe order | Candidates |
|---|---|
| 1. exact | `src/config/chartTheme` as a file (if already present) |
| 2. extensions | `…/chartTheme.ts` · `.tsx` · `.d.ts` · `.js` · `.jsx` |
| 3. directory index | `…/chartTheme/index.ts` · `index.tsx` · `index.d.ts` · `index.js` · `index.jsx` |

**Not probed:** `exports` conditions, `node_modules`, `.mjs`/`.cjs`/`.json`,
recursive project references, `package.json` `imports`. Failure leaves
`resolved` **null** — never a guessed path. Exact (non-wildcard) package keys
keep the declared barrel entry without this probe.

## Tests

| Test | What it locks |
|---|---|
| `duplicate_package_name_fails_loud_without_override` | two roots, same `@demo/registry` → index fails with `duplicate` + package name |
| `duplicate_package_name_cli_override_indexes` | `--workspace-alias @demo/registry=aa` → index succeeds |
| `package_alias_subpath_resolves_to_same_root` | `@demo/registry/client` → package `@demo/registry` / root `registry` |
| `hard_fixture_package_json_enables_cross_root_link` | hard fixture auto-map + `importers` |
| `workspace_package_alias_links_cross_root_import` | meta + linking |
| tsconfig / package.json / CLI priority tests | `discover_package_aliases` |
| `tsconfig_wildcard_resolves_existing_ts_file` | `@/config/chartTheme` → `src/config/chartTheme.ts` (exists) |
| `tsconfig_wildcard_resolves_tsx_and_index_dir` | `.tsx` + `/index.ts` probes |
| `tsconfig_wildcard_missing_file_does_not_invent_path` | no file → no invented `workspace_file` |
| `e2e_wildcard_imports_get_resolved_concrete_file` | index-time `resolved` = concrete file |
| `e2e_wildcard_missing_leaf_leaves_resolved_null` | missing leaf → `resolved` null |

## Operator monorepo smoke（公开 fixture / 非私有源码）

本机 operator 抽检（**不**提交私有仓路径源码）：

### 公开 hard fixture `ts-multi-root-client`

| 检查 | 结果 |
|---|---|
| package.json name 自动映射 | **pass** — `@demo/registry` 等 |
| `importers @demo/registry` | **pass** — `order.service.ts`；`resolved=src/index.ts` |
| 重名包 fail-loud | **pass**（`tests/workspace_package_dup.rs`） |
| 子路径 `@demo/registry/client` | **pass**（unit resolve） |

### 真实量化仓切片（operator 本地，不贴源码）

样本：**4 roots** · frontend 198 files / 7485 refs + 3 个 Rust crate（event-engine 5/384、repository 27/915、auth 5/471）。

| 规则来源 | 包名 / 路径 | 结果 |
|---|---|---|
| `package.json` `name` | `stock-trading-app` → `frontend` | **覆盖** |
| `tsconfig` `paths` | `@/*` → `frontend`（`entry=src/*`） | **别名存在** |
| `importers @/config/chartTheme` | 8 条 import | **命中 module 列表** |
| `resolved` 映射 | `@/*` 通配 | **partial covered (F6)** — 有限扩展表落到具体文件；无匹配文件保持 `null` |
| 外部包 `@ant-design/icons` 等 | `@tanstack/*`、`@testing-library/*` | **gap / non-goal** — 只列 module，不进 `node_modules` |
| Rust crate 路径 | `crates/*` | **n/a** — Cargo 路径，非 npm 包名 |
| exports conditions | — | **gap** |
| 嵌套 workspace 包 | — | **有限**（nest warn） |
| 重名包 fail-loud | 合成 fixture | **pass**；本切片无冲突样本 |

**结论：** package.json / tsconfig 别名能发现；**通配 `@/*` 在 F6 后可解析到具体文件（有限扩展表）**，无匹配不编造；外部 npm 包 / `exports` 保持 non-goal / gap。

### 非私有 / 非阻塞 residual

- 真实脏 monorepo 全量抽检：**blocked on private corpus**（operator 本地可跑；不进 CI、不贴源码）
- exports conditions、`node_modules`、重名 Agent 发现 override：residual open
- ~~通配 `@/*` resolved~~：**F6 closed**（有限扩展表；见上方 Honesty）

## Post-fix note

Recorded P0-5c/5d trajectory scores are **not** rewritten. Optional
post-fix re-score of `evals/agent-ab-c` / `evals/agent-ab-d` may be run
alongside historical tables only.
