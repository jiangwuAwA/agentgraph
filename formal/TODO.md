# agentgraph 待办 / Backlog

> 本文是**未完成事项**清单，不是能力承诺。  
> 已交付能力见 [PLAN.md](../PLAN.md)、[formal/README.md](README.md)、[docs/](../docs/)。

---

## L3 定理级 I4（Lean / Rocq）— 未开始

**状态：** backlog  
**优先级：** 研究轨，不挡发版  
**前置：** 已有可执行 containment（`src/formal/mini_lang.rs` + `tests/l4_mini_lang.rs`）

### 目标

对 mini-language **S_L**（直接调用 + 字面量表派发 + `if`，无反射/非字面量键）证明：

```text
∀ P ∈ Prog,  runtime_call_edges(P) ⊆ static_call_closure(P)
```

在 **Lean 4**（或 Rocq）中：形式语义 + 静态分析定义 + 机器核对的证明。  
**不是** JS/Python/Go 生态 sound 定理（见 PLAN §0.2 / L2 弱化承诺）。

### 范围 A（建议起步）— 对齐当前 I4

| 步骤 | 内容 | 粗估 |
|---|---|---|
| A1 | Lean 4 项目骨架（`formal/lean/`，lake） | 2–4 天 |
| A2 | 语法 + 大步（或小步）语义；与 Rust 解释器同一模型 | 1–2 周 |
| A3 | 静态闭包定义，与 `static_closure` 对齐 | 2–4 天 |
| A4 | 核心定理：仅 `Call` | 3–7 天 |
| A5 | 扩展 `DispatchLit` / `If` / 递归或燃料 | 1–2 周 |
| A6 | 文档：陈述 ↔ 代码对照；可选 CI nightly `lake build` | 1–2 天 |

**合计（有 Lean 经验）：约 2–5 周全职。**  
Lean 不熟时常见翻倍（**1–2 个月**）。

### 范围 B — 不在本 backlog 承诺内

- 对齐 L0 extract / qualifier / 跨文件 / DI 注册边的真实语义  
- 「agentgraph 全图 sound」定理  

量级：**数人月+**，属新研究课题，单独立项前需再写设计。

### 验收（范围 A）

- [ ] `lake build` 本地绿（不强制主 CI）  
- [ ] 定理陈述与 `src/formal/mini_lang.rs` 中语义/分析一一对应（README 对照表）  
- [ ] 明确写出 **非目标**：非 JS/TS/Py/Go、非 L2 `--sound` 升级  
- [ ] 不把可执行 containment 文案改成「已定理证明」直到 A 完成  

### 风险

| 风险 | 缓解 |
|---|---|
| 证明的是「另一门语言」 | A2 强制与 Rust 测试同一模型；对照表 |
| mathlib / Lean 版本漂移 | 钉版本；优先零 mathlib 或最小依赖 |
| 与产品叙事超售 | 文档继续区分：可执行 containment ≠ 定理级 |

---

## 其他未完成（摘要）

| 项 | 层级 | 备注 |
|---|---|---|
| 属性测试扩大到 S_py / S_go | L2 | 现有 generator 主要是 S_js |
| 多文件 ESM + DI 容器差分加强 | L2 | 注册≠dispatch 已文档化；框架派发边未建模 |
| `resolve_symbol_ids` 批量 SQL | L0 性能 | 全库审核 Minor m11，非 trivial |
| Lean/Rocq I4 | L3 | 见上文 |

---

## 变更记录

| 日期 | 内容 |
|---|---|
| 2026-09-16 | 建立 backlog；记录定理级 I4 范围 A 估算与验收（L3 可执行 containment / TLC 已交付） |
