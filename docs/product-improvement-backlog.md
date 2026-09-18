# 产品改善任务列表（源自 PM 复审 · v0.5.1）

> **状态图例：** open / in_progress / blocked / done  
> **原则：** 先证据与接通，再加分析特性；不超售 sound；全局 `--with-macro` 仍默认 OFF。  
> **关联：** [product-boundary-migration.md](product-boundary-migration.md)、[agent-recipes.md](agent-recipes.md)、[noise-governance.md](noise-governance.md)、[workspace.md](workspace.md)

---

## P0 — 把能力变成「可被选用的产品」（优先）

### P0-1 公开 Agent 改码任务评测

| 字段 | 内容 |
|---|---|
| **目标** | 用可复现任务证明：挂上 agentgraph 后，Agent 爆炸半径更准、误改文件更少 |
| **交付物** | `evals/agent-tasks/`：任务集（issue + 仓 fixture + 期望结构事实）；评分协议（边/文件命中、误改）；harness（CLI 或 MCP 调用）；`docs/eval-agent-tasks.md` 数字表 |
| **范围** | ≥8 个任务：干净 TS/Py/Go 各 1–2；Rust monorepo workspace 2；unsafe/sound-disabled 1（验 scoped 引导）；噪声名 `fmt` 类 1 |
| **非目标** | 不接私有 stock 源码；不宣称生产 monorepo 精度 |
| **验收** | `cargo test` 或脚本可跑通 harness；README/recipes 引用数字；相对「无工具 / 仅 LLM」基线表 |
| **涉及** | `evals/` 或 `fixtures/eval-agent-tasks/`、新 docs、CI 可选 job |
| **估** | 1–2 周 |
| **交付 (this slice)** | **done (public first ship):** `fixtures/eval-agent-tasks/`（9 个公开任务）+ `scripts/eval_agent_tasks.py` + `tests/agent_task_eval.rs` + [docs/eval-agent-tasks.md](eval-agent-tasks.md)。基线为 **name-grep**（实现于 harness，非伪造 LLM 数字）；禁止私有 corpus 路径。 |

### P0-2 接通包 Onboarding Kit（5 分钟 MCP）

| 字段 | 内容 |
|---|---|
| **状态** | **done** |
| **目标** | 新用户/Agent 宿主 5 分钟内：装好 → 索引示例仓 → 调用 `blast_radius` |
| **交付物** | `docs/onboarding.md`；`examples/mcp-claude.json` / `examples/mcp-generic.json`；`scripts/demo_blast_radius.ps1` + `scripts/demo_blast_radius.sh`；README / agent-recipes / playbook 链接 |
| **范围** | 一条 happy path + 一条 workspace happy path |
| **非目标** | 不替代完整 README；不绑死某一 Agent 产品 |
| **验收** | 按文档冷启动可成功；docs_claims 不红；演示脚本 exit 0 |
| **涉及** | `docs/`、`examples/`、`scripts/`、README 顶部链接 |
| **估** | 3–5 天 |

### P0-3 主路径一页纸（README 收束）

| 字段 | 内容 |
|---|---|
| **目标** | 产品主路径一眼可见：3 个 MCP tool + 何时 sound/不会 |
| **交付物** | README en/zh 顶部 **Agent path** 区：`blast_radius` / `who_calls` / `graph`（+ `index`）；诚实表（window、subset_ok、note）；Advanced 链到 flags 全文 |
| **范围** | 叙事与结构，不删能力文档 |
| **非目标** | 不删除底层 CLI/MCP 表 |
| **验收** | 新人 30 秒内知道「Agent 该调什么」；无超售句；docs_claims 绿 |
| **涉及** | `README.md` / `README.zh-CN.md`、`docs/agent-recipes.md` 链接 |
| **估** | 1–2 天 |

### P0-4 `blast_radius` recommendation 强化（scoped sound 引导）

| 字段 | 内容 |
|---|---|
| **目标** | S 关闸/scope 时，默认输出直接告诉 Agent **下一步合法命令** |
| **交付物** | `window=disabled/default` 时 `recommendation` 含：`sound_candidates[]`、示例 `impact --sound --workspace-root <id>`、`by_top_dir` 提示；脏 union **永不** `window=sound`（已有测试保持） |
| **范围** | `query/recipes` + CLI/MCP payload + 单测 |
| **非目标** | 不自动执行 scoped sound；不静默 union 宏边 |
| **验收** | `tests/agent_recipes.rs` / `r32` 扩展；payload 键稳定（可进 agent-recipes「勿改名」列表） |
| **涉及** | `src/query/recipes`、`src/cli.rs`、`src/mcp/server.rs`、`docs/agent-recipes.md` |
| **估** | 2–4 天 |

---

## P1 — 巩固 monorepo 与信任

### P1-1 Workspace watch / CI 增量

| 字段 | 内容 |
|---|---|
| **目标** | 多根仓改文件后不必全量 `index --workspace` 才能查 |
| **交付物** | `watch --workspace …` 或 `index_paths` 按 `root_id` 增量；文档与 `workspace.md` 对齐 |
| **验收** | 双 root 改一侧 → 另一侧 hash 不重算；subset/diff 行为有测试 |
| **估** | 1 周 |

### P1-2 Stale 提示进 MCP 默认 payload

| 字段 | 内容 |
|---|---|
| **目标** | Agent 不读 docs 也能看到 `baseline_stale` / `sidecar_stale` / workspace `missing` |
| **交付物** | 查询/状态类 MCP 返回可选或默认带上述字段（非破坏：旧客户端可忽略） |
| **验收** | e2e：watch 后 `graph_diff`/`stats` 含 stale；无 sidecar 时 `sidecar_exists=false` |
| **估** | 2–3 天 |

### P1-3 Golden Agent Suites（发版门禁）

| 字段 | 内容 |
|---|---|
| **状态** | **done** |
| **目标** | 与 docs_claims 同级：recipe/window/诚实字段回归不靠人记 |
| **交付物** | `fixtures/eval-agent-goldens/` + `tests/agent_goldens.rs`；纳入 CI `cargo test` |
| **验收** | 期望 JSON 锁 `window`/`edge_role`/`recommendation` 关键形态 |
| **估** | 3–5 天 |
| **交付 (this slice)** | **done:** `fixtures/eval-agent-goldens/`（public fixtures + `goldens.json`）+ `tests/agent_goldens.rs` + [docs/agent-goldens.md](agent-goldens.md)。recommendation 用 contains/regex，稳定键勿改名。 |

### P1-4 大 workspace 索引性能与预算文档

| 字段 | 内容 |
|---|---|
| **状态** | **done** |
| **目标** | 可预期：N 文件 × M root 的 index/status 量级与 soft SLO |
| **交付物** | `docs/eval-query-p95.md` workspace 扩充；可选 `tests/query_p95` smoke（软门禁） |
| **验收** | 文档含复现命令；与 operator 实测一致处标明 |
| **估** | 1–2 天 |
| **交付 (this slice)** | **done:** [eval-query-p95.md](eval-query-p95.md) workspace 扩充（机器/数字/复现/非 SLO 诚实）+ `tests/perf_workspace.rs` 软 smoke。 |

---

## P2 — 增长与差异化（不挡 P0）

| ID | 任务 | 说明 | 估 |
|---|---|---|---|
| **P2-1** | 仓库级 macro 默认配置 | 项目/env 打开「有 fresh sidecar 则 blast_radius 可 include」；**全局仍 OFF** | 2–3 天 |
| **P2-2** | CI 爆炸半径注释 demo | GitHub Action 样例：PR 触发 `blast_radius` → 评论；不进 required 也可 | 2–4 天 |
| **P2-3** | 更多 L1 规则（有 eval 才做） | 仅当 golden 提升 + 噪声可控 | 按包 |
| **P2-4** | 对外一句话竞争叙事 | README 对比表收束：vs RAG / CodeQL / LSP / bare SCIP | 1 天 |

### 明确不做（近两个季度）

- 生态 sound 营销  
- 通用代码搜索重做  
- 全局 `--with-macro` / 盲目 `--recall` 默认  

---

## 建议执行顺序

```text
P0-3 README 主路径（快）
  → P0-4 recommendation 强化（产品行为）
  → P0-2 Onboarding Kit
  → P0-1 Agent 任务评测（可并行设计任务集）
  → P1-2 stale 进 MCP → P1-3 golden suites
  → P1-1 workspace watch → P1-4 perf 文档
  → P2-*
```

## 与 session 任务面板的映射

session `task` 工具中已登记同 ID（`P0-1`…）；完成标准以本表「验收」为准。

---

*维护：产品/仓库负责人。变更时同步 agent-recipes 稳定键与 docs_claims。*
