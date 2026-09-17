# agentgraph 待办 / Backlog

> 本文是**未完成事项**清单，不是能力承诺。  
> 已交付能力见 [PLAN.md](../PLAN.md)、[formal/README.md](README.md)、[docs/](../docs/)。

---

## L3 定理级 I4（Lean）— 范围 A 已交付

**状态：** ✅ 范围 A 完成（`formal/lean/`，`lake build` 绿，零 `sorry`）  
**优先级：** 研究轨，不挡发版  
**前置：** 可执行 containment（`src/formal/mini_lang.rs` + `tests/l4_mini_lang.rs`）

### 已交付（范围 A）

| 步骤 | 内容 | 状态 |
|---|---|---|
| A1 | Lean 4 项目骨架（`formal/lean/`，lake，stdlib only） | ✅ |
| A2 | 语法 + 归纳语义；与 Rust 同一模型（对照表见 [lean/README.md](lean/README.md)） | ✅ |
| A3 | 静态直边 + `Reaches` 传递闭包 | ✅ |
| A4–A5 | 定理 `runtime_subset_static`：Call + DispatchLit + If | ✅ |
| A6 | README 对照表 + 非目标声明 | ✅ |

主定理：`MiniLang.runtime_subset_static` — 每个 runtime call edge 都在静态传递闭包内。

### 仍未做（范围 A 残留 / 可选）

- [x] 可选：GitHub nightly `lake build`（主 CI 仍不要求 Lean）— `.github/workflows/lean.yml`
- [ ] 可选：燃料/深度截断与归纳 `Enters` 的形式化桥接（当前由 Rust 穷举测试覆盖有界情形）

### 范围 B — 不在承诺内

- 对齐 L0 extract / qualifier / 跨文件 / DI 注册边的真实语义  
- 「agentgraph 全图 sound」定理  

量级：**数人月+**，属新研究课题，单独立项前需再写设计。

### 风险（持续）

| 风险 | 缓解 |
|---|---|
| 证明的是「另一门语言」 | 对照表 + Rust `tests/l4_mini_lang.rs` 同一模型 |
| mathlib / Lean 版本漂移 | 已钉 `v4.34.0`；零 mathlib |
| 与产品叙事超售 | 文档继续区分：可执行 containment ≠ L2 生态 sound |

---

## 其他未完成（摘要）

| 项 | 层级 | 备注 |
|---|---|---|
| 属性测试扩大到 S_py / S_go | L2 | 现有 generator 主要是 S_js |
| 多文件 ESM + DI 容器差分加强 | L2 | 注册≠dispatch 已文档化；框架派发边未建模 |
| `resolve_symbol_ids` 批量 SQL | L0 性能 | 全库审核 Minor m11，非 trivial |
| S_py/S_go AST 化（替换 lexical v1） | L2 | 承诺已按语言分层；扫描器本身仍可加强 |
| Rust S 扫描器深度 | L2 | `scan_rust` 仍偏行级；若需可再开诚实性 pass |

---

## 变更记录

| 日期 | 内容 |
|---|---|
| 2026-09-16 | 建立 backlog；记录定理级 I4 范围 A 估算与验收（L3 可执行 containment / TLC 已交付） |
| 2026-09-17 | Lean 范围 A 落地：`formal/lean/` + `runtime_subset_static`；`lake build` 绿 |
| 2026-09-18 | 夜间 Lean CI：`.github/workflows/lean.yml`（schedule + paths 过滤；不挡主 Rust CI） |
