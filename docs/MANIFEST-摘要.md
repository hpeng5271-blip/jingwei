# 文档索引（摘要层）· 2026-10-22

> **这一页只花 ~1.5k tokens**：每条一句话 + 最新数字 + 更新时间。**命中哪条再去读原文**。
> 谁维护：改文档的人顺手改这一行（`tools/check-links.mjs` 会校验这里的链接没断）。

## 先看这些（要动手 / 要判断）

| 文档 | 一句话 | 数字 / 状态 | 更新 |
|---|---|---|---|
| [`AGENTS.md`](../AGENTS.md) | 入口：节奏、红线、命令、读什么 | 846 tokens/轮 | 10-22 |
| [`docs/design/界面约定.md`](design/界面约定.md) | 加界面前对一遍的八条规矩，每条指到函数与断言 | 8 条 | 10-22 |
| [`docs/reference/失效模式清单.md`](reference/失效模式清单.md) | 我犯过的错，浓缩版 | 18 条 | 10-22 |
| [`docs/reference/债表.md`](reference/债表.md) | 下一个做哪条；每条带验收口径 | 0.13–0.46 | 10-22 |
| [`docs/decisions/`](decisions/) | 已拍板的决策（决策归 pro，执行归 flash） | 10 份 | 10-22 |
| [`docs/reference/操作细节.md`](reference/操作细节.md) | 节奏 / 门禁细则 / 分工 / 设备约束 | 搬自 AGENTS | 10-22 |
| [`docs/reference/里程碑-2026-10.md`](reference/里程碑-2026-10.md) | 历史里程碑与实测量 | 10 条 | 10-22 |

## 判断与方案（读之前先想清楚为什么读）

| 文档 | 一句话 | 大小 |
|---|---|---|
| [`框架.md`](../框架.md) | 该按什么想（§九 = 四条方法） | ~560 行 |
| [`SOP.md`](../SOP.md) | 怎么动手；S35 是总表 | ~906 行 |
| [`docs/design/设计.md`](design/设计.md) | 决策与判据全集 | ~1529 行 |
| [`docs/design/总稿.md`](design/总稿.md) | 设计总稿（与上面重叠，读一份即可） | ~925 行 |
| [`docs/design/数据层设计.md`](design/数据层设计.md) | 数据层：schema、事务、投影 | 545 行 |

## 过程与史料（只在复盘时读）

| 文档 | 一句话 |
|---|---|
| [`docs/process/工作汇报-2026-10-22.md`](process/工作汇报-2026-10-22.md) | 今天：55+ 提交、每刀证据、明确没做的三项 |
| [`docs/process/工作汇报-2026-10-21.md`](process/工作汇报-2026-10-21.md) | 昨晚的交付级小结 |
| [`docs/process/事故账本.md`](process/事故账本.md) | 800+ 行，逐次事故 |
| [`docs/process/复审记录.md`](process/复审记录.md) | 1700+ 行，复审流水（最贵的一份） |
| [`docs/process/要点-2026-10-21.md`](process/要点-2026-10-21.md) | 流水账（已加“这篇算什么”横幅） |

## 现行数字（只信 `tools/facts.json`）

- 测试：<!-- fact:tests_total -->600<!-- /fact --> 条（engine <!-- fact:engine_unit -->396<!-- /fact --> / 集成 <!-- fact:integration -->145<!-- /fact --> / model <!-- fact:model -->11<!-- /fact -->）
- 外壳断言：<!-- fact:shell_tests -->131<!-- /fact --> 条；文档数字标记：90 处（脚本会校验）
- 性能（10-22 17:35 实测）：index 548 · handover 537 · doctor 382 · next 83 · **file 42** ms；GUI 31 MB
- 包：`dist\2026-10-22\`（gui + cli + 使用说明），`smoke-pack --gui` 通过

## 怎么用这一页

1. 先看“先看这些”那 7 行，够不够判断？
2. 不够 → 按关键词在下两张表里找一行，读原文；
3. 读完之后：**如果原文该改，改完顺手改这一行**（一句话 + 数字 + 日期）。
