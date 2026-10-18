# AGENTS.md · 经纬（jingwei）

> **入口，只回答两件事：现在什么状态、下一步做什么。**
>
> | 想知道 | 读哪份 |
> |---|---|
> | 该按什么想 | `框架.md`（§九 = 2026-10-18/19 长出来的四条方法） |
> | 怎么动手（判据 + 流程 + 省流） | `SOP.md`（S29–S32 是新的） |
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

**8 道**：单测+集成（<!-- fact:tests_total -->423<!-- /fact --> 条：engine <!-- fact:engine_unit -->351<!-- /fact --> /
集成 <!-- fact:integration -->53<!-- /fact --> / model <!-- fact:model -->11<!-- /fact -->，另 <!-- fact:engine_ignored -->1<!-- /fact --> ignore）·
文档测试 · clippy 零警告 · 工程门禁 · 外壳 <!-- fact:shell_tests -->79<!-- /fact --> · 交付 · 界面（真窗口自报 DOM）· 真源。

**数字只信 `tools/facts.json`**；改数字用 `node tools/check_facts.mjs --write-docs`，**别手抄**。

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
| **扫盘真能用**：多根、边扫边出、可中断、有终态 | `jingwei discover` + `src-tauri/src/scan.rs` + `ui/scan.js` | `discover --depth 3 D:\projects D:\research` → **186 目录 / 133 项目 / 0.3s**；界面门禁 3 条绿 |
| **首页按类型×体量分组**（可折叠） | `ui/scan.js::groupFound` + `.grp-head` | 界面门禁：`heads ≥ 2` 且终态说"扫完" |
| **tauri 索引崩溃修复**（trait 默认方法） | `graph/parse.rs` | 整个 tauri **329 文件 / 5,916 符号 / 3.2s**；单测 347 绿 |
| **项目指纹**（Merkle：相对路径+内容哈希，排除 vendored） | `graph/…`→`understand/fingerprint.rs` | 实机 133 项目里认出 **2 组重复**（3 份 + 2 份克隆）；3 条单测 |

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
| **信息复用/检验层**（内容哈希 + 解析器版本 + 结果哈希） | 二次启动不重算；扫描/抽取结果可复用 |
| **Python 的"使用"没进图谱** | `graph roles` 对 Python 只是线索 |
| **LSP 只到 definition** | 找引用 / hover 还没有 |

## 6. 要什么读什么（**别一次读进去**）

| 想知道 | 读哪份 | 多大 |
|---|---|---|
| 该按什么想 | `框架.md` ★ | 560+ 行 |
| 怎么动手 | `SOP.md` ★ | 840+ 行 |
| 我错过什么 / 教训 | `docs/process/事故账本.md` | 800+ 行 |
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

## 8. 工具速查

```
门禁      powershell -File tools\gates.ps1
单测      cargo test --workspace --all-targets
静态      cargo clippy --workspace --all-targets -- -D warnings
外壳      node tools\uitest.mjs
界面      node tools\uitest-window.mjs
扫盘      jingwei discover --depth 3 D:\projects D:\research   （或界面「全盘扫描」）
索引      jingwei index
真源      node tools/collect_facts.mjs --check
打包      powershell -File pack.ps1   → dist\jingwei-<版本>-win-x64.zip
健康      node tools/health.mjs --once
看本钱    node tools/ctx-cost.mjs     （每轮注入成本）
```
