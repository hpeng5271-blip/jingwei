# 前端 ↔ 后端契约（v0）

> 后端 = `jw serve <repo>`（NDJSON over stdio）；前端 = `tools/web/`（零依赖，浏览器）。
> 桥 = `tools/web/server.mjs`（node 内置模块，唯一翻译层）。

## 1 拓扑

```
browser  ──HTTP──▶  server.mjs  ──NDJSON(stdio)──▶  jw serve <repo>  ──▶  .jingwei/jingwei.db
   ▲                    │
   └──SSE /watch────────┘（转发 subscribe 事件）
```

- 启动：`node tools/web/server.mjs <repo> [port]`（默认 7777；`JW` 环境变量可指定 jw 路径）
- 打开：`http://127.0.0.1:<port>/`
- 桥是**唯一**把前端的 `action` 翻译成 serve 的 `method` 的地方；前端不直接说 NDJSON。

## 2 桥 API

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/health` | `{ok, repo}` |
| POST | `/rpc` | 请求体 `{action, ...参数}`，原样转发；返回 serve 原始回包 `{v,id,ok,result|error}` |
| GET | `/watch?since=<seq>` | SSE；把 serve 的 subscribe 事件（`{write_seq,gap,events}`）转成 `data:` 行 |
| GET | `/` | 静态 `index.html` |

## 3 serve action 清单（前端可用）

| action | 参数 | 返回 result |
|---|---|---|
| `ping` | — | `{pong:true, write_seq}` |
| `stat` | — | `{judgment, evidence, change, write_seq}` |
| `find` | `term`, `kind?` | `{term, kind, total, hits:[{subject,kind,verdict,path?,line?,name?}]}` |
| `card` | `path`, `history?`, `also?[]` | `{exit, card}` 或 `{exit, cards[]}` |
| `edges` | `path` | `{path, total, edges:[{kind,dst,leaf,loc,from?}], unresolved_reasons?}` |
| `ask` | `subject`, `as_of?` | `{subject, reason(held/refuted/…/no_capability), masked, verdict?, at?, expires_at?}` |
| `cite` | `subject` | `{rows:[{file,line,commit,file_commit,verdict,evidence_kind}]}`；解析不到 `{reason}` 且 `ok=false` |
| `coverage` | — | `{tests:{standard,macro}, call:{total,resolved,unresolved}, unresolved_use:{code:n}}` |
| `impact` | `target`（path 或 `symbol:…#…`） | 与 `jw impact <target> --json` **同一对象**：`{target,target_kind,targets,direct,transitive,tests{scope:"file"},importers,completeness}`；目标不存在 `ok:false` + `reason` |
| `tests_for` | `target`, `depth?`（默认 3） | 与 `jw tests-for <target> --json` 同一对象：`{target,tests:[{name,file,line,kind,depth,via}],completeness{depth_cap,tests_total,…},note:"静态可达，不等于运行覆盖"}` |
| `capability` | — | 与 `jw capability --json` 同一对象：`{features,langs:[{ext,lang,symbol,use,import}]}` |
| `edge_stats` | — | `{internal, ext, unresolved, total, impl_internal, impl_ext, impl_question}` |
| `open` / `pin` / `focus` | `path` | 三个 UI 槽位（preview / pinned / last_focus） |
| `subscribe` | `since`, `timeout_ms` | 长轮询：`{write_seq, gap, events}` |

## 4 协调规则（踩过的坑）

1. **字段名**：serve 只认 `method`，不认 `action`；桥里翻译，前端无感。
2. **形状**：`stat` 的键是 `judgment/evidence/change`（不是 judgments/changes）；`card` 的结果在 `result.card`（单张）或 `result.cards`（多张）。
3. **零副作用**：`jw serve` 打开已有库；无库时 CLI 侧是 `exit 2 + no_db`，桥把 `ok:false` 原样传给前端。
4. **三值**：`ask` 的 `reason` 是字符串（`held` / `refuted` / `no_data` / `no_capability` / `expired` / 冲突原因），前端按颜色区分，**不许把 uncertain 画成绿**。
5. **位置**：`edges` 每行的 `loc{file,line}` 可直接拼 permalink；`cite` 给 `commit` 与 `file_commit` 两把哈希（行级 vs 文件级）。
6. **诚实统计**：`call` 必须同时给 resolved/unresolved；只给 total 会被误读成"全解析"。

## 5 前端现状（v0.1）

- 布局：左侧搜索 + 仓库概览（判断/依据/改动/测试/调用/导入/未解析 use/能力角标）；右侧文件视图（符号/测试/影响/可能的测试（符号级）/permalink）与**模块视图**（目录：文件/符号/测试/调用/导入实现/改动）。
- 深链：`?path=<file>`、`?symbol=<file>%23<fn>`、`?module=<dir>`（`#` 要写成 `%23`，否则被浏览器当 fragment）。
- 版本：`jw --version` → `jw 0.1.0 (rules 27)`；契约字段名以本文 §3 为准。

## 6 旧·前端现状（v0）

- 布局：左侧搜索 + 仓库概览（判断/依据/改动/测试/调用/导入/未解析 use），右侧文件视图（符号、测试、导入/实现、调用、permalink）。
- 交互：搜索框回车 → `find`；点命中 → `card + edges + cite` 并发取数；概览在启动时拉 `stat + coverage + edge_stats`。
- 不做：编辑器跳转、图可视化、写操作（open/pin/focus 已留接口未接 UI）。


## 7 LSP 交互（0.2.0）

| action | 参数 | 返回 |
|---|---|---|
| `lsp_ra_info` | — | `{found, version, path}`（不 spawn） |
| `lsp_diagnostics` | `path, mode?` | `{source, items[{file,line,col,severity,code,message,source}], skip, degraded}` |
| `lsp_real` | `path, force_down?` | 同上（真 RA；`skip:no_server/server_down`） |
| `lsp_hover` | `path,line,col` | `{type, doc, lsp_line, lsp_char, skip, source}` |
| `lsp_def` | `path,line,col` | `{targets[{file,line,col}], precision, skip, source}` |
| `lsp_ref` | `path,line,col,include_declaration?` | `{targets, total, coverage, precision, skip, source}` |
| `lsp_hints` | `path,start,end` | `{hints[{line,col,label}], truncated, badge, skip, source}` |
| `lsp_actions` | `path,line,col,message,severity` | `{actions[{title,kind,source,has_edit}], applied:0, skip}` |

交互约定：hover 300ms；definition 单目标替换 preview + origin（history≤2），多目标候选列表选中仍一跳；
references 按文件分组点击一跳；inlayHint `Alt+H`、可视区、150ms 去抖、stale 丢弃、上限 200；codeAction chips **只展开不 apply**。
