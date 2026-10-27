# main.rs 拆分映射表（0.4.0-rc，只读分析）

当前 `crates/cli/src/main.rs` = **6490 行**；目标 <800 行；一次只搬一个模块。

| 目标模块 | 函数数 | 行数（约） | 依赖 | 风险 |
|---|---|---|---|---|
| 命令层 cmd/*（ingest/docs/parse/why/ask/explain/find/verify/expire/resolve/card/edges/impact/tests_for/cite/...） | 38 | 3360 | `datacore::store` / `std` | 大量 `use` 与 `store` 生命周期；搬动时只改 `pub(crate)` 可见性 |
| 视图投影（render_/find_json/short_id/def_location/top_authors/...） | 7 | 514 | `datacore::store` / `std` | 纯函数为主，风险低；注意 `typed_path/typed_name` 等工具也要搬 |
| serve 协议层（`fn cmd_serve`，3481 起） | 1 | 约 980 | `datacore::store` / `std` | 与 trust/LSP 状态耦合；先搬纯分支，`RA_SESSION` 留最后 |
| web HTTP 层（serve_web/cmd_web） | 1 | 167 | `datacore::store` / `std` | `serve_web` 持有 child/线程；搬动易出借用错误，建议最后搬 |
| LSP 客户端（ra_session/RaSession/find_rust_analyzer/file_uri/uri_to_rel/map_*/hover_parts/lsp_frames） | 9 | 258 | `datacore::store` / `std` | 全局 `RA_SESSION` 必须在同一模块，跨模块引用会破坏串行锁假设 |
| 信任门（cmd_trust/trust_*/jw_user_dir） | 5 | 64 | `datacore::store` / `std` | 涉及用户目录/审计，纯移动即可 |
| 入口/CLI/main | 4 | 227 | `datacore::store` / `std` | `main` 只留参数解析与分发 |
| 其余/未归类 | 33 | 1501 | `datacore::store` / `std` | 杂项，按归属并入上述模块 |

## 执行顺序（pro 规则）
1. **web HTTP 试点（serve_web/cmd_web，167 行）** → 2. LSP 客户端（258）→ 3. 信任门（64）→ 4. 视图投影（514）→ 5. 命令层（3360）→ 6. serve 协议 → 7. 入口。
（pro 修订：先用最小叶子验证闭环，3350 行的大块放最后。）
每步：搬一个模块 → `cargo test --workspace` + 三 fixture `card/edges` SHA + `--version` → 红则回滚该步。

## 未映射命令（已知限制，M15）

archive UI 里 `invoke` 到、当前 shim 未映射的命令（页面加载触发约 12 个，其余交互触发）：

- `apply_edit`
- `apply_fix`
- `audit`
- `changes_take`
- `channel_request`
- `channel_stop`
- `check_project`
- `contract_draft`
- `contract_save`
- `create_project`
- `deep_check`
- `default_opener`
- `delete_note`
- `event_note`
- `inspect`
- `launch_project`
- `list_pythons`
- `msg_ingest`
- `msg_mark`
- `notify_gate`
- `open_in`
- `open_path`
- `openers`
- `pick_folder`
- `profile_save`
- `profiles_list`
- `project_log`
- `restore_draft`
- `run_task`
- `sample_now`
- `save_note`
- `scan_cancel`
- `scan_clean`
- `scan_similar`
- `scan_start`
- `set_default_opener`
- `snapshot_file`
- `tasks_state`
- `verify_project`
