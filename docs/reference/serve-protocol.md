# serve 协议（v1）

外部组件（编辑器插件、脚本、UI）与经纬的唯一长连接。**一条连接、一行一消息、全是 JSON**。

- 传输：`jw serve <repo>` 的 stdin/stdout，NDJSON（一行一条消息，`\r\n` 或 `\n` 都接受）
- 请求字段：`id`（客户端自定，回包原样带回）、`method`、其余按方法
- 回包字段：`v`（协议版本，当前 1）、`id`、`ok`；成功带 `result`，失败带 `error`
- 同一个 `id` 不会混：一次请求一次回包，**按行读即可**

## 方法

| method | 参数 | 回包 result |
|---|---|---|
| `ping` | — | `{pong:true, write_seq}` |
| `stat` | — | `{judgment, evidence, change, write_seq}` |
| `card` | `path`，可选 `history:bool`、`also:[path…]` | 单张 `{exit, card}`；多张 `{exit, cards:[…]}` |
| `refresh` | 可选 `limit:int`（默认 200） | `{write_seq}`；内部跑 docs→parse→ingest，**不打印任何文本** |
| `watch` | `since:int`、`timeout_ms:int`（50–60000，默认 5000） | `{write_seq, changed, gap, events:[{seq, subjects:[…]}]}` |
| `quit` | — | 不回包，进程退出 |

CLI 另有 `coverage --json`（同一套版本约定，见下），`serve` 暂不提供该方法的包装。

未知方法 → `{"v":1,"ok":false,"error":"unknown method: …"}`；坏 JSON → `{"v":1,"ok":false,"error":"bad_json: …"}`。

## card 的形态

`card` 返回的 `card`/`cards[]` 与 `jw card --json` **完全同构**（同一份取数代码），按选择器分四种：

| 选择器 | `mode` | 关键字段 |
|---|---|---|
| 文件路径 | （无 mode） | `v` `subject` `path` `state` `exit` `judgment` `evidence` `changes` `authors` `recent` `doc` `symbols`；`Cargo.toml` 额外有 `deps` |
| 提交哈希 | `commit` | `v` `commit` `author` `at` `message` `files` `exit` |
| 目录 | `range` / `range_history` | `v` `path` `files` `changes` `symbols` `docs` `judgments` `days` `exit` |
| `dep:<name>` | （与文件卡同构，`judgment.kind = "dependency"`） | 版本在 `judgment.scope.env`：`version` / `resolved` / `dep_kind` |

`exit` 与 CLI 退出码同义：`0` 成立 / `1` 被否 / `2` 查不了。

### 字段表（实际输出，逐字段）

**文件卡**

| 字段 | 类型 | 说明 |
|---|---|---|
| `v` | int | 契约版本（1） |
| `subject` / `path` | string | `file:<path>` 与仓库相对路径 |
| `state` | string | `held` / `refuted` / `uncertain(<reason>)` |
| `exit` | int | 0 / 1 / 2 |
| `judgment` | object\|null | `{id, kind, verdict, coverage_read, coverage_total, at, expires_at, scope{granularity, objects, time_window, env}}` |
| `evidence[]` | array | `{polarity, premise, instrument}` |
| `changes` | int | 改动次数 |
| `authors[]` | array | `{name, n}`，最多 2 人 |
| `recent[]` | array | `{commit, status, at, author, subject}`，最多 3 条 |
| `doc` | object\|null | `{subject, verdict}`（文档判断） |
| `symbols` | object | `{names, definitions, kinds}`，`kinds` 是 `{kind: 数量}` |
| `deps` | object | 仅 `Cargo.toml` 卡：`{total, normal, dev, build}` |

**提交卡**：`files{total, added, modified, deleted, list[{path, status}]}`，`list` 最多 20 条。

**范围卡**：`days[]` 是 `{day, n}`；普通范围态为**空数组**，`range_history` 才有内容。

## watch 的语义（重要）

- 服务端每 100ms 查一次 `write_seq`；涨过 `since` 就立刻回，否则等满 `timeout_ms` 回 `changed:false`。
- `changed:true` 时 `events[]` 给出这一批写事件的 `subjects`，**客户端只重查这些 subject 的卡**。
- `gap:true` 表示事件日志被截断（日志只留最近 50 条）或日志缺失——**必须全量重取**，不要相信 `events` 的完整性。
- 客户端拿到的 `write_seq` 直接当下一次的 `since`。

## 真实会话（未经修饰）

```
→ {"id":1,"method":"refresh"}
← {"id":1,"ok":true,"result":{"write_seq":3},"v":1}
→ {"id":2,"method":"ping"}
← {"id":2,"ok":true,"result":{"pong":true,"write_seq":3},"v":1}
→ {"id":4,"method":"watch","since":2,"timeout_ms":200}
← {"id":4,"ok":true,"result":{"changed":true,"events":[{"seq":3,"subjects":["file:a.md"]}],"gap":false,"write_seq":3},"v":1}
→ {"id":5,"method":"nope"}
← {"id":5,"ok":false,"error":"unknown method: nope","v":1}
→ {"id":6,"method":"quit"}
```

（`card` 的回包较长，字段见上表；真实样例可跑 `tools/watch.mjs` 看。）

## 空闲折叠

`serve` 在**空闲**（默认 300 秒没有请求，`JW_IDLE_COMPACT_SECS` 可改，`0` 关闭）时会在后台跑一次
`compact --apply`：折叠已过期的同向连续段、写一条 gist，然后 `write_seq` +1。

- 它**不往 stdout 打任何东西**，不会污染 NDJSON；
- 折叠算一次写，`watch` 的客户端会收到 `events[].subjects`（gist 所在 subject）；
- `stat` 里能看到 `last_compact <日期>`（说明后台检查过）。

## coverage --json（CLI 契约，与 card 同版本约定）

```json
{
  "v": 1,
  "lang": "rust",
  "total": 112,
  "parsed": 97,
  "unparsed": [
    {"path": "crates/core/index.rs", "reason": "deleted"},
    {"path": "tests/feature.rs", "reason": "macro_unexpanded", "evidence": {"macro_calls": 82}}
  ],
  "by_ext": {"rs": 112, "md": 24, "ts": 485}
}
```

`reason` 只有四个值：`deleted`（文件已不在磁盘）、`no_mapped_item`（只有 use/mod/impl）、
`empty`（空文件/无项）、`macro_unexpanded`（有宏调用、token 树未展开）。

`evidence` **只在能定位时给**（目前只有 `macro_calls` 与 `db_mismatch` 的 `mapped` 计数），
不是"为了让字段显得丰富"而加。

## 浅克隆语义

`git clone --depth N` 的仓库：

- `jw ingest` 当场提示：`注意：浅克隆，历史不完整（只有克隆点之后；用完整 clone 或 --unshallow）`；
- `jw timeline` 的仓库行写 `150 个提交（浅克隆：只有克隆点之后的历史，已扫描 150）`，
  **不会**写"已全量扫描"；
- `total_commits` 反映的是浅克隆的计数，不是上游仓库总数。

## 兼容规则

- **只加字段、不改字段含义**：新增字段对旧客户端无害（客户端应忽略未知字段）。
- 删字段、改语义 → 协议版本 `v` 加 1，并让新旧并存一个版本；请求里可带 `v` 指定期望版本。
- 错误只分两类：协议错误（`ok:false` + `error`）与业务三态（`result.exit` 0/1/2）。

## 外部用法

`tools/watch.mjs` 是 48 行的零依赖示例：`refresh` → `watch` → 只重查受影响的卡。
直接 `node tools/watch.mjs <repo>` 即可。插件作者不需要读 CLI 源码，只需要这一页和 `--json` 的字段表。
