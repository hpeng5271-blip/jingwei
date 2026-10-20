---
name: jingwei-graph
description: 用经纬自己的图谱回答"这个项目/符号是什么状态"。当需要了解一个仓库结构、找入口、查符号定义/调用、做交接时使用。
---

# 经纬图谱：索引 → 位置 → 解释 → 交接

## 流程（按顺序，每步都有判据）

```powershell
cd <项目根>
jingwei index            # 建/增量更新图谱
jingwei positions        # 用原文回读行号：0 错才算可信
jingwei graph stats      # 节点/边/站点按 kind 计数
jingwei explain <名字>   # 定义在哪、被调/被引用几处、判定依据
jingwei handover         # 交接一页纸：能不能跑/规模/热点/下一步/决策理由
```

## 判据

- `positions`：`× 对不上` 必须为 0；`? 查不了` 可以非 0，但要说明原因。
- `explain`：要么给出来源（文件:行 / 边数），要么写"查不了"。
- `handover`：每节要么有来源，要么明说"查不了"，不许编。

## 红线

- **不许通读源码猜结构**——先 index，再按需读片段。
- **unknown 不许写成"没有"**：没解析到 ≠ 不存在。
- 改了解析器：`EXTRACTOR_VERSION` 要 bump，旧图会重建。

## 输出

结论一句话 + 支撑数字（文件/符号/边）+ 用到的命令。别贴整张图。
