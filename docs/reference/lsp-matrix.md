# LSP 能力矩阵 v1

> 生成：2026-10-27。数据来源：`.pro/exp/research/lsp-client.json`（42 条决策，T7/D6/S5/C10/R3/X5/A6；lsp:self:none=32:9:1；fallback 42/42；32 个延迟数字为目标值；6 条未验证）。
> 所有延迟预算均为**目标值**，需 fixture 校准；`source` 与验收断言先于实现定死。

## 0 前置门（不可协商）

- **G1 未信任工作区不 spawn**：rust-analyzer 会执行 `build.rs` 与 proc-macro；未信任 → 只读自研索引 + `lsp:null`，显式信任后才启动（X01）。
- **G2 LSP 只作"现在"的观测态**：每条结果必须带 `source ∈ {lsp,db}` 与 `at`；**不得写入 judgment/历史链**，历史问题只用 DB 判断（R03）。

## 1 能力矩阵

| 能力 | 提供者 | 理由 | fallback | 渲染落点 | 延迟预算 | 验收（命令 + 期望） |
|---|---|---|---|---|---|---|
| diagnostics | 混合（LSP push+pull 为主） | 语义诊断只有 server 能算；自研只做语法 | 自研 syntax（`source=self`） | 底部 Problems / 行内 / 卡角标 | 500ms | `jw lsp probe --cap diagnostics --fixture fx/syntax` → `{source:"lsp",count>0}`；无 server → `{source:"db",reason:"server_down"}` |
| hover（含类型） | 委托 | 类型只信 LSP，自研不猜 | 自研符号卡，type 写"未知" | 悬浮卡 380px / C2 卡 | 200ms | fixture 10 例：`hover.type` 非空；无 server → `"未知"` 且出错次数 0 |
| definition | 委托 | 精确跳转依赖类型/宏展开 | 自研 def 索引（precision=low，显式标注） | 代码区跳转 / 右栏列表 | 200ms | fixture 5 例跳转命中；fallback 时 UI 显示 `precision:"low"` |
| references | 混合（LSP 金标准） | 自研边先修，LSP 做裁判 | 自研 use/call 边 + 覆盖率 | 右栏列表 / 卡引用数 | 1500ms | 抽 100 引用对 LSP references，差异 ≤2 条并进 accept-new |
| documentSymbol | 混合（LSP 优先） | 结构化准确；自研毫秒级兜底 | 自研 tree-sitter 符号表 | 右栏大纲 / 命令面板 | 300ms | 同一文件两侧符号集合差异 ≤2（fixture） |
| workspaceSymbol | 混合（自研优先） | 本地 SQLite/FTS 更快，LSP 补全 | LSP workspaceSymbol 合并 | 命令面板 | 150ms | 查询 P95 ≤150ms；合并后无重复（按 file+name+line） |
| callHierarchy | 混合（LSP 优先） | 跨 crate 调用需语义；自研只有同文件/绑定 | 自研 call 边 + 未解析计数 | 右栏调用树 | 1200ms | fixture incoming/outgoing 各 5 例；`unresolved` 必须显示 |
| completion | 不做（编辑器） | 与 IDE 抢输入法没收益 | 命令面板符号前缀跳转（自研） | 命令面板 | 150ms | `completion` 请求数 = 0（断言）；前缀查询返回 ≤20 条 |
| codeAction | 委托（只读展示） | 写操作风险高，v1 只展示 | 无 server → 不显示 | Problems 行尾 chip | 800ms | fixture 3 例显示 title；点击不做写操作 |
| inlayHint | 委托（视口 + 上限 200） | 类型提示价值高但量大 | 不显示；卡片 type 写 n/a | 代码区行内 | 150ms | 视口外请求数 0；单屏 hint ≤200 |
| 类型/泛型信息 | 委托（仅 LSP） | 自研无类型系统，猜即错 | 显式"未知"，0 次猜测 | hover / 卡 type 字段 | 200ms | 无 server 时 `type=="未知"`，推断次数 0 |
| 跨 crate 名解析 | 混合（LSP 裁判 + 自研原因码） | 自研原因码可解释，LSP 可判真值 | 只报原因码（glob/module_shadow/crate_item_missing…） | coverage 面板 | 1000ms | 50 个跨 crate 样本：precision ≥95%；未解析 100% 带 reason |

## 2 传输 / 生命周期 / 发现

- **传输**：stdio 唯一默认；socket 仅远程/共享，连不上 1s 内回退 stdio（T01/T02）。
- **生命周期**：`initialize` 应答前零业务请求；`shutdown → exit → 3s 强杀`；崩溃 5 次/180s 停止重启并落 `server_down`（T03/T05/T06）。
- **能力协商**：以 `InitializeResult` 为准，未声明=不支持，调用 0 次；能力表与返回值逐字段快照相等（T04）。
- **发现**：PATH + 显式配置；版本门限留空待实测；远程模式才开 socket。

## 3 安全

- 未信任不 spawn（G1/X01）；env 白名单（`PATH/HOME/LANG/RUSTUP_*/GOPATH`，X04）。
- RSS 上限 2GB，超限杀进程树→退避重启 1 次→`server_down`（X02）；能力连续失败 3 次熔断（X03）。
- 双向日志默认脱敏，trace off/messages/verbose，轮转 50MB×3（X05）。

## 4 降级顺序

1. 能力未声明 → 0 调用，UI 标 `source:"db"`；
2. 调用失败 → 3 次熔断，改走 fallback 并标"该能力不可用"；
3. server 崩溃/超限 → `server_down`，只保留自研索引，可手动重启；
4. 未信任工作区 → 不 spawn，只读索引。

## 5 未验证（6 条，不得当事实）

1. 五个 server 是否都支持 3.17 pull diagnostics（未逐版本核实，按能力位动态判断）。
2. clangd 最低可用版本号（门限留空待实测）。
3. typescript-language-server 内存/崩溃行为（无高星 issue 可引）。
4. gopls daemon（`-remote`）在 Windows 的可用性。
5. 各能力真实延迟（200/500/1500ms 均为目标值，未跑 `jw lsp-probe`）。
6. rust-analyzer 大仓冷启动时间（只有社区印象，无本机数字）。


## 6 实测回填（2026-10-27，RA 1.98.1）

| 原 unverified | 现状 | 证据 |
|---|---|---|
| RA pull diagnostics 支持面 | **已验**：根级 `diagnosticProvider`，走 `textDocument/diagnostic` | smoke `--lsp-real` 7/7；首次 ~1s |
| RA 冷启动 | **已验**：3.1–4.2s（目标 ≤15s） | `lsp_real` 计时，fixture 与 tauri 均 <5s |
| 真实延迟 | **部分已验**：references 热 **6ms**；hover ~1s；hints 首次 ~4.2s；actions ~1s | `.pro/exp/out/m14.json` |
| clangd 最低版本 | 仍待验（未装 clangd） | — |
| tsserver 内存 | 仍待验 | — |
| gopls daemon on Windows | 仍待验 | — |

> 未验项不得在文档/演示里写成"支持"；只在对应 server 实测后升级。
