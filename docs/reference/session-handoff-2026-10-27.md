# 会话交接（2026-10-27 夜 → 新会话）

> 单一真相：本文件 + `HANDOFF.md` + `.pro/exp/out/`。新会话先读本文件，再读 `docs/reference/sop.md`。

## 0. 角色与硬规则（用户反复强调）
- **pro 全权决策**：`D:\projects\jingwei` 的子 pro（agent id `59b15948-56ec-4789-b40f-ea21e753ac0a`）派任务、定验收；我执行；**所有结果先报 pro**，红就停报等派发；绿一行回报。
- **低流低输出**：不复述、不写长报告、每次只回一行结论 + 关键数字。
- **SOP**：`D:\projects\jingwei\docs\reference\sop.md`（§8 安全操作规范为硬约束）。
- **安全（窗口风暴事故后）**：同时只允许 1 个 GUI/浏览器；自动化禁止开默认浏览器；`jw web` 默认不开浏览器（`--open` 才开）；按 PID 树回收并复查为 0；禁循环重弹；GUI 由用户手动双击，本人可人工开**一次**观察。

## 1. 目录与角色
| 路径 | 角色 |
|---|---|
| `D:\projects\jingwei` | **引擎（唯一分析源）**：datacore + collect + cli(jw) + engine(新) + lsp(计划) |
| `D:\projects\jingwei-shell` | **产品壳**：archive 的 Tauri v2 整份复制（含 ui/ 与 src-tauri/），旧库 `.jingwei` 已移走为 `.jingwei-archive` |
| `D:\jingwei-archive-20261024\jingwei` | **只读参考**：0.3.0 原样，禁止修改 |
| `D:\projects\demo` | 另一份 0.3.0 参考（只读） |

## 2. 最终产品方向（用户 2026-10-27 定）
- **底座/UI/UX/交互 = `jingwei-shell`（archive 的 Tauri 壳）**，原样拿过来，不重写 UI 风格。
- **引擎整块替换**：删掉 shell 旧内核，换 `D:\projects\jingwei` 的 `datacore`+`collect`，通过新 `crates/engine` 门面接；"脱胎换骨"，不是冻结旧内核。
- **画图全砍**：graph tab / `graph.js` / canvas / rough（T1 已完成）。
- 然后**把决策功能全部做进去**，最后**优化打磨**。
- 六页：`home / run / editor / note / machine / clean`（graph 不留占位）。

## 3. 引擎侧现状（jingwei）
- 版本 `jw 0.4.0 (rules 27)`；`cargo test --workspace` **213 passed**；`b94` OK。
- 能力：Γ⊢J 三态 + 证据/边（use/mod/impl/import/test/call）、card/edges/impact/tests-for/cite/find/check（12 项）。
- LSP 只读六能力：信任门（用户级 trust.json + audit）、诊断(pull)、hover、definition、references、inlayHint、codeAction（只展示）；`lsp_in_history=0`；RA 1.98.1。
- `crates/engine` **骨架已建**（T2.2 绿）：`crates/engine/{Cargo.toml,src/lib.rs,src/view.rs}`，依赖 datacore/collect/serde_json，workspace 已注册；`view.rs` 仍是空壳（下一步搬 card）。
- 基线：`.pro/exp/out/t2-baseline.txt`（card SHA `74E6425F…`/`C9350B96…`/`42088312…`，warnings=124，`--version` 同上）。
- 发布物：`dist\jingwei-v0.4.zip`（4,615,774B，sha `ACDDE81C…F03D8`，含 `bin\jingwei-gui.exe`）；`m14.json` 同步；**GUI 运行未验**。

## 4. 壳侧现状（jingwei-shell）
- T1 **已完成**：`ui/graph.js` 删除；`index.html`/`events.js` 去 graph tab/区块/调用；`data-kind="deep_graph"`→`deep_link`；`ui/` 内 `graph|rough` grep=0。
- `cargo build --manifest-path src-tauri/Cargo.toml` **EXIT=0**（40.99s）；`cargo build -p jw` EXIT=0。
- 旧内核仍在（graph.rs 等），按 T5 顺序逐个替换后再删；**不许先删成红树**。
- 壳依赖：Tauri v2 + `tauri-plugin-dialog`（无 shell/opener）；capabilities 待收窄（T2）。

## 5. 待办（pro 已派，按序；每步 build 绿 + 静态 grep + 单测，红即回滚停报）
- **T2.3**：把 `cli/src/main.rs:2172 card_value` + 直接依赖的最小私有助手**原样搬**到 `engine::view`（签名 `pub`，逻辑/字段不改）；cli 只留 `use engine::view::card_value;`；验收 3 fixture `card --json` SHA 3/3 等、213 绿、`--version`/warnings 不变。
- **T2.4**：只读命令面接线（`Engine::open`、`module_map`；stats/check/files/read_file/projects/machine 后续）。
- **T3**：shell `src-tauri/Cargo.toml` 加 `engine` path dep；新增 `engine_*` 命令与旧命令并存；UI 先不动。
- **T4**：六页逐个切真数据：home/projects→stats/projects；editor→files/read_file/card/edges/problems；machine→machine；clean→check；note→docs 空态；run→终端（后置）。
- **T5**：删旧内核（`graph.rs → scan/projects/machine/notes/run`，每删一个 build 一次）；最终 `src-tauri/src` 只剩 main/lib/commands/trust/lsp。
- **T6**：补 impact/tests_for/cite/problems + LSP（抽 `jingwei/crates/lsp` 共用；trust/source 契约不变）。
- **T7**：单实例 + deny-list + 窗口数证据；允许人工开一次 GUI 观察（开前清 `%TEMP%\jingwei-gui-windows.txt`，只开一次，关窗后 PID 树=0，窗口>1 立即停报）。
- **T8**：打磨 + `0.4.1-rc` 打包（tauri build；dist 只带 gui/CLI/docs，不带 bench/docs-shots/skills；sha 与 m14/b94 同步；标"GUI 运行未验"）。

## 6. 决策功能清单（P0/P1/P2，pro 定稿）
- **P0**：判定三态+证据/边+卡片；四槽窗口+单层跳转（history≤2）；LSP 只读链；GUI 六页每页≥1 真数据；**GUI 单实例 + 外部进程 deny-list + 恰好 1 窗口**。
- **P1**：终端底栏 + cwd 跟随；布局/会话恢复（localStorage 已有，部分完成）；行内 git blame；外部改动自动重载；键盘选择/复制；命令面板搜索。
- **P2**：鼠标组合键；终端点路径开文件；scrollback 搜索。
- **砍**：graph/canvas/rough、rename/apply edit、跨仓、联网、Tauri 额外插件。

## 7. 研究结论（外部需求，10 条）
scrollback 搜索(5)、会话/布局恢复(5)、行内 git blame(5)、LSP 诊断呈现(3)、命令面板搜索(3)、鼠标组合键(2)、新标签继承 cwd(2)、外部改动重载(2)、终端点路径(2)、键盘选择复制(2)。证据在 `.pro/exp/research/features*.json|md`。

## 8. 已知风险 / 坑（都踩过）
- **GUI 多窗口**：archive UI 的 `open_*`/`pick_folder`/`launch_project` 会拉起外部程序；shim/命令层必须 **deny-list**；单实例互斥（回环端口 7960）；窗口数写 `%TEMP%\jingwei-gui-windows.txt`。
- **RA 冷启动先 null/空数组**：hover/diagnostics/hints 都要短轮询（hover ≤5s、diag ≤8s、hints ≤8s）。
- **RA inlayHint range 越界** → RA 回 error：必须 clamp 到文件真实行数。
- **codeAction 需 `codeActionLiteralSupport`**，否则 nil。
- **Windows `canonicalize` 的 `\\?\`** 前缀 → `file://` 非法，需剥；RA 需要 `HOME/RUSTUP_HOME/CARGO_HOME` 白名单缺省。
- **LSP 测试并行会互抢 RA**：`static LSP_RA_LOCK` 串行 + 结束 `taskkill /T` 杀树 + 断言 0 残留。
- **PowerShell**：`.ps1` 纯 ASCII；`$ErrorActionPreference` 别用 Stop（cargo stderr 会中断）；长命令易被中断，用后台 job。
- **不要先删成红树**；先复制/并存 → build 绿 → 切换 → 删旧。

## 9. 证据与命令
- `cargo test --workspace`（213）；`node tools/web/smoke.mjs --shell`（11/11，headless）；`smoke --lsp-*`；`b94_docs_consistency.py`；`scripts\check-split.ps1`；`scripts\package.ps1`。
- 证据目录：`.pro/exp/out/`（m14.json、t2-baseline.txt、*-package.txt、gui-windows 证据）。

## 10. 新会话第一步
1) 读本文件 + `sop.md`；2) 跑 `cargo build -p jw` 与 shell `cargo build` 确认双绿；3) 向 pro 报一行状态，领 T2.3；4) 按 T1→T8 持续推进，每步一行回报；红停报等派发。

## 11. Skills 与 SOP 工作流（新会话接续用）

### 11.1 每次任务前先载入对应 skill
| 任务 | skill | 用途 |
|---|---|---|
| 任何会话开始/接管/报告 | `agent-contract` | 1 pro + 1 flash、省流、原子改动、证据与三态 |
| 证明对/错/没漏、跑验收 | `experiment-discipline` | 黑盒优先、不变量、红可复现 |
| 窗口/布局/槽位/隐藏恢复 | `window-slots` | 四槽固定、z-order、持久化 |
| 跳转/定义/引用/返回 | `jump-model` | 替换式 preview、单层 origin、history≤2 |
| LSP 信任门/降级/source 契约 | `lsp-gate` | 未信任 0 spawn、不写历史、skip 语义 |
| 加 UI 验收/跑 smoke | `smoke-harness` | headless DOM、SKIP≠PASS、截图新鲜度 |
| 评审规格/证据分级 | `spec-review` | Reject/Defer、convention 只作目标值 |
| 改文档数字/打包 | `jingwei-docs` | 版本/数字同步、dist 规矩 |
| 跑门禁/判断能否提交 | `jingwei-gate` | gate 三仓、性能数字口径 |
| 查仓库结构/符号 | `jingwei-graph` | 用 jw 自己的图谱回答 |
| 改完测试刷新事实 | `facts-refresh` | 数字与文档同步链条 |

### 11.2 SOP 工作流（pro 派发循环）
1. **pro 派一步**（任务号 + 改哪里 + 验收命令 + 红则怎样）。
2. 我**只做这一步**：先复制/并存 → build 绿 → 切换 → 删旧；不许先删成红树。
3. 跑验收：`cargo build` / `cargo test -p …` / 3 fixture SHA / `--version` / warnings / 静态 grep / 单测。
4. **绿**：一行回报（任务号 + 关键数字），等下一步。
5. **红**：立即回滚该步（git checkout 或反向编辑），**一行报 pro 原始错误**，停手等派发；不自行修、不并行下一步。
6. 用户手动验收前不自动开 GUI；人工开一次须：清 `%TEMP%\jingwei-gui-windows.txt` → 单实例 → 记窗口数 → 关窗 → PID 树=0；>1 窗口停报。

### 11.3 常用命令
```
cargo build -p jw
cargo build --manifest-path D:\projects\jingwei-shell\src-tauri\Cargo.toml
cargo test --workspace
node tools/web/smoke.mjs --shell
python .pro\exp\b94_docs_consistency.py
powershell -File scripts\check-split.ps1
powershell -File scripts\package.ps1
```

## 12. 全部硬性要求（用户原话口径，逐条遵守）

### 12.1 子智能体数量
- **最多 2 个子智能体**：1 个 pro（决策/派发/裁定）+ 1 个 flash（机械活）。主会话执行，不额外开子智能体。
- 不允许并行开多个 pro/flash；需要更多算力时先问 pro，不许自行扩。

### 12.2 当前工作模式
- **pro 全权管理、持续派发**：pro 派一步 → 我做一步 → 一行回报 → pro 派下一步；**不停下来等用户确认**，只有红才停。
- **低流低输出**：一行结论 + 关键数字；不复述、不长篇。
- **一次只做一个任务**；不许并行改两个模块；不许"顺手"做别的。
- **先换后删**：复制/并存 → build 绿 → 切换 → 删旧；**禁止先把树弄红**。
- 每步验收固定：build 绿 / 相关测试绿 / 3 fixture SHA / `--version` / warnings 不增 / 静态 grep。

### 12.3 工作要求
- 所有结论必须有证据（命令 + 数字 + 文件路径）；没跑过就写"未验证"。
- 红即回滚该步、一行报 pro 原始错误、停手等派发；不许自行"再试一次"或换方案。
- 发布验收必须含**用户路径**（双击/一条命令能出界面），不能只验 CLI。
- 文档数字与证据同源（m14/b94），版本/sha 每次打包同步。
- 安全：不自动开浏览器/GUI；单实例；PID 树回收复查 0；`.ps1` 纯 ASCII；仓库不写信任库。
- 不自行加功能、不升版本、不重打包、不改 archive。

### 12.4 犯过的错（全部要避免重演）
1. **窗口风暴**：反复 `start.cmd`/`jw web` 自动开浏览器 + 多控制台，`taskkill /T` 打在已退出父进程上，`jw` 残留继续拉浏览器，害用户强制重启。→ 现规则：自动化禁开浏览器、`jw web` 默认不开、单实例、按 PID 树回收。
2. **把"CLI 验证过"当"用户能用"**：0.2 只发 CLI，用户双击 exe 打不开；发布验收缺用户路径。
3. **擅自加需求/加功能**（`jw web` 半成品、wry GUI 等）未先报 pro，导致返工。
4. **文档数字漂移**：README 写 128，实际 206/213；m14 `gate.fail=true` 与 `fail_count=0` 自相矛盾。
5. **未验证就宣称完成**：`smoke --shell` 前就说六页可用；`lsp_real` 初版空实现。
6. **测试并行互抢 RA**：LSP 真 RA 测试并行跑 3 条红；后加串行锁 + 杀树 + 残留断言才绿。
7. **破坏性测试环境**：把 `PATH` 砍成 System32-only 导致 Chrome 起不来，截图 0 字节，差点误判产品。
8. **注释被转义拆行**把 `main.rs` 编译弄红（python 写 `\r\n` 进 `//` 注释）。
9. **长时间命令被中断**（cargo test/build），造成"状态未知"；应改后台 job + 日志。
10. **越权解释用户意图**（"Mn 重构"、是否重写引擎），反复返工；应先报 pro 确认。

### 12.5 新会话开场自检
- [ ] 读本文件 + `docs/reference/sop.md`（含 §8 安全）
- [ ] `cargo build -p jw` + shell `cargo build` 双绿
- [ ] 向 pro 报一行状态，领 T2.3
- [ ] 确认：≤2 子智能体、一次一步、红停报、不开 GUI 自动化

## 12.6 pro 确认版（2026-10-27，以此为准）

**子智能体**：上限 **2**（1 pro + 1 flash）；主会话只执行、不再额外开；一次性调研批次必须用户明确批准，结束即回收。

**错误清单（合并为 8 条，按根因排序）**
1. **越权解释用户意图**（根因）
2. **未验就宣称完成**（含"自动 smoke 当用户验收"；子项：**静默降级/静默 null 当通过**（shim warn 事件））
3. **CLI 可用 ≠ 用户路径可用**
4. **窗口/外部进程风暴**（无单实例、无回收）
5. **擅自加功能/扩范围**
6. **文档数字与证据不同源**
7. **测试环境自造失败**（PATH 破坏 + RA 并行互抢，合并）
8. **先删/先改导致红树**（新增：未先建基线 SHA 就改；"长命令被中断"不算过错，移到环境说明）
