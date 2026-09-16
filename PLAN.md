# agentgraph 分析能力路线图（L0–L3）

> 本文是产品/工程计划，不是研究综述。  
> 开发流程默认 **TDD**（见 [AGENTS.md](AGENTS.md)）。

---

## 0. 问题与边界

### 0.1 要服务的场景

AI Agent 改代码前需要回答：

1. **爆炸半径**：改 `X` 会影响哪些符号/文件？  
2. **及时性**：磁盘上的源码变了，图何时可信？  
3. **召回缺口**：动态调用、DI 注册、字符串反射是否被图捕获？

### 0.2 理论与工程边界（必须写进产品文案）

| 能力 | 现实约束 |
|---|---|
| 含 `eval` / 运行时反射的 **sound + complete** 调用图 | **不可判定**（一般化停机问题 / Rice） |
| 全语言「完整类型推断」 | 不存在；tsc/mypy/rustc 均为语言专属多年工程 |
| 「定理级正确」覆盖 JS/Python 全生态 | 不可能；形式化只能验证 **受限子集** 或 **实现不变量** |
| 文件系统实时 | **可做**（notify/fsnotify），纯工程 |

**产品承诺分层（禁止混写）：**

| 承诺级别 | 含义 | 适用 |
|---|---|---|
| **A. 工程索引（现状 L0）** | 名字/qualifier 启发式，快，可解释，**best-effort** | Agent 日常 |
| **B. 可能边（L1）** | 动态/DI 启发式边，带 `confidence` 与证据，**不声称 sound** | 提高召回 |
| **C. 子集 sound（L2）** | 对语言子集 S 的**已建模边** over-approx（S 违例则关闭承诺；**非**全生态可证明不漏） | 高风险改动 |
| **D. 验证轨（L3）** | 验证索引器/小语言核心的不变量 | 研究/高保证场景 |

**禁止**在 README/CLI 中把 B/C/D 写成「动态特性零漏报」。

---

## 1. 总体架构（四层共存，不推倒重来）

```
                    ┌─────────────────────────────────────┐
   源码变更事件 ──► │  Watcher (fsnotify) + debounce      │
                    └──────────────┬──────────────────────┘
                                   ▼
                    ┌─────────────────────────────────────┐
                    │  Incremental Indexer (L0 核心)      │
                    │  hash 增量 · 批量事务 · sid 重链    │
                    └──────────────┬──────────────────────┘
                                   ▼
         ┌─────────────────────────┼─────────────────────────┐
         ▼                         ▼                         ▼
   ┌───────────┐            ┌────────────┐            ┌────────────┐
   │ L0 Graph  │            │ L1 Dynamic │            │ L2 Subset  │
   │ 调用/导入 │  ──merge── │ 候选边     │  ──opt──   │ Sound 分析 │
   │ qualifier │            │ DI/反射    │            │ (子集 S)   │
   └─────┬─────┘            └─────┬──────┘            └─────┬──────┘
         └────────────────────────┼─────────────────────────┘
                                  ▼
                    ┌─────────────────────────────────────┐
                    │  Query API / CLI / MCP / SCIP export│
                    └─────────────────────────────────────┘
                                  ▲
                    ┌─────────────┴───────────────────────┐
                    │  L3: 形式化不变量 + 差分/属性测试   │
                    └─────────────────────────────────────┘
```

- **L0** 永远是默认查询路径（快、稳）。  
- **L1** 合并进图，但边带 `confidence` 与 `evidence`，默认 `impact` 可过滤。  
- **L2** 可选模式：`impact --sound` 或独立命令，结果更慢、更保守。  
- **L3** 不阻塞发版；作为质量门与研究轨。

---

## 2. L0 — 工程索引基线（现状 + 硬化）

### 2.1 目标

在 5k+ 文件仓库上提供稳定、可解释的结构查询；**已基本达标**（codex-rs 实测）。

### 2.2 能力清单（保持）

- 语言：TS/TSX/JS/JSX、Python、Go、Rust  
- 符号、调用、导入、`qualifier`（注解/接收者/`New*`/返回类型 define）  
- 增量 hash、描述保留、`resolved_symbol_id` 全量重链  
- `impact` 真 BFS；`importers`/`related`  
- SCIP protobuf 二进制 + 官方 CLI `lint` 门禁  
- CLI + MCP；空索引 fail-loud；MCP root jail  

### 2.3 L0 待硬化（进入 L1 前完成）

| ID | 项 | 验收 |
|---|---|---|
| L0.1 | **fsnotify 原生 watch**（替换/并存于轮询） | ✅ `watch_events` + CLI 默认；改文件后 < 300ms 触发增量；`notify` crate；防抖 50–200ms；`tests/watch_fsnotify.rs` |
| L0.2 | 大仓性能预算 | ✅ `scripts/bench_index.ps1`；fixture 增量门禁 |
| L0.3 | 边证据字段 | ✅ CLI `callers` 输出含 `at: path:line`；DB 本就存 path/line |
| L0.4 | 查询缓存层 | ✅ callers/impact 内存缓存 + 写失效 + hit/miss 计数；`tests/query_cache.rs` |
| L0.5 | 文档诚实 | ✅ README/AGENTS/PLAN 明确 L0 = best-effort 名字/qualifier；动态边属 L1 规划 |

### 2.4 L0 非目标

- 动态边、sound 保证、跨语言类型统一 IR  

### 2.5 L0 交付物

- `notify` 集成 + `tests/watch_fsnotify.rs`（TDD：写文件→图更新）  
- 性能基准脚本 `scripts/bench_index.ps1` / `benches/`  
- 更新中英文 README  

---

## 3. L1 — 动态召回与「可能边」（工程主战场）

### 3.1 目标

在 **不撒谎** 的前提下提高对动态/DI 的召回；所有新边可解释、可关。

### 3.2 边模型扩展

```text
ref {
  name, kind, path, line,
  qualifier?, module?, resolved?,
  confidence: Exact | Heuristic | DynamicCandidate,
  evidence: Vec<Evidence>,   // 规则 id + 源片段
}
```

- `Exact`：L0 现有语法确定边  
- `Heuristic`：DI/工厂模式  
- `DynamicCandidate`：反射/字符串/计算属性  

**默认查询策略：**

- `callers` / `impact`：默认含 Heuristic，可用 `--exact-only`  
- `impact --include-dynamic`：额外纳入 DynamicCandidate（噪声更大）  

### 3.3 各语言启发式（首批）

#### TypeScript / JavaScript

| 模式 | 规则 | confidence |
|---|---|---|
| `container.register(X)` / `bind(X).to(Y)` | 注册边 | Heuristic |
| 装饰器 `@Injectable` / `@Inject` | DI | Heuristic |
| `obj[methodName]` / `obj[`m${x}`]` | 动态调用候选 | DynamicCandidate |
| `new (registry[name])()` | 工厂 | DynamicCandidate |
| 事件总线 `on('click', handler)` | 订阅边 | Heuristic |

#### Python

| 模式 | 规则 | confidence |
|---|---|---|
| `getattr(obj, "foo")()` | 反射 | DynamicCandidate |
| `importlib.import_module` | 动态导入 | DynamicCandidate |
| `@inject` / FastAPI `Depends` | DI | Heuristic |
| `__init_subclass__` 子类注册 | 框架 | Heuristic |

#### Go

| 模式 | 规则 | confidence |
|---|---|---|
| 接口方法 + receiver 实现 / `var _ I = (*T)(nil)` | 实现边 | Heuristic |
| `map[string]Handler` / `e.GET(path, h)` 注册 | DI 表 / 路由 | Heuristic |

#### Rust

| 模式 | 规则 | confidence |
|---|---|---|
| `dyn Trait` + impl 集合 | 可能实现 | Heuristic |
| `inventory`/`linkme` 式注册 | 宏/链接注册 | Heuristic（可选） |

### 3.4 存储与迁移

- `refs.confidence TEXT`、`refs.evidence TEXT`（JSON）  
- 迁移：旧数据默认 `exact`  
- SCIP：DynamicCandidate **默认不导出**为 definition 链接（可 `--include-heuristic`）  

### 3.5 评测（L1 必做，否则只是玩具）

- **Corpus**：3–5 个真实框架项目（如 NestJS + Inversify、Spring-like Go、FastAPI）  
- **指标**：  
  - 相对 L0 的 **召回提升**（人工标注黄金边，或与运行时 trace 对齐）  
  - **噪声比**（Heuristic 中被人工判假的比例）  
- 报告：`docs/eval-l1.md`（数字，不是口号）  

### 3.6 L1 交付物

- 规则引擎（按语言模块化） ✅ `src/index/rules.rs`
- `refs` schema v2 + 迁移 ✅ `confidence` + `evidence`；旧库回填 `exact`
- CLI/MCP 过滤开关 ✅ `--exact-only` / `--include-dynamic`
- 评测 corpus + 报告 ✅ `fixtures/eval-l1/` + [docs/eval-l1.md](docs/eval-l1.md)
- 全部 TDD：每个规则先写 fixture 失败测试 ✅ `tests/l1_rules.rs` / `l1_schema.rs` / `l1_eval.rs` / `l1_cli.rs`

**M2 实测（fixture + 框架向 multi-module，见 eval-l1 / eval-l1-real）：** ts-di 与 nestjs-inversify 相对提升均 ≥15%；Heuristic 未匹配噪声代理 0%。非「生产 monorepo 完备」声明。

### 3.7 L1 非目标

- 声称 sound  
- 完整指针分析  

---

## 4. L2 — 子集上的 Sound Over-Approx

### 4.1 目标

对 **明确定义的语言子集 S**，提供 **可陈述、可测试** 的不漏保证：

> 在 S 内，凡运行时可能出现的调用边，静态图必包含（可能多报，不可漏报）。

### 4.2 子集 S（v1 建议）

**TypeScript/JavaScript 子集 S_js：**

- 无 `eval` / `new Function` / `with`  
- 无 `Proxy` 元编程  
- 模块边界清晰（ESM 静态 import 为主）  
- 动态属性访问 **仅限** 字符串字面量键  
- DI 仅限 **显式注册表**（我们已识别的模式）  

**Rust 子集 S_rs：** 无 `unsafe` 函数指针黑科技、无过程宏生成调用（或宏展开已接入）。  

文档：`docs/sound-subset.md` — 精确语法/语义排除列表。

### 4.3 分析技术（务实选型）

不自研完整指针分析；组合：

1. **类型约束传播**（已有 qualifier 扩展为完整约束图）  
2. **类/接口实现闭包**  
3. **显式注册表的闭包**（registry key → 实现）  
4. **字符串字面量键** 的有限域枚举  
5. 可选：对 S 子集做 **抽象解释** 的调用闭包  

输出：`impact --sound`（只走 Sound 边集合；DynamicCandidate 降级为「S 外警告」）。

### 4.4 验证 L2「sound」的手段（非形式化全验证）

| 手段 | 作用 |
|---|---|
| **差分测试** | 同项目用运行时 instrumentation（Node/Jest、Go test cover、Python coverage 钩子）收集真实调用，断言 ⊆ 静态边 |
| **属性测试** | 随机生成 S 内程序，解释执行 vs 图 |
| **金标准 corpus** | 手工标注 S 内全边 |
| **不变量单测** | 「每个 AST call_expression 在 S 内必有边」 |

**明确不承诺：** S 外零漏报。

### 4.5 L2 交付物

- `docs/sound-subset.md` ✅（S_js 保守 AST 扫描；S_py/S_go/S_rs **v1 保守词法扫描**，非完整冻结）
- `impact --sound` / `callers --sound` ✅ S 限定（已建模边；违例关闭承诺）
- 差分测试 harness ✅ Node export tracer + 多文件 ESM + **Go cover**（`tests/l2_go_diff.rs`）
- 属性测试 ✅ `tests/l2_property.rs`（确定性 S_js 生成器：Exact 边 + impact_sound 包含）
- Go/Py S 扫描器 ✅ `scan_go` / `scan_py`（unsafe/reflect/plugin、eval/exec/setattr/getattr 非字面量）
- 评测报告 `docs/eval-l2.md` ✅

**M4 实测（fixture 级）：** `s-js-auth` 运行时边 ⊆ `--sound` 边 100%；`s-js-evil`（eval）正确 `subset_ok=false`。非全生态 sound 声明。

### 4.6 L2 工期量级

月级（数人月），依赖 L1 规则与评测基建。

---

## 5. L3 — 形式化验证轨（研究，不挡发版）

### 5.1 目标

证明 **实现不变量** 与 **小语言核心** 性质，而不是「JS 生态 sound」。

### 5.2 可形式化命题（示例）

1. **I1 索引完整性（语法级）**  
   对 tree-sitter 可解析树，凡节点 kind ∈ DirectCall 集合，extract 必产出 ≥1 条 call ref（允许 qualifier 缺失）。  

2. **I2 增量安全**  
   文件 F 内容变为 C' 后，DB 中 F 的 symbols/refs 与 extract(C') 同构。  

3. **I3 BFS 完备（图语义）**  
   在有限图 G 上，`impact(s,d)` 返回集 = 深度 ≤ d 的可达调用者（按实现的推广规则）。  

4. **I4 小语言 soundness**  
   对无反射的小命令式语言 L（自研 IR），指针分析/调用闭包 over-approx 操作语义。  

### 5.3 工具选型

| 轨 | 工具 | 成本 |
|---|---|---|
| 属性/差分（已有） | proptest / 自研 harness | 低 |
| 不变量模型检查 | TLA+/Apalache 或 Alloy | 中 |
| 定理证明 | Lean 4 / Rocq 证明 I1–I3 或 I4 | 高 |

**建议路径：** 先 TLA+ 模型「增量索引 + sid 重链」；Lean 只做 I4 小语言，不绑主 CI。

### 5.4 L3 交付物

- `formal/` 目录：模型 + 证明脚本 + README ✅ `IncrementalIndex.tla` **TLC 无错**（568 states / 63 distinct，见 `tlc-results.txt`）
- CI：仅检查文件存在与文档，不强制定理编译（可选 nightly） ✅ `tests/l3_invariants.rs` + `tests/l4_mini_lang.rs`
- **I4** ✅ 可执行形式化：`src/formal/mini_lang.rs` 小语言（直接调用 + 字面量表派发），有界穷举/性质测试证明 runtime ⊆ static closure。**非** Lean/Rocq 定理证明。

**说明：** 主 CI 不跑 TLC/Lean；TLC 需本地 Java + `tla2tools.jar`（见 formal/README.md）。

### 5.5 L3 非目标

- 验证 tree-sitter  
- 验证 LLVM/浏览器引擎  
- 把 L1 启发式「证明」成 sound  

---

## 6. 跨层非功能需求

| 项 | 要求 |
|---|---|
| 性能 | **L0 查询 p95 &lt; 50ms**：5k 低扇入 p95≈0.08ms；**hot `run` ~4k fan-in 专用轨道** cold p95≈**35ms**（max≈49ms，仍 &lt;50）— [docs/eval-query-p95.md](docs/eval-query-p95.md) |
| 兼容 | 旧 index.db 自动迁移；SCIP 导出保持 `scip lint` 0 |
| 可观测 | 每次 index 输出：文件数、边按 confidence 分布、耗时 |
| 隐私 | L2/L3 不外传源码；enrich 仍可选 |
| 文档 | 中英文 README 与本计划同步；**禁止超售 sound** |

---

## 7. 里程碑与粗估

| 阶段 | 内容 | 粗估 |
|---|---|---|
| **M1** | L0.1 fsnotify + L0.3 证据 + L0.5 文档 | 1–2 周 |
| **M2** | L1 规则引擎 v1（TS DI + 反射字面量）+ 评测 corpus 起步 | 2–4 周 |
| **M3** | L1 Python/Go/Rust 启发式 + MCP/CLI 开关 + 报告 | 2–3 周 |
| **M4** | L2 子集文档 + `--sound` + Node/Go 差分 harness | 4–8 周 |
| **M5** | L3 TLA+ 增量模型；可选 Lean I4 | 持续，不挡版本 |

版本策略：M1 后可发 `v0.2`；M2/M3 后 `v0.3`；M4 后 `v0.4`（标注 experimental sound）。

---

## 8. 测试与 TDD 门禁

按 [AGENTS.md](AGENTS.md)：

1. 每条启发式：**先失败 fixture 测试**（输入源码 → 期望边 + confidence）  
2. 每条 sound 规则：**差分或金标准** 先红后绿  
3. 每次发版：`fmt` + `clippy` + `cargo test` + CLI E2E + `scip lint`  
4. L1/L2 评测数字进 `docs/eval-*.md`，README 只引用数字  

---

## 9. 风险

| 风险 | 缓解 |
|---|---|
| 启发式噪声拖垮 impact 有用性 | 默认过滤阈值；UI/CLI 展示 confidence |
| L2 范围膨胀成「重写 CodeQL」 | 子集 S 范围受控（S_js 保守 AST；S_py/S_go v1 词法）；新特性先进 L1 |
| fsnotify 在网络盘/Windows 抖动 | 回退轮询；debounce；集成测试 |
| 形式化空转 | L3 独立目录与里程碑；不设为 M1–M4 阻塞 |
| 文档再次「打脸」 | 发布前 doc 与实现对照清单（对抗审核流程） |

---

## 10. 成功判据（可验收）

1. **M1**：编辑文件 → MCP/CLI 在 300ms 内看到新符号（本地 SSD）。  
2. **M2**：至少 1 个真实 DI 项目上，Heuristic 召回相对 L0 提升 **≥15%**，人工抽检噪声 **≤30%**（数字以评测报告为准）。  
3. **M4**：在 S_js corpus 上，运行时 trace 边 ⊆ `--sound` 边 **100%**；S 外案例在文档中列明。  
4. **全程**：官方 `scip lint` 保持 0；CI 三平台绿；无「零漏报」虚假宣传。  

---

## 11. 下一步（建议立即执行）

1. 冻结本计划为 `PLAN.md`（本文）。  
2. ~~**M1 / L0.1**：TDD 实现 fsnotify watch。~~ **完成**  
3. ~~L0.2 / L0.4 / L0.3 / L0.5~~ **完成**（L0 硬化项）  
4. ~~启动 L1 规则引擎骨架 + 一条 TS DI 规则（TDD）。~~ **完成（M2/M3 规则面）**  
5. ~~L2：`docs/sound-subset.md` 已起草；实现 `impact --sound` + 差分 harness（M4）。~~ **实验性落地**（含属性测试、多文件 ESM；S_py/S_go 为 v1 保守词法扫描）  
6. ~~L3：TLA+ 增量模型（不挡发版）。~~ **收尾完成**：TLC 无错 + I1–I3 不变量 + **I4** 可执行小语言 containment（非 Lean）  
7. ~~验收缺口：Py `__init_subclass__`、Go 接口/路由、框架向 corpus、Go 差分、版本 tag。~~ **完成**（见 eval-l1-real / l2_go_diff；Lean I4 仍在 formal/TODO.md）  
8. ~~大仓评测。~~ **完成（私有量化仓）**：`stock-trading-app` ~995 源文件 / 20k 符号 / 167k refs，full index ~39s；`order` callers L0 7→L1 24。见 [docs/eval-large-repo.md](docs/eval-large-repo.md)（源码不入库）

---

*计划所有权：仓库维护者。变更需更新中英文 README 中的能力表述，并保留本节「禁止超售」原则。*
