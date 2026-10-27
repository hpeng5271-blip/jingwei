# 黑盒实验报告 · 2026-10-25

协议：**先盲跑**（只看输出，不看实现），结果出来后用**与 jw 无关的实现**（git / grep / python tomllib）独立复算，最后才允许看源码解释差异。

## 五个目标一张表

| 目标 | 语言 | 规模（浅克隆 depth 150） | 符号 | 依赖 | 独立复算 | 结论 |
|---|---|---|---|---|---|---|
| ripgrep | Rust | 150 提交 / 542 改动 / 240 文件 | 3568 符号，97/112 个 .rs 有符号（86%） | 40（直接 30 · 开发 10） | 依赖 40==40；符号 97 与 grep 99 差 2 条（宏 token 树 / raw string 假阳性，jw 对） | 全链通过 |
| requests | Python | 191 提交 / 1057 改动 / 139 文件 | 0（`未解析（rust only）`） | 无 Cargo.toml | 改动/文件数一致；`card src/requests/api.py` held、改动 6 | git 线通过 |
| tauri | Rust+TS | 150 提交 / 2032 改动 / 1312 文件 | 5779 符号，330/332 个 .rs 有符号（99%） | 166（直接 141 · 开发 15 · 构建 10；workspace 声明 10 条不计入）；**npm 未采集** | 依赖 166==166；符号 330==330 | 混合通过 |
| django | Python | 150 提交 / 7786 改动 / 7107 文件 | 0（rust only） | 无 Cargo.toml | 改动 7786==7786、文件 7107==7107、query.py 提交 8==卡片改动 8 | git 线通过；浅克隆已报 |
| vue | TS | 150 提交 / 1211 改动 / 706 文件 | 0（ts 485 / tsx 5 / js 31 全部未解析，db 里 0 条 .ts 符号） | `无 Cargo.toml（npm 依赖未采集）` | 改动 1211==1211、文件 706==706 | git 线通过；浅克隆已报 |

所有目标 `check` 七项全 0（覆盖/孤儿/未注册 kind/悬空 source/write_log）。

## 实验暴露并修掉的问题

| 问题 | 触发目标 | 修法 |
|---|---|---|
| 非 Rust 文件的 0 符号是裸 `符号 0`（分不清语言缺口与真无符号） | requests / vue | 卡片写 `未解析（rust only）` 或 `未解析（不在 parse 结果里）` |
| `parse` 全局 2000 上限静默截断 | tauri（74/332） | 默认改 `--limit 0` 全量，并打 `覆盖：rust X 文件 / Y 有符号` |
| `macro_rules!` 未映射 | ripgrep（23 → 15 未解析） | 映射为 kind `macro`（另加 union/mod） |
| 同名依赖在 normal/dev 撞 subject，第二次被 DO NOTHING 吞 | tauri（169 vs 151 live） | `dep:<name>` / `dep:<name>@dev|@build` |
| 只扫 workspace 成员，漏独立 crate（fuzz/） | ripgrep（deps 39 vs 40） | 全树扫 Cargo.toml |
| `[workspace.dependencies]` 被当成真依赖 | tauri（多 3 条） | 只计数：`workspace 声明 N 条（不计入依赖）` |
| 依赖删掉后旧判断仍 live | tauri（169 → 166） | 重跑 deps 时过期不再出现的依赖 |
| 浅克隆时 timeline 说"已全量扫描" | django（150 vs 25000+ 实际） | 浅克隆显式提示 + timeline 分支 |

## 已知限制（不是 bug）

- `macro_rules!` 调用体里的定义 tree-sitter 不展开 → `coverage` 只说“有宏调用 N 处”，不推断符号在宏里；
- 非 Rust 仓库只有 git 线（改动/时间线/提交卡/范围卡），`parse` 与符号类卡片不可用；
- 浅克隆只有克隆点之后的历史（depth 150），`total_commits` 反映的是浅克隆的计数。

## 复现

```
git clone https://gh-proxy.com/https://github.com/<owner>/<repo>.git   # 或本地代理 127.0.0.1:23333
jw init <repo> && jw ingest --limit 0 <repo> && jw docs <repo> && jw parse <repo> && jw deps <repo>
jw check <repo> && jw timeline <repo> && jw coverage <repo> --json
```

交叉验证脚本：`.pro/exp/b17_crosscheck.py`（git/grep/tomllib）与 `.pro/exp/b19_vue_sym.py`。
