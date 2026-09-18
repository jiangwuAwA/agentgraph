# agentgraph

[![CI](https://github.com/jiangwuAwA/agentgraph/actions/workflows/ci.yml/badge.svg)](https://github.com/jiangwuAwA/agentgraph/actions/workflows/ci.yml)

[English](README.md) | 简体中文

面向 Agent 的代码理解：**符号图、调用图、影响分析** —— CLI + MCP 服务器。

不是又一个 embedding RAG。Agent 需要知道「谁在调它」「改这里会坏什么」「该读哪些文件」时，要的是**结构化事实**，不是相似文本片段。

| 方案 | 问题 |
|---|---|
| 切块 + embedding RAG | 丢失调用/导入结构 |
| CodeQL / Sourcegraph | 过重、偏企业，Agent 不好直接调用 |
| 裸 LSP | 为 IDE hover 设计，不适合多跳推理 |

**流水线：** tree-sitter 解析 → SQLite 符号/引用库 → 面向 Agent 的查询 API。

## 快速开始

```bash
cargo install --path .
cd /path/to/your/repo

agentgraph index
agentgraph find createUser
agentgraph callers validateEmail
agentgraph impact validateEmail --depth 3
agentgraph graph validateEmail --depth 2   # 离线 HTML 图
agentgraph diff                             # 已索引边集合差（相对 index 快照基线）
agentgraph importers src/auth.ts
agentgraph export scip --out index.scip   # 官方 scip CLI 可读
```

索引文件：`<root>/.agentgraph/index.db`（请加入 `.gitignore`）。

## 功能

### 语言

TypeScript、TSX、JavaScript、JSX、Python、Go、Rust。

### 查询

| 命令 | 用途 |
|---|---|
| `find` | 符号定义（精确；`--fuzzy` 为 LIKE 模糊）。workspace 行带 `root_id` + `root_path` |
| `callers` | 调用/导入点 + L1 候选边（含 `module`、`resolved`、`qualifier`、`confidence`、**`edge_role`**、`root_id`）。默认在存在 implementor 时返回 `{callers, implementors, …}` 分离噪声；高频名 demote —— [docs/noise-governance.md](docs/noise-governance.md) |
| `impact` | 真 BFS 爆炸半径（默认 Exact+Heuristic） |
| `graph` | 离线自包含 HTML 代码图；`--sound` 仅渲染 sound-eligible 边并展示 `subset_ok`/`promise_tier`；`--workspace-root` 可过滤 + 节点 `root_id` 徽章 —— 见 [docs/graph-html.md](docs/graph-html.md) |
| `diff` | 相对 `index` 时写入的快照基线，做**已索引边集合差**（非运行时调用图 diff）—— 见 [docs/graph-diff.md](docs/graph-diff.md) |
| `related` | 定义 + 导入方 + 引用（用于收敛阅读范围） |
| `importers` | 谁 import 了该文件 |
| `workspace status` | 多根 workspace 索引健康度（每根 files/symbols/refs/exact/heur/violations、`promise_tier`、`index_seq`、`missing`）—— 一行上手：`agentgraph index --workspace-root api --workspace-root web --workspace-db ./ws.db` 后 `workspace status` —— [docs/workspace.md](docs/workspace.md) |
| `blast-radius` | 高层爆炸半径配方：自动 `window=sound\|default`（仅 `subset_ok` 时用 sound；否则默认 Exact+Heuristic，**不是**盲 `--recall`）+ `recommendation` / 诚实 `note` —— [docs/agent-recipes.md](docs/agent-recipes.md) |
| `who-calls` | 高层 who-calls 配方：默认分离/折叠 implementor（`--noisy` 合并）；高频名 demote —— [docs/agent-recipes.md](docs/agent-recipes.md) |

`callers` / `impact`（以及 MCP 工具）的 confidence 窗口：

- 默认：**Exact + Heuristic**（L1 DI/工厂/事件等候选）。**噪声治理：** 行内带 `edge_role`（`call`|`implementor`|`registration`|`dynamic`）；存在 implementor 时 `callers` 返回 `{callers, implementors, implementor_count, implementors_truncated, truncated, note}`（零 implementor 时仍为纯数组）。`--include-implementors` 合并；`--implementors-only` 仅实现边；高频名（`fmt`/`drop`/`clone`/…）implementor 截断到 20。存储保留全部边；`impact` 仍展开 implementor（已打标）。见 [docs/noise-governance.md](docs/noise-governance.md)。
- `--exact-only`：仅 L0 语法确定边
- `--include-dynamic` / **`--recall`**：额外纳入 DynamicCandidate（反射/计算属性，噪声更大）
- `--sound`（L2，S 限定）：在 `subset_ok` 时对**已建模**引用边（直接调用、字面量键、**emit↔on 派发**、DI/路由注册）做 over-approx；注册 ≠ HTTP ServeHTTP。见 [docs/sound-subset.md](docs/sound-subset.md)。查询 p95：[docs/eval-query-p95.md](docs/eval-query-p95.md)。**承诺档位：** 已交付语言（js/ts/tsx/jsx/python/go/rust）均为 **`ast_modeled`**（tree-sitter AST S 门，工程子集，**不是**生态 sound）。仅类型位置的 `typeof Function` 留在 S 内；对 `Function`/`eval` 的**值使用**会离开 S。

**怕漏时：** 用 `--sound`（`subset_ok`）或 `--recall`。干净的图不等于完整的图。

所有非 Exact 边都带 `evidence`（规则 id + 源码片段）。SCIP 导出默认 Exact+Heuristic（不含 DynamicCandidate）。评测数字见 [docs/eval-l1.md](docs/eval-l1.md)、[docs/eval-l2.md](docs/eval-l2.md)。

**可选宏旁路（P2/M1，默认关闭，非 sound）：** `index --macro-expanded-root` 把**已有**的 expand 影子树写入 sidecar；`callers`/`impact`/`graph --with-macro` 联合查询时映射回源码路径并默认去重（`--no-macro-dedup` 为调试开关；`--exact-only --with-macro` 忽略 sidecar）。`macro status` 暴露 `stale`/`path_map_present`/`dedup_stats`；`macro rebuild` 幂等重建（不调用 `cargo expand`）。`origin=macro_expanded`；与 `--sound` 互斥。详见 [docs/macro-sidecar.md](docs/macro-sidecar.md)。

### 索引质量

- **导入解析** — TS/JS 相对路径、Python 包、Rust 本地 `crate::`/`super::`/`self::`（按段计数）、保守的 Go 包路径
- **增量索引** — 内容 hash 跳过；LLM 描述在 reindex 后保留
- **类型感知（务实）** — `qualifier` 来自参数注解、receiver、`New*` 构造、返回类型 `define` 边；`callers` 支持 `Type.method` / `Type::method`。不是完整类型检查器
- **空索引契约** — CLI 与 MCP 在未 `index` 时都会明确报错，而不是静默返回 `[]`

### 导出（SCIP）

`export scip` 写出 **protobuf 二进制**（官方 `scip` CLI 可直接读取）。  
`export scip-json` 写出 protobuf JSON，便于调试。

官方描述符（已通过 `scip lint` exit 0）：

- 类型：`Store#`
- 方法：`Store#save().`
- 函数：`loginHandler.`
- 命名空间：`ns/`

已与 crate [`scip`](https://crates.io/crates/scip) 0.10 及官方 CLI（`print` / `lint` / `stats`）做过互操作验证。  
`export lsif` 仅为简化 JSONL 导出（非完整 LSIF 实现）。

### Enrich（可选 LLM）

```bash
export OPENAI_API_KEY=sk-...
# 可选：OPENAI_BASE_URL、AGENTGRAPH_MODEL、AGENTGRAPH_ENRICH_CONCURRENCY
agentgraph enrich --limit 50
```

并发调用 OpenAI 兼容 API；即使后续失败，已成功的描述也会落库。

### Watch

```bash
agentgraph watch --interval 5
```

优先 **fsnotify** + 防抖；失败时回退轮询（mtime 纳秒 + 大小）。尽量路径级增量 reindex。

## MCP 服务器

```bash
agentgraph --root /path/to/repo mcp
```

工具：`index`、`find_symbol`、`callers`、`impact`、**`blast_radius`** / **`who_calls`**（高层 Agent 配方：自动窗口 + implementor 分离 —— [docs/agent-recipes.md](docs/agent-recipes.md)）、`related_files`、`importers`、`enrich`、`stats`、**`subset`**（S 违例报告，门控 `--sound`）、**`graph_diff`**（已索引边集合差；非运行时语义 —— [docs/graph-diff.md](docs/graph-diff.md)）、**`workspace_status`**（多根健康度 —— [docs/workspace.md](docs/workspace.md)）、可选 **`macro_status`** / **`macro_rebuild`**（P2/M1 旁路，默认关闭 —— [docs/macro-sidecar.md](docs/macro-sidecar.md)）。查询工具可选 `workspace_db` / `root_id` 过滤（默认关闭）。

**安全：** 每次调用的 `root` 默认限制在服务器启动时的根目录内；需显式设置 `AGENTGRAPH_MCP_ALLOW_ANY_ROOT=1` 才可越界。

客户端配置示例：

```json
{
  "mcpServers": {
    "agentgraph": {
      "command": "agentgraph",
      "args": ["--root", "C:/path/to/repo", "mcp"]
    }
  }
}
```

## 安装

### 源码安装（在打标签 Release 前推荐）

```bash
cargo install --path .
# 或
cargo build --release
```

### 预编译（发布 `v*` Release 后）

```bash
# macOS / Linux（SHA256 校验失败即中止）
curl -fsSL https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.sh | bash

# Windows PowerShell
iwr -useb https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.ps1 | iex
```

CI 在 tag `v*` 时构建多平台产物（linux gnu/musl、windows、macos）并附带 `SHA256SUMS`。

## 演示（fixture）

```bash
agentgraph --root fixtures/sample-app index --force
agentgraph --root fixtures/sample-app impact validate_email --depth 3
agentgraph --root fixtures/sample-app importers src/auth.ts
agentgraph --root fixtures/sample-app export scip --out /tmp/index.scip
scip lint /tmp/index.scip   # exit 0
```

## 架构

```
源码文件
    │  ignore（尊重 gitignore + 路径段噪声过滤）
    ▼
tree-sitter（TS / TSX / JS / JSX / Python / Go / Rust）
    │  rayon 并行解析 + LineIndex（UTF-16 列）
    ▼
符号 + 引用（+ 导入解析 + qualifier）
    │  单事务批量写入 SQLite
    ▼
.agentgraph/index.db
    │
    ├─ CLI
    ├─ MCP stdio
    ├─ enrich（并发）
    └─ export scip / scip-json / lsif
```

调用解析以**名字**为主、可选类型 `qualifier` —— 务实方案，**不是完整类型推断**。影响分析在 enclosing 符号上做真 BFS。

## 测试与流程

```bash
cargo test
cargo test --test e2e_cli          # 真实二进制 E2E（CLI + MCP + scip）
powershell -File scripts/e2e.ps1   # 本地完整门禁 + fixture 冒烟
```

CI（ubuntu / windows / macos）：`fmt` + `clippy -D warnings` + `build` + `test` + CLI E2E；Linux 额外跑官方 `scip lint`。

**开发默认 TDD** —— 见 [AGENTS.md](AGENTS.md)。先写失败测试，再实现，最后重构。

**路线图（L0–L3）：** 分析能力计划见 [PLAN.md](PLAN.md)。L1 DI/动态**候选边**已交付（非 sound）。L2 `--sound` 为**实验性**弱化资格承诺（见 [docs/sound-subset.md](docs/sound-subset.md)）；L3 形式化轨不挡发版。

## 许可证

MIT
