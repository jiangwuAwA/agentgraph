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

### P0-5 真实 Agent 对照评测（Agent±MCP）

| 字段 | 内容 |
|---|---|
| **状态** | **done (scripted S1+S2 + live P0-5b host-session slice):** 协议 + 回放 + 脚本化 policy A/B/C + live host-session A/B。Scripted = **非** standardized lab LLM；live = 单 host session（`mimo-desktop-host-session`），**非** public benchmark model；**无超售**（live A/B 噪声未分离） |
| **目标** | 在公开任务集上回答产品问题：**挂上 agentgraph MCP 的 Agent，是否比不挂更准、误改更少** — 不再仅用 name-grep 代替 Agent 行为 |
| **背景** | P0-1 已交付 structure-fact 打分 vs **name-grep**（确定性 token 基线）。文档已诚实声明这不是 LLM/Agent 产品证明。P0-5 补齐该对照 |
| **交付物** | ① `evals/agent-baseline/`（或 `fixtures/eval-agent-tasks/` 扩展）：同一套公开 mini-repo + 任务 JSON；② harness 协议：**A 组** Agent 仅允许 grep/read；**B 组** Agent 允许 MCP `blast_radius`/`who_calls`/`graph`（或 CLI recipes）；③ 评分：期望文件/符号命中、**噪声文件**、错误改码范围（若有补丁）；④ `docs/eval-agent-baseline.md`：任务级与汇总表、复现命令、模型/温度/重复次数；⑤ 可选：把汇总指标挂到 README / eval-agent-tasks 交叉链接 |
| **范围** | 优先复用 P0-1 的 9 个公开任务（至少跑通 **≥6** 个）；固定 prompt 模板与「禁止超售」指令；每任务 ≥N 次重复（建议 N≥3）报 mean；记录失败模式 |
| **基线说明** | **B−A** 为主要产品指标；name-grep 保留为 **确定性下界/对照**，不删 P0-1 表 |
| **非目标** | 不伪造未跑通的 LLM 数字；不提交私有 stock；不把单次幸运 run 写成结论；不把 `window=sound` 写成生态 sound |
| **约束** | Agent 运行环境需网络/API 的部分：**可选 CI / 操作员轨** — 主仓 harness 必须能在 **无 LLM** 时仍解析「录制好的 tool 轨迹」做回放打分（若先交付录制协议） |
| **验收** | ① 文档含 A/B 定义与非声称；② 至少一份可复现汇总表（任务 × A/B 指标）；③ harness/脚本可本地跑；④ 与 eval-agent-tasks 的关系写清（structure facts vs Agent 行为）；⑤ docs_claims / 相关 `cargo test` 绿；⑥ backlog 本卡状态改为 done |
| **涉及** | `docs/eval-agent-baseline.md`、`docs/eval-agent-tasks.md`（交叉链接）、`scripts/eval_agent_baseline.py`（或等价）、`tests/agent_baseline.rs`（可选：协议/回放锁）、`fixtures/eval-agent-tasks/**`（只读复用优先）、README 链接一句 |
| **估** | 1–2 周（含首次 Agent 跑批；回放协议可先于 live LLM） |
| **建议切片** | **S1** 协议 + 录制/回放 JSON 格式 + 无网打分；**S2** 操作员 live A/B 跑批写入 docs；**S3**（可选）CI 手动 workflow_dispatch 产出 artifact |
| **交付 (this slice)** | **done (scripted S1 + policy S2-sim + P0-5b live host-session):** [eval-agent-baseline.md](eval-agent-baseline.md) + `scripts/eval_agent_ab.py` + `evals/agent-ab/**`（≥9 tasks × ≥3 A/B/C runs）+ `tests/agent_ab_eval.rs`；**P0-5b:** `scripts/eval_agent_ab_live.py` + `evals/agent-ab-live/**`（9 tasks × 3 seeds × arms A/B = **54** trajectories）+ `tests/agent_ab_live.rs`。**标签（强制）：** scripted = **scripted tool-policy agents**；live = **host-session LLM**（`model_note=mimo-desktop-host-session`，**非** public benchmark model / **非** standardized lab harness）。operator 标签 **A=MCP/CLI recipes，B=read/grep，C=name-grep**。Scripted numbers：A noise **0.00**；B **1.56**；C **0.78**。Live numbers（recalled only）：A live recall 1.00 / noise **0.00**；B live recall 1.00 / noise **0.00**（本 session 未分离 — **禁止**据此宣称 live MCP 产品优势）；contamination + N small 已写入 docs。Offline replay：`python scripts/eval_agent_ab.py score --traj-dir evals/agent-ab` 与 `--traj-dir evals/agent-ab-live`。**S3 CI workflow 本切片跳过。** |
| **P0-5c 状态** | **done (protocol + hard fixtures + ≥2 runners + extended metrics + recorded N=3):** 见下条「交付 (P0-5c)」 |
| **P0-5d 状态** | **done (S1 harness + S2 isolated live matrix):** `lab_ready=true` (mimo-pro + mimo-flash, N=5); **非**生态 sound |
| **交付 (P0-5c)** | **done:** [eval-agent-baseline.md](eval-agent-baseline.md) § **P0-5c** + `scripts/eval_agent_ab_c.py`（write/stamp/score/--runner）+ `fixtures/eval-agent-tasks-hard/**`（≥4 hard tasks）+ `evals/agent-ab-c/**`（4 tasks × 2 runner kinds × 2 arms × 3 seeds = **48** trajectories + `task_randomization.json`）+ `tests/agent_ab_c_eval.rs` + `evals/agent-ab-c/README.md`。**Runner kinds:** `host_session_llm`（`independent_session=false`，披露 contamination）+ `scripted_external_runner`（decision-path `independent_session=true`）。**Extended metrics:** `mcp_or_cli_calls` / `chose_correct_workspace_root` / `file_budget` / `read_budget` / `approx_tokens=null` / `runner_id` / `model_note` / `saw_labels_before_commit=false`。**Recorded hard-task means (no oversell):** host A noise **0.00** vs host B **1.50**；scripted A noise **0.25** vs scripted B **3.25**（scripted A recall **0.9375** — multi-root path alias gap 已诚实记录）。**N target ≥5；本 host 记录 N=3。** **禁止**将 host-session 分离写成 multi-model lab / 产品优越性证明。**无**独立 isolated-subagent 第三 runner（incomplete，不伪造）。 |

### P0-5d 真隔离 live 对照（isolated lab）

| 字段 | 内容 |
|---|---|
| **状态** | **done (S1 harness + S2 isolated live matrix):** `lab_ready=true`（见交付） |
| **目标** | 在 **真隔离** 条件下复测：live Agent±agentgraph 是否在 hard 任务上稳定降低噪声 / 提高 `workspace-root` 正确率 — 作为可对外引用的选用证据候选 |
| **背景** | P0-5b easy fixture 上 live A/B **未分离**；P0-5c hard 上有噪声与 root 选择信号，但 `host_session_llm.independent_session=false`、fixture 作者同 session、N=3、scripted runner 非 live LLM。P0-5d 补齐 lab 级隔离 |
| **交付物** | ① 协议扩展：`docs/eval-agent-baseline.md` § **P0-5d**（隔离、随机化、盲评、禁止污染源清单）；② **Isolated runner 接口**：每 arm/seed **独立进程或独立 agent 会话**（无共享中间 file-set、无 fixture `task.json` expected 可见）；③ **≥2 live runners**：至少 1 个 **非 host-session** 模型/runner id + 可选 host 作对照臂；④ 任务集：复用 `fixtures/eval-agent-tasks/**` + `eval-agent-tasks-hard/**`（建议 easy≥4 + hard≥4）；⑤ **N≥5 seeds / task / arm / runner**（未达标必须在表头标红）；⑥ 轨迹 `evals/agent-ab-d/**` + 随机化日志；⑦ 评分与指标对齐 P0-5c（recall / extra-noise / cwr / mcp_calls / file_budget；`approx_tokens` 仅真实值或 null）；⑧ 汇总表 + 非声称 + `docs_claims` 门禁；⑨（可选）`tests/agent_ab_d_eval.rs` 锁协议字段与「禁止伪造 N/模型」 |
| **范围** | 主指标仍为 structure-fact 文件集；产品解读维度：**noise 分离**、**cwr**、**是否触发 scoped sound 建议**。允许操作员/外部 API runner；结果必须可 `score` 回放 |
| **非目标** | 不把 null 结果写成阳性；不合成未跑通的模型格；不把 scripted 冒充 live；不提交私有 monorepo 源码；不把 `ast_modeled` 写成生态 sound |
| **污染门禁（必须）** | ① 决策路径看不到 golden/expected；② arm 间无共享状态文件；③ 任务顺序随机化并落盘；④ runner 元数据含 `independent_session` / `model_note` / harness 版本；⑤ 若仍用 author-session，**整表降级为 non-lab** 并标 `lab_ready=false` |
| **验收** | ① 文档写清 lab vs non-lab 判定；② ≥2 live runner + N≥5 的完整表 **或** 明确 `lab_ready=false` + 缺口清单；③ 48+ 可回放轨迹（按任务数×臂×seed×runner）；④ 与 P0-5b/c 差异表（easy vs hard、host vs isolated）；⑤ 相关测试/docs_claims 绿；⑥ 本卡状态改为 done 或 blocked（缺第三方 runner 时） |
| **涉及** | `docs/eval-agent-baseline.md`、`scripts/eval_agent_ab_d.py`（或扩展 `eval_agent_ab_c.py --isolated`）、`evals/agent-ab-d/**`、`fixtures/eval-agent-tasks*/**`（只读）、README 一句链接（仅当 lab_ready=true） |
| **估** | 1–2 周（取决于是否有外部 live runner/API；无外部模型则先交付隔离 harness + `lab_ready=false`） |
| **建议切片** | **S1** 隔离协议 + 外部 runner 接口 + 盲评字段；**S2** 第二 live runner 跑批（N≥5）；**S3** 汇总 + 对外叙事（仅 lab_ready=true 时可称 lab） |
| **交付 (P0-5d)** | **done (S1 harness + S2 isolated live):** 协议 + `scripts/eval_agent_ab_d.py` + `evals/agent-ab-d/**`（easy 4 + hard 4；live runners `mimo-pro`/`mimo-flash` × A/B × N=5；`incomplete_cells=0`）+ `tests/agent_ab_d_eval.rs`。**lab_ready=true**（harness 离线判定：≥2 非 author live、N≥5、双臂、easy+hard、independent_session）。**汇总（离线 stamp）：** live A recall **0.952–0.971** / noise **0.00**；live B recall **0.879–0.902** / noise **0.00**；cwr A **15/0** vs B **12/0**；scripted isolated B noise **1.875**。**叙事纪律：** live **噪声未拉开**——只可引用召回与 cwr 信号 + 矩阵完整性；**不得**写「live 噪声优势」或生态 sound。`approx_tokens=null`。 |
| **依赖** | P0-5c 轨迹/指标 schema 保持兼容；hard fixtures 可复用 |
| **并行可选** | 修 scripted A 的 multi-root **路径别名/re-export 召回缺口**（产品侧，非本卡必做） |

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

| ID | 任务 | 说明 | 估 | 状态 |
|---|---|---|---|---|
| **P2-1** | 仓库级 macro 默认配置 | 项目/env 打开「有 fresh sidecar 则 blast_radius 可 include」；**全局仍 OFF** | 2–3 天 |
| | **状态** | **done:** `.agentgraph/config.toml` / `agentgraph.toml` `macro_default=off\|if_fresh\|on`; env `AGENTGRAPH_MACRO_DEFAULT` overrides file; CLI explicit wins. `if_fresh` + fresh sidecar → `include_macro=true` + `include_macro_reason=repo_config_if_fresh`. Sound/stale/nested/missing still refuse. `macro status` shows `macro_default_source`. Global default remains **OFF**. Tests: `tests/macro_default_config.rs`. |
| **P2-2** | CI 爆炸半径注释 demo | GitHub Action 样例：PR 触发 `blast_radius` → 评论；不进 required 也可 | 2–4 天 | **done (demo slice):** `.github/workflows/blast-radius-demo.yml` + `examples/ci/blast-radius.yml` + `scripts/ci_blast_radius_markdown.py` + [ci-blast-radius-demo.md](ci-blast-radius-demo.md)。Step summary + soft-fail PR comment；无 expand install；非 required。 |
| **P2-3** | 更多 L1 规则（有 eval 才做） | 仅当 golden 提升 + 噪声可控 | 按包 | open（eval-gated） |
| **P2-4** | 对外一句话竞争叙事 | README 对比表收束：vs RAG / CodeQL / LSP / bare SCIP | 1 天 | **done:** README en/zh「Positioning / 定位」表（Chunk RAG / CodeQL enterprise / bare LSP / raw SCIP）+ 诚实非声称；链接 [eval-agent-tasks.md](eval-agent-tasks.md) + [onboarding.md](onboarding.md)；docs_claims 绿。 |

### 明确不做（近两个季度）

- 生态 sound 营销  
- 通用代码搜索重做  
- 全局 `--with-macro` / 盲目 `--recall` 默认  

---

## 2026-09-18 复核结论（二次）

| ID | 复核 |
|---|---|
| P0-1…P0-4 | **shipped** — 代码/文档/测试齐；`agent_task_eval`/`agent_goldens`/`agent_recipes`/`docs_claims`/`e2e_cli`/`noise_roles` 全绿 |
| P0-5 | **shipped (scripted + 5b host-session + 5c multi-runner hard + 5d isolated lab matrix)** — harness + 轨迹齐；**P0-5d `lab_ready=true`**（mimo-pro/flash × N=5）；live 噪声未分离，主信号为 **recall + cwr**；**无超售** |
| P1-1…P1-4 | **shipped** — workspace watch、MCP stale 字段、goldens、perf_workspace 文档+smoke |
| P2-1, P2-2, P2-4 | **shipped** — macro_default（全局 OFF）、CI demo（非 required）、README 定位 |
| P2-3 | **open（eval-gated）** — 符合「无 eval 数字不做」 |

**残差（低优先）：**

1. P0-1 基线是 **name-grep**，不是真实 LLM/Agent 基线 — 文档已诚实声明。→ P0-5/5b/5c/**5d** 已交付；**P0-5d `lab_ready=true`**（见 [eval-agent-baseline.md](eval-agent-baseline.md)）。Residual（非阻塞）：第三方 API runner、更大 N、真实脏 monorepo 上的 live 噪声分离、`approx_tokens` 真实计量。
2. ~~`fixtures/**/.agentgraph/index.db` 二进制索引~~ — **done**：根 `.gitignore` 增加 `**/.agentgraph/`；本地 fixture 索引目录已删除（勿再提交）。
3. Session 任务面板 ID（T7–T18）与文档 P0-x 编号不一致 — **以本文档为准**。
4. Workspace **union callers** CLI 含 spawn 时 p95 可到秒级（文档已标非 SLO）— Agent 侧**优先 root filter 或 MCP**；已写入 eval-agent-tasks / recipes。

---

## 建议执行顺序

```text
（已 shipped）P0-1…P0-4, P0-5/5b/5c, P1-1…P1-4, P2-1/2/4
  → P0-5d 真隔离 live 对照 — **shipped**（`lab_ready=true`；叙事限 recall/cwr）
  → 下一刀候选：multi-root 路径别名/re-export 召回缺口；或打 `v0.5.4` 锁定评测面
  → P2-3 L1 规则（eval-gated）
  → （并行可选）multi-root 路径别名/re-export 召回缺口
  → P2-3 L1 规则（eval-gated）
```

## 与 session 任务面板的映射

- **权威编号与验收标准：以本文档 P0-x / P1-x / P2-x 为准。**
- Session 面板 ID（如 T7–T18）仅作会话跟踪，**与文档编号不一致时忽略面板 ID**。
- 面板摘要应尽量写入文档编号（例：`P0-5 …`），便于对照。

---

*维护：产品/仓库负责人。变更时同步 agent-recipes 稳定键与 docs_claims。*
