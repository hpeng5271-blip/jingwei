# Skills 索引（经纬相关）

> 位置：`C:\Users\cjz\.dsh\skills\<name>\SKILL.md`；本文件只做索引，正文以 skill 为准。
> 用法：任务匹配描述时先 `skill <name>` 载入，再按 checklist 干活。

| skill | 解决的问题 | 关键验收 |
|---|---|---|
| `window-slots` | 四区槽位/布局/z-order/隐藏恢复/持久化 | `node tools/web/smoke.mjs --windows --jump` |
| `jump-model` | 替换式 preview、单层 origin、history≤2、防嵌套 | `smoke --jump --lsp-def --lsp-ref` |
| `lsp-gate` | 信任门、source 契约、降级、不写历史 | `smoke --lsp-*` + `jw check`（12 项=0） |
| `smoke-harness` | 页面自测、截图新鲜度、SKIP≠PASS | 全 flag smoke |
| `spec-review` | 证据分级、Reject/Defer、冲突审计 | `b94_docs_consistency.py` + `smoke --theme` |

## 与既有 skill 的关系
- `agent-contract`：协作硬规则（谁决策、谁执行、报告格式）。
- `experiment-discipline` / `core-experiment`：实验与证据协议。
- `facts-refresh` / `jingwei-gate`：改数字/跑门禁的固定链条。
- 本组 5 个是**经纬产品自身的不变量**，属于实现约束，不替代上面两组方法论。
