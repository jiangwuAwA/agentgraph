# 产品改善任务列表（源自 PM 复审 · v0.5.1+）

> **状态图例：** open / in_progress / blocked / done  
> **原则：** 先证据与接通，再加分析特性；不超售 sound；全局 `--with-macro` 仍默认 OFF。  
> **关联：** [product-boundary-migration.md](product-boundary-migration.md)、[agent-recipes.md](agent-recipes.md)、[noise-governance.md](noise-governance.md)、[workspace.md](workspace.md)、[eval-agent-baseline.md](eval-agent-baseline.md)、[eval-package-map.md](eval-package-map.md)

---

## 产品边界前移的验收定义

| 前移类型 | 含义 | 不算前移 |
|---|---|---|
| **默认路径** | 不加开关时 Agent 能得到的结构事实变多/更准 | 只增加可选 flag 的运维用法 |
| **承诺档位** | `promise` / `promise_tier` / `subset_ok` 适用范围或诚实度升级 | 文案变响、数字只进私有仓 |
| **API 面** | CLI/MCP/HTML 新增 Agent 可调用的结构能力 | runbook 脚本、影子树操作步骤 |
| **可信度门** | 发版自动证明「实现 ⊆ 文档承诺」 | 人工对照清单（可保留，不够） |

---

## P0 — 把能力变成「可被选用的产品」

### P0-1 公开 Agent 改码任务评测 — **done**

| 字段 | 内容 |
|---|---|
| 目标 | 公开任务上结构事实打分 vs 基线；可复现 harness |
| 交付 (this slice) | **done (public first ship):** `fixtures/eval-agent-tasks/`（9 个公开任务）+ `scripts/eval_agent_tasks.py` + `tests/agent_task_eval.rs` + [docs/eval-agent-tasks.md](eval-agent-tasks.md)。基线为 **name-grep**（实现于 harness，非伪造 LLM 数字）；禁止私有 corpus 路径 |
| 非目标 | 不接私有 stock 源码；不宣称生产 monorepo 精度 |

### P0-2 接通包 Onboarding Kit（5 分钟 MCP）— **done**

| 字段 | 内容 |
|---|---|
| 目标 | 新用户 5 分钟：装好 → 索引示例 → `blast_radius` |
| 交付 | `docs/onboarding.md`、`examples/mcp-claude.json`、`examples/mcp-generic.json`、`scripts/demo_blast_radius.ps1` / `.sh` |
| 验收 | 冷启动可成功；docs_claims 绿；演示脚本 exit 0 |

### P0-3 主路径一页纸（README）— **done**

| 字段 | 内容 |
|---|---|
| 目标 | 主路径一眼可见：`blast_radius` / `who_calls` / `graph` + `index` |
| 交付 | README en/zh 顶部 **Agent path** + 诚实表；Advanced 链接 |
| 验收 | 无超售句；docs_claims 绿 |

### P0-4 `blast_radius` recommendation 强化 — **done**

| 字段 | 内容 |
|---|---|
| 目标 | S 关闸时输出下一步合法命令 |
| 交付 | `sound_candidates[]`、示例 `impact --sound --workspace-root <id>`；脏 union 永不 `window=sound` |
| 验收 | 稳定 payload 键；agent-recipes「勿改名」列表 |

### P0-5 真实 Agent 对照评测（Agent±MCP）— **done（P0-5d lab_ready=true）**

| 字段 | 内容 |
|---|---|
| 状态 | **done:** scripted P0-5 + live P0-5b host-session + P0-5c multi-runner hard + **P0-5d isolated lab matrix** |
| **P0-5d 状态** | **done (S1 harness + S2 isolated live):** `lab_ready=true` (mimo-pro + mimo-flash, N=5)；**非**生态 sound |
| 交付 (P0-5d) | 协议 + `scripts/eval_agent_ab_d.py` + `evals/agent-ab-d/**`（easy 4 + hard 4；live × A/B × N=5；`incomplete_cells=0`）+ `tests/agent_ab_d_eval.rs`。**汇总（离线 stamp）：** live A recall **0.952–0.971** / noise **0.00**；live B recall **0.879–0.902** / noise **0.00**；cwr A **15/0** vs B **12/0**；scripted isolated B noise **1.875**。**叙事纪律：** live **噪声未拉开**——只可引用召回与 cwr 信号 + 矩阵完整性；**不得**写「live 噪声优势」或生态 sound |
| 后续 A（re-export） | **done:** `export { X } from` 提取 + `tests/ts_reexport_recall.rs`（v0.5.4） |
| 后续 B（发版卫生） | **done:** v0.5.4 / v0.5.5 / v0.5.6；README 叙事句固定 |
| 后续 C（package map 覆盖） | **done:** 重名包 fail-loud + 子路径 e2e + [eval-package-map.md](eval-package-map.md)（v0.5.6） |
| 后续 D（post-fix re-score） | **done（不改写历史）:** target 下并列对照输出 |

---

## P1 — 巩固 monorepo 与信任 — **done（摘要）**

| ID | 状态 | 摘要 |
|---|---|---|
| P1-1 workspace watch / 按 root 增量 | **done** | `write_root_id`；兄弟 root 不重哈希 |
| P1-2 MCP 默认 stale 字段 | **done** | `baseline_stale` / `sidecar_*` |
| P1-3 Golden Agent Suites | **done** | `fixtures/eval-agent-goldens` + `tests/agent_goldens.rs` |
| P1-4 workspace 性能预算 | **done** | `docs/eval-query-p95.md` + `tests/perf_workspace.rs` |

---

## P2 — 增长与差异化

| ID | 任务 | 状态 |
|---|---|---|
| **P2-1** | 仓库级 macro 默认配置（全局仍 OFF） | **done** `macro_default = off\|if_fresh\|on` |
| **P2-2** | CI 爆炸半径注释 demo | **done** `blast-radius-demo.yml`（非 required） |
| **P2-3** | 更多 L1 规则（有 eval 才做） | **open（eval-gated）** |
| **P2-4** | 对外一句话竞争叙事 | **done** README 对比表 |

### 明确不做（近两个季度）

- 生态 sound 营销  
- 通用代码搜索重做  
- 全局 `--with-macro` / 盲目 `--recall` 默认  

---

## 2026-09-18 复核结论（多次汇总）

| ID | 复核 |
|---|---|
| P0-1…P0-5d | **shipped** — 含 isolated lab `lab_ready=true`；live 噪声未分离已披露 |
| P1-1…P1-4 | **shipped** |
| P2-1, P2-2, P2-4 | **shipped** |
| P2-3 | **open（eval-gated）** |
| next-cut A re-export | **done**（v0.5.4） |
| package map | **done fail-loud + subpath**（v0.5.6）；真实 monorepo 抽检 residual open |

**残差（低优先 / 非阻塞）：**

1. P0-1 基线是 name-grep → 已由 P0-5/5b/5c/5d 补齐对照；Residual：第三方 API runner、更大 N、真实脏 monorepo live 噪声分离、`approx_tokens` 真实计量  
2. fixtures `**/.agentgraph/` — **done** gitignore  
3. Session 面板 ID 与文档 P0-x 不一致 — **以本文档为准**  
4. Workspace union CLI 含 spawn p95 可到秒级 — Agent 优先 root filter / MCP  
5. **真实 monorepo package map eval（非阻塞）** — partial package map 已交付；缺口：exports conditions、`node_modules`、嵌套 workspace 包、重名包 Agent 是否发现 override。**禁止**写成 full TS resolution / 生态 sound  
6. **post-fix 评测轨迹并列表** — 可选；不覆盖历史 stamp  

---

## 建议执行顺序（当前）

```text
已完成：P0-3 → P0-4 → P0-2 → P0-1 → P0-5 系列 → P1-* → P2-1/2/4
下一步候选（按产品判断）：
  1) 真实 monorepo package map 抽检（residual #5，补证据）
  2) P2-3 L1 规则包（仅当有 eval 金标数字）
  3) 停机 / 发版叙事复审
```

## 与 session 任务面板的映射

session `task` 面板 ID 可能与本文档 P0-x 编号不一致；**完成标准以本表「验收」为准**。

---

*维护：产品/仓库负责人。变更时同步 agent-recipes 稳定键与 docs_claims。*
