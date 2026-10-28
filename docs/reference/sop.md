# SOP：经纬交付工作流（2026-10-28 版，主会话即 pro）

> 生效：2026-10-28。**违反本文件的改动一律回退。**
> 配套：`handoff-2026-10-28.md`（断点与决策）、`jingwei-final-design.md`（目标态）。
> 一句话：**pro 决策与验收，flash 执行与取证，一刀一证，红即回滚。**

---

## 0 环境前置（每次新会话开始先核）

| 项 | 期望 | 命令 |
|---|---|---|
| 工作目录 | `D:\projects\jingwei`（引擎）、`D:\projects\jingwei-shell`（壳） | `pwd` |
| 进程干净 | `jingwei-gui` / `jw` / `rust-analyzer` = 0 | `Get-Process jingwei-gui,jw,rust-analyzer -EA SilentlyContinue` |
| git 可用 | `git --version` ≥2.40；`user.name/email` 已设 | `git config --global user.name` |
| 推送凭据 | `credential.helper=wincred` 可用 | `git -c credential.helper=wincred ls-remote origin` |
| 代理 | 仅在 push 时设 `HTTPS_PROXY=http://127.0.0.1:23333` | `Test-NetConnection 127.0.0.1 -Port 23333` |
| 基线备份 | `.pro/bak/usable-shell-20261027/` 存在 | `Test-Path` |
| 门禁脚本 | `scripts/check-split.ps1`、`parity-check.ps1`、`scripts/review.ps1` 存在 | `Test-Path` |
| 临时证据 | `.pro/exp/out` 可写；`%TEMP%\jingwei-gui-windows.txt` 已清 | — |

任一项不满足 → 先修环境，别开刀。

---

## 1 角色与边界

| 角色 | 谁 | 能做什么 | 不能做什么 |
|---|---|---|---|
| **pro** | **主会话（主 agent）** | 拆活、定判据、验收、抽样复验、改 SOP/设计稿 | 不直接改代码（除文档）；不跳过验收放行 |
| executor | 1 个 flash 子智能体 | 改代码、跑命令、截图、写证据文件 | 不设计、不扩范围、不自行换方案 |
| 转达 | — | 派给其它子智能体时由 executor **原文转达** | 不许二次创作任务书 |

上限：**1 pro + 1 executor**；需要更多算力先由 pro 找用户。

---

## 2 每刀 SOP（12 步，缺一步不算完）

**开始前**
1. `git -C D:\projects\jingwei log --oneline -3` + 看 `.pro/exp/out` 最新文件 → 确认不重复做已完成项。
2. 用一句话写清本刀：**改哪一处、期望行为变化是什么、判据是什么**（写进回报首行）。
3. 备份将被改的文件（`.pro/bak/<刀名>/`），记 sha256。

**动手**
4. **只改这一处**：不顺手格式化、不改测试、不改名、不加注释；diff 目标 ≤150 行。
5. 加字段/改契约时**先加字段再换 UI**（先让旧逻辑继续用，编译绿后再切换）。
6. 删除类改动：先 `grep` 引用面 → 打印将删行范围 → 确认无活引用 → 再删（工具 `.pro/exp/del_fn.py`）。

**验证（预算内）**
7. **≤2 条 cargo 命令**：1 编译（`cargo check -p <包>` 或 `build -p jw`）+ 1 定向测试（`cargo test -p <包> <过滤>`）。
8. 需要跑界面：**1 次**开窗（流程见 §7），用 `.pro/exp/cdp_step.py` 自动点，截图落盘。
9. 生成本刀证据：`review-<刀>.md`（诊断数 + 关键日志 ≤20 行 + 截图路径），必要时 `parity-<页>.txt`。

**收尾**
10. 指标核对：见 §4 阈值表；数字变化必须在回报里**一句解释**。
11. 一 commit：`<type>(<范围>): 一句话` + body 三行（为什么 / 改了什么 / 证据文件）。
12. 报 pro **≤3 行**：`刀名 | 关键数字 | 证据文件`；等 pro 回"过/退"，**过才进下一刀**。

---

## 3 证据规范

| 类型 | 命名 | 必须含 |
|---|---|---|
| 审查杯 | `review-<刀>.md` | 诊断数（files/errors/warnings/exit）、关键日志 ≤20 行、截图路径 |
| 对账 | `parity-<页>.txt` | 旧值 vs 新值逐项、差异清单（应为空） |
| 测试 | `<刀>-test.txt` | 整份日志（口径：求和 passed，不许手工数前 N 条） |
| 截图 | `<页>-<动作>-<ts>.png` | 时间戳在文件名或 `Get-Item` 输出里 |
| 开窗 | `s2-open.txt` / `t7-*.txt` | PID、`windows=N`、`argv`、关窗后进程数 |

放置：`.pro/exp/out/`（不进发布包）。日志/截图**必须是本次生成**，旧图不算证据。

---

## 4 门禁与阈值（每次收尾核对）

| 项 | 阈值 | 口径 |
|---|---|---|
| 单元/集成测试 | **≥264 passed / 0 failed** | `cargo test --workspace --all-targets`，整份日志求和 |
| warnings | **≤109**（debug）/ release 0 | `check-split.ps1` 的 `: warning` 计数 |
| 页面 parity | **12/12**（8 页结构断言） | `parity-check.ps1` |
| 自审诊断 | **0 errors**（warnings 允许存在） | `jw lsp-check <改过的文件>` |
| 开窗 | `windows=1`、`jingwei-gui` 1→0、`jw` 0 | `%TEMP%\jingwei-gui-windows.txt` + `Get-Process` |
| 单实例 | 第二实例 `exit=2`、无第二窗口 | 回环 7960 + `tag=second-instance-blocked` |
| 提交 | 每刀 1 commit、body 三行 | `git log --oneline -1` |
| 数字变化 | 必须一句解释 | 回报内 |

---

## 5 红处理与回滚

1. **红了先停**：不重跑、不换方案、不"再试一次"。
2. 判定归属：代码错 / 测试错 / 环境错（并发、进程占用、RA flaky）。
3. 回滚顺序（三选一）：
   - 只改了一个 commit → `git revert --no-edit HEAD`（保留历史）；
   - 未提交 → `git checkout -- <files>` 或从 `.pro/bak/<刀名>/` 回拷；
   - 大范围坏了 → 回 `usable-shell-20261027` 基线（`src-tauri/src` + `ui` + 两个 Cargo.toml）。
4. 回滚后**复跑一次**最小判据确认树绿，再报 pro：`原始错误原文 + 回滚范围 + 复跑结果`。
5. 同一刀失败 3 次 → 停手，pro 改判据或拆刀。

---

## 6 提交与推送

- 提交：`<type>(<范围>): 一句话`；type ∈ feat/fix/refactor/test/docs/chore。
- body 三行：`为什么 / 改了什么 / 证据`（证据写文件名，不写"已测试"）。
- 推送：`$env:HTTPS_PROXY='http://127.0.0.1:23333'` + `GIT_TERMINAL_PROMPT=0` + `git -c credential.helper=wincred push`。
- **best-effort**：失败记 HANDOFF"push 待重试"，**继续下一刀**，不重试超过 2 次，不阻塞。
- 禁止：token 写进 URL/文件；`--no-verify`；把 `target/`、`dist/`、`.pro/bak/`、`*.db`、`*.exe` 提交进库。

---

## 7 开窗与进程纪律（事故后硬约束）

事故背景（2026-10-27）：反复启动 `jw web` + 自动开浏览器 + 多控制台，`taskkill /T` 打在已退出父进程上，`jw` 残留再拉浏览器，窗口叠到无法关闭。

规则（违反即停手）：
1. **同时只允许 1 个** GUI/浏览器/控制台测试进程；启动前记 PID，结束后复查 0 再下一步。
2. 自动化**禁止打开默认浏览器**；需要网页验收用 headless + 独立用户数据目录，且一次一个。
3. 自动化**禁止用 `start.cmd`**（含 pause、会开浏览器）；它只用于用户人工双击。
4. 每次 spawn 必须能按 PID 树回收（`taskkill /T /F /PID`），并复查进程与端口；回收失败 → 停手报告，不再起新窗口。
5. **禁止循环 spawn**、禁止"再试一次"式重开；开窗预算是**次数**，不是时间。
6. 关窗必须走 `CloseMainWindow()`（= 点 X），**不许用 `Stop-Process` 代替**（会绕过"关窗退出"逻辑）。
7. 强杀 GUI 会留 `jw` 孤儿（Job Object 未做，R7）→ 验收期**禁止强杀**。
8. 每次开窗：清 `%TEMP%\jingwei-gui-windows.txt` → 记录基线进程 0 → 启动 → 3 秒内取证 → 操作 → 关窗 → 2 秒后复查 0。
9. 自动点用 `.pro/exp/cdp_step.py`（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`），UIA 点不到 WebView 内容。
10. 收工清场：`jw` / `jingwei-gui` / `rust-analyzer` / 端口 7960 / 临时证据文件，全部确认 0。

---

## 8 例外上报（只有三类可以停）

1. **必须人本人做**：真机双击验收、提权/装东西、网络凭据；
2. **会破坏用户数据或既有文件**：删库、删用户目录、覆盖 `D:\projects\jingwei-gui.exe`；
3. **与已定 spec 直接冲突且无法最小改动绕开**。

其余一律自决，写一句理由进 HANDOFF，继续。

---

## 9 HANDOFF 更新（每刀一条）

- 位置：`HANDOFF.md` 顶部"当前段"（不是文末）。
- 一条一行：`<刀名> | 关键数字 | 证据文件 | 备注(坑/偏差)`。
- 偏差、降级、缺口**必须写**；不写等于隐瞒。
- 里程碑（七页全绿 / 打包 / 回滚）另起一段，含数字与待办。

---

## 10 命令速查

```powershell
# 编译与测试（预算内）
cargo check -p engine
cargo build -p jw
cargo test -p jw <过滤词>
cargo test --workspace --all-targets        # 每 3–4 刀一次

# 自审与审查杯
jw lsp-check <文件…>                         # 0 error 才算过；有 error 退出码 1
powershell -File scripts\review.ps1 -Files "a.rs,b.rs" -Run "jw scan ." -Shot "x.png"

# 对账与门禁
powershell -File .\parity-check.ps1
powershell -File scripts\check-split.ps1

# 开窗取证（先确认进程 0）
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
Start-Process .\target\debug\jingwei-gui.exe -ArgumentList 'D:\projects\jingwei'
python .pro\exp\cdp_step.py state
(Get-Process jingwei-gui).CloseMainWindow()

# 提交与推送
git add -A; git commit -m "feat(x): …" -m "为什么… / 改了什么… / 证据…"
$env:HTTPS_PROXY='http://127.0.0.1:23333'; git -c credential.helper=wincred push
```

---

## 11 Pitfalls（每条都踩过，详见 `handoff-2026-10-28.md` §7 与设计稿 §14）

`String::truncate` 非字符边界 panic；`count("<表>")` 白名单外恒 0；INNER JOIN 吞悬空行；`read_dir` 无越界检查；tree-sitter `links` 只能一版；用 sha256 不用 mtime；cargo 并发假红；状态只进 HANDOFF；Tauri 参数 camelCase；UIA 点不到用 CDP；`jw serve` 首行问候要跳过；`.cmd` 需 `cmd /C call`；`.ps1` 存带 BOM；删函数连属性行一起切。

**SHA 口径**：一律原始字节（`subprocess` 捕获 bytes → sha256）；**禁用** `Out-File -Encoding utf8`（带 BOM 会造成假红与误回滚）；基线必须"同命令连跑两次一致"才可复现。

---

## 12 与用户沟通（低频，别刷屏）

- **默认不打扰**：pro 自行推进；用户已明确"不用找我"。
- **只有四类才找用户**：① 需要人本人操作（双击验收/提权/凭据）；② 破坏性操作；③ spec 冲突无法绕开；④ 里程碑（七页全绿 / 打包 / 需要方向决策）。
- 找用户时**一次说完**：现状数字 + 卡点 + 两个可选项（各自代价），不写过程叙事。
- 汇报给用户用"结论先行 + 数字 + 证据路径"，不用"我觉得/大概"。

---

## 13 会话交接触发（什么时候重写 handoff）

| 触发 | 动作 |
|---|---|
| 上下文轮数 ≥12 或剩余 <40% | 更新 `handoff-2026-10-28.md`（现状/断点/决策/风险），新会话按它接管 |
| 用户要开新会话 / 换主 agent | 同上，并在 `HANDOFF.md` 顶部加指针 |
| 里程碑完成（七页全绿、打包、大回滚） | 在 HANDOFF 追加一段（数字 + 待办 + 坑） |
| 出现新的硬约束（安全/合规/工具链） | 先写 SOP（本文件），再改代码 |

交接必须包含：**断点（下一刀是什么）、判据、数字基线、回滚点、未决项**——四缺一不算交接。
