# Package map coverage — workspace cross-root imports

**Status:** shipped partial package map (not full TypeScript resolution).  
**Scope:** multi-root `agentgraph index --workspace` / `--workspace-root` +  
`--workspace-alias`, `tsconfig` paths, `package.json` `name`.  
**Non-goals:** `node_modules` external resolution; `exports` condition graphs;
TS project references; monorepo build-system graph.  
This is **not** ecosystem sound and **not** a product guarantee.

Reproduce: `cargo test --test workspace_package_alias --test workspace_package_dup`.

## Scenario coverage

| Scenario | Coverage | Behavior |
|---|---|---|
| `package.json` `name` auto-map | **shipped** | `{"name":"@demo/registry"}` → root; lowest priority |
| CLI `--workspace-alias pkg=root` | **shipped** | highest priority; overrides all |
| `tsconfig` `compilerOptions.paths` | **shipped** | medium priority; first usable target wins |
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

## Tests

| Test | What it locks |
|---|---|
| `duplicate_package_name_fails_loud_without_override` | two roots, same `@demo/registry` → index fails with `duplicate` + package name |
| `duplicate_package_name_cli_override_indexes` | `--workspace-alias @demo/registry=aa` → index succeeds |
| `package_alias_subpath_resolves_to_same_root` | `@demo/registry/client` → package `@demo/registry` / root `registry` |
| `hard_fixture_package_json_enables_cross_root_link` | hard fixture auto-map + `importers` |
| `workspace_package_alias_links_cross_root_import` | meta + linking |
| tsconfig / package.json / CLI priority tests | `discover_package_aliases` |

## Operator monorepo smoke（公开 fixture / 非私有源码）

本机 operator 抽检（**不**提交私有仓路径源码）：

| 检查 | 结果 |
|---|---|
| public hard fixture `ts-multi-root-client`（含 `package.json`） | **pass** — `package_aliases` 含 `@demo/registry` 等 |
| `importers @demo/registry` | **pass** — `order.service.ts` 行；`resolved=src/index.ts`（barrel） |
| 重名包 fail-loud | **pass**（`tests/workspace_package_dup.rs`） |
| 子路径 `@demo/registry/client` | **pass**（unit resolve） |
| nestjs-starter `package.json` `name` | 可发现（单包名 `nest-typescript-starter`；无跨 root 包名冲突样本） |
| exports conditions / node_modules | **gap**（保持 open） |
| 真实脏 monorepo 全量抽检 | **blocked on private corpus** — 仅 operator 本地可跑；不进 CI、不贴源码 |

结论：公开 fixture + hard fixture 已锁主要行为；真实脏 monorepo 证据仍 residual open。

## Post-fix note

Recorded P0-5c/5d trajectory scores are **not** rewritten. Optional
post-fix re-score of `evals/agent-ab-c` / `evals/agent-ab-d` may be run
alongside historical tables only.
