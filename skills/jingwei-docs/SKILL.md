---
name: jingwei-docs
description: 经纬的文档与交付物规矩。当要改文档数字、写报告、打包、更新 MANIFEST 或说明书时使用。
---

# 文档与交付：数字真源、结构约束、可复现

## 数字

- 文档里的规模/测试数一律用 fact 标记（写法见 docs/ 里现成的 N 处），键必须先在 `tools/facts.json` 登记。
- 改数字只跑 `node tools/check_facts.mjs --write-docs`，**禁止手抄**。
- 新增数字：先加进 `tools/facts.json` 的采集逻辑，再写进文档。

## 结构约束（门禁守）

- `AGENTS.md` ≤260 行（目标 120）；单文件 ≤1000 行；`tools/*.mjs` 必须过 `node --check`。
- 文档里"声明 N 项"必须等于实际枚举数。
- 本地链接必须点得开。

## 交付

```powershell
powershell -File pack.ps1                 # → dist\jingwei-<版本>-win-x64.zip
node tools/smoke-pack.mjs --gui           # 解压真包 → index/positions/GUI 3 秒
```

只在用户说"收束打包"时打包；打包前 `JINGWEI_DELIVER=1` 跑门禁。

## 收尾

结论进仓库（债表/要点/决策记录），会话只留一句引用。`handover` 的"关键决策与理由"读的就是 `docs/decisions/`。
