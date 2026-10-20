# v1 最小完备（2026-10-20，pro 定稿）

> 目的：**能跑通 + 人能接手**。v1 期间**不新增功能、不追外部研究、不扩 UI**。
> 判据一律可执行；做不到就写"查不了"。

## 1. 必须有的能力（6 条）

| # | 能力 | 判据（怎么算有） |
|---|---|---|
| 1 | `jingwei index`（全量/增量/截断） | 对 flask 跑出「83 文件 / 1,922 符号」；再跑一次「处理 0」 |
| 2 | `jingwei handover` | 一页纸六节齐全，每节有来源或写"查不了" |
| 3 | `jingwei explain <符号>` | 给出定义位置 + 调用/引用数 + 判定依据；查不到就说"图谱里没有" |
| 4 | `jingwei positions` | 对不上必须为 0（flask / tauri / 本仓库） |
| 5 | `jingwei rename` | 判不准时**不发编辑**；能写时回读通过 |
| 6 | `jingwei discover` + `watch` | 整盘扫有终态；journal 空就明说退回全量 |

## 2. 明确砍掉/挂起（6 条）

| 挂起 | 为什么 |
|---|---|
| `deps` 对账 | 非核心；依赖闸（只降不升）已能防膨胀 |
| `graph roles` 细分 / `graph stats` 扩展 | 理解层不靠统计页 |
| LSP hover/诊断打磨 | 已有，不再投入 |
| `guide` 多语言规则 | 只保 Rust / Python 判定 |
| `discover --similar` 同源检测 | 冷扫完成后挂起 |
| Tauri / UI 壳 | **v1 = CLI**；界面挂起 |

## 3. 管理结构（最少集合）

- **门禁 4 道**：单测+集成 · clippy · 真源 · 交付。文档测试并入单测；工程门禁/外壳/界面门禁**挂起**（UI 挂起后无意义）。
- **skill 3 条**：`jingwei-gate` / `jingwei-graph` / `jingwei-triage`；其余挂起（目录保留，registry 标 `deprecated`，不安装）。
- **文档只留 6 份**：`AGENTS.md` / `框架.md` / `SOP.md` / `设计.md` / `数据层设计.md` / `债表.md`；
  其余（process 史料、复审、真输入、今日复盘、标注、总稿）**停更并归档**。`使用说明.md` 是唯一的用户向文档。

## 4. 用户接手（3 个动作 + 1 份文档）

```powershell
jingwei index        # 建图谱
jingwei handover     # 一页纸：能不能跑 / 规模 / 热点 / 下一步 / 决策理由 / 文件状态
jingwei explain <名字>  # 这个符号是什么、在哪、谁在调
```

只读 `使用说明.md`（3 步上手 + 命令表 + 读哪看哪）。

## 5. 下一步（每件 ≤1 小时）

1. 本页写进 MANIFEST（已做）——验收：每条带一句判据。
2. 门禁收口到 4 道——验收：4 道全绿、交付门禁过。
3. `使用说明.md` 重写成"3 步上手"——验收：照文档跑 3 条命令能到 handover 页。

## 6. 版本与产物

- 版本 `0.3.0`（三处一致）；产物 `dist/jingwei-0.3.0-win-x64.zip`。
- 打包冒烟：`node tools/smoke-pack.mjs --gui`（v1 挂起 UI 后可去掉 `--gui`）。
