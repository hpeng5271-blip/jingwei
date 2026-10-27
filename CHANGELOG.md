# CHANGELOG

## 0.4.0-rc — 2026-10-27

- 新增 `jingwei-gui`（wry/WebView2 桌面壳，内嵌 `jw web` 同一 HTTP+bridge，端口 0 自动分配）。
- `jw web` 默认**不打开浏览器**，只有 `--open` 才开；端口占用直接报错退出。
- 测试：LSP 真 RA 用例串行锁 + 关进程树断言（默认并行全绿，213 passed）。
- 已知：GUI 未人工验收；UI 尚未迁移 archive 的六页（0.4.0 正式版再做）。


## 0.2.0 — 2026-10-27

**LSP 全链路（只读）**：信任门 → 诊断 → hover → definition → references → inlayHint → codeAction（只展示）。
rules 仍为 27（本次没有改派生语义，故不 bump）。

- 新增：`jw trust status|grant|revoke`（用户级 `%APPDATA%/jw/trust.json` + audit.log，仓库不可自证）。
- 新增 serve：`lsp_status / lsp_trust / lsp_spawn / lsp_real / lsp_diag(嵌套) / lsp_hover / lsp_def / lsp_ref / lsp_hints / lsp_actions / lsp_ra_info`；
  统一 `{source:"lsp", at, capability, seq}` 包，全部只读、只进 now 缓存。
- 新增 UI：RA 状态栏版本、hover 卡（380px / type+doc）、definition 跳转（单目标替换、多目标候选、history≤2）、
  references（按文件分组、点击一跳）、inlayHint（Alt+H、可视区、去重 150ms、stale 丢弃、上限 200）、
  codeAction chips（只展开，不 apply）。
- 安全：未信任 0 spawn；env 白名单 + `taskkill /T /F`；`check` 第 12 项 `lsp_in_history` 恒 0。
- 文档：`docs/reports/v0.2.md`、`docs/reference/lsp-matrix.md`（实测口径）、`docs/reference/skills-index.md`；
  5 个 skill（window-slots / jump-model / lsp-gate / smoke-harness / spec-review）。

## 0.1.0 — 2026-10-26

首个可运行版本：judgment/evidence/edge 模型、三值判定、gate 三仓、v1 四层验收、cite、impact、tests-for、range card、web 工作台。
