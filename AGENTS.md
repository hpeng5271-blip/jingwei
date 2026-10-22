# AGENTS.md · 经纬（jingwei）

> **入口，只回答两件事：现在什么状态、下一步做什么。**
>
> | 想知道 | 读哪份 |
> |---|---|
> | 该按什么想 | `框架.md`（§九 = 2026-10-18/19 长出来的四条方法） |
> | 怎么动手（判据 + 流程 + 省流） | `SOP.md`（S35 是总表；S29–S34 是细则） |
> | 现在什么状态 / 欠什么 | **本文件** + `docs/reference/债表.md` |
>
> **每轮开工先问**：这一轮引入「我之外」的信息了吗？没有 = 整理，不是验证。
> **每轮收尾**：结论进仓库（`债表.md` / `docs/process/事故账本.md`），会话只留一句引用。
>
> 维护：**120 行是目标，260 是上限（门禁守）**。长出来的去 `docs/{process,reference,design}`。

## 0. 三十秒定位

- **是什么**：终端与 IDE 之间的**理解层**——把「这个项目到底是什么状态」讲清楚，需要时才讲。价值 = 过滤 + 反馈。
- **代码**：`D:\projects\jingwei`（唯一活跃仓库；`D:\projects\pylaunch-rs` 只读）。
- **技术栈**：Rust（`jingwei-model` / `jingwei-engine` / `jingwei` CLI / `jingwei-app` Tauri）+ 静态 `ui/`（无打包器）。
- **真实瓶颈**：**闭环只提高自洽度，不提高真实性；裁判权必须交给环路外面**（`框架.md` §二）。

## 0.5 工作节奏（2026-10-20 用户定的）

- **不默认打包、不写详细报告**；只有用户说"收束打包"（用来验证分工是否可行）才打包。
  收束打包时带 `JINGWEI_DELIVER=1` 跑门禁（交付门禁会严格检查"包比源码旧"）。
- **不做定时报告**（2026-10-21 用户去掉计划任务）：没有"每 12 小时"的聚合任务，需要报告时人会说。
- 干活途中**只记要点**（一行一条：时间 + 做了什么 + 数字/结论），不求精确、不重复验证。
- 结论仍进仓库（债表/事故账本/要点），会话只留一句引用。

## 1. 门禁：工具，不是仪式

| 什么时候跑 | 怎么用 |
|---|---|
| 收口 / 交付 | 跑全量，拿**结尾那张表**当证据 |
| 回头看 | 当底线检查：已发生的失效模式有没有复发 |
| 发现新失效模式 | **增补一条**进去——门禁数量 = 事故数量 |

日常一轮**不跑**全量。跑的时候：

```powershell
cd D:\projects\jingwei
powershell -NoProfile -ExecutionPolicy Bypass -File tools\gates.ps1
```

**流程图**：`SOP.md` S35 是一张 8 行的总表（开工/决策/执行/验收/学习/监督/熔断/入库），细则见 S29–S34。

**4 道**（v1 收口，其余挂起）：单测+集成（<!-- fact:tests_total -->589<!-- /fact --> 条：engine <!-- fact:engine_unit -->392<!-- /fact --> /
集成 <!-- fact:integration -->138<!-- /fact --> / model <!-- fact:model -->11<!-- /fact -->，另 <!-- fact:engine_ignored -->1<!-- /fact --> ignore；
文档测试并入这道）· clippy 零警告 · 真源 · 交付。
工程门禁 / 外壳 / 界面门禁**挂起**（理由见 `docs/MANIFEST-v1.md` §3），要跑仍可单跑 `node tools/gate.mjs` 等。

**数字只信 `tools/facts.json`**；改数字用 `node tools/check_facts.mjs --write-docs`，**别手抄**。

**数字判据**（2026-10-20 加）：性能/规模数字必须带**复现命令**与**对照基线**（`tools/regress.mjs` / `bench/`）；跑分没有对照，不得写进结论。

**验收三样**（缺一不算）：① 门禁结尾表 ② `git status` ③ **新判据在改之前确实红过的输出**。

## 2. 四条不许破的线（抓手 + 守它的测试）

| 线 | 抓手 | 测试 |
|---|---|---|
| 数据正确优先 | `store::apply_file_update` 四步事务 | `a_bad_row_rolls_back_the_whole_batch` |
| 不生成修复命令 | `fix.rs` | `fix` 6 条 |
| 无自查不报错 | `rules.rs::enforce_evidence` 三道闸 | `an_error_without_a_self_check_is_downgraded` |
| 关系必有来源 | `Edge::new` 的 `source` 必填 | `illegal_node_pairs_cannot_be_constructed` |

**三态是核心**：是 / 不是 / **查不了**；`unknown` 永不许显示成"没有"。

## 3. 当前状态

| 里程碑 | 状态 |
|---|---|
| M1 领域模型 / M2 SQLite / M3 解析 / M4 增量 / M5 一条龙接图谱 / M6 外壳 | ✅ 完成 |

**2026-10-19 收口的三件**：

| 事 | 落点 | 证据 |
|---|---|---|
| **跨文件模块属性**（2026-10-20） | `parse_python` 模块级常量 + `resolve` 模块属性 | `from pkg import cfg` + `cfg.DEBUG` → `references` 边；flask 符号 **1,922**、positions **1922/1922**；`EXTRACTOR_VERSION=parse@11` |
| **Python 的"使用"层补全**（2026-10-19 夜） | `parse_python` + `graph/link.rs` + `registers` 边 | `__main__` 块内调用 → File→Symbol；装饰器注册 → `registers`（flask **82 条**）；类体常量 → 符号（flask 1,580→**1,920**）；`Cfg.DEBUG` / `self.x` 的 `references`（flask **67 条**，债 0.32 已修）；positions **1920/1920** |
| **LSP：定义 / 引用（精确到调用点）/ hover / 诊断推送** | `lsp/mod.rs` + `edge_sites` 小表 | 真协议往返：引用落在 `cmd_graph.rs:491`（真调用行）；hover 出"是什么/在哪/被 2 个符号调用"；**didOpen 后主动推诊断**；**rename 返回 WorkspaceEdit**（判不准不发编辑）；单测 7 条 |
| **Python 的"使用"进图谱**（File → Dependency 的 `depends_on`） | `graph/index.rs` + `python_deps` 测试 | flask：**183 条**依赖边（83 文件）；抽样 3 条回文件核对全对；本地模块与 `self/super` 已排除 |
| **依赖对账**（清单 vs 锁文件，两边都是外部真值） | `understand/deps_check.rs` + `jingwei deps` | tauri：**10 条声明 / 10 对**；engine crate：9 条 / 8 对 / **1 判不了**（path 依赖——判不了就说判不了）；4 条单测 |
| **位置回读**（用原文当裁判核图谱行号） | `graph/verify.rs` + `jingwei positions` | 经纬自己：**1878 对 / 0 错 / 7 位置不准**（修闭包行号前是 **300 错**）；正例+反例测试 |
| **扫描结果持久化**（下次打开不必从头扫） | `understand/scan_cache.rs` + `scan.rs` | 实测：第一次跑落盘 **133 项目 / 266 目录 / 跳过 63**；第二次启动立刻有卡片；坏缓存当没有（4 条单测） |
| **扫盘真能用**：多根、整盘、**并行**、边扫边出、可中断、有终态 | `jingwei discover` + `understand/dirs_index::build_parallel` | 整机 C:+D: → **423 项目**；**首条结果 0.8s**（≤5s 判据）；总时长 **6s 热 / 146s 冷**（2026-10-19 改前实测；2026-10-20 指纹改按需后**未复测冷态**）；`--why` 报跳过原因 |
| **首页按类型×体量分组**（可折叠） | `ui/scan.js::groupFound` + `.grp-head` | 界面门禁：`heads ≥ 2` 且终态说"扫完" |
| **tauri 索引崩溃修复**（trait 默认方法） | `graph/parse.rs` | 整个 tauri **329 文件 / 5,916 符号 / 3.2s**；单测 347 绿 |
| **早期截断**（内容变了但解析结果没变 → 图不重写） | `graph/index.rs` + `store::file_results` | 实测：只加一行注释 → `skipped_same_result=1`、图逐字节相同；再跑 **处理 0**；测试 3 条等价性全绿 |
| **项目指纹 + 同源检测**（Merkle + Jaccard≥0.8） | `understand/fingerprint.rs` + `discover --similar` | 实机 133 项目：**2 组完全相同**、**5 对同源**（含 `jingwei\src-tauri` ↔ `pylaunch-rs\src-tauri` **88%**）；5 条单测 |

## 4. 分工（别越界）

| 角色 | 干什么 | 不许 |
|---|---|---|
| **人** | 出题、定判据、当裁判 | 写重复胶水 |
| **DSH** | 方案、读码、验收、维护本文件 | 没跑门禁就说完成 |
| **Codex** | 改文件、跑构建、修编译、对照复核 | 改判据 / 为过门禁改数字 |
| **学习子智能体** | 按任务类型查一手来源 → ≤20 行要点+出处 | 只做浅层搜索、复述背景 |

**人的话优先于本文件**。

## 5. 欠什么（明细在 `docs/reference/债表.md`）

| 现在最该做的三件 | 一句话 |
|---|---|
| **冷缓存**（0.13 → 0.27） | 并行已上；合并 `read_dir` 试过无收益已撤；**已做（2026-10-20）**：`jingwei watch`（RDCW 内核）+ `discover --journal`（只扫变过的项目根；没记录/overflow 明说退回全量） |

## 6. 要什么读什么（**别一次读进去**）

| 想知道 | 读哪份 | 多大 |
|---|---|---|
| **不知道读哪份** | `docs/README.md` | 索引（现行 vs 史料） |
| 该按什么想 | `框架.md` ★ | 560+ 行 |
| 怎么动手 | `SOP.md` ★ | 840+ 行 |
| 我错过什么 / 教训 | `docs/process/事故账本.md` | 800+ 行 |
| **我犯过的错，浓缩版** | `docs/reference/失效模式清单.md` | 60 行 |
| 决策与判据 | `docs/design/设计.md` | 1400+ 行 |
| 数据层 | `docs/design/数据层设计.md` | 545 行 |
| 真项目实测 | `docs/process/研究对象.md` + `研究对象实测.json` | —— |
| 操作手册 + skill | `docs/reference/工作方法.md` | 362 行 |
| 便携边界 | `docs/reference/工具链-便携与安装.md` | 94 行 |
| 下一个该做哪条 | `docs/reference/债表.md` | —— |

**读代码**：先 `jingwei index` 看图谱，别通读猜结构。

## 7. 省流与设备（硬约束，2026-10-19 加）

1. **一次会话只办一件能决定的事**；结论能一句话说完的，不许占半屏。
2. **>500 行**交给子智能体摘要，只要结论 + 行号。
3. **命令必须有超时**：跑门禁/界面测试一律 `WaitForExit(N 秒)`，超时判卡死并杀。
4. **跑界面前后做进程审查**：先 `Get-Process jingwei-gui | Stop-Process -Force`，结束后复查残留 = 0。
5. **设备健康**：重活（全量编译 / 扫全盘）前后 `node tools/health.mjs --once`；编译用 `CARGO_BUILD_JOBS=4`。细节见 `SOP.md` S30。
6. **硬决策才用 pro**：不可逆/连续失败 2 次/人点名 → 子智能体跑 `deepseek-v4-pro`，上下文走 `tools/llm/digest.mjs`+`slice.mjs`（≤2K+8K），一次约 ¥0.01；日限 60 次/¥15（`record.mjs` 记账）。**决策归 pro、执行归 flash；先写 `docs/decisions/` 再动手；`limits.mjs check` 超限即停。**

## 8. 工具速查

```
门禁      powershell -File tools\gates.ps1
单测      cargo test --workspace --all-targets
静态      cargo clippy --workspace --all-targets -- -D warnings
外壳      node tools\uitest.mjs
界面      node tools\uitest-window.mjs
扫盘      jingwei discover --depth 3 D:\projects D:\research   （或界面「全盘扫描」）
索引      jingwei index
交接      jingwei handover   （接手人一页纸：能不能跑/规模/热点/下一步/关键决策与理由）
解释      jingwei explain <名字|symbol:id>   （定义在哪、被调/被引用几处）
真源      node tools/collect_facts.mjs --check
打包      powershell -File pack.ps1   → dist\jingwei-<版本>-win-x64.zip
包冒烟    node tools\smoke-pack.mjs [--gui]   （解压真包 → index + positions；--gui 再加"GUI 起来 3 秒没退"）
健康      node tools/health.mjs --once
看本钱    node tools/ctx-cost.mjs     （每轮注入成本）
```
