---
name: jingwei-gate
description: 运行并解读经纬门禁（tools/gates.ps1）。当需要跑门禁、门禁红了、判断能否提交、或要写性能/规模数字时使用。
---

# 经纬门禁：跑、读、红不提交
## 何时用 / 不用

- 用：收口、交付、改了解析/索引/图谱、要写"多少秒/多少条"的数字。
- 不用：只改文档措辞且不涉及数字；纯讨论。

## 命令（固定）

```powershell
cd D:\projects\jingwei
powershell -NoProfile -ExecutionPolicy Bypass -File tools\gates.ps1
```

门禁有哪些、各守什么，看 `tools/gates.ps1` 的结尾表（不在这里抄数量）。

## 读结果

1. 只认结尾表里的 failed 数。2. 红了先看 `FAIL` 那道对应的脚本，不重跑整轮。
3. 交接口径：门禁表 + `git status` + "新判据改之前确实红过"的输出。

## 铁律

- **红不提交**。红着提交 = 事故，记 `docs/process/事故账本.md`。
- **先红后绿**：新测试/新判据必须先证明它能红。
- **数字不手抄**：改文档数字用 `node tools/check_facts.mjs --write-docs`。
- **性能数字要可复现**：带命令与对照（`tools/regress.mjs`），没有对照不写结论。

## 失败处理

同一项检查**连续红两次** → 停，写卡点（`docs/reference/债表.md` + `docs/process/要点-<日期>.md`），不许靠改数字/放宽判据过。

## 省 token

只看结尾 10 行；细节按 FAIL 行去读对应脚本输出，不整段粘贴日志。
