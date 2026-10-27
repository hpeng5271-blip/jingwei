# 经纬 UI 规格 v1.1

> 生成：2026-10-27。v1.1 把 P0 改为 6 条基本盘（代码视图/搜索/诊断/跳转/终端/布局恢复），原 P0 六条移到 P1（P1 共 16 条），新增审查基本盘与 Reject/Defer。
> 证据分级：A = 一手 issue/讨论/发布说明且与需求直接对应；B = 单源或引用在评论/二手。`confidence` 只评**需求强度**。
> **px/ms 全部是目标值**（无外部出处），需 fixture 校准后才可写成验收阈值。审查基本盘（§7）的证据待第 4 个 flash 补齐，标 `TBD`。

## 1 P0（6 条基本盘）

第一版必须先是"编程工作台"，不是终端。P0-a/P0-b 为定义性能力；F01/F11/F15/F16/F26/F38 全部移到 P1。

| # | 需求 | keys | px（目标值） | ms（目标值） | 验收 |
|---|---|---|---|---|---|
| P0-1 | 代码视图（只读编辑器：行号/高亮/诊断波浪线/面包屑/当前符号） | 0（方向键滚动；`Ctrl+G` 定位行） | 行号栏 48px；行高 20px；字号 13px；波浪线 2px | 首屏 ≤200ms；滚动帧 ≤16ms | `smoke --code`：行号与内容逐行对齐；1 条诊断出波浪线；面包屑 3 段；滚 1000 行掉帧 ≤2 |
| P0-2 | 搜索一切（文件/符号/测试/文档，`/` 唤起，结果带路径+行+verdict） | 2 keys（`/` + 输入） | 结果行 24px；路径省略中段；命中高亮 2px | 输入→结果 ≤120ms P95 | 5 类查询各 3 条：结果含 `path:line` 与 verdict；空查询不发请求 |
| P0-3 | 诊断面板（LSP 行内 + 底部 Problems，可跳转、可过滤） | 1 key（`Ctrl+Shift+M` 开 Problems） | 底部行 24px；行内波浪线 2px | 首屏诊断 ≤500ms（LSP 目标值） | fixture 3 条错误全显示；点击跳转到行；severity 过滤后计数正确 |
| P0-4 | 定义/引用跳转（替换 preview、单层 origin、面包屑、返回栈 ≤2） | 1 click（Ctrl+Click 定义）；1 key（`Shift+F12` 引用） | preview 不改变主布局；面包屑 3 段 | ≤200ms | `smoke --jump`：history≤2；Esc 回 origin；焦点/滚动复原 |
| P0-5 | 底栏终端（cwd 三模式跟随、新 tab 继承、危险命令确认、失败降级） | 1 key（Ctrl+反引号）；2 keys（新 tab） | 默认高 320px；min 160px；max 60% | 切换 ≤100ms；首提示 ≤600ms | 见 §2 的 8 项验收；取消危险命令后进程未执行 |
| P0-6 | 布局与状态恢复（四区比例、pinned 列表、滚动/焦点；localStorage） | 0（自动） | pinned 行 24px；把手 6px | 冷启动恢复 ≤300ms | 冷启动后比例 ±1px、pinned 顺序、滚动位置一致；清 localStorage 回默认 |

证据：P0-2 见 F23（即时搜索）与 F25（实时 grep）；P0-3 见 F33（行内错误）与 F35（项目级诊断列表）；P0-5 见 F11/F38；P0-6 见 F09/F06。P0-1/P0-4 属骨架能力，A/B 数据未直接覆盖，标"骨架（无直接数据）"。

## 2 底栏终端面板（v1 必做）

| 子项 | 规则 | keys | px | ms | 验收 |
|---|---|---|---|---|---|
| 开关 | Ctrl+反引号 显示/隐藏；隐藏保留会话与滚动位置 | 1 key | 默认高 320px；min 160px；max 屏高 60% | 切换 ≤100ms | `smoke --terminal`：开→关→开，输出行数与 cwd 不变 |
| shell 探测 | 顺序 `pwsh → powershell → cmd → $SHELL`；设置页可覆盖 | 0（自动） | 启动条 28px | 首个提示符 ≤600ms（目标值） | `jw term probe --json` 返回 `{shell,version,ok}`；缺 shell → `ok:false` + 安装链接 |
| cwd 跟随 | 三模式：仓库根 / 当前文件目录 / 当前选中节点；自动跟随当前文件；新 tab 继承当前 cwd | 1 key（Alt+C 切模式） | 状态行显示 cwd，单行省略中段 | 跟随 ≤100ms | 打开 `src/a.rs` → cwd 变其目录；`cd` 只改终端内部，不反向改 UI |
| 新 tab | 继承当前 cwd；最多 8 个；关闭确认仅在运行中进程时弹 | 2 keys（Ctrl+Shift+T） | tab 180px、标题省略 24 字符 | 新建 ≤80ms | `smoke --terminal`：新建后 `pwd` == 前一个 cwd |
| 搜索 | 仅面板聚焦时 `Ctrl+F` 生效，不抢全局；匹配高亮 2px；`Enter/F3` 下一个 | 2 keys | 搜索条 28px 高 | 80ms/击键 | 输入 3 个词：命中数 == 高亮数；无命中显示 0 而非空白 |
| 危险命令 | `rm -rf /`、`git reset --hard`、`curl … 管道 sh`、`sudo`、磁盘格式化 → 确认条；默认焦点在"取消"；只提醒不阻断 | 1 key（Enter 确认） | 确认条 320×80px | 识别 ≤50ms | 5 类 fixture 全部弹确认；取消后命令未执行（进程表断言） |
| 失败降级 | shell 启动失败/无写权限 → 只读提示 + 可复制的启动命令；不自动重试超过 1 次 | 1 key（复制） | 提示条 320×64px | 失败提示 ≤300ms | 伪造坏 shell → 提示出现且无无限重启（重启次数 ≤1） |
| 粘贴安全 | 多行粘贴先显示行数与首行，确认后写入；不自动回车 | 1 key（确认） | 提示 320×80px | 提示 ≤100ms | 粘贴 3 行 → 默认不执行；确认后逐行写入 |

## 3 跳转模型（防嵌套地狱）

- **一跳 = 替换 preview 槽**，不入栈；`main` 槽只在显式"在 main 打开"时改变。
- **back 只回上一处（单层）**：保存 `origin`（跳转前位置）与 `current`；history 长度 ≤2，不允许递归栈。
- **Esc**：preview 有内容 → 回 origin；已在 origin → 关 preview。最多退一层。
- **面包屑**：`repo / file / symbol / line` 表达当前位置；点击面包屑任一段 = 新一跳（仍替换 preview）。
- **焦点恢复**：返回时焦点回到触发元素（列表项/代码行）；**滚动恢复**：origin 的 scrollTop ±0。
- **深链**：`?path=<file>#L<line>` 冷启动可还原 preview；origin 缺省=仓库根；深链 ≤2s 出首屏。
- **不变量**：任意操作序列后 history ≤2、无重复项；连续跳 5 次后 Esc 一次即回 origin。

验收：`node tools/web/smoke.mjs --jump` → 5 连跳后 `history.length<=2`、Esc 回 origin、焦点/滚动断言通过。

## 4 P1（16 条：骨架后置 6 + 精致 10）

### 4.1 从 P0 移入（骨架后置，功能成立）

| ID | 需求 | keys | px（目标值） | ms（目标值） | render_slot | 证据 | 置信 |
|---|---|---|---|---|---|---|---|
| F01 | 终端/输出里的路径可点开文件 | 1 click（Ctrl+Click） | 下划线 1px；命中区 ≥1 字符格 | 150ms | 代码区 | https://github.com/ghostty-org/ghostty/issues/1972 | A |
| F11 | 新标签继承当前目录 | 2 keys（Ctrl+Shift+T） | tab 180px | 50ms | 底栏 | https://github.com/microsoft/terminal/issues/3158 | A |
| F15 | blame 并跳到 commit | 1 key（Space g） | gutter 160px；字号 14px | 100ms | 代码区 | https://github.com/helix-editor/helix/issues/3035 | A |
| F16 | 光标行内联 blame | 0 keys（移动光标） | 行内 1 行；14px；60% 不透明度 | 120ms | 代码区 | https://github.com/zed-industries/zed/issues/4793 | A |
| F26 | 外部改动自动重载 | 0 keys（watcher） | toast 320×48px | 200ms 内提示 | 状态栏 | https://github.com/helix-editor/helix/issues/1125 | A |
| F38 | 回滚缓冲搜索 | 2 keys（Ctrl+F，输入） | 搜索条 28px 高、≥320px 宽；命中高亮 2px | 80ms/击键 | 底栏 | https://github.com/ghostty-org/ghostty/issues/189 | A |

F11/F38 依赖底栏终端；终端延期只影响这两条，不影响 P0。

### 4.2 精致小功能（10 条）

| ID | 需求 | keys | px（目标值） | ms（目标值） | render_slot | 证据 | 置信 |
|---|---|---|---|---|---|---|---|
| F05 | 搜索词记忆可关掉 | 1 key（Ctrl+Shift+F） | 搜索条 28px | 50ms 打开 | 底栏 | https://github.com/wezterm/wezterm/issues/1988 | B |
| F10 | 多会话保存与切换 | 1 key（F1）+1 选择 | 会话行 24px、列表 ≥320px | 150ms 切换 | 命令面板 | https://github.com/kovidgoyal/kitty/issues/8911 | B |
| F14 | gutter blame 可开关 | 1 key（toggle） | gutter 140px | 100ms | 代码区 | https://github.com/microsoft/vscode/issues/2248 | B |
| F17 | 状态栏显示当前行 blame | 0 keys | 状态栏 22px 高 | 100ms | 状态栏 | https://github.com/zed-industries/zed/issues/12133 | B |
| F18 | 任意两文件对比 | 2 keys（命令面板选 diff + 2 文件） | split 50/50；gutter 40px | 200ms | 右栏 | https://github.com/zed-industries/zed/issues/4523 | A |
| F22 | 鼠标组合键可自定义 | 1 click（Alt+Click） | 命中行高 20px | 50ms | 代码区 | https://github.com/microsoft/vscode/issues/3130 | A |
| F27 | 重载策略可选 | 0 keys | toast 320×48px | 150ms 内决策 | 状态栏 | https://github.com/microsoft/vscode/issues/23107 | B |
| F30 | 一键清屏含回滚 | 2 keys（Ctrl+K） | 不新增像素；scrollback 归零 | 50ms | 命令面板 | https://github.com/microsoft/terminal/issues/1882 | A |
| F34 | 按键提示浮层 | 1 key（prefix） | 浮层 320×240px；行 20px | 150ms 延迟 | 状态栏 | https://github.com/zed-industries/zed/issues/10910 | A |
| F37 | 输出整段丢进编辑器 | 1 click（菜单项） | tab 180px | 300ms | 底栏 | https://github.com/ghostty-org/ghostty/issues/2504 | B |

## 5 窗口与布局（不变）

- 槽位 `main/preview/pinned/inspector` + 底栏 + 状态栏；单占，不做自由浮动。
- 边界柔软：把手 6px、收放动画 180ms ease-out、最小宽 320px、比例持久化。
- z-order：槽内栈、聚焦上浮；不用模态做常规流程。
- 隐藏/恢复：卸视觉留状态；Esc 关顶层。
- 持久化：localStorage `jw:<repo>:<proto>`；服务端不存 UI。

## 6 双主题（不变）

- 默认"密"：13px/20px、4/8px 间距、单强调色、JetBrains 信息架构。
- "MC"：只换 token（2px 像素边、8 色板、页眉/空态纹理），不改布局与层级。
- 切换 ≤100ms，只动 transform/opacity，无重排；两种主题都满足 §7 对比度。

## 7 审查基本盘（逐类目查漏）

证据来源：`.pro/exp/research/baseline-evidence.json`（28 条；basis standard 15 / documented 7 / convention 5 / study 1；confidence A 19 / B 9）。表中 `TBD` 由该文件的对应 id 填充；`convention` 项仍只作**目标值**，不升为证据。

| 类目 | v1 要求（keys/px/ms） | 验收 | v1/后续 | 证据 |
|---|---|---|---|---|
| 可访问性 | 对比度 ≥4.5:1；所有 P0 键盘可达（0 鼠标）；`prefers-reduced-motion` → 动画 0ms；字号 ≥13px 且 200% 不裁切 | `smoke --a11y`：axe critical=0；Tab 走完 6 条 P0；缩放 200% 无溢出 | v1 | TBD（WCAG 2.2 AA） |
| i18n 与文案 | 文案外置；伪本地化膨胀 30% 不裁切；错误文案含"下一步"而非只报错 | 伪 locale 渲染 0 溢出；错误清单 100% 有 action | v1 | TBD |
| 性能预算 | 冷启动 ≤2s；数据到达后首屏 ≤200ms；大仓 card P95 ≤500ms；RSS ≤500MB；终端首提示 ≤600ms | `smoke --perf` 输出 5 个 P95；超预算 FAIL | v1 | TBD |
| 离线/冷启动 | 默认零网络；无 LSP 全部功能可用（标注降级）；无库 → exit 2 `no_db` | 断网跑全流程 0 失败；`netstat` 出站 0 | v1 | TBD |
| 错误与恢复 | 无库/LSP 挂/watch 失败/崩溃各有一条恢复路径；反馈 ≤300ms；崩溃后恢复上次布局 | 6 个坏输入 fixture：exit∈{1,2,3}、有恢复动作 | v1 | TBD |
| 信任与安全 | 未信任不 spawn（G1）；危险命令确认；粘贴多行不自动执行；env 白名单 | 未信任仓 `lsp` 进程数=0；5 类危险命令全拦截确认 | v1 | TBD |
| 引导与学习（git） | 首启 3 步：git 状态→安全默认（user/branch）→撤销入口；显示命令与效果，不自动 commit | `smoke --onboard`：3 步可跳过；`git status` 与实际一致 | v1 | TBD |
| 文件监听/增量 | 单文件改动 → 重解析 ≤1 文件、≤1s；debounce 200ms；只有 rules 变更才全量 | 改 1 文件：`reparse_files<=1`、`wall<=1s` | v1 | TBD |
| 多仓 | v1 单仓；切仓 ≤500ms，布局按 repo 持久化 | 切仓后布局/深链不串 | v1（多仓后续） | TBD |
| 主题 | 双主题；切换 ≤100ms；两主题对比度均达标；不改布局 | `smoke --theme`：切换前后 bounding box 不变 | v1 | TBD |
| 扩展点 | v1 只开放 LSP server 配置；无插件 API | 配置可覆盖 server 路径/参数 | 后续 | TBD |
| 数据与隐私 | 不联网、不遥测；日志脱敏；删 `.jingwei` 即删数据 | 会话期间出站连接 0；日志扫描无密钥 | v1 | TBD |

## 8 Reject / Defer（12 条）

| # | 项 | 处置 | 理由 | 替代 |
|---|---|---|---|---|
| 1 | 自由浮动/任意拆分窗口 | Reject | z-order、持久化、验收组合爆炸 | 固定槽位 + 槽内栈 |
| 2 | UI 状态存后端/多端同步 | Reject | 协议与 UI 互相锁死 | localStorage |
| 3 | 先做 MC 皮肤 | Defer | 信息架构未定会整页返工 | IA 先行，皮肤后置 |
| 4 | 全量实时推送 | Defer | 无 backpressure 会拖垮前端 | seq + 环形缓冲 + ACK |
| 5 | 递归跳转栈/多级 back | Reject（用户已拍板） | 嵌套地狱 | 单层 origin + 面包屑 |
| 6 | 终端自动执行/自动重试危险命令 | Reject | 不可逆风险 | 只提醒 + 确认 + 重试 ≤1 |
| 7 | 默认联网/遥测 | Reject | 数据与隐私 | 本地优先、显式导出 |
| 8 | 自研 completion | Reject | 与编辑器抢输入法无收益 | 命令面板前缀跳转 |
| 9 | 跨仓代码图进 v1 | Defer | 依赖单仓稳定 | v2 |
| 10 | 把 px/ms 目标值当硬阈值 | Defer | 未校准会锁死设计 | fixture 校准后收紧 |
| 11 | 8 条单源 P1 升 P0 | Reject | 证据不足 | 补第二来源再评 |
| 12 | 多会话恢复（F10）进 v1 | Defer | 依赖会话持久化 | v1.1 |

## 9 验收

- `node tools/web/smoke.mjs --visual --code --search --problems --terminal --jump --a11y --perf --theme`：全部 PASS 才可演示。
- P0 逐条：keys 用 Playwright 记录；px 用截图 bounding box；ms 用 `performance.now()` P95。
- LSP 能力与降级见 `lsp-matrix.md`；分析能力见 `analysis-gap.md`。

## 10 假设与未验证

1. 底栏终端是 v1 明确进面板；若实现延期，只影响 P1 的 F11/F38，不影响 P0 基本盘。
2. px/ms 与 LSP 32 个延迟均为目标值；§7 的 `TBD` 需第 4 个 flash 补证据后升 A/B。
3. "密/MC" 两主题的具体色板未定，但对比度与不重排是硬约束。

## 附录 D：第 4 个 flash brief（已派：a11y/性能/安全/i18n）

- 目标：补 a11y / 性能 / 安全 / i18n 四类的**可引用证据**，只做检索与对表。
- 来源：WCAG 2.2、WAI-ARIA APG、MDN、web.dev、NN/g、Chrome/VS Code release notes、OWASP（终端/子进程安全）。
- Schema：`id, category, requirement, metric(keys|px|ms), evidence_url, source_type(standard|issue|doc|study), clause(条款号), acceptance, v1_or_later, confidence`。
- 判据：≥20 条；≥8 条带标准条款号；URL 全 200；每条有 clause 或原文 quote；不得只有观点。
- 预算：60 分钟，20 条。
- 禁止：编造条款/URL；厂商营销当唯一证据；把工程惯例写成标准要求。


## 10 v1.2 TBD 回填（2026-10-27，依据 `baseline-evidence.json`）

| 类目 | 回填条目（id） | 状态 |
|---|---|---|
| a11y | 键盘全路径/焦点可见/对比度/减动效/字号 200%：WCAG 2.1.1、2.4.7、1.4.3、1.4.11、1.4.4、2.3.3 | 标准条款在；实现待 M15 验收 |
| perf | LCP/INP/CLS、50ms 长任务、虚拟化、内存回归（P05/P06 为 convention） | 目标值，未校准 |
| security | 未信任不 spawn、无 shell、CWE-78、CWE-150（ANSI/OSC 注入）、多行粘贴告警 | 已实现（信任门/白名单/粘贴确认）；CWE-150 待测 |
| i18n | `lang`/`translate`、RTL 不破版、CLDR 复数 | 待做 |

> convention 5 条（P05/P06/S05/S06/I05）仍只作"目标值"，不写成研究结论。
