# 产品边界迁移改动清单（全量）

> **文档性质：** 实施规格，不是宣传文案。  
> **项目：** agentgraph（`D:\projects\agentgraph`）  
> **上游讨论：** expand 旁路 ≠ 产品边界；迁移边界 = 改 **默认承诺 / CLI·MCP 默认路径 / `src/` 能力**。  
> **开发约定：** TDD（见 [AGENTS.md](../AGENTS.md)）；门禁 `cargo fmt` + `cargo clippy --all-targets -- -D warnings` + `cargo test` + CLI E2E + `scip lint`。  
> **禁止超售：** 任何 track 落地后，README/CLI **不得**出现「零漏报 / 生态 sound / 宏完整」表述（PLAN §0.2、§6）。

**产品边界前移的验收定义（本清单统一用语）：**

| 前移类型 | 含义 | 不算前移 |
|---|---|---|
| **默认路径** | 不加开关时 Agent 能得到的结构事实变多/更准 | 只增加可选 flag 的运维用法 |
| **承诺档位** | `promise` / `promise_tier` / `subset_ok` 适用范围或诚实度升级 | 文案变响、数字只进私有仓 |
| **API 面** | CLI/MCP/HTML 新增 Agent 可调用的结构能力 | runbook 脚本、影子树操作步骤 |
| **可信度门** | 发版自动证明「实现 ⊆ 文档承诺」 | 人工对照清单（可保留，不够） |

---

## 0. 全局原则（所有 track 共用）

1. **默认仍是源码 L0/L1**；任何更宽召回必须可关、可解释、带 `confidence` + `evidence`。
2. **`--sound` 仍与 `--with-macro` 互斥**，除非某 track 明确交付「S 认证的 expand 边」——本清单 **M1 不做** sound expand-graph。
3. **金标/评测数字必须进** `docs/eval-*.md`；README 只引用，不新造口号。
4. **不把私有 corpus 源码或 expand 产物提交进本仓**；可提交 harness、期望边、fixture。
5. **每个 track 独立可合并**；未达验收不得改 README 能力句。
6. **已知诚实债优先修：** over-flag、`promise_tier` 文案与实现漂移、sidecar 与主图重复计数。

### 建议实施顺序

```text
M5（门禁骨架，可与其它并行）
  → M1（宏边产品化：路径映射+去重+重建策略）  【默认路径前移，优先】
  → M2（L2 承诺档位：AST 升档 + over-flag）   【承诺档位前移】
  → M3（L1 默认召回规则 + 评测）             【默认路径前移】
  → M4（时序 diff / workspace / S 重认证 / HTML L2）【API 面前移】
```

依赖关系：M1 的重建策略与 M2 的 S 重认证在 index/watch 管道上相遇，**实现时对齐 `Indexer::index` / `index_paths` 钩子**；M5 为所有 README 变更提供门禁。

---

## Track M1 — 宏展开旁路产品化（path map + de-dup + 重建 + 默认策略）

### 1.1 目标（落地后的产品句）

- 默认或**显式一键**路径下，derive/proc-macro 形状的**候选边**可进入查询结果，且：
  - 路径映射回源码 crate（不是 `expanded-view/...` 影子路径）；
  - 与源码 Exact/Heuristic **去重**；
  - sidecar 过期可检测、可重建；
  - 仍 **`origin=macro_candidate`（或保留 `macro_expanded`）+ 非 sound**。
- 运维不再手写双 root diff 才能用上宏边。

**产品边界变化：** 默认/一键路径的图内容与去重语义 —— *默认路径前移*。

### 1.2 非目标

- 不做 sound expand-graph；不把 expand 边并入 `--sound` 集合。
- 不做任意 proc-macro 完备；编译失败 crate 仍可跳过。
- 不把 expand 设为 required Rust CI。
- 不自动 `cargo expand` 外呼（工具链非 hermetic）；索引阶段只消费**已有** expanded 产物或用户提供的 root。

### 1.3 改动清单（文件级）

| # | 文件 | 改动 |
|---|---|---|
| M1.1 | `src/model.rs` | 扩展 `MacroSidecarStatus`：`path_map`（若存在）、`dedup_stats`、`rebuild_policy`、`stale`、`source_fingerprint`；扩展 index JSON 中 `macro_sidecar` 段。Schema 向后兼容（serde default）。 |
| M1.2 | `src/index/mod.rs` | 新增 `Indexer::map_expanded_path()`：expanded 相对路径 → 源码相对路径（strip expand 目录前缀 / crate 根对齐）。新增 `Indexer::macro_rebuild_if_stale()`：比较主 index 源码指纹（文件 mtime+size hash 聚合或 `meta` 中 `index_fingerprint`）与 sidecar 记录。`index_macro_expanded` 写入 `meta.source_fingerprint` + path map 元数据。 |
| M1.3 | `src/index/store.rs` | 查询层：`callers`/`impact` 在 union 时 **de-dup**（键：`name + resolved/enclosing + mapped_path`，行级策略见 1.4）；sidecar 行在入库或查询时打 `origin` 与 `mapped=true`。新增 `meta` 读取 fingerprint 的辅助。 |
| M1.4 | `src/index/resolve.rs` 或新文件 `src/index/macro_map.rs` | 集中 path-map 与 de-dup 策略，便于测试。**推荐新模块** `macro_map.rs`，由 `index/mod.rs` re-export。 |
| M1.5 | `src/cli.rs` | 新 flag：`index --macro-expanded-root` 保留；新增 `--macro-default`（或配置）控制是否在 sidecar 存在时默认 union；`callers`/`impact` 新增 `--no-macro-dedup`（调试用，默认 **dedup on**）；`macro status` 输出 stale/dedup/path_map；`macro rebuild` 子命令（显式重建）。`--sound && --with-macro` 互斥 **保留**。 |
| M1.6 | `src/mcp/server.rs` | `with_macro` 描述更新：默认行为、de-dup、mapped path；可选 `macro_rebuild` tool 或文档指向 CLI；`macro_status` 返回新字段。 |
| M1.7 | `src/query/mod.rs` | 若查询走此层，对齐 de-dup 参数透传。 |
| M1.8 | `src/viz/mod.rs` | HTML：sidecar 行显示 **mapped 源码路径** + `macro` 徽章；图例注明「候选、已去重（若启用）」；诚实行保持 “not a complete runtime graph”。 |
| M1.9 | `docs/macro-sidecar.md` | 重写「What it is / Non-goals」：加入 path map、de-dup、rebuild、默认策略；删除「永远手写双 root」的隐含前置。 |
| M1.10 | `docs/eval-macro-expand.md` | 增补「Productized path (M1)」小节：与 spike 对照；明确仍非 sound。 |
| M1.11 | `README.md` / `README.zh-CN.md` | **仅当**验收通过后：`--with-macro` 句改为说明去重与 mapped path；默认策略写清；禁止写「宏完整」。 |
| M1.12 | `AGENTS.md` | P2 段更新：path map / de-dup / rebuild / stale 字段；互斥规则不变。 |
| M1.13 | `PLAN.md` | §2.2 能力清单或 L1 附注：宏候选为可选产品路径（非 L2）。 |
| M1.14 | `scripts/expand_index_diff.py` / `stock_macro_expand_spike.ps1` | 保留为 operator spike；可选改为调用产品 path-map 做对照，**不**作为产品依赖。 |

### 1.4 去重语义（必须写进测试）

| 场景 | 期望 |
|---|---|
| sidecar 行 mapped 后与主图 Exact **同 path + 同 name + 同 enclosing** | 只保留主图行；`dedup_stats.merged_exact += 1` |
| sidecar 行仅 name 相同、path 无法 map | 保留 sidecar 行，`origin=macro_expanded`，`mapped=false` |
| sidecar 独有符号（`fmt`/`clone` 等 derive 实现） | 保留为候选；**不得**因噪声过滤而默认丢弃（可用 `--exact-only` 排除 Heuristic） |
| `--exact-only --with-macro` | 可定义：exact-only **忽略** sidecar（推荐，与 exact 语义一致）——测试锁死 |
| 主图 Heuristic 与 sidecar 同逻辑边 | 优先主图 Heuristic（有源码 evidence）；sidecar merge 计数 |
| fingerprint 过期 + `--with-macro` | 默认：**警告 + 仍用旧 sidecar** 或 **拒绝 union**（二选一，推荐警告 + `stale:true`）；`macro rebuild` 可修复 |
| nested expanded root | 保持 R26/R27 hard-reject，不因产品化放宽 |

### 1.5 测试（TDD，先红后绿）

| 测试文件 | 内容 |
|---|---|
| **新** `tests/macro_pathmap.rs` | map 函数表：prefixed shadow、`../` sibling、crate-root 对齐、Windows 盘符。 |
| **新** `tests/macro_dedup.rs` | §1.4 全表；JSON 输出含 `dedup_stats`；`--exact-only` 行为。 |
| **新** `tests/macro_rebuild.rs` | fingerprint 稳定/变更；stale 字段；`macro rebuild` 幂等。 |
| 扩展 `tests/macro_sidecar.rs` | 默认 union 在存在 sidecar 且新策略开启时的 de-dup；absent sidecar 行为不变。 |
| 扩展 `tests/e2e_cli.rs` | CLI flag 矩阵；`--sound && --with-macro` 仍失败。 |
| 扩展 `tests/graph_html.rs` | mapped 路径与徽章。 |
| 扩展 `tests/r26_adversarial.rs` / `r27` | 嵌套/相对 root 产品化后仍 fail-closed。 |

### 1.6 验收

- [ ] 无 sidecar：默认查询 JSON 与现网一致（回归）。
- [ ] 有 sidecar + 去重：同一逻辑调用不出现「源码路径 + expanded 路径」双行（fixture 锁死）。
- [ ] `macro status` 暴露 `stale` / `mapped` / `dedup_stats`。
- [ ] 金标：至少一条「源码 L0 无、expand 后有、map 后可定位到源码 crate」的边（可用 fixture 合成 + 文档指向 stock 对照）。
- [ ] README/AGENTS/macro-sidecar.md 三处能力句一致且无超售。
- [ ] `fmt` / `clippy -D warnings` / `cargo test` / `scip lint` 全绿。

### 1.7 粗估

设计 + path map + de-dup + 测试：**约 1–1.5 周**（不含大规模 stock 重跑）。

---

## Track M2 — L2 承诺档位升级（AST S + over-flag + 适用范围）

### 2.1 目标（落地后的产品句）

- Python / Go 的 S 扫描与边界文档达到与现网 **AST-modeled** 同级诚实度（或明确升为 AST 建模）；
- `promise_tier` 与真实扫描器一致，**不再**出现「文案 ast、实现词法」漂移；
- 已知 over-flag（如 type-only `typeof Function`）改为精确判定；
- `--sound` 在更大**已建模边**集合上仍诚实关闸（违例 → `promise_tier=disabled`）。

**产品边界变化：** *承诺档位前移*（工程 S 门更准、适用边更清晰，**不是**生态 sound）。

### 2.2 非目标

- 不证明 JS/Py/Go 全生态 sound。
- 不把 L1 启发式「证明」成 sound。
- 不验证 tree-sitter / LLVM。
- expand 边不进入本 track 的 sound 集合。

### 2.3 改动清单（文件级）

| # | 文件 | 改动 |
|---|---|---|
| M2.1 | `src/index/subset.rs` | **主战场。** 拆分/扩展扫描器：`scan_js` 保持 AST；`scan_py` / `scan_go` 从词法升级为 tree-sitter AST 违例集（eval/exec、非字面量 getattr/import、reflect/unsafe/plugin…）；`typeof Function` 等 over-flag 改为 **类型位置 vs 值使用** 区分。导出 `SOUND_PROMISE_*` 常量与语言→tier 映射。 |
| M2.2 | `src/index/rules.rs` | 若 M2 需要更多 **sound-eligible** 边（有限域）：扩展 allowlist 规则并 **全部** 进 `subset` 可 walk 集合；每条规则 `rule_id` + evidence。 |
| M2.3 | `src/index/store.rs` | `callers_sound` / `impact_sound`：walk 集合与新 modeled 边对齐；`subset_violations` 分类（language、kind）。 |
| M2.4 | `src/model.rs` | `SubsetViolation` / sound payload：`promise_tier`、`promise_languages`、over-flag 清理后的 violation kinds；serde default 保持旧库可读。 |
| M2.5 | `src/cli.rs` | `subset` 输出字段与文档一致；`--sound` 错误/警告文案引用 tier；不改互斥矩阵。 |
| M2.6 | `src/mcp/server.rs` | `subset` / sound 描述与常量同步；禁止 MCP 文案超售。 |
| M2.7 | `docs/sound-subset.md` | 重写 Promise table：Py/Go 的真实 tier；S 排除列表按 AST 语义写清；over-flag 修复记录。 |
| M2.8 | `docs/eval-l2.md` | 新增差分矩阵：Py/Go AST S fixture + 运行时 tracer 结果；`subset_ok=false` 案例表。 |
| M2.9 | `README.md` / `README.zh-CN.md` | L2 句更新为与 `sound-subset.md` 一致的诚实 tier 描述。 |
| M2.10 | `AGENTS.md` | L2 段：删除「S_py/S_go v1 词法」旧句，改为验收后的真实状态；保留「非生态 sound」。 |
| M2.11 | `PLAN.md` | §4.2/§4.5 状态与交付物勾选对齐；风险表「L2 范围膨胀」保留。 |

### 2.4 S 语言违例集（升级后最低覆盖）

| 语言 | 必须检出（fail S） | 不得误杀（保持 in S） |
|---|---|---|
| Python | `eval`/`exec`/`compile`，`importlib.import_module` 非字面量，`getattr` 非字面量，`__import__` 动态，`ctypes` | 字面量 `getattr(obj,"m")`，`import_module("pkg.mod")` 字面量，已有 FastAPI `Depends` 等 modeled DI |
| Go | `reflect` 调用目标、`unsafe` 函数指针、plugin；**unsafe 包内调用点** 保持 L0 可见但 S 关闸 | 标准 `map[string]Handler` + 字面量路由注册（modeled） |
| JS/TS | 现网已有 + **type-only `typeof Function` 不得单独违例** | 字面量键 computed call、emit/on 字面量、Nest allowlist |

### 2.5 测试

| 测试文件 | 内容 |
|---|---|
| 扩展 `tests/l2_lang_subset.rs` | Py/Go AST 违例与允许表（表驱动）。 |
| 扩展 `tests/l2_promise_lang.rs` | `promise_tier` 随 corpus 语言组合变化；无 lexical_v1 谎报。 |
| 扩展 `tests/l2_property.rs` | S 内生成程序：runtime ⊆ sound 边（Py/Go 生成器若缺则补最小集）。 |
| 扩展 `tests/l2_go_diff.rs` | Go cover 差分在 AST S 下仍绿。 |
| **新** `tests/l2_py_diff.rs` | Python coverage/trace 差分（可用 subprocess + 简单 tracer）。 |
| 扩展 `tests/l2_sound.rs` / `l2_esm_diff.rs` | over-flag 修复前后：type-only Function 场景 `subset_ok` 期望。 |
| 扩展 `tests/e2e_cli.rs` | `subset` JSON 字段稳定。 |

### 2.6 验收

- [ ] Py/Go 不再依赖「词法 v1」话术，除非代码仍词法——**文实一致**。
- [ ] S_js fixture + 新 Py/Go fixture：运行时边 ⊆ `--sound` 边 **100%**（S 内）。
- [ ] over-flag 修复后，「无实质动态」的干净 fixture `subset_ok=true`。
- [ ] 含真实 `eval`/`reflect` 的 fixture `subset_ok=false` 且 `promise_tier=disabled`。
- [ ] README 引用 eval 数字；无「生态 sound」。
- [ ] 全门禁绿。

### 2.7 粗估

Py/Go AST S + 差分 + 文档：**约 2–4 周**（视 tracer 基建是否复用）。

---

## Track M3 — L1 默认召回扩展（规则进默认路径 + 评测数字）

### 3.1 目标（落地后的产品句）

- 更多框架/语言模式成为**默认** Heuristic/Dynamic 候选（可 `--exact-only` 关掉）；
- 每条新规则有 `rule_id` + evidence + eval 噪声比；
- 默认 `callers`/`impact` 相对 L0 在指定 corpus 上召回提升可引用。

**产品边界变化：** *默认路径前移*（默认图内容）。

### 3.2 非目标

- 不声称 sound。
- 不做完整指针分析。
- 不用 expand 原始边「代替」可解释 L1 规则（expand 属 M1 可选候选）。

### 3.3 候选规则包（按优先级）

| 包 | 语言 | 模式 | 建议 confidence | 主要文件 |
|---|---|---|---|---|
| M3-A | Rust | `dyn Trait` + impl 集合方法调用候选（限定已索引 impl） | Heuristic | `src/index/rules.rs` |
| M3-B | Go | 接口实现闭包（`var _ I = (*T)(nil)` / 方法集匹配）系统化 | Heuristic | `rules.rs` |
| M3-C | Python | `__init_subclass__` / entry-points 形态补强；FastAPI 依赖边完善 | Heuristic | `rules.rs` |
| M3-D | TS | 计算属性/工厂已有基础上补常见 router.register 等 | Heuristic / Dynamic | `rules.rs` |
| M3-E | Rust | `inventory`/`linkme` 源码规则核对缺口（**不以 sidecar 替代**） | Heuristic | `rules.rs` |

### 3.4 改动清单（文件级）

| # | 文件 | 改动 |
|---|---|---|
| M3.1 | `src/index/rules.rs` | 按包 TDD 实现；模块内按语言分区（现有结构延伸）；禁止无 evidence 入库。 |
| M3.2 | `src/index/extract.rs` | 仅当 AST 遍历缺节点时最小改动（Track 边界：与 unsafe extract 等现有所有权不冲突时再动）。 |
| M3.3 | `src/index/store.rs` | 通常无需改 schema（`confidence`/`evidence` 已有）；核对 `impact` BFS 对新 Heuristic 的 expandable 行为。 |
| M3.4 | `src/model.rs` | 若需新 `rule_id` 常量集中处，保持与 rules 一致。 |
| M3.5 | `fixtures/eval-l1/` + 可能 `fixtures/eval-l1-real/` | 每包最小多模块 fixture + 期望边。 |
| M3.6 | `docs/eval-l1.md` / `docs/eval-l1-real.md` | 每包：召回提升 %、噪声抽检、失败模式。 |
| M3.7 | `README.md` / `README.zh-CN.md` | Features 中 L1 一句 + eval 链接；仍写 “candidates, not sound”。 |
| M3.8 | `AGENTS.md` | L1 shipped 列表补充新 rule_id。 |
| M3.9 | `PLAN.md` | §3.3 表与 §3.6 交付物状态。 |

### 3.5 测试

| 测试文件 | 内容 |
|---|---|
| 扩展 `tests/l1_rules.rs` / `l1_rules_gaps.rs` / `l1_rules_inventory.rs` | 每 rule_id 先红后绿。 |
| 扩展 `tests/l1_eval.rs` / `l1_eval_real.rs` | 阈值：相对 L0 召回 ≥15%（PLAN §10）；噪声报告进 docs。 |
| 扩展 `tests/l1_cli.rs` / `l1_schema.rs` | 默认含新 Heuristic；`--exact-only` 排除。 |
| 扩展 `tests/store_impact.rs` | 新边进入 impact BFS 的深度行为。 |

### 3.6 验收

- [ ] 每包规则：fixture 金边命中；evidence 非空。
- [ ] 至少一份真实向 corpus 报告数字（无源码入库则边期望入库）。
- [ ] 默认查询 JSON 变化可解释（confidence 直方图）。
- [ ] README 无超售。
- [ ] 全门禁绿。

### 3.7 粗估

每包 **约 3–7 天**；建议 M3-A/B 先做，与量化仓场景相关度高。

---

## Track M4 — 查询 API / 图语义产品能力

### 4.1 目标（落地后的产品句）

| 能力 | CLI/MCP 形态（建议） | Agent 用途 |
|---|---|---|
| 时序边 diff | `agentgraph diff --since <snapshot\|time>` 或 `graph-diff` | watch 后「谁新依赖了 X」 |
| 多根 workspace | `agentgraph index --workspace <file\|多 root>` | monorepo 一次图 |
| watch 后 S 重认证 | `index_paths` 自动刷新 `subset` / `promise` | 避免陈旧 `--sound` |
| HTML 上的 L2 | `agentgraph graph --sound` | 可视化 S 限定邻域 |

**产品边界变化：** *API 面前移*。

### 4.2 非目标

- 不做跨仓远程服务端。
- 不做「实时浏览器内图」。
- diff 不声称语义等价，只声称**已索引边集合差**。

### 4.3 改动清单（文件级）

| # | 文件 | 改动 |
|---|---|---|
| M4.1 | `src/index/store.rs` | 快照元数据：`meta.index_seq` / `indexed_at`；边变更辅助查询或离线 diff 比较（symbols/refs 按 path+name+confidence）。可选 `refs` 增加 `index_seq`（注意迁移成本——**优先双库/双 meta 比较，避免大迁移**）。 |
| M4.2 | `src/index/mod.rs` | `index_paths` 结束后触发 **增量 S 刷新**（重扫脏文件违例 + 汇总 promise）；`workspace` 索引进度与 root 列表 meta。 |
| M4.3 | **新** `src/index/diff.rs` | 实现 snapshot diff：added/removed symbols & refs，按 confidence 过滤；输出结构化 JSON。 |
| M4.4 | `src/index/subset.rs` | 提供「单文件/路径集合」违例重算 API（供 M4.2 调用），避免全库重扫。 |
| M4.5 | `src/cli.rs` | 新子命令 `diff`；`graph --sound` / `--with-macro` 组合规则（sound 仍与 with_macro 互斥）；`index --workspace`（若做多 root）。 |
| M4.6 | `src/mcp/server.rs` | tools：`graph_diff`、`graph`（可选 sound）；schema 与 honesty 描述。 |
| M4.7 | `src/viz/mod.rs` | sound walk 邻域渲染（仅 sound-eligible 边）；页脚标明 `subset_ok` 与 tier；违例时 UI 不显示「sound 图」。 |
| M4.8 | `src/query/mod.rs` | 若查询层需 diff 透传，对齐。 |
| M4.9 | `docs/graph-html.md` | 更新：L2 可视化条件与限制。 |
| M4.10 | **新** `docs/graph-diff.md`（或并入 README） | 语义：差的是索引边，不是运行时；watch 工作流示例。 |
| M4.11 | `docs/sound-subset.md` / `eval-query-p95.md` | S 重认证成本与 p95 注意事项；workspace 索引预算。 |
| M4.12 | `README.md` / `README.zh-CN.md` | Commands 表增加 `diff` / `graph --sound`（验收后）。 |
| M4.13 | `AGENTS.md` / `PLAN.md` | 能力清单与场景对齐（爆炸半径 + 及时性）。 |

### 4.4 接口语义（须锁测试）

| 项 | 语义 |
|---|---|
| `diff` 默认 | Exact+Heuristic 的 added/removed；`--exact-only` 仅 Exact |
| 无旧快照 | fail-loud，提示先 `index` 或提供 snapshot 路径 |
| watch 后 `--sound` | `promise` 反映**当前磁盘** S 状态，不是上次 full index |
| `graph --sound` | 仅 sound 边；`subset_ok=false` 时 exit 非 0 + 仍写诚实空/警告页（与现网空图策略对齐） |
| workspace | 每个 root 独立 `.agentgraph` 或单一库内 `root_id` 字段——**实现时二选一写进文档**（推荐单一库 + `root_id`，避免 Agent 开多连接） |

### 4.5 测试

| 测试文件 | 内容 |
|---|---|
| **新** `tests/graph_diff.rs` | 删改符号后 diff 集合；无快照错误。 |
| 扩展 `tests/watch_fsnotify.rs` | 改文件后 subset/promise 刷新。 |
| 扩展 `tests/graph_html.rs` | `--sound` 页边集合与违例 UX。 |
| **新** `tests/workspace_index.rs` | 多 root 符号隔离/查询（若实现 workspace）。 |
| 扩展 `tests/e2e_cli.rs` / MCP 测试 | 新 tool/flag。 |
| 扩展 `tests/query_p95.rs` | diff 与 sound-重认证的延迟预算记录。 |

### 4.6 验收

- [x] Agent 可用 CLI/MCP 回答：「索引层面，改文件后哪些调用边新增？」（`agentgraph diff` / MCP `graph_diff`；honesty: indexed edges only）
- [x] watch 后 `--sound` 不使用陈旧 S。（`index_paths` + `refresh_subset_for_paths`；`tests/s_recert_watch.rs`）
- [x] HTML `--sound` 在违例时**不**展示为合格 sound 图。（仍写 HTML；`subset_ok=false` 标记 disabled；exit 非 0；`tests/graph_html.rs`）
- [x] 文档与实现一致；全门禁绿。（`docs/graph-diff.md`、`graph-html.md`、`sound-subset.md`；workspace 多 root 记为 backlog）

**M4 落地备注：** workspace `index --workspace` 未做，backlog 见 `docs/graph-diff.md`；推荐将来单库 + `root_id`。

### 4.7 粗估

diff + S 重认证：**约 1 周**；workspace 与 HTML L2 再 **+3–5 天**。

---

## Track M5 — 评测与发版门禁产品化（可信度边界）

### 5.1 目标（落地后的产品句）

- 发版时自动核对：**README/AGENTS 能力句 ⊆ 实现与 eval**；
- 公开 harness 可复现关键数字（stock 私有源码除外）；
- expand/sound 的诚实字段（`promise_tier`、`origin`、`stale`）有回归锁。

**产品边界变化：** *可信度门前移*。

### 5.2 非目标

- 不把私有 stock 源码开源。
- 不在 required CI 跑 `cargo expand`。
- 不把人工对抗轮取消（可并行）。

### 5.3 改动清单（文件级）

| # | 文件 | 改动 |
|---|---|---|
| M5.1 | **新** `scripts/check_docs_claims.py`（或 `.ps1` + rust 断言） | 解析 README/AGENTS/PLAN 中能力关键词（sound、默认、macro、promise）；与白名单/实现标志对照；失败非 0。 |
| M5.2 | **新** `tests/docs_claims.rs` | 把 M5.1 检查纳入 `cargo test`（或 CI 独立 job 调用脚本）。 |
| M5.3 | `tests/e2e_cli.rs` | 扩展：能力句涉及的 flag 均存在；默认 JSON 形状锁（含新 meta 字段 optional）。 |
| M5.4 | **新** `fixtures/eval-goldens/` | 可公开的最小 edge goldens（合成 crate/模块），供 M1/M2/M3 回归。 |
| M5.5 | `docs/eval-*.md` | 统一头部：Status / 非声称 / 复现命令。 |
| M5.6 | `AGENTS.md` | 发版清单增加：docs claim check、eval 链接有效性。 |
| M5.7 | CI 配置（`.github/workflows/ci.yml` 等） | job：`cargo fmt/clippy/test` + `scip lint` + docs claim check；**无** expand/nightly 必选。 |
| M5.8 | `PLAN.md` §9 风险「文档打脸」 | 指向 M5 机制为已落地缓解。 |

### 5.4 测试 / 门禁

- [ ] 故意在 README 写一句超售 → claim check **红**。
- [ ] 删除实现中的 flag 但 README 仍写 → **红**。
- [ ] 正常主分支 → **绿**。
- [ ] 私有 eval 路径在 CI 中被跳过而非报错。

### 5.5 粗估

**约 2–4 天**，建议最先落地以便其它 track 改 README 时被门禁挡住超售。

---

## 总表：文件 → 涉及 Track

| 文件 | Tracks |
|---|---|
| `src/model.rs` | M1, M2, M3(可选), M4 |
| `src/index/mod.rs` | M1, M4 |
| `src/index/store.rs` | M1, M2, M3, M4 |
| `src/index/rules.rs` | M2(allowlist), M3 |
| `src/index/subset.rs` | M2, M4 |
| `src/index/macro_map.rs` **新** | M1 |
| `src/index/diff.rs` **新** | M4 |
| `src/index/extract.rs` | M3（最小） |
| `src/index/resolve.rs` | M1（可选） |
| `src/cli.rs` | M1, M2, M4 |
| `src/mcp/server.rs` | M1, M2, M4 |
| `src/query/mod.rs` | M1, M4 |
| `src/viz/mod.rs` | M1, M4 |
| `docs/macro-sidecar.md` | M1 |
| `docs/sound-subset.md` | M2, M4 |
| `docs/eval-macro-expand.md` | M1 |
| `docs/eval-l1.md` / `eval-l1-real.md` | M3 |
| `docs/eval-l2.md` | M2 |
| `docs/graph-html.md` | M4 |
| `docs/graph-diff.md` **新** | M4 |
| `README.md` / `README.zh-CN.md` | M1–M4（验收后）, M5 检查 |
| `AGENTS.md` / `PLAN.md` | 全部 |
| `tests/macro_*.rs` **新/扩展** | M1 |
| `tests/l2_*.rs` | M2 |
| `tests/l1_*.rs` | M3 |
| `tests/graph_diff.rs` **新**, `watch_fsnotify.rs`, `graph_html.rs` | M4 |
| `tests/docs_claims.rs` **新**, CI | M5 |

---

## 明确「不做 / 不算产品边界」清单

| 动作 | 分类 |
|---|---|
| 给更多 stock crate 手工 expand + 双 root 索引 | 量化仓运维覆盖 |
| 只加 scripts/影子树，不改默认查询与文档承诺 | 不算 |
| 把 expand 边写进 `--sound` 且无 subset 证明 | **禁止**（超售） |
| required CI 安装 cargo-expand / nightly expand | **禁止**（非 hermetic） |
| 提交 stock 源码或 expand 产物进 git | **禁止** |
| 仅在私有仓加金标、README 无对应 eval 链接 | 可信度不足 |
| 声称「宏完整」「动态零漏报」 | **禁止** |

---

## 发版切片建议

| 版本标签（示意） | 内容 | 边界类型 |
|---|---|---|
| `v0.x+M5` | docs claim check + goldens 目录 | 可信度 |
| `v0.y+M1` | path map + de-dup + rebuild + 文档 | 默认路径（宏候选） |
| `v0.z+M2` | Py/Go S 升档 + over-flag 修复 + eval-l2 | 承诺档位 |
| `v0.z+M3` | 新 L1 规则包 + eval-l1 数字 | 默认路径（召回） |
| `v0.w+M4` | diff + S 重认证 + HTML L2 (+ workspace) | API 面 |

---

## 附录 A — M1 实现草图（非最终代码）

```text
index --force
index --force --macro-expanded-root <sibling>
  → validate (R26/R27)
  → index main
  → index expanded → sidecar
  → write meta: origin, expanded_root, source_fingerprint, path_map

callers/impact [--with-macro]
  → main rows
  → if sidecar exists:
       if fingerprint mismatch → warn + stale
       map paths
       union
       dedup (default on)
       tag origin=mapped|macro_expanded

macro status
  → exists, path, counts, stale, nested, subset_violation_count, dedup_stats
```

## 附录 B — 文档发布检查单（每 track PR）

1. 实现与 `docs/` 同一 PR 或明确 follow-up issue 链接。  
2. README 能力句 ≤ eval 可证范围。  
3. 新 flag：中英文 README + MCP description + AGENTS 一句话。  
4. 默认行为变更：e2e 断言 + 变更说明（Breaking 则 major/minor 按项目习惯）。  
5. `cargo test` 全绿；不引入新 clippy warning。  

## 附录 C — 与量化仓（stock-trading-app）的关系

| 事项 | 产品仓动作 | 私有仓动作 |
|---|---|---|
| M1 path map/de-dup | 实现 + 合成 fixture | 可选：真 expand 对照，不提交源码 |
| M2 S 升档 | 合成语料进 fixtures | 可选跑 subset 对照 |
| M3 L1 规则 | fixtures + eval harness | 可选金标边（期望文件可进产品仓） |
| M4 diff/workspace | 产品测试 | 不阻塞 |
| 任何 track | **不依赖**私有仓 CI | 不进 agentgraph required CI |

---

## 附录 D — M1–M3 落地核验（2026-09-18）

**门禁抽检：** 相关测试全绿（`macro_pathmap/dedup/rebuild/sidecar`、`l2_lang_subset`、`l2_promise_lang`、`l2_py_diff`、`l2_property`、`l2_go_diff`、`l1_rules_m3`、`l1_eval`、`l1_cli`、`docs_claims`、`e2e_cli`、`graph_html`）。CI 已跑 `scripts/check_docs_claims.py`（M5 骨架提前落地）。

### 已落地（与清单对齐）

| Track | 关键交付 |
|---|---|
| **M1** | `src/index/macro_map.rs`；de-dup 默认 ON + `--no-macro-dedup`；`--exact-only --with-macro` 忽略 sidecar；`meta.source_fingerprint` / `stale` / `path_map` / `dedup_stats` / `rebuild_policy`；CLI `macro rebuild` + MCP `macro_rebuild`；HTML MACRO 徽章 + mapped path；`docs/macro-sidecar.md` + `eval-macro-expand.md` §8 + README en/zh + AGENTS |
| **M2** | `scan_py`/`scan_go` tree-sitter AST；全语言 `promise_tier=ast_modeled`；type-only `typeof Function` 保持 in S；`l2_py_diff` + `scripts/py_trace.py`；`l2_property` 覆盖 S_py/S_go；`sound-subset`/`eval-l2`/AGENTS/PLAN L2 文实一致 |
| **M3** | `rs.di.dyn_trait_method`(A)、`go.di.interface_impl_v2`(B)、`py.di.entry_points`+Security(C)、`ts.framework.register`(D)、`rs.di.linkme_distributed_slice`(E)；`tests/l1_rules_m3.rs`；`fixtures/eval-l1` 多数包 + golden；`eval-l1.md` 规则表与 sound allowlist（dyn_trait/entry_points **不** allowlist） |

### 残差 / 缺漏（按严重度）

> **2026-09-18 close-out (G1–G9):** residual gaps below are closed in-repo. Product Tracks M1–M5 naming disambiguated in PLAN.md. Full test suite + `fmt`/`clippy`/`docs_claims` green after close-out. G10/G11/G12 remain intentional low-priority notes.

| ID | Track | 缺漏 | 严重度 | 建议动作 | Status |
|---|---|---|---|---|---|
| G1 | M3 | **M3-E linkme 无 eval-l1 fixture/golden**（仅 unit 测试） | 中 | 补 `fixtures/eval-l1/rust-linkme/` + golden.json 条目 | **closed** — fixture + golden + eval-l1.md table |
| G2 | M3 | **eval-l1-real 无 M3 形状** | 中 | 在 `fixtures/eval-l1-real/` 加 multi-module 形状 | **closed** — dyn-trait / go-iface-v2 / ts-express-router / py-entry-points / rust-linkme-plugins + honest L0/L1 numbers in eval-l1.md |
| G3 | M3 | `tests/l1_cli.rs` / `store_impact.rs` **未**为新 Heuristic 扩默认含边 / `--exact-only` / impact BFS 用例 | 中 | 各加表驱动用例（rule_id 级） | **closed** — CLI default/exact-only rule_id matrix + impact BFS through M3 Heuristics |
| G4 | M1 | 验收金标「L0 无 → expand 有 → **map 后 crates/… 源码路径**」未做成端到端测试 | 中 | 在 `macro_dedup` 加 crates 目录布局 fixture | **closed** — `macro_dedup::e2e_golden_l0_miss_expand_finds_mapped_source_crate_path` |
| G5 | M2+M5 | `fixtures/eval-goldens/` 仅 nest + inventory | 中 | 扩 golden.json + mini 目录 | **closed** — rust-dyn-mini / go-iface-mini / ts-router-mini / ts-typeonly-function / py-overflag + `expected_in_s` + docs_claims lock |
| G6 | 全 | **README 未写 M2** `promise_tier=ast_modeled` / typeof Function 诚实句 | 中 | README L2 段对齐 sound-subset | **closed** — README.md + README.zh-CN.md |
| G7 | 文档 | `eval-stock-boundary.md` 仍写 **「path map not automatic」** | 中 | 改为 M1 product path | **closed** — spike history vs M1 product path split |
| G8 | 文档 | `docs/graph-html.md` 未记录 M1 mapped path / MACRO 徽章 | 低 | 补一小节 + test assert | **closed** — docs section + `graph_macro_badge_and_sound_mutex_contract` |
| G9 | 文档 | **PLAN.md 里程碑 M1–M5 与产品 Track M1–M5 同名冲突**；§3.3 未列 shipped M3 rule_id | 中 | PLAN 术语区分 + rule 列表 | **closed** — PLAN naming note + §3.3 shipped table + AGENTS L1 blurb |
| G10 | M1 | 清单中的 `--macro-default` **未实现**（`--with-macro` 仍默认 OFF，符合「默认源码 L0/L1」） | 低（有意） | 维持默认 OFF；标为 **explicit opt-in only** | intentional (unchanged) |
| G11 | M1 | `scripts/expand_index_diff.py` 未接产品 path-map（清单标可选） | 低 | 保持 operator spike | intentional (unchanged) |
| G12 | 清单 | 本文 §1.6/§2.6/§3.6 验收复选框尚未勾选；实现状态以本附录为准 | 低 | 残差关闭后统一勾选 | backlog (appendix is source of truth) |

### 有意不做成「缺漏」的产品选择

1. **宏旁路默认仍 OFF**：没有 `--macro-default` 自动 union —— 与全局原则 1 一致；产品句是「可选候选 + 映射去重」，不是默认宏完整图。  
2. **origin 保持 `macro_expanded`**：清单允许 `macro_candidate`；实现选前者以免破坏已有消费者。  
3. **`--sound` ∩ `--with-macro` 互斥**：M1 明确非目标，已用测试锁死。  
4. **M3-A dyn / M3-C entry_points 不进 sound allowlist**：开放域，已写入 `sound-subset.md`。

### 残差关闭顺序（建议）

1. G7 + G6 + G9（文档文实，半天级，避免再超售/再混淆）  
2. G1 + G5（公开金标补齐，支撑后续回归）  
3. G4 + G3（测试钉死产品语义）  
4. G2（真实向 eval，可与 stock operator 对照并行）  
5. G10/G11/G12 收尾  

---

*文档所有权：与 PLAN.md 相同——仓库维护者。变更本清单时，同步核对 README 是否超售。*
