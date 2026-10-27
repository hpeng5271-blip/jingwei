# 经纬 v0 设计（architecture / 全貌 / 骨架）

> 来源：子 pro 一次性交付（2026-10-26），与代码同步维护；数字以 `HANDOFF.md` 顶部为准。

## 1 全貌

定位：终端/IDE 之间的"理解层"——把代码事实变成**可引用、可回溯、三值**的判断与关系，不猜、不骗、可重建。

| 领域 | 主链路（命令 → 数据 → 输出） |
|---|---|
| 编码学习 | `find/card/explain/edges` → judgment(symbol)+edge(use/mod/impl/call/test) → 术语卡(file:line + 定义处数 + 测试 + 调用) |
| 研究 | `ask/why/cite/timeline/fold/cross` → judgment+evidence(commit/file:line)+change → 可粘贴 permalink、演化、跨库同口径 |
| 项目 | `project/stat/check/coverage/timeline/compact/resume` → change+source+edge 统计 → 现状/影响/交接/健康 |

共享核心：`subject`（`symbol:/file:/doc:/decision:/dep:/test:`）→ `judgment`（kind + verdict `held/refuted/uncertain` + coverage + env）
→ `evidence`（`supports/contradicts`，指向 source/commit）→ `edge`（kind 多态关系）→ `kind_registry`（kind→layer/parent）→ `capability`（语言×feature 闸）。

## 2 架构

| crate | 职责 | 关键约束 |
|---|---|---|
| datacore | model/ids/store/compose/query/schema：六表、事务、三值合成、as_of | 不依赖 collect；只认 judgment/evidence/change |
| collect | git/docs/parse/capability：git log→change、md→doc、tree-sitter→symbol+edge | 只产事实/派生，不做输出 |
| cli | 全部子命令、edges_resolve 后置 pass、serve | 唯一写库入口；JSON = UI 契约 |
| control | 独立对照 B（共享 grammar、独立遍历），只读源码 | 不 import jw，是门禁真源 |

数据流：源（工作树 / git / md）→ collect 产 judgment+edge → datacore 落库 → resolve 重写 `?` 边 → query/serve 读投影。

- **事实**（不可重建）：git 提交、工作树、文档原文。
- **历史证据**（重采不重写语义）：change / source / snapshot。
- **派生**（可清空重建）：symbol/doc/dep judgment、全部 edge、gap/capability 派生视图、版本 meta。

时序：`init`(schema+meta) → `ingest`(git log→change+source) → `docs`(md→doc)
→ `parse`(tree-sitter→symbol + use/mod/impl/import/test/call 边，记 rules) → `resolve`(重写 `?use/?py/?ts/impl/?call`) → `check`（十一项全零）。

## 3 实现骨架

```
crates/{datacore,collect,cli,control}/src
datacore: model.rs  ids.rs  store.rs  compose.rs  query.rs  schema.sql  capability.rs
collect:  git.rs  docs.rs  record.rs  parse.rs  capability.rs  gap.rs
cli:      main.rs  edges_resolve.rs
control:  main.rs  expand.rs
```

关键签名（12）：

```
Store::open(root) | Store::open_existing(root)          // 后者：无库不建库，exit 2 + no_db
put_edge(src, dst, kind, at, source_id, leaf, line)
unresolved_use_edges()            delete_use_edge(src, dst, line)
parse::parse_all(root, limit, f)  parse::leaf_of(text)   parse::module_map(root)
capability::{supports, symbols_status, feature_of_kind}
edges_resolve::resolve_all(store, root)
query::ask_with(store, subject, as_of, caps)
card_value(store, root, input, history, state)
control::expand::expand_imports(text, line)
```

edge kind 语义（src | dst | leaf | line）：

| kind | src | dst | leaf | line |
|---|---|---|---|---|
| use | 文件 | 文件 / `ext:crate` / `?use` | 本地绑定名（别名） | use 行 |
| mod | 文件 | 模块文件 / `?x` | — | mod 行 |
| impl | `type:X` | trait 文件 / `ext:` / `?trait` | 声明所在文件 | impl 行 |
| import | 文件 | 文件 / `ext:pkg` / `?py` / `?ts` | 导入名 | 行 |
| test | `test:<file>#<fn>` | 所在文件 | fn | fn 行 |
| test_macro | 同上（`rstest`/`quickcheck`/`*::test`） | 所在文件 | fn | fn 行 |
| call | `symbol:<f>#<caller>` | `symbol:<f>#<callee>` / `?call` | callee | 调用行 |

命令面（v0.1）：`init/ingest/docs/parse/resolve/check`、`find/card <file|dir>/edges [--kind] [--to]/impact/tests-for`、
`cite/ask/timeline/fold/cross`、`coverage/stat/project/capability/serve`；全部 `--json`。

JSON 契约（均 `--json`）：`card`(symbols{status,note}/tests/edges/deps)、`coverage`(tests/unresolved_use{total,reasons})、
`edges`(loc/from/unresolved_reasons；`--to` 反查 incoming)、`cite`(file,line,commit,file_commit,verdict)、`capability`、`cross`、
`timeline/project/stat/check`、serve NDJSON(v/gap/events)、`tests/fixtures/v1-golden/expected.json`。

## 4 细节

- **并发/事务**：parse 并行解析、单写者落库（WAL），末尾 `note_write` + `commit`；读命令 `open_existing`，无库 → exit 2 `no_db`。
- **rules_version**：meta 与常量不等 → `clear_edges` + 全量 parse（旧 dst 不残留）；`check.rules_version_bad` 看门。
- **capability 闸**：读时 mask，只有 `symbol` 依赖 `Feature::Symbol`；缺能力 → `uncertain(no_capability)`（exit 2），`no_data` 仍 3；
  `file:` 的 held 是"存在/有历史"，不降级。
- **gap 原因码**：`glob / crate_root_alias / path_attr_missing / module_shadow / crate_item_missing / extern_absolute / other`；`other==0`、`sum==total`。
- **cite**：symbol 走 `git blame -L n,n`；file 走 `git log -1 -- <path>`；行内同时给 `commit` + `file_commit`；解析不到 exit 2 + reason，不造哈希。
- **验收分层**：`cargo test`（单元/ product / golden fixture）→ `gate-control.ps1`（三仓对称差 0/0）→ `b70_loc_gate.py`（每条边 loc 合法）
  → `exp-v1.ps1` 四段（golden / self-consistency / accept-new / dashboard；只锁不变量，不锁计数）。

## 5 风险与已知边界（v0.1 冻结）

- 空目录 `card`：全 0 + exit 2、JSON 无 `reason`；`card <dir>` >300 文件时 `definitions` 退回 DB 下界（`symbols.definitions_note` 标注）。
- `card` 写 focus 槽（`ui.preview`/`ui.last_focus`），不是严格只读。
- 调用图 v1.5：同文件 / `use` 绑定 / 全仓唯一 fn 名；方法、宏生成、trait 分发、std 实现不解析。
- `impact.tests` 与 `tests-for` 是**文件级 / 静态可达**，不是运行覆盖。

## 6 风险与下一刀

- call v1.5（rules 27）：**作用域优先**——先查 `use` 绑定（内部绑定指到目标文件；`std/core/alloc` 保持 `?call`），无绑定才走"全仓 fn 名唯一"。
  精度复算：std_bound 7/30/33→0、internal_mismatch 4/15/58→0；解析率 rg 1456/2630、tauri 1599/4380、nushell 10724/31650（宁可少解析，不许指错）。
- 剩余未解析主要是方法调用（`x.foo()`）、宏生成、trait 动态分发、标准库；`edges --stats` 已显示 resolved/unresolved，不许读成"没有调用"。
- 下一刀候选：方法调用的 receiver 类型（需类型推导，成本高）、`mod` 级聚合的影响面、或发布级收尾（docs/包/演示）。


## 7 LSP 层（0.2.0，只读）

```
UI ──rpc──▶ serve(lsp_*) ──▶ RaSession(单例) ──stdio LSP──▶ rust-analyzer
                │                  │
                │                  └─ 只进 now 缓存（source="lsp", at, seq）
                └─ 未信任 → 0 spawn；失败 → skip（no_server/server_down/no_capability）
```
- 信任门：`jw trust`（用户级 trust.json + audit.log），仓库不可自证。
- 能力按 `InitializeResult` 声明：diagnosticProvider(pull)/hover/definition/references/inlayHint/codeAction。
- 所有结果 `{source:"lsp", at, capability, seq}`，**不写 judgment/edge/evidence**；`check` 第 12 项 `lsp_in_history` 必须 0。
- 降级：未信任/超时/无 server → 自研索引兜底（`source:"db"`、`precision:"low"`、显式 `skip`）。
