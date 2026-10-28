> 目标态总图：[`docs/reference/jingwei-final-design.md`](docs/reference/jingwei-final-design.md)（目标态；当前态只认本文件与证据；冲突裁决：代码与证据 > HANDOFF > 设计文档）
>
> **★ 2026-10-28 新会话（主会话即 pro）请先读**：[`docs/reference/handoff-2026-10-28.md`](docs/reference/handoff-2026-10-28.md) —— 角色契约、已定决策 D1–D11、R 系列断点（当前断点 = inspect 换芯）、每刀预算与监督回路、暗坑 14 条、未决风险。

# HANDOFF · 经纬核心（2026-10-25）

> **新会话只读这一份**。里面是继续干活需要的一切：模型、代码、路径、踩过的坑、角色分工、skill。
> 不要重新讨论历史；有疑问先在这里找，找不到再问用户。

> **2026-10-26 迁移入口**：根目录 `D:\projects\README.md`（目录布局 / 可复用决策 / 已验证偏差 / 新会话开场白）；
> 方法 skill：`experiment-discipline`（实验严谨）、`agent-contract`（协作硬规则）。
> 当前数字：**188 测试全绿**（`jw 0.1.0 (rules 27)`）；gate 三仓全绿（rg/tauri/nushell，54s，对称差 0/0）；位置门禁 use/mod/impl 三仓零差；check 十一项全零（含能力两项）；`?use` 原因码 other=0（0/85/186）；`parse_rules_version = 27`。

## ★★ 收束快照 2026-10-26（历史，已被 0.2.0 取代）

> 0.2.0 起 LSP 全链路已接（信任门/诊断/hover/def/refs/hints/actions），下面这段只作归档。

## ★★ 收束快照 2026-10-26（归档）

**当前状态（已验证绿）**：`jw 0.1.0 (rules 27)`；**189 测试**；gate 三仓 ok 56s（对称差 0/0）；v1 四段 30/30；docs 一致性 OK；
dist = `dist\jingwei-v0.1.zip`（3.08MB，11 个 ASCII 条目，含 bin/web/docs；`scripts\package.ps1` 可一键重打）。

**已完成（本轮）**：测试映射 v0 → cite v0（+`file:` 双哈希）→ call v0 → call v1（跨文件唯一名）→ call v1.5（use 绑定优先、std 排除）
→ impact v0（文件/符号/目录三种目标，direct/transitive/tests/importers/completeness）→ tests-for v0（符号级静态可达）→
range card（tests/call 聚合 + files/definitions 与逐文件同源）→ v0.1 冻结（package.ps1 / reports/v0.1.md / --version / ASCII 文件名）。

**下次开工第一件事（pro 已派、未做，P1）**：原因码统一 + web 出口。
- `impact crates/nope` 应为 `no_path`（不是 `no_symbol`）；`card crates/nope` 要带 `reason:"empty_range"`；
  `tests-for`/`impact` 统一：`no_path`（不存在/非文件非目录）、`no_symbol`（仅 symbol 形式）、`bad_subject`（格式错）、`empty_range`（合法目录但无已解析文件），exit 2。
- web 模块视图（`?module=`）接 range impact 面板：`direct_total` / `tests.items`（标 file 级）/ `importers` / `completeness`，`direct_truncated` 要透传；`frontend.md` 补原因码表。
- 判定：6 个坏目标 reason+exit 固定；web 数字 == serve result（独立 node 深比 6 条）；红：no_path 改回 no_symbol → 合同测试红。
- 完成后做 **v0.1.1 冻结**（数字：189 测试 + 新命令）。

**恢复命令**：`cargo test --workspace` → `tools\gate-control.ps1` → `scripts\exp-v1.ps1` → `scripts\package.ps1`。

## ★ v0.4.0-rc 2026-10-27（LSP 全链路 + 213 测试）

> **2.3b 绿（2026-10-27）**：card_value + 6 助手 → engine::view（view 20,340 B / main 253,089 B，与 10:13 逐字节同）；build EXIT=0；3 卡 json SHA 3/3；备份 .pro/bak/t23-final/。
> **2.3c 绿（2026-10-27）**：render 层（render_card_text_raw/display_width/clip_display/density_budget/short_commit/day_str/civil_from_days）→ engine::view（view 36,638 B / main 235,728 B）；文本卡 3/3 + json 3/3；main 侧 8 个定义命中 0（doc P5）；**day_str 由私有提升为 pub（对外 API 面变化）**。
> **口径（2026-10-27 修订）**：tests 以 `cargo test --workspace --all-targets` 的**整份日志求和**为准，基线 **247**（`t2-baseline.txt`）；旧的 213/216/217 都是手工截断统计（只加前 N 条 `test result:`），不可再混用。warnings 以 `scripts\check-split.ps1` 为准（沿革 124→123→115→114→113），build 的 `^warning` 不再写入基线。
> **T2.4 现状（2026-10-27）**：`crates/engine/src/lib.rs` 已有 Engine::open/root/store/module_map + 3 单测（来源未确认，第三方落盘）；**pro 审查后保留**，经 3 处修正：`store()` 降 `pub(crate)`、只读测试改为 size+bytes+无多余文件断言、注释改事实描述。
> **T2.4 stats 绿（2026-10-27）**：Engine::stats（source/files/symbols/edges/rules/write_seq）；files=当前树 `collect::parse::source_files`、symbols=DB 判定符号数、write_seq 缺失=0；rules 单点化到 `datacore::model::PARSE_RULES_VERSION`（engine/cli 中 `"27"` 字面量命中 0）；parity 测试 `crates/cli/tests/engine_parity.rs`（剔 seq/at）；`cargo test --workspace` 219 passed；check-split SPLIT_OK / warnings 115。
> **修（2026-10-27）**：serve stats 的 files/symbols 长期恒 0（`count("file")` 不在白名单 → `unwrap_or(0)` 静默降级），改为同源 Engine::stats 口径；P6 活教材。
> **T2.4 check 绿（2026-10-27）**：`Engine::check()` 返回 `{source,rules,ok,items[12]}`（12 名固定）；serve check 臂改调 facade（旧 4 项 stub 废）；`typed_path` 复用 `view::subject_path`（提 `pub(crate)`，P5 无第二份）；rg 实测 `jw check` 115ms（全表扫，未加缓存）。tests 222 passed / warnings 115 / SPLIT_OK。T4 待验：clean 页 12 项都在且名字对得上。
> **修（2026-10-27）**：`count_lsp_evidence` SQL 引用不存在的 `evidence.instrument`（该表无此列）→ `cmd_check` 的 `unwrap_or(0)` 吞错、`lsp_in_history` 恒 0；已改 join `source.instrument`，cmd_check 去 `unwrap_or`。
> **修（2026-10-27 补强）**：`count_lsp_evidence` 改 `LEFT JOIN + ifnull(s.instrument,'')`（防悬空 source_id 整行被丢）；三条红测已落 datacore `store::lsp_tests`（instrument 命中 / premise 命中 / 悬空 source_id 仍计）——该不变量此前从未被真正验证。
> **T2.4 files 绿（2026-10-27）**：`Engine::files(rel)` 带 canonicalize+starts_with（越界 `Err(Io("outside root"))`）、`truncated` 显式、read_dir 错误不吞；serve files 改调 facade；单测三例（正常/`..`/绝对路径）。tests 228 passed / warnings 115 / SPLIT_OK。
> **T2.4 read_file 绿（2026-10-27）**：只读前 200_004 字节 + UTF-8 边界回退截 200_000（修 `truncate` panic）；二进制→`Err(not utf8)`（不再空串成功）；目录→`is a directory`；越界→`outside root`；返回 `{path,text,truncated}`。单测四例含超长 CJK 不 panic + 二进制 Err。
> **T2.4 machine/projects 绿（2026-10-27）**：`Engine::machine()` 纯 sys（cpus 取不到=0）；`Engine::projects()` 父目录 200 项内、`.git/.jingwei` 过滤、读目录失败显式 Err；两者 serve 臂均改调 facade；parity 覆盖。
> **设计文档更新（2026-10-27）**：`docs/reference/jingwei-final-design.md` 追加 §18 运行时契约（[rc] 5 条：异步隔离/stdio 排空/NDJSON flush/弱仓体检/LSP warming_up）、附录 A 暗坑 10 条（本轮真实踩到）、ADR-9~13、§16.1 差距表、§18.1 31 条外部建议处置（rc 前 5 条 = 5/13/14/15/23；否决 2/16/26/agent 流程）。全文 412 行已读，7 条判据据此改。
> **T2.5 静默降级审计（2026-10-27）**：`.pro/exp/out/silent-degrade-audit.md`（411 命中 = 表行数，无抽样；疑吞错 50 = 39 疑 + 6 DB/IO + 3 坏数据 + 2 IO）。A 档（影响三态/证据）：store.rs:237/390/1334 scope 坏数据、cli:3304/3356/3364 impact 反查、cli:2948/4864 读文件空串、collect/parse.rs:1418 size=0。待 pro 排优先级，未改代码。
> **T3.1 绿（2026-10-27）**：shell `src-tauri/Cargo.toml` 加 `engine = { path = "../../jingwei/crates/engine" }`（只动 1 行，shell 源码 0 改动）；shell build EXIT=0（29.6s）；warnings 改前 1 → 改后 1（新增依赖 3 条单列：engine 1 + collect 2）；`cargo tree -i tree-sitter` 只 **0.27.1 一版**（links 冲突已解）；`cargo build -p jw` EXIT=0。
> **T2.5 A 档全修（2026-10-27）**：scope 解析 3 处 `?`（坏数据→Err，带 subject）；impact 反查 3 处吞错→`ok:false` 错误；serve file 读失败→`ok:false`（不再空串）；parse size 拿不到元数据按超大跳过（`u64::MAX`）；serve stats 改调 facade（edges 不再 unwrap_or 0）；view 决策原文区分"目录不存在/没有该 id"。红测 6 条（bad scope、DROP TABLE edge、file 目录、missing size、decision 目录文案、parity file 目录）全绿。tests **242 passed** / warnings 114 / SPLIT_OK / 6 fixture + text 3 全 True。
> **scope 兼容探测（2026-10-27）**：`SELECT COUNT(*) FROM judgment WHERE scope='{}'` — rg 0/3185、tauri 0/8625 → 无历史空 scope 数据，本轮不加兼容代码（已探测记录在案）。
> **T3.2 绿（2026-10-27）**：`Engine::card(input,history,state)`（state=def/dups/original/deps 全保留）+ `Engine::edges(path)`（{path,total,edges,unresolved_reasons?}）；serve card/edges 两臂改调 facade（信封 {exit,card}/{exit,cards} 不变）；parity 10 条（含 card 4 例：file/range/state=def/history=true）；tests **244 passed** / warnings 114 / SPLIT_OK / 6 fixture + text 3 全 True。
> **T3.3 绿（2026-10-27）**：新增 `jingwei-shell/src-tauri/src/engine_cmds.rs`（8 个 `engine_*` 命令，async + spawn_blocking，闭包内 open Engine，零业务逻辑，grep `SELECT|read_to_string|std::fs`=0）；`main.rs` 只加 `mod engine_cmds;` + invoke_handler 追加 8 行；旧命令 64 个全在、handler 命令数 64→72；shell build EXIT=0、`jingwei-app` warnings 仍 1（依赖 engine 1 + collect 2 单列）；主仓 4 受保护文件 sha 不变；jw build EXIT=0、check-split `SPLIT_OK`（244 passed / warnings 114）。未启动 GUI。
> **门禁假红根治（2026-10-27）**：`check-split.ps1` 入口加 `Get-Process jw,jingwei-gui` 断言 → 存在即 `gate busy: jw.exe still running` + **exit 2**（不是 SPLIT_FAIL）。红证据：后台起 `jw serve`（无 --open）→ gate exit **2**；杀进程后 gate **SPLIT_OK**（`.pro/exp/t33_gate_red.py`）。
> **T4 第 0 步 shell 映射表（2026-10-27）**：`.pro/exp/out/shell-invoke-map.md`（132 行）；口径 **行数=出现次数**（ui/*.js 77 行/77 次 invoke，无一行多处）；六组 home 57 / run 2 / editor 9 / note 4 / machine 3 / clean 2 = **77**；engine_* 可对应 10 行、待 T6 2 行（check_project→problems、changes_take→impact）、无对应 65 行；死命令 13 个单列（表内 0 行）。T4 切换会丢字段的点：engine_projects 丢 parent/depth、engine_files 单层、engine_read_file 无 encoding/eol/drafts、engine_machine 仅 os/arch/cpus、engine_check 为 12 项自检非 A/B/C 分区。
> **T4 clean 绿（2026-10-27）**：clean 页改 `engine_check` 12 项（12 name 固定契约 + UI 分组常量表：数据一致性/边层/能力声明/LSP 契约；删除动作显式"未由内核提供（T6）"）；smoke --shell **13/13 PASS**（红证据 `.pro/exp/out/t4-clean-red.txt` 11/13，clean_12names + clean_nonzero 两 FAIL；绿 `.pro/exp/out/t4-clean-green.txt` 13/13）；改 `projects.js`/`events.js` 两副本 + web shim/index/smoke（测试基础设施）；shell build EXIT=0、`jingwei-app` warnings 1、handler 72 不变、主仓 4 文件 sha 不变。
> **T4 machine 绿（2026-10-27）**：machine 页改 `engine_machine`（os/arch/cpus），其余显式"未由内核提供（T6）"，cpus=0 显示"取不到"；删掉 hardware_fast/machine_report/deep_check 三处调用（deep_check 标 T6）；smoke --shell **16/16 PASS**（红 `t4-machine-red.txt`：machine_engine FAIL 全 false；绿 `t4-machine-green2.txt`：os=windows arch=x86_64 cpus=16）；shell build EXIT=0、warnings 1、handler 72、主仓 4 文件 sha 不变。
> **修（2026-10-27）**：主仓库 `.jingwei/jingwei.db` 的 meta 表原为空 → 补 `parse_rules_version=27`；smoke clean 的 `rules_version_bad` 1→0，`clean_nonzero` 9→8（数字变化解释：少了一个伪异常 1）。
> **新增漂移门（2026-10-27）**：smoke --shell 前置 `ui.copy_sync` 检查 8 个可同步文件（core/datacore/datacore-panel/editor/notes/projects/scan/scan-extra）shell vs web 内容一致；shim.js 排除（web 独有）。不一致 FAIL 并列文件名。
> **T4 home 绿（2026-10-27）**：home 页新增"内核概览"（engine_stats：文件 43 / 符号 866 / 边 2615）+ "同级项目"（engine_projects，含 demo/jingwei）；旧扫描面板整块 `display:none` + 显式"未由内核提供（T6）"；home 渲染路径旧命令 grep=0。修 `Engine::projects` 相对路径 canonicalize（`.` 的 parent() 为空导致列表恒空）；主仓库补 `jw parse`（889 symbols、边 2615）。smoke --shell 红→绿（`t4-home-red.txt` FAIL → `t4-home-green3.txt` PASS）；check-split `SPLIT_OK`（warnings 114）；shell build EXIT=0、warnings 1、handler 72、`ui.copy_sync` PASS。
> **数据变更（2026-10-27）**：`jw parse .`（主仓库工作图谱）→ symbols 889 / 边 2615（此前均 0，未 parse）；`parse_rules_version` 仍 27；4 个受保护源文件 sha 未变。
> **T4 editor 绿（2026-10-27）**：新增 `ui/engine-editor.js`（单层 `engine_files` 逐层钻取 ≤8 层 + 越界提示；`engine_read_file` + `truncated` 文案"已截断（仅前 200KB）"；`engine_card` 渲染三态 held/refuted/uncertain（exit 2 → uncertain(no_data)）；`engine_edges` + total + unresolved_reasons；problems/impact/rename/草稿显式 T6）；旧编辑器 `ed-wrap` 整块 display:none。打开 `crates/engine/src/lib.rs`：状态 `uncertain(no_data)` / exit 2 / 边 11 条 / 越界"读取失败：io: outside root"。smoke 红→绿（`t4-editor-red.txt` FAIL → `t4-editor-green5.txt` PASS，含 `editor_outside_root`）；同源 grep 旧命令=0；handler 72 / build EXIT=0 / warnings 1 / `ui.copy_sync` PASS（含 engine-editor.js）。
> **T4 note/run 绿（2026-10-27）**：note 纯空态（"文档索引未由内核提供（T6）"，旧 noteTop/notebook display:none，events.js note 分支改 `engNote()` 不再调旧命令）；run 空壳（"终端未接（P1）" + deny 文案），旧 pipeline 按钮 **disabled** + 隐藏、runs 隐藏；smoke 加 `note_t6/run_t6/run_deny/run_no_spawn`（selftest 点击 run 页全部按钮，tasklist 采样 **3→3** 无 spawn）。红 `t4-note-run-red.txt`（3 FAIL）→ 绿 `t4-note-run-smoke-final.txt`（23 checks 全 PASS）。
> **T4 六页完成（2026-10-27）**：clean/machine/home/editor/note/run 全部切换（engine_check / engine_machine / engine_stats+projects / files+read_file+card+edges / 空态 / 空壳），六页各自先红后绿；handler 72、shell build EXIT=0、warnings 1、`ui.copy_sync` PASS、未开 GUI。CLI/UI 对账：`jw card crates/engine/src/lib.rs --json` = `uncertain(no_data)`/exit 2，与 editor UI 一致。
> **T4 收尾（2026-10-27）**：① 对账表 `.pro/exp/out/t4-parity-ui-cli.md`（六行，四页逐值相等；note/run 空态断言替代；`jw edges` 58 vs serve 11 口径差异已记）；② 同源汇总：六页新渲染路径旧命令命中 0/0/0/0/0/0，shell.js 旧 home 函数区残留 20 处（T5 靶子）；③ 死 UI 清单 `.pro/exp/out/t4-dead-ui.md` + HTML `<!-- T5 待删 -->` 六处标记；④ 干净环境总 smoke `.pro/exp/out/t4-final-smoke.txt` = **23 checks 全 PASS**（run_no_spawn jw 1→1）。
> **过程教训（2026-10-27）**：**测试数一律从整份日志求和（或脚本）取**——手工把输出前 N 条 `test result:` 相加会漏统计（旧 213/217 即由此而来）。另：`--workspace` 与 `--all-targets` 本仓当前同值（247），基线口径固定为 `--all-targets`。
> **T7 结论降级（2026-10-27，T8.3b 发现）**：之前"GUI 已验"只证明**可开窗**（1 窗口、关窗进程 0、无白屏截图）；**数据链路未验**——带参启动时 `S.root` 未被 `app_root` 设上，probe `root_ready` 全 false、home 未落盘（证据 `.pro/exp/out/t8-probe-6of6.txt`：4/6 条、0 条 root_ready=1）。恢复"已验"需 6 条 `root_ready:1`。**教训**：没有机器断言的"看起来对"不能写进结论。
> **T8.3b 未达（2026-10-27）**：第 4 次开窗（预算用尽）仍 4/6 条、0 条 `root_ready=1`、home 未落盘；按红线不再开，T8 带差距发。待查：`initRoot()` 的 `app_root` 返回形状 / 是否被更早异常短路。
> **T8 收官（2026-10-27）**：T8.1 版本 0.4.1；T8.2 单实例（TCP 7960 互斥、第二实例 exit 2、`tag=second-instance-blocked`）；T8.3 根因定为 **Tauri v2 参数 camelCase**（`root_ready`→`rootReady`），修复后 release 开窗 **5/5 数据页 `root_ready=true`**（`run` 为 P1 空壳、按设计无数据，记"不适用"）；T8.4 release exe **6170624 B / sha `c0cfe01311411fdd9afe82435854ccf1fd0bd0e5baa75678536b2da21c7e5f41`**、**PE subsystem=2**、release warnings **0**；T8.5 打包 `dist/jingwei-v0.4.1.zip`（**5350927 B / sha `08839fa0448b04a63317e3eac642ed8985e28f28a8d9fb66b05c55d3e2245573`**）+ `src-manifest`（15 文件）+ `m14.json`；b94 **OK**。验收表 `.pro/exp/out/t8-accept.md`。**GUI 结论**：`GUI 已验（release exe；1 窗口；5 个有数据页 probe 全 root_ready=true；关窗进程 0；run 页为 P1 空壳、UIA 未命中故无 probe 行——按设计无数据）`。
> **T8.1 版本与同源（2026-10-27）**：workspace `version` 0.4.0→**0.4.1**、`crates/engine/Cargo.toml` 字面量同步（Cargo 版本不带 `-rc`，rc 只进包名/README/标签）；`scripts/check-split.ps1` 的 version 断言同步 0.4.1。判据：`jw --version` = **jw 0.4.1 (rules 27)**；`b94_docs_consistency` **OK**；`check-split` **SPLIT_OK**（tests **257**、3 卡 json + text 3 + edges 6 fixture 全 True、warnings 109、main_lines 5085）。
> **T6.5 最小 preview（2026-10-27）**：引用列表项点击 → `#engPreview` 填 `engine_card` 摘要 + `engine_read_file` 片段（≤20 行，行号用 card.line）；**替换式**（再点直接替换，不追加/不新开/不嵌套）；**origin 单层**（Esc 一次回 origin，回后清 origin、二次 Esc 不再变）；不做 pinned 栈（§16 差距表已记）。smoke 加 3 条：`preview_open`（含 symbol:）、`preview_replace`（p2 不含 p1 的符号名）、`preview_esc`（回 origin 文案）。判据：smoke **27→30 全 PASS**、handler 26 不变、build EXIT=0、`console_errors=[]`、`ui.copy_sync` PASS。
> **T6.4 UI 接线 cite/references（2026-10-27）**：新增 `engine_cite`/`engine_impact` 两个壳命令（零业务、spawn_blocking 同模板）、web shim 加 `cite`/`impact` 映射；editor 页加「引用（engine_cite）」（显示 `commit:file:line`，无 commit 显示 `skip:no_commit` 等原文，不空白）与「引用列表（engine_impact）」（`direct` + `importers` 渲染，纯文本不做外部打开）。smoke 加 `shell.editor_cite`/`shell.editor_refs` 断言（红 `t6-4-ui-red.txt` → 绿 `t6-4-smoke2.txt`）；handler **23→26**；smoke **25→27**；`console_errors=[]`、`ui.copy_sync` PASS。**演示注意**：主仓 `D:\projects\jingwei` 没有 commit（git 根在 `D:\projects`，main 0 commit），cite 在此恒 `skip:no_commit`——自测/演示请用有历史的仓库（rg/tauri fixture）。未做：references 点击的"替换式 preview"（§9.2 跳转不叠栈）——当前只渲染列表，点击无 preview；跳转模型待 T6.5/T8。
> **T6.3 cite 归位 + §18.4 弱仓体检（2026-10-27）**：`collect::git::{blame_line, last_commit}` 原样归位（cli 两函数删除）；`Engine::cite` 搬 `cite_payload`（serve 臂 + `cmd_cite` 改调）。§18.4：`weak_repo_skip()` 检测 `no_git`/`shallow_clone`/`no_commit`/`detached`，命中即返回 `{v,subject,skip}` 显式降级（不造 permalink、不 panic）。测试：`cite_no_git_skips`、`cite_no_commit_skips`、`cite_detached_head_skips`、`cite_shallow_clone_skips`（`git clone --depth 1 file://` 本地造，不走网络）、`cite_normal_keys_and_serve_parity`（正常分支 keys `rows/subject/v` + rows 项 6 keys + serve 同值）、`cite_skip_branch_keys`（skip 分支 keys `v/subject/skip`）、`cite_perf_ms`。判据：`cargo test --workspace --all-targets` **257**、`SPLIT_OK`（main_lines 5085、warnings **109**）、fixture 全 True、smoke **25 checks 全 PASS**。
> **T6.2 impact 归位（2026-10-27）**：`impact_payload`（165 行）原样搬进 `Engine::impact`；`typed_path` 已迁 `datacore::core`、`fns_of_file` 成 engine 私有唯一实现（cli 删除）；serve `impact` 臂 + `cmd_impact` 改调 facade。测试：`serve_tests_for_matches_facade`、`impact_fixture_is_honest_about_symbol_level`（**自建 fixture**：a() 调 b() → direct 非空且为 `symbol:src/a.rs#a`，证 call 边是符号级）、`impact_perf_ms`、engine `impact_empty_db_is_no_symbol`（空库 reason=no_symbol）。**修正一个误判**：主仓 `card` direct 空是选样问题，不是 call 边缺失（fixture 证明）；§16 行已更正为"已验"。判据：`cargo test --workspace --all-targets` **250**、`SPLIT_OK`（main_lines 5210、warnings **110**）、fixture 全 True、smoke **25 checks 全 PASS**。另：smoke 的 selftest 文本截断从 30000 提到 60000（engine lib.rs 变长后卡片/边被截断）。
> **T6.1 契约固化（2026-10-27）**：新增两条契约测试并进 `cargo test`：① `crates/engine/src/lib.rs::tests::tests_for_shape_bad_subject`（keys-only：`v/target/reason`）；② `crates/cli/tests/engine_contract.rs::serve_tests_for_matches_facade`（spawn `jw serve` → 发 `tests_for` → 与 `Engine::tests_for` 逐字段比（剔 seq）+ 顶层/tests[] keys-only）。先红后绿：把 `note` 改成 `note_x` → **RED_EXIT=101**（`tests_for 顶层 key 变了: left [note_x] right [note]`）→ 改回 → GREEN。`cargo test --workspace --all-targets` **EXIT=0 / 247 passed**。规矩：以后每搬一个命令，同时交一条 keys-only 契约测试 + 一条 serve/CLI 同值测试。
> **T6.1 tests_for 归位（2026-10-27）**：`tests_for_payload`（cli 75 行）原样搬进 `engine`（`Engine::tests_for(target, depth)`，`bad_subject`/`no_symbol` 以 Value 返回）；`serve` 的 `tests_for` 臂与 `cmd_tests_for` 改调 facade；cli 的死函数删除；engine 新增 `tests_for_no_db_is_no_db` 单测。判据：`cargo test --workspace` **EXIT=0**（217 tests）；serve/CLI parity **同值**（`symbol:…` 与 `path#name` 两种 target 都 same=True）；check-split **SPLIT_OK**（warnings **113**、3 卡 json + text 3 + edges 6 全 True）；smoke **25 checks 全 PASS**；无复制 helper（复用 datacore Store + engine 既有 Subject 解析）。
> **T7 前置 root 链（2026-10-27）**：新增 `app_root`（argv[1] 取仓库目录）与 `engine_pick_root`（tauri dialog 目录选择，**不 spawn 任何进程**）命令；UI `initRoot()`：localStorage → `app_root()` → 空态；3 处 `pick_folder` 改 `engine_pick_root`；12 个已删命令的 UI 调用改 T6 对象/toast（`inspect/check_project/contract_draft/run_task/launch_project/changes_take/audit/apply_fix/verify_project/event_note/open_in/default_opener`）；smoke 新增 **`ui.command_closure`**（`ui/*.js` 的 invoke 集合 ⊆ handler 注册集合，差集即 FAIL）。`invoke('inspect')`/`invoke('pick_folder')` 命中 **0**；handler **23**；smoke **25 checks 全 PASS**；build EXIT=0。
> **T5 收官（2026-10-27）**：`grep jingwei_engine src-tauri/src` = **0**；`src-tauri/Cargo.toml` 删 `jingwei-engine` dep；shell 根 members/default-members 收窄为 `["src-tauri"]`；`crates/` 整棵删除（备份 `.pro/bak/t5b-crates` 2.42 MB）；`Cargo.lock` 已无 `jingwei-engine`/`jingwei-model`/`jingwei-datacore`/`jingwei-cli`；shell 根 `cargo build` EXIT=0；smoke **24 checks 全 PASS**；handler **21**（8 engine + 13 基础设施）；**旧内核残留度 = 506 行**（notify 94 / tasks 137 / envelope 89 / dlog 186）。壳里 0 行旧内核分析代码、0 个 `graph.db` 读取方、0 FFI 采样。
> **T5b sample_now T6（2026-10-27）**：`sample_now` 命令 + `SysSampler` 结构 + setup 里的每秒 `sys://sample` 线程删除；UI `pollSample` 改本地 T6 对象（状态栏 CPU 显"未由内核提供（T6）"）；§16 加行"状态栏实时 CPU/内存采样 | T6 | 旧内核 FFI + windows-sys 不进壳"。handler **24→23**；`grep jingwei_engine` **1**（只剩 `use jingwei_engine::{editors, machine}`）；build EXIT=0；smoke **24 checks 全 PASS**。剩最后一步：`editors::detect/App` 抄本地（~150 行）→ grep=0。
> **T5b 3 模块落地（2026-10-27）**：`profiles_list/profile_save` 改 T6 toast + 删命令（写旧路径 `%LOCALAPPDATA%\jingwei`，§16 差距表加 2 行）；`default_opener/set_default_opener` 同 T6（`set_default_open` 写旧 LOCALAPPDATA 设置）；删死函数 `run_with_stream/run_with_stream_purpose`（无调用者，含 `launch::` 引用）；use 收窄到 `{editors, machine}`。handler **28→24**；`grep jingwei_engine` **1**（只剩 `use jingwei_engine::{editors, machine}`）；build EXIT=0；smoke **24 checks 全 PASS**。剩：`editors`（detect/App，不抄 open）与 `machine::Sampler`（+配套类型）最小定义 → grep=0。
> **T5b 5 模块核账（2026-10-27）**：`grep jingwei_engine engine_cmds.rs` = **0**（pro 怀疑的边界破坏是笔误：engine_projects 走 facade，未碰旧 crate）；`clean`/`projects` 在 Rust 里**无真实引用**（只 use 行/注释/JS 字符串），直接删 use；真需最小定义只剩 3 个：`editors`（detect/App/default_open/set_default_open/open）、`machine`（Sampler）、`profiles`（Profile/list/save）。
> **T5b 第 5 步 17 模块判定（2026-10-27）**：main.rs 的 17 模块 use 收窄到 6 个活模块（clean/editors/launch/machine/profiles/projects）；11 个死条目删除；`tasks.rs` provenance 注释改措辞（去 `jingwei_engine::` 字面量）。判定表 `.pro/exp/out/t5b-modules.md`（17 行齐）。`grep jingwei_engine` **2→1**（仅剩 main.rs:6 的 6 活模块 use）；build EXIT=0；smoke **24 checks 全 PASS**。搬入壳的旧内核行数：notify_impl 94 + tasks_impl 137 + envelope 89 + dlog 186 = **506 行**。下一步：5 个活模块本地最小定义（machine::Sampler / editors::detect / profiles::Profile / clean / projects）→ grep=0。
> **T5b 第 3/4 步 notify + tasks（2026-10-27）**：`understand/notify.rs` 原样复制 → `src-tauri/src/notify_impl.rs`（纯数据结构 + 去重/状态机，**无 spawn 线程、不写 graph.db**，1:1 照搬）；`notify.rs` 9 处引用改本地，`grep jingwei_engine src-tauri/src/notify.rs` = **0**。`understand/tasks.rs` 同样复制 → `tasks_impl.rs`（3 处引用改本地）。`grep jingwei_engine` **14→2**（剩 main.rs:6 的 17 模块 use + 1 行注释）；build EXIT=0；smoke **24 checks 全 PASS**。
> **T5b 第 2 步 Envelope + dlog 返工（2026-10-27）**：dlog `now_iso` 改为**完整 ISO**（复制旧 `facts::now_iso` 的 civil_from_days 实现，格式 `YYYY-MM-DDTHH:MM:SSZ`，与旧一致）；`graph/envelope.rs` **原样复制**到 `src-tauri/src/envelope.rs`（sha256 **6BEB6E32… 与旧文件逐字节相同**，diff 为空）；`main.rs`/`selfcheck.rs` 的 use 改为 `crate::envelope::Envelope`，加 `mod envelope;`。`grep jingwei_engine` **16→14**；build EXIT=0；smoke **24 checks 全 PASS**。
> **T5b 第 1 步 dlog（2026-10-27）**：`crates/engine/src/util/dlog.rs` 原样复制到 `src-tauri/src/dlog.rs`（自包含，仅 `crate::facts::now_iso` 改为本地 `now_iso()` epoch 秒实现）；`main.rs` 6 处 `jingwei_engine::dlog::` → `dlog::`，加 `mod dlog;`。`grep jingwei_engine` **22→16**；build EXIT=0；smoke **24 checks 全 PASS**。台账：`event_note` 归属改"分析类随命令删（写 graph.db、note 页已空态）"；§16 差距表加 3 行（note 写入/项目创建/草稿应用）。
> **T5a 收官（2026-10-27）**：`grep jingwei_engine` = **22**（= 迁移清单）；handler = **28**（8 engine + 20 旧，偏差见下）；`payload.rs`/`scan.rs` 删文件（文件内无 `pub fn`，已逐个 grep 证明零活引用）；`verify.rs` 删（函数删后仅剩注释 + use）；`selfcheck.rs` 的 3 处 dlog 改本地 no-op；11 条未归类命令删（apply_fix/changes_take/project_log/apply_edit/snapshot_file/restore_draft/contract_save/save_note/delete_note/list_pythons/create_project/launch_project）；UI 对应改 T6/删调用；build EXIT=0；smoke **24 checks 全 PASS**（ui.syntax、console_errors=[]）。**偏差**：`event_note` 被当分析类删除，但分诊表列为基础设施（21 infra 之一）→ handler 20 旧而非 21；需恢复或确认。
> **T5a 尾步（2026-10-27）**：删 9 命令（`check_project/contract_draft/run_task/event_note/verify_project/scan_start/scan_status/scan_cancel/scan_similar`）+ handler 条目；UI 对应改 T6 toast（`check_project/contract_draft/run_task/verify_project`）、`event_note` 删调用、scan.js 的 `loadCachedScan` 死 IIFE 删、`metrics` 改**显式降级对象**（`{source:'db', degraded:'metrics 未由内核提供（T6）'}` 且 UI 显示，不再返回 null）。handler **49→40**；`grep jingwei_engine` **37→34**（未到 22：`payload.rs` 6 行 + `scan.rs` 8 行 use 仍在，scan 命令删了但文件/use 留）；build EXIT=0；smoke **24 checks 全 PASS**。
> **T5a 5b 簇 2/3（2026-10-27）**：`audit` 面板 `display:none` + `runAudit` 改 T6 toast + Rust `audit` 删除；editor `rename` 两处 `cli_json` 改 T6 toast；`shell.js` 的 `channelFetch(root,'metrics')` 残留改 `Promise.resolve(null)`（home 已用 engine_stats）。handler **72→49**；`grep jingwei_engine` **37**；smoke **24 checks 全 PASS**（六页逐 tab、`ui.syntax` PASS、`console_errors=[]`）。
> **T5b 入场券（2026-10-27 定死）**：① 37 行归属表齐（22 待迁移 + 15 随删，`.pro/exp/out/t5a-37-attribution.md`）；② 22 行迁移完成（`grep jingwei_engine` = 0，含本地 `Envelope` 逐字段比对、notify 1:1 照搬）；③ 六页巡检绿；④ handler 只剩 `engine_*` 与基础设施。四条缺一，不许动 `crates/` 树。**核心收益**：T5b 完成时壳里再没有 `graph.db` 读取方，双库并存的结构性误导才真正消除。
> **T5a 5b 簇 1 完成（2026-10-27）**：删 scan/proj 残留 19 处（patchBanner/openInline/scanCommon/openDetail/detailAct/paintNav 段/renderGrouped/scanAll/pollScan/findSimilar/paintProjectBanners/renderRailProjs/renderSwitcher）；`#railProjs` 隐藏（display:none，DOM 保留，§16.1 差距表已记）；删 DOM `#scanPanel`（26 行）；删 Rust `list_projects`/`inspect`（`del_fn --apply`，备份 `del-20261027-115749`）。数字：handler **72→50**；`grep jingwei_engine` **仍 37**（备份 grep 证实这两函数体不含 jingwei_engine，非分诊表错）；`ui/*.js` grep `projList|scanPanel|renderProjList|renderDetail|analyzeRange|scanProjects` = **0**；smoke **24 checks 全 PASS**（ui.syntax + console_errors 0）；display:none 面板 **6→6**。
> **T5a 5b 簇 1 主体（2026-10-27）**：删 `scanProjects/analyzeRange/renderProjList/renderDetail` 四函数 + 绑定（events.js 分页/按钮/Enter/note 扫描、shell.js scanInit 内调用、scan.js/scan-extra.js 的 analyzeRange）；`ui.syntax`（node --check 两副本）已进 smoke 前置。smoke **24 checks 全 PASS**（含 syntax、console_errors 0）；`grep projList|scanPanel|renderProjList|renderDetail|analyzeRange|scanProjects` 仍 **19**（残留：shell.js scanCommon/analyze 调用、events.js 分页、scan.js/scan-extra.js、editor.js:773）；DOM #scanPanel 未删；Rust `list_projects/inspect` 未删（handler 仍 52）。下一刀需清这 19 处再删 DOM/Rust。
> **T5a 5b 簇 1（2026-10-27，部分）**：① 摘 editor 旧入口 `events.js:45`（不再调 `edLoadFiles`，T4 已用 `engStart`）；② 删 `edLoadFiles/edOpen/edSnapshot` 三函数 + 调用点（projects.js:59、events.js:189/204/285、editor.js:147/425/426/795）；③ Rust 删 `list_files`/`open_file` 命令（`del_fn.py --apply`，备份 `del-20261027-115432`）。handler **72→52**；`grep jingwei_engine` 仍 **37**（这两命令不引用旧内核）；build EXIT=0；smoke **23 checks 全 PASS**（含 console_errors 0）；`display:none` 面板 6→6（scanPanel/ed-wrap 等 DOM 未删）。教训：删 JS 函数必须先 `node --check`——这次删 `edSnapshot` 留下孤立 `else`、删 `edOpen` 留孤立 `});`，两次都是语法错但 smoke 文本断言抓不到（只有 console_errors 抓到）。
> **T5a 第 4 步（2026-10-27）**：18 个已删命令的 UI 引用清零——删 `editor.js` 的 `renderEdHistory`（`events_summary`）两副本、web `shim.js` 的 `graph_stats/hardware_fast/machine_report/clean_items` 映射 + DENY 里 `scan_clean`；grep 命中 **shell 1→0 / web 5→0**；`grep jingwei_engine` 仍 **37**（Rust 未动）；build EXIT=0；smoke **23 checks 全 PASS**（`ui.copy_sync` PASS）。DOM 块未删：6 个 `display:none` 面板（scanPanel/ed-wrap/noteTop/notebook/pipeline/runs）保留——删 DOM 需连带删 `scanProjects/renderProjList` 等旧函数（它们 `$('#projList')` 判空才安全），留第 5 步按可达性一起处理。
> **T5a 第 3 步（2026-10-27）**：删 `src-tauri/src/graph.rs`（4 函数第 2 步已删，仅剩 `graph_db_path` 引用）+ `mod graph;` + `use graph::graph_db;`；`grep jingwei_engine` **40→37**；handler 仍 **54**；build EXIT=0；smoke **23 checks 全 PASS**。`state/Verdict/native/clean` 在 `jingwei-shell/crates/engine`（旧内核 crate）内、`src-tauri` 无引用 → 删它们属 T5b（crates 树），不在本步范围。
> **删除工具（2026-10-27）**：`.pro/exp/del_fn.py`（输入 `file:symbol` → 打印起止行+首尾预览+attr_included；`--apply` 才写盘并自动备份 `.pro/bak/del-<ts>/`）。教训：删 `#[tauri::command]` 函数必须从属性行切到配对右括号；不要用行首 `}` 判断；括号计数 + 预览。
> **T5a 第 2 步（2026-10-27）**：删 18 个死命令（`selfcheck::save_report`、`scan::scan_roots`、`graph::{index,stats,dump,runs}`、`channel::channel_start`、main 的 `hardware_fast/machine_report/deep_check/watch_start/watch_stop/deep_changes/scan_clean/clean_items/note_kinds/profile_remove/events_summary`）——函数体 + invoke_handler 条目；`grep jingwei_engine` **54→40**；handler 命令数 **72→54**；shell build EXIT=0；smoke **23 checks 全 PASS**。回滚点 `.pro/bak/t5a-start/` 实测可用（前两次删坏均恢复：①行首 `}` 边界误删 main.rs 结构；②`#[tauri::command]` 属性残留致重复宏；第 3 次按括号计数 + 属性前缀整块删才绿）。
> **ADR-14（2026-10-27）**：edges 在 0.4.1-rc 只返回结构边（use/mod/impl），与 `jw edges` 全量（58）不一致；UI 显式标注"仅结构边"并指向 `jw edges`；T6 统一（加 `kinds` 参数或全层）。接受偏差=被记录的决策，不是无人管的漏洞。
> **T4.1 edges 同名不同义（2026-10-27，fallback 路线）**：`jw edges` total 58（全量含 call/test）vs `engine_edges`/serve 11（仅 use/mod/impl）。5 分钟内改 facade 全量牵动 engine/serve/parity/UI 四处，选 pro 允许的诚实 fallback：editor 显示 `边 11 条（仅结构边 use/mod/impl；调用/测试边未显示，全量见 jw edges）`；对账表 editor 行标"部分：state/exit 一致、边数口径不同（11 仅结构边 vs 58 全量），已显式标注"。全量 `kinds` 参数留 T6。
>
> 版本 `jw 0.4.0 (rules 27)`；rules 未变（本次只加只读 LSP 层，不改派生语义）。

```
cd D:\projects\jingwei
target\debug\jw.exe --version                 # jw 0.1.0 (rules 27)
cargo test --workspace                          # 188 passed / 0 failed
powershell -File tools\gate-control.ps1         # rg/tauri/nushell ok，55s
powershell -File scripts\exp-v1.ps1             # 四段：golden + self 16/16 + accept-new 30/30 + dashboard（6s）
powershell -File scripts\package.ps1            # dist\jingwei-v0.1.zip（内容 + architecture.md hash 校验）
```

发布清单/证据索引：`docs/reports/v0.1.md`。
**已知边界**（照此讲，不许外推）：① 空目录 `card` 全 0 + exit 2、JSON 无 `reason`；② `card <dir>` >300 文件时 `definitions` 退回 DB 下界，`symbols.definitions_note` 标注；③ `card` 写 focus 槽（非严格只读，DB mtime 变）；④ 调用图只覆盖同文件/`use` 绑定/全仓唯一 fn 名；⑤ `impact.tests` / `tests-for` 是文件级/静态可达，不是运行覆盖。

- **数字**：gate `raw == edge` 1039 / 4415 / 20954、`dedup 0`、对称差 0/0；位置门禁 use 1039/4415/20954、mod 81/265/1886、impl 249/550/2165（`null 0`）；
  `?use` 0 / 85 / 186（tauri 41 item_missing + 22 glob + 22 shadow；nushell 114 glob + 71 shadow + 1 item_missing）；`mod ?` 3→0；`check` 十一项 × 四库（三仓 + demo）全零。
- **本轮交付**：Step2/3 leaf 别名与 `pub use` 污染（rules 6→9）→ E1 位置多态（mod/impl 行号、子模块路径、`r#`；11→15）→ E2 能力三态 +
  A 读时能力闸与 `check` 两项（`jw capability --json` 对外契约）→ F′ `?use` 原因码 + `::` 绝对路径修复（17）→ I `use crate as X` 落 crate 根（18）→ H `#[path]` 的 mod 目标（19）→
  G 验收层第 4 段（`.pro/exp/b79_accept_new.py`，20 条结构不变量，计数只做证据）。
- **已知边界（不是缺陷）**：`#[path]` 属性给的 mod 目标（nushell 3 条）未做；capability 不落库；`no_symbols` 未再细分（宏/空/只 use-mod-impl）；
  dedup 下降的书面解释（README #3）未写；**仓库尚无 git 提交**（回滚靠 `.pro/bak/step2|step3`）。
- **清理**：所有 `.ps1` 纯 ASCII（`exp-scale`/`exp-v1` 已改；dashboard 断言改成 ASCII 锚 `<h1>`+`<table>`）；临时 fixture 未留；跑 jw 的任务不并行（会锁 `jw.exe` 并读到旧二进制）。
  **测试临时目录不再累积**：`product.rs::temp_dir` 改为"每轮开跑先清 `%TEMP%\jw-prod-*` 历史残留"（实测清掉 11531 个目录 / 2.8GB；现在每轮上限 = 一轮的量，
  下一轮自动清）；`idempotent.rs::temp_dir` 也改成固定命名（tag 唯一）。清完后实测单轮残留 148 目录 / 18MB（datacore 的 `jw-test-*` 本就是固定名）。

---

# 0. 一页速览

- 项目：终端与 IDE 之间的「理解层」。核心 = **判断（judgment）**：`Γ ⊢ J`，三值 `held / refuted / uncertain`。
- 代码：`D:\projects\jingwei`（Rust workspace，3 crate）。**121 条测试全过**（datacore 31 / collect 5 / cli 85）。
- 练习仓：`D:\projects\demo`（从归档仓 clone，已 ingest/docs/parse；决策记录模板 `docs\decisions\_模板.md`，断言文法 `.pro\断言文法.md`）。
- 当前阶段：**13 道工序全部有实现**（采 git/文件/构建/记录 → 归一化 → 判定 → 合成 → 校验 → 折叠 → 投影 → 呈现 → 交互 → 订阅 → 触发）。`jw serve`（NDJSON：ping/stat/card/refresh/watch，带 v/gap/events）+ `refresh`（三条流入单入口）+ `deps`（Cargo 清单/lock）。外部契约 `docs/reference/serve-protocol.md`，示例 `tools/watch.mjs`。
- 三仓实测（2026-10-25）：ripgrep / requests / tauri 全链跑通；parse 改为**默认全量**（`--limit 0`），并打 `覆盖：rust X 文件 / Y 有符号`；`macro_rules!` 已映射为 kind `macro`（rg 覆盖 89→97/112，tauri 330/332）；`jw coverage` 列未解析 .rs 清单；依赖 subject 命名 `dep:<name>` / `dep:<name>@dev|@build`（同名不再撞 id）；非 Rust 卡片写 `符号 未解析（rust only）`，deps 遇 package.json 写 `npm 未采集`。
- 实验协议（用户定的）：**黑盒**——先盲跑记录数字，再用与 jw 无关的实现（git / grep / python tomllib）独立复算，最后才允许看源码解释差异。
- 交叉验证结果（rg / tauri）：符号 97 vs grep 99（2 条是宏 token 树 / raw string 里的假阳性，jw 对）；tauri 330 vs 330；deps 修完后 rg 40==40、tauri 166==166。修的三处：全树扫 Cargo.toml（原来只扫 workspace 成员，漏 rg 的 fuzz/）、`[workspace.dependencies]` 只计数不入依赖（原来当依赖，多算 3 条）、重跑时过期已删的依赖。
- 决策卡口径：机器只认**显式结论**（选它/否决）；散文式选项属记录写法问题，由模板约束（表格+结论列），解析器不猜——jw record --lint 只警告、exit 0。
- 折叠语义：ingest 的 change 行是**历史事件**（永不过期，否则 as_of 断）；只有“关于当前状态”的判断在重采时过期旧的（parse/verify/resolve）。fold 只折“反复采过的状态判断”。
- rg/k8s 上 `fold` = 0 段（新鲜库：每个文件只采过一次，没有旧行）——预期结果，不是 bug。
- scratch 仓验证（同文件 10 版本、mtime 每次 +1 天、各 parse 一次）：fold 1 段 / 删 8 留 2 / gist 1；15 个 as-of 点前后一致；二次 apply 幂等。
- 卡片密度：W=48 预算（实际裁剪 44 留余量）；行预算 C1 文件 6 / C2 提交 5 / C3 依赖 6 / C4 模块 8 / C5 决策 7；ρ 用非空行算容量，真实卡 0.48–0.75。
- 对照组也在 `cargo test` 里（仅 fixture）：`crates/control` 已拆出 lib，`crates/cli/tests/control_fixture.rs::control_fixture_zero_diff` 在临时 fixture 上比 git/parse/edges 原始层，零差；改坏 control 一个计数→测试红（101），恢复→绿；<1s。rg/tauri 大仓仍留给 `tools/gate-control.ps1`。
- nushell 扩仓（浅克隆 300 提交）：`ingest --limit 200`（1249 改动）/ parse 40s / check 七零；raw-stats {use 9301, mod 2690, impl 2890} 与 B 完全一致（零差）。
- 发现两处待办：① 部分文件（如 crates/nu-protocol/src/parser_path.rs）有 use 却 0 边/0 符号——parse 跳过原因未明，导致 gate-control 在 nushell 上 FAIL（raw 20954 vs edge 12259）；② `card` 不按 entity 聚合（k8s 移动过的文件 27 vs 39）。
- A 已修（nushell 漏文件真因）：①`parse --limit N` 达到上限会**静默跳过剩余文件**——现在打印「剩余 K 个文件未解析（要全量用 --limit 0）」；②`resolve` 重写 `?use` 行时按 (src,dst) 删除，把同 dst 的其它行一起删了——已改成按 (src,dst,kind,line)；nushell 全量后 use 边 20954 = 原始层，`gate-control` 三仓全绿（rg/tauri/nushell），历史追加在 `.pro/exp/out/control-history.jsonl`。
- B 已做（card 按 lineage 聚合）：沿 `change.old_path` 向上追（≤20 跳）得 lineage，改动数跨旧路径累加；`--json` 加 `lineage`。k8s 被移动文件 27 → **39**（= path+old），P95 100–136ms；fixture（1 文件改名 2 次）改动 3、lineage 3 条。`b:<blob>` 仍只表内容同一性。
- C5 前向跟随：card <旧路径> 沿 status R* AND old_path 链式跟随（≤20 跳），文本显示「已改名 → 新路径」「改动 39（跨 2 个旧路径）」，--json 加 moved_to；新增 ix_change_old_path，k8s P95 91–97ms。
- impl 解析：trait → 内部定义文件（唯一才解析）、找不到标 ext:、多义留 ?；edges --stats 与 serve 都出 impl 三分类（rg 120/121/4、tauri 38/430/9，和=总数）。黄金层含 moved_to。
- `edge.leaf` 语义按 kind：use 的 leaf = 导入名；impl 的 leaf = **声明所在文件**（src 仍是 type:X）；不再塞第三种含义，需要时加列。
- 边层进卡片：C1 文本行「导入 n（内部 a / ext b）· 实现 c」，C4 目录聚合同款；`--json` 加 `edges:{use:{internal,ext},impl}`；C1 `--state deps` 展开 top3 导入目标 + 还有 N。impl 边的 `leaf` 存**所在文件**（src 仍是 type:X），因此按文件/目录统计实现数；tauri acl.rs：internal 2 / ext 20 / impl 0，与 SQL 逐项相等；deps 态 13–27ms。
- 跨仓 v0：jw cross <A> <B> --dep [--json]，版本取自 dep 判断 env（version/resolved，未知标 ?）。实测 rg↔tauri 共有 9·不同 8；tauri↔nushell 48·47；与两库 SQL 逐项相等；未建表、未联网。
- 跨仓版本漂移分级：same-major / minor-drift / major-drift / unknown（0.x 按 minor 比；*/git/path=unknown，不猜），手写 semver 无新依赖；输出按 major→minor→same→unknown 排序，--json 给 drift 计数。rg↔tauri：4/5/0/0；tauri↔nushell 见输出。
- npm（第二生态）：package.json deps/devDeps + package-lock.json resolved；subject dep:npm:<name>；workspace:/ile:/git:/link: 记 unknown 不猜。tauri 42（unknown 1=模板 file:）、vscode 411（unknown 1=keytar file:），与 node 独立计数逐项相等；cross tauri vscode --dep 可跨生态。
- Python 最小版：只读 `requirements*.txt`（`==`/`>=` 取声明串），subject `dep:py:<name>`；`pyproject.toml` 不解析，显式输出「pyproject 未采集」。requests 7、django 5，与独立解析相等。`cross --dep --eco cargo|npm|py` 过滤（默认全部），`--json` 带 `eco`。
- 跨仓同内容文件补充：(1) 默认**排除 0 字节**并报 `excluded_empty`，`--include-empty` 才计入；(2) 报 `no_blob`（快照里没进过 git 的文件，`--files` 覆盖不到，不许当 0 对）；(3) 修了一个真 bug：`git log` 原来输出**缩写 blob**（7 位），跨库比对与空文件判定都不可靠——已加 `--no-abbrev`，并给 `ingest` 加 `--force` 重写历史行；rg/tauri/vscode/nushell 已重采。
- 跨仓同内容文件：`cross <A> <B> --files`，按 **git blob（内容寻址）** 分组，不含路径、不建表。样例：rg↔tauri 1 对（两边都是空文件，blob e69de29 —— 真实但噪声，后续可加「排除空文件」开关）；tauri↔vscode 0 对（照报）。碰撞：SHA-1 在 10^6 级可忽略（比 FNV-64 更强）。
- 从零验证（rg-full 完整克隆）：2298 提交 / 5618 改动 / wall 1s。
  ① timeline 首日 2016-02-27 == git log --reverse 首日 ✓；② days 829 > 100 ✓；③ check 七零 ✓；
  ④ 累计文件 469 ≠ HEAD 树 226，差 243 已逐项归因：242 个已删/改名掉的路径 + 1 个 submodule(gitlink benchsuite)；
  head_not_in_change = 0。这是“累计=历史路径数”与“HEAD=当前树”的定义差，不是漏采；不改 timeline 迁就。
- timeline 三数（不再只有一个累计）：历史 469 / 当前 226 / 已删 243；不变量 `历史 == 当前 + 已删`；
  `当前` 直接取 `git ls-tree -r HEAD --name-only`（与判据同源）；`gitlink` 用 ls-tree 的 160000 模式单列（rg-full 为 0——
  `benchsuite` 是普通目录树 040000，不是 submodule）；文本一行 `文件 469 历史 · 226 当前 · 243 已删`。
- dashboard：	ools/dashboard.mjs（node 内置、零依赖、86 行）→ 时间线三数/最近 30 天、Top10 文件、跨仓 deps 漂移；数据全来自 jw --json（新增 jw top --files N --json）。校验：页面 469/226/243 == timeline json；Top10 == SQL GROUP BY。
- 第三语言 Python（`tree-sitter-python`，同族唯一新 crate）：kind `py.func` / `py.method` / `py.class` +
  `import` 边 → `ext:<module>`（内部包解析不做，未识别留 ?）；`raw-stats` 出 py 四数；`coverage` 出 py 子块。
  requests：py 30/37 覆盖、import 边 300；django：2021/2945、import 11853、check 七零。
  对照组 B 加独立 Python 遍历：requests 166/545/96/311、django 2126/30774/11124/11982，**与 jw 零差**。
- Python 内部包解析（**按导入名计边**，与 raw 口径统一）：`from . import a, b` → 2 条边；`from x import *` 单列 `?py:star:<module>` 不展开。
  requests：内部导入解析后 边缘 580 条 == control 导入名 580（逐文件 0 差）；django：19420 == 19420（逐文件 0 差，star 36）；两边 check 七零；内部边 dst 文件全部存在。
  教训：旧库里的“语句级”旧边不会被增量 parse 重写（skip 集命中），必须重建库或用版本号强制全量。
- `parse_rules_version` 机制化：meta 存整数版本（当前 2）；`parse` 发现版本不符 → **全量重建**并打印 `重建：rules vX→vY`，`check` 新增 `rules_version_bad`（不符即 FAIL）。不需要删库/`--force`。
- 第四语言 TypeScript（`tree-sitter-typescript`）：kind `ts.func` / `ts.class` / `ts.interface` / `ts.type` +
  import 边（按导入名）：相对路径解析到文件（`ts:<路径>`，补 .ts/.tsx/.d.ts/index.ts），裸包名 `ext:<pkg>`。
  vscode：coverage ts 13357/15235（88%）· import 边 266337（内部 31077，dst 文件全部存在）· parse 530s。
  raw 对照：func 159614 / class 15407 / interface 16385 / type 7049 / import 269991，**与 control 零差**。
  **遗留**：逐文件「边数 == 导入名数」在 vscode 有 20 个文件不等（多为 edges = names-1），待归因（疑似同一行重复名被 PK 合并）。
- TS 逐文件归因结果：20 → **2**（都 >2MB：test-checker.ts 8.5MB、agentService.test.ts 2.4MB，被 MAX_FILE_BYTES 显式跳过，
  不是静默漏）；其余文件 jw import 边数 == control 导入名数。vscode 内部 ts 边 31078、dst 文件缺失 0。
  `A, A as B` 已按**本地绑定名**记 leaf（TS），Rust/Go 的 as 暂保留原名（改本地名会破 control 零差，待两边同步）。
  gate：rg/tauri/nushell 全绿（rules v4 全量重算）。
- Step2 leaf=本地绑定名（Rust `as`→别名、Go alias/包末段、Py 别名/首段、TS 已本地名）：jw parse/edges_resolve 与 control b_use_items 同步切换；`parse_rules_version` 升到 5。
  162 测试全绿；rg 直跑 b50 零差（raw 1039 == edge 1039，dedup 0）；tauri/nushell/requests/django/vscode 子集尚未重验（下轮补）。备份在 `.pro/bak/step2/`。
- **Step2 收尾（2026-10-26，rules v9，165 测试全绿，gate 三仓全绿 45s）**：旧说法「只有 tauri 5 条」被推翻——
  修前 gate 三仓**全红**：rg 对称差 4/4、tauri only_jw 31 / only_b 36、nushell 9/9（tauri 的 "5" 只是 |B|−|JW|，不是差集条数）。
  三处 leaf 退化 + 一处 dst 污染：①`edges_resolve.rs::expand_use` 先剥 `as` 再展开（多行组的别名一律丢，还会把 `a::{B as C` 截断）；
  ②`resolve_target` 的 `?use` 回退不保留别名 → 二次 parse 把 leaf 从别名翻回原名；③`parse.rs::expand_items` 的 `self` 分支丢别名
  （`crate::{self as nu_protocol}` 变 leaf=crate；nushell 106 处）；④`pub use` 未剥可见性 → dst=`ext:pub use …`，rg 88/tauri 225/nushell 1546 条。
  修法：expand_use 不预剥；`?use` 回退用 raw；self+别名输出 `parent as alias`；parse 与 control 各自剥可见性（B 的 local 不许是 "pub use tao" 这种非标识符）。
  `parse_rules_version` 6→9（resolve 只扫 `?%` 行，已写成 file/ext 的旧行不会重扫，只能全量重建）。
  证据：三仓 `raw == edge`（1039 / 4415 / 20954）、`dedup 0`、对称差 0/0、`check` 七零；红可复现三次（还原 expand_use→`left ["Baz"] right ["Q"]`；
  还原 expand_items→`left ["foo","y"] right ["F2","y"]`；跳过剥可见性→`use ext:pub use crate::foo::x`）。
  `?use` 增量归因：tauri 71→85（+14＝pub use 行）、nushell 111→217（+106＝pub use 行），rg 0；是"原来错标 ext、现在诚实标未解析"，未归因项 0。
  备份 `.pro/bak/step3/`；剩余已知差异：B 对 `a::b::self`（无花括号）取 "self"、jw 取 "b"（三仓 0 处，未改）。
  **版本升了就要重解析所有夹具库**：`D:\projects\demo`、`.pro\tmp\v1demo`（v1 自洽层用的 demo 副本）不重建会 `rules_version_bad=1`，
  `scripts\exp-v1.ps1` 报 `demo.structural_check_zero` 失败；重建后 v1 16/16 PASS、golden_ok=true。
- **E1 位置多态第一刀（2026-10-26，rules v11，166 测试全绿）**：边补"可点击位置"——`mod` 边原来 line 全 NULL（121/389/2671 条）、
  `impl` 边 line 全 0（不是行号），终端/IDE 里不可点（需求：OSC 8 / VS Code 终端链接，T2 文档）。
  改动：`parse.rs` 的 mod_item/impl_item 补 `line=row+1`（两处副本同步）；`store::impl_edges` 不再 `COALESCE(line,0)`；
  `cmd_edges` 新增位置投影 `loc{file,line}`（use/mod/import→src，impl→leaf 声明文件），文本输出 `  use   dst  @file:line`，`--json` 同构。
  证据：三仓 mod line null 0、impl line null/0 均 0；impl 行数 245/477/2097（比 rules v10 的 244/476/2076 多 1/1/21——旧行因 line=0 全被 PK 合并）。
  红可复现：impl 行改回 None → `edges_expose_loc_for_every_kind` 红（loc 缺失），恢复→绿；gate 三仓仍绿、v1 16/16。
  已知边界（未改）：`mod x { }`（inline）也产生一条 dst=`?x` 的 mod 边（rg 40 / tauri 134 / nushell 789 条），B 侧只数 `mod x;`，比对时必须分子集。
- **E1 收尾（rules v15，167 测试，gate 三仓绿 45s，v1 16/16）**：mod 边从"能数"修到"能指"——
  ①inline `mod x { }` 不再产文件边（但仍下钻体内 use，否则丢 in-mod test 的 use 边——v12 踩过：三仓 gate 全红）；
  ②`foo.rs` 的 `mod c;` 先找 `foo/c.rs`/`foo/c/mod.rs`，再回退同目录（集成测试 `tests/tests.rs`、`build.rs` 也是 crate 根）；
  ③`mod r#type;` 剥 `r#` 再找 `type.rs`。`parse_rules_version` 11→15（中间版本都是修上一版回归）。
  验收（独立 python 扫描 `.pro/exp/locate_scan.py`，只读源码+DB，不调 jw）：
  mod 行号多重集 rg 81==81、tauri 265==265 **零差**；nushell 1880 vs 1883 差 9 条**全部归因**（3×`#[path]` 属性、5×`r#` 原始标识符是 B 正则漏、1×B 词法假阴），真漏 0。
  use 行号集合：rg 12/4、tauri 38/4、nushell 60/19（B 多出/DB 多出）。B 多出的 110 条里 92 条在字符串或注释里（B 正则假阳），18 条在 `macro_rules!` 体内（jw 已知不展开，非漏）；
  DB 多出的 27 条全在函数体内（B 的注释剥离误吞，jw 对）。结论：**差额全归因、真漏 0**；不宣称"独立复算零差"（B 是行锚正则，不是解析器）。
  事故与恢复：用 PowerShell `WriteAllText` 改 `main.rs` 的版本号时把文件写成 0 字节（违反"只用 edit/write 改源码"的规矩），
  从 `.pro/bak/step3/cli/main.rs` 恢复后重放两处改动（版本号 + `cmd_edges` 的 loc 投影），167 测试复绿——规矩再确认一次：**PowerShell 不碰源码**。
- **E2 能力维度（2026-10-26，rules 仍 v15，170 测试全绿）**：对齐终端的 capability negotiation（先声明能力、缺了就显式降级）。
  实测问题：vscode 135 个 `.js`、vue 42 个（31 js + 11 vue）文件的 `card` 原文案是「符号 未解析（rust only）」——把"我们没有 js 能力"说成"文件没符号"，且状态 held。
  改动：新 `crates/collect/src/capability.rs`（ext → {lang, symbol, use, import}；`feature_of_kind`；`symbols_status ∈ {held,no_symbols,no_capability}`）；
  `card --json` 的 `symbols` 加 `status`/`note`，文本按三态输出（删掉硬编码 "rust only"）；`ask` 对**登记过但不支持符号**的语言（js/vue/c…）落 `no_capability`
  （`file:` 若已有存在性判断则加一行「能力 …（符号数不作数）」，不拿能力冒充命题真值）；未登记扩展名（md/toml）仍走 `no_data`，不误伤文档。
  证据：`card build/builtin/browser-main.js --json`（vscode）→ `no_capability` + 「无能力（javascript 未支持符号解析）」；vue `ast.ts` → `no_symbols`（有能力、没解析，诚实三态）；
  红可复现：把 js 标成 `symbol: true` → `capability::tests::unsupported_languages_are_no_capability` 红（exit 101），恢复→绿；gate 三仓绿、v1 16/16、cargo test 170。
  未做边界：capability 未入 judgment/schema 列（不落库）；`no_symbols` 的文案未再细分（宏未展开/空文件/只 use-mod-impl），留给 coverage 侧。
  B 侧独立复算（flash 的 `.pro/exp/capability_scan.py`，自带手写扩展名策略表、不读 jw 源码/DB，逐文件跑 `jw card --json`）：
  vscode checked 777（不支持符号 177 = js135/jsx1/mjs11/cjs5/c1/cc1/cpp17/h2/hpp4，支持 600），**violations 0 · supported_violations 0 · errors 0**；
  vue checked 252（不支持 47 = js31/mjs5/vue11，支持 205），**violations 0 · 0 · 0**。截断口径：按扩展名各取前 200（vscode ts 200/15138、tsx 200/268；vue ts 200/485）。
  与 pro 判据的一处偏差（有数据）：`ask file:b.js` 返回 **held + 一行「能力 …（符号数不作数）」**，不是 uncertain(no_capability)——`file:` 的 held 是"文件存在"的命题，
  用能力覆盖它会变成"拿能力冒充真值"；`ask symbol:b.js#x`（符号类）才是 `no_capability`。`.md/.toml` 等未登记扩展名仍走 `no_data`（不误伤文档）。pro 已裁定"你对"。
  `ask --json` 另带 `capability:{lang,symbol,use,import,note}`，消费者不会把 held 读成"已理解"。
- **E1-B 升级（2026-10-26，rules v16，170 测试）**：把对照组 B 从"行锚正则"换成 **AST 位置表**（`control` 新增 `locs.by_file`：use/mod/impl 的行号多重集，
  use 重数=导入名个数、mod 只算 `mod x;`、impl 只算带 trait 的），新门禁 `.pro/exp/b70_loc_gate.py` 逐文件比多重集。**结果三仓零差**：
  use 1039 / 4415 / 20954，mod 81 / 265 / 1886，impl 249 / 550 / 2165，`null_or_zero_line=0`，全部 `bad 0`；
  旧的那三组"归因差额"（use 12/4、38/4、60/19）**全部消失**——原来都是 B 的正则噪声，不是 jw 的错。
  顺带修掉 jw 一个真缺陷：`file_edges` 原来只收 depth==1 的 mod/impl，inline `mod tests { impl … }` 里的 impl 全丢；现在不按深度过滤，
  impl 行数 245→249、477→550、2097→2165。
  红可复现：把 control 的 use 行号 +1（仅 control，不动 jw/不重解析）→ loc gate rg `bad 90`、exit 1；恢复→三仓 OK。
  **踩坑**：`cargo test` 不会重建 `control.exe`（只建测试用的 bin），改完 control 后必须 `cargo build --workspace` 再跑门禁，否则门禁读旧二进制（这次先红后绿就是它）。
- **A：能力闸落到读时（2026-10-26，rules 仍 v16，174 测试全绿）**：`datacore::capability` 定义 `Feature/Caps/AllowAll`、`feature_required`（**只有 `symbol`→Symbol**，`doc/file/change/decision/dependency…` 都不依赖语言能力）与 `path_of_subject`；
  `query::ask_with(store,subject,as_of,caps)` 在读取时 mask 掉"需要能力但该路径不支持"的判断：全 mask → `uncertain(no_capability)`，部分 mask → 只 compose 剩下的并把 `masked` 计数带出来（不静默丢）；`query::ask` = `ask_with(AllowAll)`，旧语义不变（回滚=传 AllowAll）。
  CLI 四处（ask/card/explain/why）统一传 `collect::capability::LangCaps`；`check` 增 **`capability_violation`**（判断侧）与 **`edge_capability_violation`**（边侧：use/mod/impl 要求 rust、import 要求 py/ts/go）两项，三仓+demo 全 0；新命令 `jw capability --json` 输出能力表（外部契约，不落库）。
  顺手修一个真 bug：subject 解析原用 `rsplit_once('#')`，遇到原始标识符 `symbol:…/process.rs#r#async` 会把 path 切成 `…process.rs#r`（能力核对因此误报 1 条）；`rsplit→split` 改 9 处（datacore/collect/cli）。
  证据：`ask symbol:a.js#x --json`（vscode）→ `reason=no_capability`、exit **2**（no_data 仍是 3）；`check` 三仓 `capability_violation=0 edge_capability_violation=0`；B 黑盒（flash 脚本，逐文件跑 jw、不读源码/DB）：
  vscode 不支持侧 **177/177 → violations 0**、支持侧 567（skip 33）→ 0 反例、errors 0；vue 47/47 → 0、支持侧 189（skip 16）→ 0、errors 0。
  红可复现：把 cmd_ask 传 `AllowAll` → `capability_masks_symbol_judgment_on_unsupported_lang` 红（held 而非 no_capability）；恢复→绿。测试 170→174（+datacore 2、+cli 2）。
  已知边界：`file:b.js` 的 held 仍 held（存在性命题，加一行能力说明）；`.md` 等未登记扩展名不开口；capability 不入 judgment/schema。
- **F′：`?use` 原因码台账（2026-10-26，rules 仍 v16，177 测试全绿）**：不再"沉默地留 `?`"，每条未解析 use 给一个闭集原因码
  （`collect/src/gap.rs`：`glob` / `crate_root_alias` / `path_attr_missing` / `module_shadow` / `crate_item_missing` / `extern_absolute` / `other`；现算不落库）。
  `jw coverage --json` 出 `unresolved_use:{total,reasons}`；`jw edges --json` 与 serve 的 edges 动作共用同一函数出 `unresolved_reasons`（IDE 能分清缺文件/遮蔽/glob）。
  实测：rg 0；**tauri 85 = crate_item_missing 41 + glob 22 + module_shadow 22**；**nushell 217 = glob 114 + module_shadow 71 + crate_root_alias 22 + extern_absolute 9 + crate_item_missing 1**；三仓 `other=0`、`sum==total`。
  样例（tauri/nushell 各一）：`crate::interface::AppInterface`（tauri build.rs:5）→ crate_item_missing；`bundle::*` → glob；`image::ImageDecoder` → module_shadow（本仓有 image 模块）；
  `crate as nu_protocol` → crate_root_alias；`::windows::Win32::…` → extern_absolute（**真 bug，已于下一刀修掉，见下条**）。
  B 独立（flash 脚本按文件系统/Cargo.toml/源码自己判码，只从 jw 拿"有哪些行"）：tauri expected = 22/41/22、nushell = 9/114/22/1/71，**逐码计数与 jw 全等**、other 0、total 相等。
  红可复现：注释掉 `glob` 分支 → `unresolved_use_reasons_are_closed_set` 红（glob 归到 other，`other!=0`）；恢复→绿。测试 174→177。
  **踩坑**：`jw.exe` 被并发探测任务占用时 `cargo test` 会 `failed to remove jw.exe / 拒绝访问`，且会**读到上一次构建的旧二进制**（这次 flash 的对比因此先"不等"，
  rebuild 后逐码全等）。规矩：跑门禁/探测前先 `cargo build --workspace`，且别让两个"跑 jw"的任务并行。
- **修 `::` 绝对路径（2026-10-26，rules v17，178 测试全绿）**：F′ 的 `extern_absolute` 暴露的真 bug——`use ::windows::Win32::X;`
  是 extern prelude 的绝对路径，原来被当内部模块查、留 `?use`。`parse::resolve_use` 与 `edges_resolve::resolve_target` 都在求 segs 前判 `::` 前缀 → `ext:<rest>`。
  证据：nushell `?use` **217→208**、`extern_absolute` **9→0**，这 9 条现在是 `ext:windows::Win32::Foundation::…`（库里 `dst like 'ext:windows%'` 共 23 条）；
  rg 0、tauri 85 不变；三仓 gate 绿、位置门禁零差、v1 16/16、`check` 十一项全零。
  红可复现：去掉 `::` 分支 → `absolute_extern_use_is_external` 红（`?use ::…` 又出现）；恢复→绿。测试 177→178。
  其余原因码是**合理的 `?`**，不再修：`glob`（`use a::*` 指模块内容，没有单一文件目标）、`module_shadow`（跨 crate 同名模块，无法二义消解）、
  `crate_item_missing`（模块内没有该 item）、`crate_root_alias`（`use crate as X` 指向 crate 根，模型里没有"crate 根"节点）。
- **G：外部契约验收层（2026-10-26，178 测试，rules 仍 v17）**：`scripts/exp-v1.ps1` 从 3 段变 **4 段**（golden / self-consistency / **accept-new** / dashboard），
  新增 `.pro/exp/b79_accept_new.py`：自带临时 fixture（不依赖 demo），真仓证据跑 tauri + vscode（缺仓 skip 不 FAIL）。
  **只锁结构不变量、不锁计数**：`sum(reasons)==total`、`other==0`、code ∈ 闭枚举；`edges --json` 每行 loc 的 file 存在且 `1<=line<=len(file)`（直接读源文件，不经 DB）；
  能力用手写 spec（rs/ts/js/vue，不 import collect）：`card` 的 `symbols.status`、`ask symbol:<js>#x` 必须 `reason=no_capability`+exit 2；
  计数（tauri 85 / nushell 208）只写进 `accept-new.json` 当证据，**不进阈值**（下一刀修 H/I 时计数会变，断言不动）。
  结果：`b79` **20/20 PASS**；`exp-v1.ps1` → golden_ok、self 16/16、accept-new 20/20、dashboard ok、wall 5s。
  红可复现两次：① `gap` 的 `module_shadow` 分支临时返回 `other` → `gap_other_zero` 红（17/20，exit 1）；② mod 的 `line` 临时改回 `None` → `edges_loc_valid` 红（19/20）；
  恢复→20/20。踩坑同前：改完 jw 必须先 `cargo build`（b79 直接跑 jw.exe）。
- **I：`use crate as X` 落到 crate 根（2026-10-26，rules v18，179 测试全绿）**：新增 `parse::crate_root_file(root, rel)`（从文件往上找最近 `Cargo.toml`，
  取 `src/lib.rs` 或 `src/main.rs`），`parse::resolve_use` 与 `edges_resolve::resolve_target` 两段都认 `target=="crate"`。
  证据：nushell `?use` **208→186**、`crate_root_alias` **22→0**（这 22 条现在是各自的 crate 根文件）；rg 0、tauri 85 不变；
  gate 三仓绿（54s）、位置门禁零差、v1 四段全绿（golden/self 16/16/accept-new 20/20/dashboard）、`check` 十一项全零。
  红可复现：把两处 `crate` 分支同时关掉 → `crate_alias_resolves_to_crate_root_file` 红（`left ["?use crate as me"] right ["src/lib.rs"]`）；恢复→绿。测试 178→179。
  剩余 `?use`（tauri 85 = 41+22+22；nushell 186 = 114 glob + 71 shadow + 1 item_missing）都是**合理的 `?`**，不再修。
- **H：`#[path = "..."] mod x;`（2026-10-26，rules v19，180 测试全绿）**：属性目标由属性给出（相对声明处文件目录），
  `parse.rs` 新增 `path_attr_of`（读紧邻的连续 `attribute_item` 里最后一个 `#[path]`）与 `resolve_attr_path`；`mod` 边不再留 `?x`。
  证据：三仓 `mod` 未解析 **3→0**（nushell 的 `tls/mod.rs:10,14 impl_tls` + `build.rs:23 theme`）；rg 81 / tauri 265 / nushell 1886 条 mod 边全部有目标文件；
  gate 三仓绿、位置门禁零差、v1 四段全绿、`check` 十一项全零。
  红可复现：让 `path_attr_of` 直接返回 None → `path_attr_mod_target_is_resolved` 红（`?impl_tls` 复现）；恢复→绿。测试 179→180。
- **测试映射 v0（2026-10-26，rules v21，181 测试）**：`#[test]` / `#[tokio::test]` → 边 kind `test`；其它 `*::test` 自定义属性宏（nushell 的 `nu_test_support::test`）→ kind `test_macro`（**两类分开，不混报**）。
  边语义：`src = test:<file>#<fn>`、`dst = 所在文件`、`leaf = 测试函数名`、`line = fn 行`；`card --json` 出 `tests:{n, macro_n, items[{name,line,kind}]}` 与文本「测试 N（…）」；
  `coverage --json` 出 `tests:{total, standard, macro, by_file, macro_by_file}`。属性链要越过注释行（`#[test]` 与 fn 之间夹注释仍算），否则会漏 10 条（tauri 2 / nushell 8）。
  证据（B 机械对表，`.pro/exp/t_probe_tests2.py`，只看源码正则、不读 DB）：rg std 494/510、tauri 348/352、nushell std 7529/7580 + macro 109/110；
  差值**逐条分类精确相等**：rg 16=16（全在 `macro_rules!` 体内，jw 正确地不展开）、tauri 4=4（同因）、nushell 51 = 注释 29 + 字符串 1 + 宏体 21、macro 1=1（文档行）；三仓 `unknown=0`、无 jw>regex 反向差。
  测试 180→181；`check` 十一项仍全零（test 边按 `dst` 文件判能力）；gate 三仓绿、位置门禁零差、v1 四段绿（accept-new 20→27 条：加 tests 结构不变量 + 卡片测试一致性，仍不锁计数）。
  红可复现：让 `has_test_attr` 直接返回 None → `test_edges_map_test_functions` 红（`left Some(0) right Some(4)`）；恢复→绿。
- **测试映射 v0 收尾（2026-10-26，rules v22）**：pro 查验发现两个 P0 缺陷，已修——
  ① `edges` 看不到 test 边（`edges_located` 只按 `src==path` 取，而 test 的 src 是合成 subject）→ 改成 `src=?1` UNION `(kind in ('test','test_macro') and dst=?1)`；
  `jw edges <file> --kind test|test_macro` 可查，文本显示测试名 + `@file:line`，loc 取该文件；
  ② 属性闭集漏 `#[rstest]` / `#[quickcheck]`（nushell 539、tauri 4）→ 归 `test_macro`（`#[proptest]` / `proptest!` 宏体仍不纳入：token 树里没有可遍历 fn，列 P2）。
  计数（rules v22 全量重建）：rg 494（全 standard）、tauri standard 348 + macro 4、nushell standard 7529 + macro 574（rstest 539 + `rstest::rstest` 9 + `nu_test_support::test` 等）；`card defs.rs`（rg）→ 测试 112。
  验收 accept-new 27→**28** 条（新增 `edges_tests_visible`：edges 里的 test 行数==card.tests.n 且 loc 合法）。
  红可复现两次：① 属性分支只留 `::test` → `test_edges_map_test_functions` 红（Some(4) vs Some(5)）；② `edges_located` 的 test 分支加 `AND 0=1` → `fixture.edges_tests_visible` 红（27/28）；恢复→绿。
- **cite v0（2026-10-26，rules 仍 23，182 测试）**：研究引用要"能复现到具体提交"（GitHub permalink 语义）。
  `jw cite <subject> --json`，v0 支持 `symbol:<path>#<name>` 与 `file:<path>`；每行 `{file,line,commit,verdict,evidence_kind}`，commit 来自 `git blame --porcelain -L n,n`
  （不读 DB 造 commit）；symbol 多定义/多行 → 逐行一条；文件级引用定位第 1 行；空文件/不在版本里/坏 subject → **exit 2 + reason**（`bad_subject` / `no_line` / `git_failed`），失败不给 rows。
  证据：B 机械核对（`.pro/exp/c_probe_cite.py`，只调 git、不读 DB，seed=20261026）：rg / tauri 各 checked=20、**commit_match=20/20**、reason_ok=15/15、idempotent=5/5、`blame_log_mismatch=0`、errors=0；
  flash 另做破坏对照（哈希换成 `0*40`）→ commit_match=0、mismatch=20，证明比对是活的。
  验收 accept-new 28→**29** 条（`cite_permalink`：行在文件内、commit==HEAD、两次输出逐字节相同）。
  红可复现：把 `git_blame_line` 临时改成 `git rev-parse HEAD` → `cite_gives_permalink_commit` 红（`left c2 right c1`）；恢复→绿。
- **cite `file:` 语义修正（2026-10-26，P1）**：pro 复算发现 `cite file:…` 用第 1 行 blame（许可证行）冒充"文件引用"会误导 →
  现在 `file:` 的 `commit` = `git log -1 --format=%H -- <path>`（文件最后一次变更），并给每行加 `file_commit`（symbol 行也带）；
  rg `defs.rs` 从 5c42c8c 改成 **cff2849**，与 `git log -1` 一致。红：分支改回 `git_blame_line(...,1)` → 该测试红（c1 vs c2）；恢复→绿。
- **调用图 v0（2026-10-26，rules v24，183 测试）**：LSP Call Hierarchy 的最小落地——**同文件、裸标识符调用**。
  parse 加 `call_expression` → 边 kind `call`：`src=symbol:<file>#<caller>`、`dst=symbol:<file>#<callee>`（同文件命中）或 `?call <name>`、`leaf=callee`、`line=调用行`；
  `jw edges <file> --kind call [--to <subject>]`（`--to` = incoming 反查，`store.call_edges_to`）；`edges_located` 按 `symbol:<file>#%` 把 call 行并入文件视图。
  实测：rg 2630（未解析 1739）、tauri 4380（3160）、nushell 31650（24089）——未解析多是 `Ok(...)`/方法/std，v0 只认同文件裸调用，**不猜**。
  不变量（`.pro/exp/b82_call_inv.py`）：每条 call 边的行去空格后确实含 `callee(`（三仓 bad_line=0）、行号在范围内（bad_range=0）、caller 是真实 `fn`。
  fixture：10 条手工期望表（含递归、未知 callee；方法调用不入图）10/10；`--to` 反查 c 得 4 条。
  验收 accept-new 29→**30** 条（`call_edges_reverse`：outgoing 每条都能被 `--to` 反查回来）；gate 三仓绿 54s、位置门禁零差、v1 四段绿。
  红可复现两次：① 注释掉 `collect_call_edges` → `call_edges_same_file_only` 红（`no edges`）；② `call_edges_to` 临时返回空 → `fixture.call_edges_reverse` 红（29/30）；恢复→绿。
- **后端健壮性（2026-10-26，P0）**：pro 在 `D:\projects` 触发 `edges --stats --json <repo>` 把 repo 当 path、在 cwd 静默建库 → 已修：
  ① `Store::open_existing`（无 `.jingwei` → `exit 2 + no_db`，**绝不建库**），只读命令（edges/coverage/check/stat/card/ask/why/explain/find/deps/conflicts/alt-list/cite/cross/project）全部改用它；
  ② `edges --stats <repo>` / `--to <subject> <repo>` 单位置参数按 root 解释；
  ③ `edges --stats` 补 `call{total,resolved,unresolved}` 与 `test{standard,macro}`；④ `--to` 未知 subject 带 `subject_known:false`。
  冒烟：空目录 `edges --stats .` → `no_db` + exit 2 且**无 `.jingwei`**；rg 上 stats 出 `call 2630（已解析 891 · 未解析 1739）`、`test 494`。误建的 `D:\projects\.jingwei` 已删。
- **serve 补 action（前端契约，2026-10-26）**：`find` / `ask` / `cite` / `coverage` 四个只读 action（复用 `find_json`、`cite_payload`、`query::ask_with`、gap 台账），
  与 CLI 同一份实现；`docs/reference/frontend.md` 记字段名（serve 只认 **`method`**、`stat` 键是 `judgment/evidence/change`、card 在 `result.card`）。
- **前端 v0（2026-10-26）**：`tools/web/server.mjs`（零依赖桥：HTTP `/rpc` → NDJSON `method`，SSE `/watch` 转发 subscribe）+ `tools/web/index.html`（单文件 UI：
  左侧搜索 + 仓库概览，右侧文件卡/测试/导入/调用/permalink）。实测：浏览器 → 桥 → `jw serve` 全链路通，截图 `.pro/exp/out/web-ui.png`（rg：判断 3185、依据 4251、测试 494、调用 2630 = 891+1739）。
- **发布包（2026-10-26）**：`target/release/{jw.exe,control.exe}`（release 编译 1m13s）→ `dist/jingwei-v0/`（bin/web/docs/README）+ `dist/jingwei-v0.zip`（3.0MB）。
- **跨文件 call v1（2026-10-26，rules v25，184 测试）**：`?call X` 在**全仓 fn 定义唯一**时解析为 `symbol:<定义文件>#X`；多义/未定义保持 `?call`（不猜）。
  `resolve_all` 用 `store.symbol_def_index()`（symbol judgment + evidence premise 的 kind）建 name→files 索引。解析率：rg **891→1460**（2630 中未解析 1170）、tauri **1220→1618**（4380 中 2762）、nushell **8261→11620**（31650 中 20030）。
  不变量（`b85_call_v1_inv.py`）：每条 resolved call 的 dst 文件里确实有 `fn <name>`（三仓 bad_fn=0）、调用行去空格含 `callee(`（bad_line=0）。
  fixture：`a.rs` 调 `b.rs` 唯一的 `helper` → 解析；`dupe` 在 c.rs/d.rs 各有一份 → 保持 `?call dupe`。
  红：把 `files.len()==1` 分支短路 → `call_cross_file_unique_only` 红；恢复→绿。
  顺带修 `edges` 位置参数：**只有**一个位置参数且 `--stats/--to` 时才按 root 解释（同时给 path+root 时不再把 path 当 root——b79 的 `call_edges_reverse` 因此先红后绿）。
- **call v1.5 精度（2026-10-26，rules v27，185 测试）**：pro 用 use 绑定表交叉复算，发现两类**误报**（std 绑定被本地同名 fn 抢走、内部绑定被别处同名抢走）。
  `resolve_all` 改为**作用域优先**：先建 `(file, use.leaf)→dst` 绑定表 → ① 内部绑定且目标文件确有该 fn → 指过去；② `ext:std|core|alloc` 绑定 → 保持 `?call`；
  ③ 无绑定才走"全仓 fn 唯一"；④ 都不中 → `?call`。规则对所有 call 边生效（含 parse 阶段已同文件解析的）。
  复算（`b86_call_precision.py`）：**std_bound 7/30/33→0**、**internal_mismatch 4/15/58→0**（rg/tauri/nushell）；`ext_binding` 只排 std/core/alloc（其余多为 workspace crate 真阳性）。
  解析率变化：rg 1460→1456、tauri 1618→1599、nushell 11620→10724（宁可少解析，不许指错）。
  红：`call_scope_binding_beats_name_match`；关掉 std 分支 + 去掉 bound 守卫 → 红（`read` 指到 b.rs）；关掉内部绑定分支 → 红（`helper3` 变 `?call`）；恢复→185 测试绿。
  gate 55s、v1 30/30、loc 门禁零差。
- **impact v0（2026-10-26，rules 仍 27，186 测试）**：只读命令 `jw impact <path|symbol:…> [--json]` —— IDE Find Usages / 改动波及面。
  `direct` = 反向 call（file 目标 = 该文件所有 fn）；`transitive` = 反向闭包 **depth≤2**（去重、排序、排除目标自身与已在 direct 的）；
  `tests` = 受影响**文件**里的 test/test_macro 边（`scope:"file"`，**不**承诺"覆盖该符号"）；`importers` = `use` dst 指向该文件的边；
  `completeness{call_resolved,call_unresolved,depth_cap:2}` 随结果给（"没人调用"与"没解析出来"不许混）；symbol 目标不存在 → exit 2 `no_symbol`。
  证据：fixture（a→b→c、c 自递归、lonely 无调用者、b.rs 内 `#[test] tb`）全过；真仓等价：tauri 抽 5 个 fn，`impact.direct` 与 `edges --to` 的 total **5/5 相等**（50/28/25/19/19）。
  红：砍掉 `trans.push` → `impact_reverse_closure_and_tests` 红（depth-2 的 a 丢）；恢复→186 绿。gate 56s、v1 30/30。
- **serve/web 出口 parity（2026-10-26，P1）**：新能力必须走同一 NDJSON 契约，否则发布包里不可达。
  serve 增 `impact`（target；返回与 `jw impact --json` **同一对象**）与 `capability`（与 `jw capability --json` 同一对象）；`tools/web/` 右栏加"影响"区块（direct/transitive/tests 标 file 级/importers/completeness）、概览加"能力"角标；`?path=<file>` 可深链。
  证据（`v_serve_parity.mjs`，独立 node 客户端、不 import jw crate）：tauri 10 个 subject 的 `serve impact` 与 CLI **10/10 深等**；`capability` 深等；3 个坏输入（no_symbol / bad_subject / 不存在文件）形状与 CLI 一致（`ok:false` + 同 reason）。
  web 渲染核对：`?path=crates/tauri-build/src/acl.rs` 显示 direct 14 / trans 0 / tests 16（file 级）/ importers 6 / completeness 1599·2781，与 serve result 逐项相等（截图 `.pro/exp/out/web-impact2.png`）。
  红：把 serve 的 `impact` 路由改名 → `deep_equal` 10→0、坏输入 reason 对上失败；恢复→10/10。

- **tests-for v0（2026-10-26，rules 仍 27，187 测试）**：只读命令 `jw tests-for symbol:<file>#<fn> [--depth 3] [--json]` —— "哪个测试是这个符号的例子/改完该跑哪个"。
  实现：call 图反向 BFS（seen 带 depth+via，默认 cap 3），与 test/test_macro 的 `test:<f>#<fn>` → `symbol:<f>#<fn>` 取交集；输出 `{target,tests:[{name,file,line,kind,depth,via}],completeness{depth_cap,tests_total,call_resolved,call_unresolved},note}`；
  `note` 固定"静态可达，不等于运行覆盖"（文本输出也带这句）。
  证据：fixture（t1→helper→target、t2 无关、t3 在 c1↔c2 循环里）——t1 depth2、t2 不出现、t3 只在目标 c1 时出现、depth=1 时 cap 内为空、目标本身是测试时 depth0 命中；**独立 BFS 复算**（`b90_testsfor_bfs.py`：用 CLI `edges --kind call --json` 自建 331 文件全图、算法独立、不读 DB）抽 5 个符号 **5/5 与 CLI 完全一致**；serve `tests_for` 与 CLI 5/5 深等、坏输入同形状（`b91_testsfor_parity.py`）。
  web：符号视图新增"可能的测试（符号级）"区块 + `?symbol=<file>%23<fn>` 深链（截图 `.pro/exp/out/web-testsfor2.png`：1 条 depth2、impact 6/3/1/13、能力角标同屏）。
  红：砍反向 BFS → 测试空；把"文件里有测试"当命中 → t2 会被列出（fixture 断言红）；删文案 → 文本断言红。

- **range card 补齐（2026-10-26，rules 仍 27，188 测试）**：模块/目录视图（学习"由浅入深"）原来只有 files/symbols/use/impl，**没有 tests/call**——链路断在模块层。
  现在 `card <dir> --json` 增 `tests{total,standard,macro}`、`call{total,resolved,unresolved}`、`edges.call`；文本卡补"测试 / 调用"两行（range 行预算 6→10）。
  **口径修正（同一份 per-file 源）**：`files` = 该前缀下**已解析 .rs 数**（parse::walk_paths，不再是历史 change 路径数）；`symbols.definitions` = **Σ 每文件 definitions（facts.mapped）**，与逐文件 card 同源；>300 文件时退回 DB 下界（不静默换口径）。
  证据（`b92_range_b.py`，黑盒逐文件求和）：tauri `crates/tauri-utils` files 27 / defs 836 / tests 63 / call 301+295 / use 104+256 / impl 211；rg `crates/core` 20 / 1252 / 115 / 609+366；nushell `crates/nu-cli` 74 / 1244 / 335 / 661+1258 —— **三项目录全 OK（0 差异）**。
  web：模块视图（`?module=crates/tauri-utils`，截图 `.pro/exp/out/web-module2.png`：27 文件 / 836 定义 / 63 测试 / 596 调用=301+295 / 104+256+211）。
  红：把 tests 聚合置 0 → `range_card_aggregates_match_per_file` 红（tests 0 vs 1）；恢复→188 绿。
  连带修正验收口径：`tests/fixtures/v1-golden/expected.json` 的 `files` 3→2（src 下只有两个 .rs；c.rs 是历史移动），`b53` 的 `dir_card_files` 改为对 `.rs` 文件计数（原为 change 路径数），golden/self 重新转绿。

- **range impact（2026-10-26，rules 仍 27，189 测试）**：`jw impact <dir> --json` → `target_kind:"range"`、`targets`=前缀下 fn 数；
  `direct_total` = 指向该前缀内符号的 call 边**全量**（明细截断 100 条 + `direct_truncated`）；`transitive` depth≤2（同样截断 + `transitive_truncated`）；
  `tests` 仍只到**文件级**；`importers` = dst 落在前缀内的 use 边；`completeness` 同 symbol/file。serve `impact` 自动同契约（同一 payload）。
  证据：tauri `crates/tauri-utils` **direct_total 302**、rg `crates/core` **609**、nushell `crates/nu-cli` **664**（与 pro 给的 SQL 期望逐项相等）；
  独立复算 `b96_range_impact.py`（逐文件 `edges --kind call --json` 自建图、不读 DB）：rg `independent_direct_total=609 == cli=609`，dst 全前缀内、三元组无重复、截断标志=true/明细 100。
  fixture：`impact_range_direct_total_and_scope`（pkg 内 2 fn、包外 2 调用、包内 1 调用 → direct_total=3；tests 带 `scope:"file"`）。
  红：把 `callers_into_prefix` 的 `dst LIKE symbol:<prefix>/%` 改成 `%` → direct_total 4 vs 3，测试红；恢复→189 绿。
- **发布包文件名 P1（2026-10-26）**：zip 里 `serve-协议.md` / `实验报告-…md` 是 GBK 字节挂 UTF-8 flag（PS 5.1 Compress-Archive 的坑）→
  改名 `serve-protocol.md` / `experiment-report-2026-10-25.md`（引用同步），`scripts/package.ps1` 加断言"所有 entry 名必须 ASCII"，重打 dist。
  **edges 输出带 caller**：`edges_located` 改返回 5 元组（多 `src`），`edges --json` 的 call/test/test_macro 行加 `from`；文本行显示 `call  caller → callee @file:line`
  （B 对表指出正向 JSON 没有 caller，这是可用性缺口；补齐后 fixture 断言 `from` 前缀，accept-new 要求每条 call 行 `from` 以 `symbol:` 开头）。
- k8s rename：72,603 条 R*；R100 抽样中「旧路径有前史」的 3/3 entity 相同，另 17 条无前史不可判。
- v1 黄金值也在 `cargo test` 里：`crates/cli/tests/golden.rs::v1_golden_acceptance`（<1s），与 `scripts/exp-v1.ps1` 共用同一份 `tests/fixtures/v1-golden/expected.json`；改坏 expected → 测试红（exit 101），改回 → 绿。
- v1 验收两层：**黄金层**（`tests/fixtures/v1-golden/` + `expected.json`，改坏任一固定计数→exit 1，改回→绿；`--update-golden` 需输入 yes 并打印 diff）+ **自洽层**（渲染==SQL、demo 结构性不变量 sum(kind)==judgment、check 七零）；入口 `scripts/exp-v1.ps1`，16/16 PASS、1s。
- v1 验收：`scripts/exp-v1.ps1`（ASCII）→ `.pro/exp/out/v1-accept.json/.md`；demo 5 卡×态 + timeline + find + fold、tauri 5 卡，每条“输出数==SQL 数”；14/14 PASS、0.3s；去掉 demo 库→3 条 FAIL/exit 1，恢复→全绿。
- 已知边界：非 Rust/Go 无符号；`fold` 新库 0 段（实体稳定长链才有效）；Go 内部 import 已映射 `dir:`、npm 依赖未采集。
- 已知限制：`macro_rules!` 调用体里的定义 tree-sitter 不展开（rg 的 tests/feature.rs 里 const 在宏 token 树内）→ 这类文件归为"无符号"是对的，但要在 coverage 里能被解释。
- `coverage` 现在标原因（只标可机械判定的）：`文件已不存在（只出现在历史里）` / `空文件/无项` / `有宏调用 N 处，token 树未展开` / `只有 use/mod/impl` / `与库不一致（AST 里有 N 个可映射项）`。
- 浅克隆：`ingest` 会当场提示；`timeline` 写 `浅克隆：只有克隆点之后的历史`，不再说"已全量扫描"。
- django 实跑（Python，浅克隆 depth 150）：7786 改动 / 7107 文件，独立 git 复算完全一致；`card django/db/models/query.py` held、改动 8（git 也是 8）、`符号 未解析（rust only）`；范围卡 `django/db` 123 文件 56ms（<200ms 判据）；check 七项全 0。
- vue（TS monorepo，浅克隆）通过 pro 判据：ts 485 / tsx 5 / js 31 全部 `未解析（rust only）`，db 里 0 条 .ts 符号；deps 打 `无 Cargo.toml（npm 依赖未采集）`；`card packages/compiler-core/src/ast.ts` held、改动 4；范围卡 30 文件 53ms；改动 1211/文件 706 与 git 复算一致。
- 五目标实验报告：`docs/reference/experiment-report-2026-10-25.md`；coverage--json 与浅克隆语义写进 `docs/reference/serve-protocol.md`。
- 已知未做（v2 方向，不是缺口）：语言层 edge（impl/关系）、模块/画布聚合、推送（现为长轮询）、LSP 诊断、record 自动化（pro 明确不做）。
- 铁律：没有脚本的数字不算结果；阶段性结果不许说成"最终/已验证"。

---

# 1. 模型 v0.1（冻结，改动需实验证据）

**12 基本概念**
`judgment / evidence / source / ctx / snapshot / change / associate+mapping / hypothesis / prediction / verify / owner / entry`

**4 子概念**（`kind_registry` 已登记，带 parent）
`impact`(←change) / `candidate-source`(←change) / `time-series`(←change) / `mastery`(←file)

**规则**
| 规则 | 落点 |
|---|---|
| `refuted` 必须覆盖完整 | DDL CHECK：`verdict<>'refuted' OR coverage_read>=coverage_total` |
| 过期不能早于产生 | DDL CHECK：`expires_at IS NULL OR expires_at>=at` |
| 证据有极性 | `evidence.polarity ∈ {supports, contradicts}` |
| 矛盾闸 | 两边都有证据 → `uncertain(conflict)`，不硬选 |
| 覆盖闸 | `coverage_read<total` → `uncertain(coverage_short)` |
| 过期 | `expires_at <= 判定基准` → `uncertain(expired)`（含端点） |
| as_of | 按 **T** 判过期（不是 now）；`threshold = as_of.unwrap_or(max(now, 该 subject 最新 at))`。为什么要 max(at)：写新判断时 at 可能被推到 now+1（躲同秒 id 撞车），旧判断的 expires_at 也随之是 now+1；基准若只用墙钟会留 1 秒窗口（E3 v2 抓到的） |
| 冲突/背离 | 同 kind 相反=冲突；跨 kind=背离，不判输赢 |
| 父子关系 | 必须显式（不能靠 subject 前缀猜） |
| 同名符号 | `symbol:path#name` = **「该文件里至少存在一个名为 name 的符号」**；每个定义一条依据（同文件两处定义 → 一条判断两条依据）；`verify` 只看这个名字还能不能找到，删掉一个定义仍 held |
| subject 与标签 | `kind` **单值**（决定 id 与合成读法，多值会让 id 不稳）；细分标签多值：环境类（`lang/cfg/vis/features`）进 `scope.env`，性质类（`generated/deprecated`…）v1.5 再加 `judgment.tags`。**`ask` 不猜**：只答给定 subject，查不到就列候选（`file:`/`doc:`/`symbol:` 形式）；路径工具（`why`/`explain`/`card`）才按 file → doc 解析，并打印所用 subject。`parent` 树只放 is-a，不放属性 |
| env 兼容 | 默认精确相等，宽松必须显式登记 |

**明确不做**：真公理、根因断言、自动因果、运行时采集（`time_series` 只有概念）、多仓库图。

**证据等级**：T1=脚本可跑；T2=一手文献；T3=推断；T4=意见。T3 不许当结论。

---

# 2. 代码结构（D:\projects\jingwei）

```
Cargo.toml                 workspace，依赖版本集中
run.cmd                    一行跑：run.cmd <命令> [参数]
HANDOFF.md                 本文件
crates/datacore/src/
  model.rs                 Judgment/Evidence/Source/Scope/Verdict/Polarity
  ids.rs                   FNV-1a 稳定 id；judgment id **带 at**
  store.rs                 六表 + 事务 + 冲突扫描 + alternative_of + meta
  compose.rs               R2 主从 + 矛盾闸 + 覆盖闸
  query.rs                 ask（as_of 过滤、过期剔除）
  schema.sql               DDL（含 CHECK）
crates/collect/src/
  git.rs                   git log 解析（0x1F 分隔）
  docs.rs                  Markdown 扫描
  parse.rs                 tree-sitter 抽 Rust 符号
  record.rs                人工记录 D-*.md 解析（decision / hypothesis）
crates/cli/src/main.rs     全部子命令
crates/datacore/tests/core.rs   27 条测试
crates/collect/tests/           5 条（git 路径 1 + record 解析 4）
crates/cli/tests/               51 条（record 4 + verify/resolve/expire 7 + 幂等/搜索 4 + 产品入口 36）+ 单元 2 条（normalize 层）
tests/fixtures/records/         record 导入夹具（2 条真实决策）
```

**命令面**
```
init / ingest / docs / parse / why / ask / verify / expire /
conflicts / search / stat / check / alt-add / alt-list / record / resolve
  explain / project / timeline / find / card / compact / resume / unpin / deps / serve / refresh

**外部契约文档**：docs/reference/serve-protocol.md（请求/响应/错误/版本/兼容规则）；示例 tools/watch.mjs
```

**跑法**（项目根）
```
run.cmd stat D:\projects\demo
run.cmd ask "symbol:crates/cli/src/cmd_clean.rs#cmd_clean" D:\projects\demo
run.cmd record D:\projects\demo\docs\decisions D:\projects\demo
cargo test --workspace
```

---

# 3. 已真实验证的行为（有输出，不是设计）

| 行为 | 观察 |
|---|---|
| git → 改动判断 | 200 commits / 1921 changes，held（T1：.pro/out/e0_recon.txt） |
| tree-sitter → 符号 | limit 2000 → 1971 条 symbol 判断（demo；T1：.pro/out/b1_review.txt） |
| 删文件后 verify | 已修（B3）：写新判断 + 旧判断过期 → `ask` = `refuted`（不再是 conflict）；T1：`verify_after_delete_turns_refuted` |
| 同判断两种证据 | `ask` → `uncertain(conflict)`，链上两种极性都在 |
| expire 后 ask | `uncertain(expired)` |
| `ask --as-of 1000` | `uncertain(no_data)` |
| `expire --at 1000` | 被 DDL CHECK 拒绝 |
| alt-add / alt-list | 候选→决策关系可写可查，自引用被拒 |
| issue 导入 | ~~open→uncertain、closed+completed→held、closed+not_planned→refuted~~ **已裁掉**（A1：流程状态冒充命题真值；旧 issue 3 条判断已删） |
| record 导入 | 11 条 / 11 decision + 15 hypothesis；`paths: raw=44 unique=27 existing=27; alts=15`；held 6 / uncertain 5（T1：.pro/out/e1.txt）。同一份记录文件同时产生 **doc 判断**（文档内容存在）与 **decision 判断**（决策状态），subject 与语义都不同，别混 |
| checker 规则（本体未建） | 16 个规则用例 **16/16**、4 个错误实现 **4/4 被抓**（T1：`.pro/out/e7_rules.txt`）；开 checker 的三层门槛见 `.pro\断言文法.md` §4 |
| `ask` 认裸路径 | `jw ask crates/cli/src/main.rs` 自动落到 `file:`（之前直接 `no_data`）；`路径#名字` 落到 `symbol:`（T1：`ask_accepts_plain_path`） |
| `explain <路径|路径#符号>` | 一屏给状态 + 改动次数 + 作者 top2 + 最近 3 次 + 文档 + 符号数；`--json` 给编辑器；退出码跟状态（T1：`explain_gives_one_screen_state`） |
| `project` | 项目概览 + 按天演进：demo 实测 147 文件 / 1921 改动 / 200 提交 / 122 文档 / 1971 符号 / 11 决策（T1：`project_shows_growth`） |
| `timeline` | 按天：提交 / 新增 / 改动 / 删除 / 新文件 / 累计文件；仓库总提交数与「已扫描」并列，截断时明确报「历史不完整」；`ingest --limit 0` 全量（T1：`timeline_counts_match_change_table`、`timeline_warns_when_truncated`；demo 600/200） |
| `find <词> [--kind]` | 按名字找符号/文件/文档/决策，符号带 `path:line`（从 `def:` 依据解析）；`search` 是别名（T1：`find_symbol_shows_line`） |
| `card <路径> [--history] [--json]` | 单文件结构卡（只读、不画图——没有 edge 数据）：状态 · 改动次数 / 符号数（定义处数 + kind 分布）/ 最近改动 / 文档 / 依据；`--history` 切按天态；`--json` 是 UI 数据契约（字段冻结）。demo `main.rs` → 30 符号 31 处定义、改动 14（T1：`card_shows_structure_and_definitions`、`card_history_shows_days`、`card_json_is_the_ui_contract`） |

---

# 4. 下一步（按顺序，一步一文件）

> **4.1 parked（2026-10-25）**：`claim` / checker / 断言文法 / 记录模板整体**推迟**——用户判定这些是"地基上的抽象"，不是产品。文件留在 `.pro`（E4/E7/文法/模板），demo 的两个 `_` 文件保留但不再推进；等真有"机器核对某条记录"的需求再启动。

1. **scope 落库**：已完成（`judgment_row` 收 scope；ingest=file、docs=file、parse=symbol、record=decision）；测试 `scope_roundtrip`。demo 已跑（4040 判断 / 4069 依据；T1：`run.cmd stat`）。
2. **as_of/expiry 按 T 判**：已完成（`query.rs` threshold），测试 `as_of_uses_t_for_expiry`。
3. **record 导入**（A1）：issue 语义已删除，命令改 `record`；11 条记录 → 11 decision + 15 hypothesis（T1：`run.cmd stat`，4040）
4. ~~PR 采集~~ 已裁掉（A1：用户不需要联网/PR）
5. **冲突 resolve**：已完成（B3）：`resolve <subject> --verdict --by --why` 写新判断 → 旧判断过期 → 冲突清账；机器不许调用
6. alternative_of 的自动来源：**已完成**（`jw record` 把显式替代项写成 hypothesis + `alternative_of`；不做 RFC/ADR 采集器）

---

# 5. 角色分工（重要，别搞错）

| 角色 | 干什么 | 注意 |
|---|---|---|
| **你（主 flash）** | 与用户对接、写代码、跑测试、维护本文件 | 不越权替用户定产品方向 |
| **子 pro** | 后台设计与审计：模型细节、施工指导、偏差清单、架构演进 | 一次派**一批**活，否则它一份就停；它的"偏差"常按旧稿写，必须让它重读当前文件 |
| **子 flash** | 外部信息检索：文献、竞品、语料、平台可达性 | 必须要求"脚本+种子+原始输出"；它自报的数字要抽查 |

**派活模板**：目标 + 产出格式（表格/文件）+ 硬约束（不编、给数字、标 T1/T3）+ 一次做完再回。

**注意**：agent 每次只跑一轮，turn 结束就停——这是机制，不是它偷懒。要连续就分批派。

---

# 6. 已经探过、别再重复的路

## 6.1 外部语料
| 源 | 结果 |
|---|---|
| GitHub API（`GITHUB_TOKEN` 已设，5000/h） | **唯一高产**：185 PR → 13,034 行内评论 → 160 严格命中（复核精度 48%） |
| GitHub HTML | 只有对话级评论，拿不到行内 review，**弃** |
| Reddit | JSON/RSS 全 403/Blocked，**弃** |
| LKML（lore git 镜像） | 可达；linux-mm 157 thread 已饱和，fsdevel clone 常坏 |
| Debian BTS | PoW 反爬（"I Challenge Thee"），**弃** |
| Lambda the Ultimate | `/node/` 502，**弃** |
| Apache JIRA | API 通但 WONT-FIX 评论多为 0–1 条，**低产** |
| Discourse（Rust/Python/Julia/Kotlin/Swift） | 可用；dependent 主要来源 |
| Gitee | 国内镜像多、讨论少，**弃** |
| 中文平台（V2EX/博客园/CSDN） | 用户明确放弃 |

## 6.2 关键数字（reviewed 口径，非启发式）
- `corpus-final.jsonl`：68 条 reviewed（content 25 / dependent 10 / coverage 7 / none 26）。
- 冲突类型有"栖息地"：dependent 只在评审过程（LKML/GitHub），coverage_gap 只在代码评审语言（GitHub），文档里几乎没有。
- 规则比较：R2 与 R3 区间重叠、**不可判**（差异 5%，要分开需 n≈739）。

---

# 7. 犯过的错（别再犯）

| 错 | 教训 |
|---|---|
| 把"语义承载力 78%"换成"结构可实例化 93%"叫"翻转" | 回放必须**同问题同度量**；换度量叫重定义 |
| judgment id 不含时间 | 历史被覆盖、冲突不可能存在；id 必须带 `at` |
| 用 subject 前缀猜父子关系 | 会配错对象；`refines/part_of` 必须显式 |
| 把 "separate PR" 当 coverage_gap | 那是 deferral；coverage_gap 必须是"承认没验证" |
| 按"封邮件"数 LKML | 同一 Re: 链重复，必须按 thread 去重 |
| 启发式判冲突（101 条） | 人工复核只剩 9 条（10:1），结论只能用复核后 |
| 用没测过的维度证明"非承重" | 矩阵里没有该维度的格子时，放松实验无效 |
| `read-tree --empty` 清死 stat 缓存 | 旧仓教训；新仓未用 |
| PowerShell 读非 BOM 脚本按 ANSI | `.cmd/.ps1` 一律 ASCII，中文注释会炸 |
| 用 PowerShell `Get-Content`/`Set-Content` 改源码 | 本机按 GBK 读 UTF-8 源文件，写回即 double-encode：中文变 mojibake、换行被吞、字符串字面量丢字节（`main.rs` 整文件损坏过一次，靠 gb18030 逆变换 + 逐行重写注释 + 编译器逐轮验证恢复）。**规矩：源码只用 edit/write 工具或显式 UTF-8 脚本改；PowerShell 只跑命令、不读写源码** |
| `cmd /c "路径.cmd" 参数` 引号形式报错 | 用 `cmd /c 路径.cmd 参数` |
| 多进程 clone 被单次 pwsh 结束杀掉 | 长任务放后台 job 并写 state |
| `put_judgment` 曾用 `INSERT OR REPLACE` | 同 id 重写时先删行 → FK CASCADE 连带删掉旧证据；同名符号 29 条 symbol 证据被吃（4040 是吃过证据的状态，4069 才是恢复）。B1.2 改成 `ON CONFLICT DO NOTHING` 后才对 |
| 直接用 sqlite3 改库没开 `PRAGMA foreign_keys` | DELETE 不级联，留 1262 条孤儿证据；改库必须走本 crate 或显式开 FK，再用 `check` 验 orphan |
| 度量脚本扩展名交替 `js` 写在 `json` 前 | `.json` 被截成 `.js`，3 个真实文件被当成不存在（E1 v4）；交替要长的在前，或改用 `ends_with` |
| 给某 subject 写入未来日期的 at | 该 subject 的过期基准会被推后（**per-subject**；`as_of` 显式给定不受影响）。未来 at 是数据 bug，**不加 cap**——加 cap 会让同秒连发失效（B3.2 的已知残留） |

---

# 8. 环境与工具

- Rust `1.98.1`，cargo 同版本；workspace edition 2021。
- `GITHUB_TOKEN`：已设为**用户环境变量**（来自 git credential 里的 `gho_` OAuth），只读公开 API 够用。撤销：`[Environment]::SetEnvironmentVariable('GITHUB_TOKEN',$null,'User')`。
- 代理：`http://127.0.0.1:7897`（抓外部语料时用）。
- Chrome：`C:\Program Files\Google\Chrome\Application\chrome.exe`，用于把 SVG 渲成 PNG（`--headless=new --screenshot`）。
- RustRover `2026.2.4`：配置目录 `%APPDATA%\JetBrains\RustRover2026.2`；已装插件 GitToolBox / Key Promoter X / String Manipulation / .env；`rustrover64.exe.vmoptions` 已设 `-Xmx4096m`。
- RustRover 运行配置：不必建；用 Terminal 跑 `run.cmd`（IDE 的 Shell Script 模板在 Windows 上极易报 "Interpreter not found"）。

---

# 9. Skill

`C:\Users\cjz\.dsh\skills\core-experiment\SKILL.md`（v8）

核心规则（违反任何一条，实验结论不算数）：
1. 脚本 + 固定种子 + 原始输出
2. 不许构造样例冒充统计（给总数/有效/淘汰）
3. 判别力优先：判别力 0 = 散点，淘汰
4. 失败给最小复现
5. 确定性：同种子同结果
6. 不许改规则迁就数据（改则记 rule-vN）
7. 枚举空间写清（参与项、公式）
8. "救活"必须映射到已有需求编号
9. 条件不可机械检查 → 降 T3
10. 区分"不适用"与"缺失"（null 语义）
11. 参数做敏感性（≥3 档）
12. 数据修复清单与模型问题分开
13. 交叉 ≠ 新需求
14. 基本概念数不固定，只报冗余候选
15. 协议升版后回放旧数据
16. 回放必须同问题同度量
17. 复用样本 ≥10 才叫子网络
18. 自动冲突标注必须人工复核
19. 语料单位先声明（封/thread/PR/issue）

---

# 10. 文档位置

```
D:\jingwei-archive-20261024\delivery\
  MODEL-v0.1.md         模型冻结 + 变更规则
  IMPL-ARCH.md          实现架构（12 节）
  IMPL-DETAILS.md       模块细节（含 contradicts/expiry/alternative 三节）
  DESIGN-AUDIT.md       审计 v2（真偏差 5 → 已修 2）
  IMPL-GUIDE\01..17     逐文件施工指导
  K-核心模型讲解.md      从里到外的讲解
  diagrams\             模型分层图 / 数据流图（svg + png）
  G-模型与决策清单.md    全部决策与实验记录（§12.x）
D:\jingwei-archive-20261024\research-20261024\
  LOOP-STATE.md         实验轮次与真实数字
  corpus-final.jsonl    68 条 reviewed 语料
  crawl\                抓取脚本、候选池、抽样名单
```

---

# 11. 开工前的自检（新会话第一件事）

1. `cd D:\projects\jingwei && cargo test --workspace` → 应 121 passed（datacore 31 / collect 5 / cli 85）
2. `run.cmd stat D:\projects\demo` → 有数字
3. 第 4 节已全部完成；先看 §0 的「当前阶段」，再问用户下一步方向（别自己开新需求）
4. 不要重讲模型；不要重跑语料；**agent 只有两个：1 个 pro + 1 个 flash**。新需求**复用**（`send_message` 到既有会话），不许按问题新建子智能体——多建一次就是违规（本会话犯过：3 个 flash）


## ★ 0.2.0 证据（2026-10-27）

- `cargo test --workspace` → 128 passed（含 lsp_gate/lsp_diag/lsp_real/lsp_hover/lsp_def/lsp_ref/lsp_hint/lsp_action）。
- `node tools/web/smoke.mjs --proto --windows --jump --code --problems --theme --visual --lsp-gate --lsp-diag --lsp-real --lsp-hover --lsp-def --lsp-ref --lsp-hint --lsp-action`（全 flag 一次跑）。
- `tools\gate-control.ps1` 三仓 ok；`scripts\exp-v1.ps1` 四段绿；`scripts\package.ps1` 产 `dist\jingwei-v0.4.0.zip` + `.sha256`。
- 实测：RA 1.98.1；诊断 pull ~1s；references 热 6ms；hints 首次 ~4.2s；actions 3 chips；`lsp_in_history=0`（三仓）。

> 发布包 sha256 以 `dist\jingwei-v0.4.0.zip.sha256` 为准（README 里也记了同一值；HANDOFF 在 zip 内，不写死以免循环）。

## 已知限制（0.4.0-rc）

- **GUI 未人工验收**：已验 CLI/web + headless DOM（`smoke --shell` 11/11），窗口未自动启动。
- **UI 未映射命令**：静态全量 **39 个**；页面加载触发约 **12 个**，其余为交互触发。未映射时界面右下角显示“未映射（M15）”，不静默。按页归类（同一命令可能出现在多页）：

  - **home**（23）：`apply_fix`、`audit`、`changes_take`、`channel_request`、`channel_stop`、`check_project`、`contract_draft`、`contract_save`、`inspect`、`launch_project`、`msg_ingest`、`msg_mark`、`notify_gate`、`open_in`、`open_path`、`pick_folder`、`run_task`、`scan_cancel`、`scan_clean`、`scan_similar`、`scan_start`、`tasks_state`、`verify_project`
  - **editor**（4）：`apply_edit`、`event_note`、`restore_draft`、`snapshot_file`
  - **note**（9）：`create_project`、`delete_note`、`list_pythons`、`open_in`、`pick_folder`、`profile_save`、`profiles_list`、`project_log`、`save_note`
  - **run**（15）：`channel_request`、`channel_stop`、`contract_draft`、`contract_save`、`deep_check`、`default_opener`、`inspect`、`notify_gate`、`open_in`、`open_path`、`openers`、`pick_folder`、`sample_now`、`set_default_opener`、`tasks_state`
  - **machine**（15）：`channel_request`、`channel_stop`、`contract_draft`、`contract_save`、`deep_check`、`default_opener`、`inspect`、`notify_gate`、`open_in`、`open_path`、`openers`、`pick_folder`、`sample_now`、`set_default_opener`、`tasks_state`
  - **clean**（15）：`apply_fix`、`audit`、`changes_take`、`check_project`、`contract_draft`、`inspect`、`launch_project`、`msg_ingest`、`msg_mark`、`pick_folder`、`run_task`、`scan_cancel`、`scan_clean`、`scan_start`、`verify_project`
- 画图（graph tab / canvas / rough）已按决策删除。
- LSP 能力面冻结；`check` 第 12 项 `lsp_in_history` 必须 0。



## 项目顶层现状（pro 定稿，2026-10-27）

- **已完成**：CLI/serve/Γ⊢J 三态+证据/边（use/mod/impl/import/test/call）/card/impact/tests-for/cite；四槽窗口+单层跳转；LSP 只读 6 能力（信任门/诊断/hover/def/refs/hints/actions）；web 六页；0.4.0-rc 包。
- **已验**：213 tests；`b94`；`smoke --shell` 11/11；LSP 各 smoke 全绿；`lsp_in_history=0`；三仓 gate 历史 0/0。
- **未验**：GUI 人工（用户实机"满天窗口"，未复现）；六页仅 headless DOM；run/note 未映射；39 命令未映射；终端底栏未做。
- **冻结**：LSP 契约、命令面、graph/画图、rename/apply、跨仓、联网。
- **风险**：GUI 多窗口/外部进程（`open_*` 未拦截、无单实例）；39 未映射误导；rc 包/文档数字漂移；`main.rs` 6490 行未拆。

### 优先级（pro 2026-10-27 修订）
- P0：判定/证据/边/卡片；四槽跳转；LSP 只读链；GUI 六页真数据；**GUI 单实例 + 外部进程 deny-list + 恰好 1 窗口**（阻塞项）
- P1：终端底栏 + cwd 跟随（由 P0 降级）；布局恢复（部分完成：localStorage 已有）；行内 git blame；外部改动重载；键盘选择复制；命令面板搜索
- P2：鼠标组合键；终端点路径开文件；scrollback 搜索

## 新会话入口（2026-10-27）
先读：`docs/reference/session-handoff-2026-10-27.md`（本会话全部决定/现状/待办 T2.3–T8/坑）。

## ★ 0.4.1-rc 已发（2026-10-27，T8 收官）

交付口径：**0.4.1-rc：解压 → 双击 bin\jingwei-gui.exe → 选仓库（或命令行带路径）；home/editor/note/machine/clean 五页有真数据（probe 5/5 root_ready=true），run 页是 P1 空壳；编辑器可开文件、看引用、单层 preview；关窗进程归零；第二实例 exit 2。zip sha `08839FA0…`，tests 257，smoke 31。**

### 随包声明的 6 条限制（README“已知限制”同文）
1. **run 页 = 终端未接（P1）**，不是“已实现”；
2. **problems / LSP 未接壳**——LSP 只在 CLI 级（P0 的“只读链”在 CLI 成立，壳内没有）；`crates/lsp` 未抽；
3. **pinned 栈未做**（四槽只到三槽：main/preview/inspector）；
4. **双击无参 → 目录选择对话框未验**（自动化点不了原生框），需人工点一次才算闭环；
5. **无 git 提交**：本仓 `D:\projects` 零 commit，重建只能按 `src-manifest` 校验；
6. cite 在无历史仓库恒 `skip:no_commit`（演示请用有历史 fixture）。

### post-rc 建议顺序（未做）
① 本地 git commit 一次（可追溯从 manifest 升级为真快照）；② 用户手动点一次“双击 → 选目录”闭掉最后未验路径；③ `crates/lsp` 抽取 + problems（壳要用 LSP 前必须先抽）；④ run 终端 + pinned 栈（P1）。

**状态：0.4.1-rc 已发，等用户实机验收。**

### 差距表增行（2026-10-28，I1 后补）

| # | 差距 | 事实 | 处置 |
|---|---|---|---|
| 1 | env 探测多出 shim 一级 | 旧壳只有 PATH（`crates/engine/src/util/winutil.rs:28-37`）+ 常见目录（`crates/engine/src/detect/probe.rs:306-320`），**没有 rustup/nvm/pyenv 的 shim 概念** | 新 `collect::env` 的 `Source::Shim` 是**增强不是对齐**：同一台机器上结果可能与旧壳不同（多了「找到了但旧壳看不见」的情况），属改进；如实记在这里，不当回归 |

### 测试环境两条（2026-10-28，pro 要求记录）

1. **LSP 4 条 flaky 是既有的**：`lsp_def/hint/hover/real` 在全量并行跑时返回 `skip:"server_down"`；单跑 `cargo test -p jw --test product lsp_` → 8/8 过。**全量跑前先确认 `Get-Process rust-analyzer` = 0**（并行抢 RA 是根因）。
2. **`serve_scan_matches_facade` 的 keys 修正属补欠账**：S3 换 `Engine::scan` 时加了 `kind/has_git/has_contract/has_index`，旧断言没跟着改，I2 顺手补齐（记在 S3 那刀下，不算 I2 新债）。

### L2 · `crates/lsp` + `jw lsp-check`（2026-10-28）

- 新增 `crates/lsp`（诊断专用客户端：Content-Length 帧 / stderr 排空留 50 行 / initialize→initialized→didOpen / shutdown→exit→taskkill /T）+ `jw lsp-check <路径…>`（有 error 退出 1，查不了退出 2）。
- 实测：`crates/lsp/src/lib.rs` → **errors=0 warnings=0 exit 0**；临时 crate 插类型错 → **errors=1 exit 1**。全量 **275 tests / 0 failed**；warnings **109**（持平）。
- 两个坑写进注释：① clap 里加 `LspCheck` 变体会让 `jw --help` 栈溢出 → 改**手写分发**（改动小可回滚，理由记录在 `review-L2.md`）；② `workspace_root` 相对路径产生空祖先 → 先 canonicalize，并优先选带 `[workspace]` 的最外层。
- 证据：`.pro/exp/out/review-L2.md`、`l2-green3.txt`、`l2-red.txt`、`l2-debug.err.txt`

### R1 · 审查工具链（2026-10-28 收）

- 三件齐：`crates/lsp` + `jw lsp-check`（绿 0/0 exit0；红 1 error exit1）、`jw --debug` 五类事件（serve.call/scan.start/scan.end/env.probe/lsp.check）、`scripts/review.ps1`（诊断数 + 关键日志 + 截图清单 → `review-<step>.md`）。
- 证据：`.pro/exp/out/review-R1.md`（files=2 errors=0 warnings=0）、`review-L2.md`。
- 坑（已修）：PowerShell 5.1 读**无 BOM 的 UTF-8 .ps1** 会把中文读坏 → 所有 `.ps1` 统一存 UTF-8 **带 BOM**；`review.ps1` 的 `-Files` 支持逗号串，`--debug` 放在子命令之后。
- 调研：`.pro/exp/out/research-20261028.md`（三表、8 来源、6 行如实标「未核实」）。
- git：本地 commit `1f9a39f`、`72d7e3d`；push 走代理 `127.0.0.1:23333` 成功（直连 443 失败，已按 best-effort 处理，不再重试）。

### 收尾 · clean 数据面（2026-10-28 12:09，commit 781f1e0）

- **做了什么**：`Engine::clean_scan` 只清点项目内可重生成产物（`target`/`node_modules`/`dist`/`__pycache__`），字段与旧页 zone A 对齐（`path/label/action/zone/recommended/size_bytes/reason/consequence/truncated`）+ §4.4 scope 三件套；体积有上限（20k 文件 / 300ms 截断显式标 `truncated`）。serve 加 `clean_scan`，壳加 `engine_clean_scan`。
- **没做什么（缺口，必须写清）**：**UI 未切**。clean 页是"会删东西"的页，旧页 13 条里含全局缓存（回收站/崩溃转储/缩略图等 zone b/c），新源只覆盖项目产物；半换源会把"可清理总量"算错，所以这一刀只落数据面，UI 切换与全局缓存移植放下一刀。
- **判据**：contract **18 tests**（新增 clean 1 条：target 2048B 量出来、字段 9 个齐、`human` 含 KB）；`review-R2-clean.md` = 2 文件 0 error / 0 warning；commit `781f1e0` 已推（23333）。
- **下一步（接手先做这个）**：① 移植全局缓存清单（对齐旧页 13 条 + `human/humanRecommended/zoneA.count`）；② 一次切 UI（`scanClean`→engine，失败回落旧源并显式 `S.cleanError`）；③ parity 加 `clean.items=13`；④ 截图对账后本页才算绿。

### 收尾 · 全局状态（2026-10-28 12:09）

- git：**12 commit**，最新 `781f1e0`，远端已同步（push 走 `127.0.0.1:23333`；直连 443 不通）。
- 测试：`cargo test --workspace --all-targets` 最近一次 **275 passed / 0 failed**；`jw lsp-check` 自审 45 文件 **0 errors / 116 warnings**（116 条 backlog，未清）；`cargo build -p jw` warnings **109**（与基线持平）。
- R2 进度：scan / inspect / run / editor / note / machine 六页已换（inspect 只换 5 组，`findings/hygiene/guide` 仍旧源且 `unavailable` 显式标注）；clean 只到数据面。
- 已知缺口：① clean UI 与全局 13 项；② inspect 三组（需先移植 rules/guide）；③ 强制杀 GUI 会留 `jw` 孤儿（T7 Job Object）；④ graph 入口还在（R2 要求删）。
