# 分析能力补课清单 v1

> 生成：2026-10-27。依据：`lsp-client.json` 的 A01–A06、`features.json` 中 `lsp_related` 项、以及当前 DB 的实测差距。
> 数字口径：`jw edges --stats --json`、`jw coverage --json`；三仓 = rg / tauri / nushell。

## 0 裁决原则

1. **能用 LSP 标准能力且本机可复现 → 委托**，自研只做兜底与解释层。
2. **jw 独有语义（judgment/edge/impact/原因码）→ 自研或混合**，LSP 只做裁判。
3. 每个能力必须有 **fallback** 与 **可跑验收**；没有 fallback 的委托项不许进 v1。

## 1 六项裁决

### A. 类型 / 泛型信息
- **裁决**：委托 LSP（A01）。
- **理由**：自研没有类型系统；做类型推断的投入/风险远高于收益，猜错比"未知"更糟。
- **fallback**：显式"类型未知"，0 次猜测；卡片 `type` 字段写 `"未知"`。
- **最小验收**：`jw lsp probe --cap hover --fixture fx/types --json` → 10/10 条 `type` 非空；无 server → 10/10 `"未知"`，推断次数断言为 0。
- **当前差距证据**：judgment 无 type 字段；`card --json` 无类型信息；无 server 时没有任何"未知"标注（缺）。

### B. 作用域（局部变量/遮蔽/hoisting）
- **裁决**：混合——LSP definition 做裁判，自研 use 绑定表继续维护（A03/A05）。
- **理由**：自研 call 只到"文件级绑定 + 唯一名"，块级遮蔽会静默指错；但全量交 LSP 会丢掉 jw 的批量/离线能力。
- **fallback**：自研绑定表（`(file, use.leaf) → dst`）+ 未解析计数，不猜。
- **最小验收**：fixture 5 个遮蔽用例（同名变量/局部 use/嵌套块）→ 绑定命中 ≥4/5，跨文件误配 0；`jw edges --kind call --json` 对每个 fixture 行给出 `unresolved` 或正确 dst。
- **当前差距证据**：绑定表只按文件聚合，不区分块；call 解析在无绑定时退"全仓唯一名"（1456/2630 已解析，1174 未解析；tauri 1599/4380；nushell 10724/31650）。

### C. 跨 crate 名解析
- **裁决**：混合——LSP 裁判 + 自研原因码（A02）。
- **理由**：workspace 全局 module map 会把同名模块当本地（`module_shadow`），需要 crate 边界与外部 crate 判定；LSP 能给真值，但代价高。
- **fallback**：只报原因码：`crate_root_alias / module_shadow / glob / crate_item_missing / path_attr_missing`，`other=0`、`sum==total`。
- **最小验收**：`jw coverage --json` 三仓 `unresolved_use.reasons`：`sum==total`、`other==0`；抽 50 个跨 crate 样本与 LSP 对照，precision ≥95%，未解析 100% 带 reason。
- **当前差距证据**：`?use` = 0 / 85 / 186；tauri 85 条中 `crate_item_missing 41 / glob 22 / module_shadow 22`；`crates` 级 module map 仍可能跨 crate 遮蔽（无 crate 边界字段）。

### D. 引用精确度（references / 谁用到我）
- **裁决**：混合——先修自研边，再用 LSP references 做金标准（A03）。
- **理由**：自研 use/call 边是 go-to/impact 的基础，直接换 LSP 会失去离线与批量；但自研缺类型时必然有误报。
- **fallback**：自研 use/call 边 + 显式覆盖率（resolved/unresolved 必显示）。
- **最小验收**：随机 100 条引用，对照 LSP references：差异 ≤2 条；差异逐条进 `accept-new`（不许静默过滤）；`jw edges --stats --json` 的 `call.resolved+unresolved==total`。
- **当前差距证据**：use 1039/4415/20954 已解析；call 未解析率 44.6% / 63.5% / 66.1%；无 LSP 对照，故当前 precision 未知（只有"文件里确有该 fn"的弱校验）。

### E. 增量索引
- **裁决**：自研（A04），按 `文件+版本` 重解析；全量重建只作兜底。
- **理由**：LSP 的增量是它自己的内存态，不能替代 jw 的持久索引；jw 的价值在可重建、可回溯。
- **fallback**：全量重建；≥5s 时切后台并显示进度。
- **最小验收**：改 1 个文件后 `jw parse`：重解析文件数 ≤1、wall ≤5s（fixture 仓），且 `check` 十一项仍 0；`rules` 变更才允许全量。
- **当前差距证据**：`parse_rules_version` 变化 → `clear_edges` + 全量；无 per-file LSP 版本号；大仓 nushell 全量 parse 约 40s 级。

### F. 诊断
- **裁决**：混合——自研只做语法（A06），语义诊断交 LSP。
- **理由**：iw 自研语义诊断会猜错；语法诊断便宜且可离线；LSP 诊断带 range/code，可直接渲染。
- **fallback**：无 server → 只显示语法诊断；语义问题不显示、不猜。
- **最小验收**：语法 fixture 20 例全中（括号/字符串未闭合）；语义错误自研断言数 0；`jw lsp probe --cap diagnostics` 返回 `source∈{lsp,db}`。
- **当前差距证据**：当前无 diagnostics 通道，只有 `check` 的一致性计数；`card`/`serve` 无 Problems 数据。

## 2 依赖顺序

1. **F 语法诊断 + C 原因码**（离线可做，不依赖 LSP，先把"为什么不知道"说清）。
2. **B 作用域 + D 引用**（先自研绑定表/边，再用 LSP 抽样裁判）。
3. **A 类型 + LSP 全能力**（依赖 G1 信任门与 `source` 契约，最后接）。
4. **E 增量**贯穿：先保证"文件+版本"可重解析，再接 LSP 通知。

## 3 与 v1 的边界

- v1 只承诺：语法诊断、原因码、引用/调用"已解析/未解析"可见；类型与语义诊断在 LSP 可用时展示、不可用显式"未知"。
- 不承诺：跨 crate 100% 解析、trait 动态分发、宏展开后的引用、跨仓分析。


## 7 LSP 接入后的状态（2026-10-27）

| 能力 | 之前 | 现在 | 仍缺 |
|---|---|---|---|
| 类型/泛型 | 无 | RA hover（拉取到 `type`，取不到写"未知"） | 批量类型索引（逐点 hover，不做全库） |
| 作用域 | 自研 use 绑定 | 混合：definition/references 由 RA 裁判 | 块级遮蔽仍以自研为准（call 边不重算） |
| 跨 crate 名解析 | 自研唯一名 | 混合：定义跳转走 RA，调用图仍自研 | 调用图不依赖 LSP（离线可用），跨 crate 调用仍 `?call` |
| 引用精确度 | 低（同名/绑定） | **高**（RA references，去重排序） | 每次需 LSP；无 server 时回落到低精度 |
| 增量索引 | 全量 parse | 不变（LSP 侧有自己的分析缓存） | jw 自身仍全量；M15+ 可考虑增量 |
| 诊断 | 无 | RA diagnostics（pull） | codeAction 仅展示，不 apply |
