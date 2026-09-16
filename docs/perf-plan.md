# agentgraph 性能优化方案（修订版）

> 状态：**待实施**。基线来自私有仓 `stock-trading-app`（~995 源文件 / 20 924 symbols / 167 600 refs）。  
> 本文取代口头/聊天中的零散优化建议；实现须 **TDD + 先测量后改代码**。  
> 主代理审核结论已并入：测量前置、P1-3 降级、export↔sid 一致性、mtime 失败模式。

---

## 0. 问题与预算

| 场景 | 当前（release，本机） | 目标 |
|---|---:|---:|
| Full `index --force`（~1k 文件） | ~39 s | 保持可接受（软目标 < 20 s，**不承诺 2s**） |
| **Incremental noop**（内容零变更） | **~21 s** | **< 2 s**（stretch < 500 ms） |
| Incremental 1 文件变更 | ~21 s 量级 | **< 2 s**（parse 1 + 局部 SQL） |
| `callers` / `impact` 查询 | 未在 5k 文件钉死 p95 | PLAN：5k 文件 p95 < 50 ms（P2） |

**根因（已读码确认，非猜测）：**

1. Phase 1 对**每个**文件串行 `fs::read` + SHA256，再比 hash（`src/index/mod.rs` ~81–126）。  
2. 无论是否脏，`prune_missing` + **全表** `resolve_symbol_ids`（先 `UPDATE refs SET resolved_symbol_id=NULL`）+ `resolve_qualifiers` 必跑（`mod.rs` ~186–190；`store.rs` ~646+）。  
3. Phase 2（tree-sitter + L1 rules）已 rayon；**不是** noop 21s 的主因。

`callers_uncached` / `impact_uncached` 按 **`name` / `qualifier`** 查询，**不读** `resolved_symbol_id`（`store.rs` ~614–627）。L0 查询 SLO 与 sid 解耦——这是安全砍 sid 的前提。

---

## 1. 实施原则

1. **先 phase timer，后优化**；每项 P0 用数字验收。  
2. **内容 hash 仍是真相**；mtime/size 只是短路。  
3. **单文件失败不得污染 batch**（保留 per-file savepoint 语义）。  
4. **`export scip` / scip-json 前**若 sid 可能过期 → 强制全量 relink。  
5. 不换存储引擎；不把 LLM/enrich 塞进 index 热路径。  
6. 正确性门禁：`sid_incremental` / `qualifier_propagate` / `query_cache` / `watch_fsnotify` / `l1_*` / `l2_*` / e2e 全绿。

---

## 2. 步骤 0 — 测量层（P0，必须先做）

### 0.1 Phase timers

- 环境变量 `AGENTGRAPH_TRACE=1` 时，index 各阶段向 **stderr** 打一行 JSON：  
  `walk_ms, stat_hash_ms, parse_extract_ms, db_replace_ms, prune_ms, resolve_sids_ms, resolve_qualifiers_ms, stats_ms, files, dirty, skipped`
- 不用 tracing 框架也可：`Instant` + `eprintln!` 即可（避免新依赖）。

### 0.2 Bench 脚本

扩展 `scripts/bench_index.ps1`：

| 参数 | 含义 |
|---|---|
| `-Root` | 任意树（含 `stock-trading-app` 或生成的 1k fixture） |
| 输出 | full / noop / 1-file 三档墙钟 +（若有）TRACE 分项 |

**生成 fixture：** `scripts/gen_fixture.ps1 -N 1000`（小 TS/RS 文件，无嵌套依赖亦可）。

### 0.3 基线任务

在真仓上记录 noop / 1-file 的 phase 分项，写入 `docs/eval-large-repo.md` 附录。  
**验收：** 能指出 21s 中 `read_hash` vs `resolve_*` 占比。

---

## 3. P0 — 达成 noop / 小脏集 < 2s

### P0-1 mtime + size 短路（读盘与 hash）

**现状：** 即使 hash 未变仍读全文并 SHA256。

**改动：**

1. `files` 增加 `mtime_ns INTEGER`、`size INTEGER`（`ALTER` 迁移；旧行 NULL → 下次强制读+hash 回填）。  
2. Walker/Phase1 一次 `metadata`：`len()` + `modified()` → ns。  
3. `!force` 且 DB 中 `(mtime_ns, size)` 均相等 → **跳过 open/hash**，计 `skipped`。  
4. 任一不等、无行、或 `--force` → 现行路径：read + SHA256，以 hash 为准。

**失败模式（必须测）：**

| 场景 | 期望 |
|---|---|
| 仅 touch mtime、内容不变 | rehash，hash 同 → skip parse，更新 mtime |
| 内容变、mtime 被工具改回旧值 | **漏检风险**；文档要求可疑时 `--force`；watch 收到 `Modify` 对该路径强制 hash |
| 旧库无 mtime 列 | 首次全量 hash 回填 |

**风险：** 中（错误「未变」）。缓解：hash 仍是权威；不拿 mtime 替代 hash 写入。

**测试（TDD）：**

- 计数器/TRACE：mtime+size 命中时 `read_bytes=0`。  
- 改内容 + 恢复 mtime（若可行）→ 必须检出变更。  
- 迁移：打开旧 `index.db` 不炸，下一轮回填。

**预期：** noop 从「全树 IO+hash」→ 几乎仅 `stat`；与 P0-3 叠加后通常 **>>5×**。

---

### P0-2 Phase 1 并行 read+hash

**现状：** Phase1 串行 `for`；Phase2 已 `par_iter`。

**改动：** 对「需要 hash 的候选」用 rayon 分块（64–128）并行；**跳过**的文件不进内存。  
`--force` / 首次迁移受益最大。

**风险：** 峰值内存——分块即可。Windows 杀软可能吃并行收益——以 TRACE 为准。

**测试：** 1k fixture 上 Phase1 墙钟 ≥2× 相对串行（多核）；结果集合与串行一致。

---

### P0-3 脏集 early-out

**现状：** `to_parse` 为空仍 `prune` + 全量 sid/qualifier + stats。

**改动：**

```text
dirty = to_parse 路径集
deleted = DB paths − walker paths
if !force && dirty.is_empty() && deleted.is_empty() && oversized_only_unchanged:
    跳过 begin_batch / prune / resolve_*；
    仍更新 stats（或复用缓存 stats）并正确累计 skipped / failed / oversized
```

- 仅 `deleted` 非空：prune + **悬挂 sid 清理**（见 P0-4），不做全表 relink。  
- `failed_read` / parse fail **不能**被当成「干净」而静默；计数进 stats。

**风险：** 低；条件漏算 deleted 会留幽灵文件——必须用 DB path 集合差集。

**测试：**

- noop：`callers`/`impact`/`related` 与强制全量在同一内容上 **排序后 JSON 一致**。  
- TRACE：noop 时 `resolve_sids_ms ≈ 0`。  
- 预算：1k SSD noop < 2000 ms。

---

### P0-4 增量 / 延迟 `resolve_symbol_ids`

**现状：** 每次全表 NULL + 全表重链（~167k refs）。

**改动（分层）：**

1. **脏路径 D 上的 replace 前：** 记下 D 内旧 `symbols.id`。  
2. **commit 后：**  
   - `resolved_symbol_id = NULL` where `path ∈ D` **或** `resolved_symbol_id ∈ old_ids`  
   - 为 `resolved_symbol_id` 建索引（若无）  
   - **仅**对这些 NULL 行重链（复用现有 lookup SQL）  
3. **`export scip` / `scip-json`：** 若 `sid_dirty` 标志为真 → 先 `resolve_symbol_ids()` 全量，再导出。  
4. `--force` 保持全量 relink。  
5. 可选 name-delta：D 中新出现/消失的符号名，对引用这些名的 refs 补链（防「新符号无入边 sid」）。

**风险：** 中高。sid 与 SCIP/未来消费者相关；**不得**在 dirty 时导出过期 sid。

**测试：**

- 1 文件变更后：无关符号的 `callers` 不变。  
- SQL 反悬挂：`resolved_symbol_id` 不指向缺失 `symbols.id`。  
- dirty 后 export 前自动全量 relink（单测 mock 或集成）。  
- 1-file 增量墙钟 < 2s。

---

### P0-5 批量化 / 跳过 `resolve_qualifiers`

**现状：** 对每条带 qualifier 的 call ref 单独 `COUNT(*)` 探类型（`store.rs` ~764+）。

**改动：**

1. 一次 `SELECT DISTINCT name FROM symbols WHERE kind IN (class,struct,interface,enum,trait)` → `HashSet`。  
2. 在内存判断，或改写为集合式 SQL UPDATE。  
3. 脏集未改任何 `define` ref 且未改带 `return_type` 的符号 → **整段 skip**。

**风险：** 低（谓词与 COUNT 等价）。

**测试：** 与现有 `qualifier_propagate` / `type_aware` 同结果；TRACE 显示该阶段大幅下降。

---

## 4. P1 — Watch 与全量路径（第二波）

### P1-1 Watch 路径级 reindex

- debounce 窗口内收集路径；只 index 这些路径（Remove 并入 prune）。  
- 路径数 > N 或未知 → 退回全量 `index(false)`。  
- 保留周期性 fingerprint 安全网。  
- **验收：** 改 1 文件，watch 触发的 reindex 在 1k 树上 < 500 ms（P0 之后）。

### P1-2 `replace_file_with_subset` SQL 仪式

- `prepare_cached` + `executemany` 插入 symbols/refs。  
- 不削弱 savepoint 隔离（坏文件仍不能毒 batch）。  
- **验收：** `--force` DB 阶段 1.5–3×；故障注入测试仍绿。

### P1-3 并行 walk / 单次收集元数据

- `WalkBuilder::build_parallel`；`(path, rel, mtime, size)` 收集一次供 Phase1 与 `known` 复用。  
- gitignore 语义与串行集合相等。

### P1-4 Import 解析索引

- Go：目录 → files；Rust：mod 路径映射，避免 O(known_files) 线性扫（`resolve.rs`）。  
- **验收：** `resolved` 列与现实现一致（resolve_* 测试）。

### P1-5 `prune_missing` 集合 SQL

- 批量 DELETE，保留 FK cascade。

---

## 5. P2 — 查询 SLO 与 CPU 打磨

| ID | 项 | 说明 |
|---|---|---|
| **P2-1** | 限定名查询索引 | 物化 `qual_name`（`qualifier\|\|'.'\|\|name`）+ INDEX；impact 层缓存 `file_language`；replace 后 **按 path 失效** 缓存而非清空全部 |
| **P2-2** | 更快 change hash | blake3/xxhash；迁移强制一次 rehash |
| **P2-3** | 缓冲/mmap 读 | 次要；先测杀软 |
| **P2-4** | stats 增量计数 | watch 日志路径可跳过全 COUNT |
| **P2-5** | `col_utf16` 延迟计算 | 仅 export 需要时 |
| **P2-6** | `var_types` CoW / delta 栈 | 少 clone |
| **P2-7** | **单次 AST walk**（原 P1-3 降级） | 融合 L0+L1；**对 21s noop 无直接贡献**，服务 full index CPU |

---

## 6. 推荐实施顺序

```text
0  TRACE + gen_fixture + 真仓基线数字
1  P0-1 mtime/size 短路
2  P0-3 early-out（含 failed/oversized 语义）
3  P0-5 qualifier 批量化
4  P0-4 增量 sid + export 强制全量
5  P0-2 并行 hash
6  回归：bench 1k noop / 1-file；更新 docs/eval-large-repo.md
7  P1-1 watch 路径级
8  P1-2…5、P2-*
```

每步：失败测试 → 实现 → `fmt`/`clippy`/`test` → bench 数字 → 再下一步。

---

## 7. 验收清单

- [x] P1-1 watch **路径级** `index_paths`（≤64 路径，否则全量）  
- [x] P1 sid **内存批量 relink + 事务内 point UPDATE**（force sid 27s→**3.4s**）  
- [x] P2-1 `refs.qual_name` + 索引；qualified callers 走索引列  
- [x] L2 `emit('evt')` 有限域 DynamicCandidate（`ts.event.emit`）  
- [x] 公开 NestJS starter 冒烟索引  
- [ ] 5k 真树 p95 钉死（有合成 200 文件软预算测试 `perf_p2_query`）  
- [ ] Lean 定理（`formal/TODO.md`，明确不在本计划）

**P0 实现摘要：** mtime/size 短路、并行 hash、脏集 early-out、增量 sid、export 前 `ensure_sids_for_export`、qualifier 类型名 HashSet。

---

## 8. 明确不做

- 仅靠 mtime 弃 content hash  
- 去掉 per-file savepoint 而无替代隔离  
- 让 `callers`/`impact` 依赖 sid 再优化 sid  
- 只并行 extract 却不动串行全树 hash  
- 主 CI 强制 criterion/tracy  
- 宣称 full index 39s → 2s  

---

## 9. 与 PLAN 的关系

- 不改变 L0–L3 能力承诺；本节是 **非功能（性能）** 跟踪。  
- 完成 P0 后更新 PLAN §6「性能」行与 `docs/eval-large-repo.md`。  
- 未完成前 **不得**在 README 写「1k 文件增量 <2s」。
