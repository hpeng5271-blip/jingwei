# AGENTS.md · 经纬（jingwei）

> 每轮必读，只留"现在什么状态、下一步做什么"（2026-10-22 瘦身 3224→~800 tokens）。
> 细则 `docs/reference/操作细节.md` · 历史数字 `docs/reference/里程碑-2026-10.md` ·
> 决策 `docs/decisions/` · 欠什么 `docs/reference/债表.md` · 一页索引 `docs/MANIFEST-摘要.md`。

- **是什么**：终端与 IDE 之间的理解层——把项目/文件的状态讲清楚，需要时才讲。
- **代码**：`D:\projects\jingwei`（唯一活跃仓库），Rust workspace + Tauri + 静态 `ui/`。
- **瓶颈**：闭环只提高自洽度；**裁判权在环路外面**。

## 每轮节奏（缺一不算完成）

1. **先红后绿**：新判据改之前必须红过，红的输出留着。
2. `node tools\collect_facts.mjs` → `check_facts --write-docs` → `check_facts`（逐个等于）→ `check-links.mjs` → `tools\gates.ps1`。
3. 数字只信 `tools/facts.json`，用脚本写，**不许手抄**。
4. 提交 = 门禁绿 + `git status` 干净 + 有红证据；信息写 为什么/改了什么/证据。
5. push 带代理 `127.0.0.1:7897`（HTTPS_PROXY 与 HTTP_PROXY）。

## 不许破的线

- 三态：是 / 不是 / **查不了**；`unknown` 不许写成"没有"。
- 数据正确优先（`store::apply_file_update` 四事务）· 不生成修复命令（`fix.rs`）·
  无自查不报错（`rules::enforce_evidence`）· 关系必有来源（`Edge::new` 的 `source`）。

## 数据位置

- 图谱 `<项目>\.jingwei\graph.db`（**仓库根那份是工作图谱，别删**）
- 用户数据 `%LOCALAPPDATA%\jingwei\`（tasks/perf/缓存；**不放 TEMP**）

## 分工

人出题定判据；**flash** 改代码与验收；**pro** 只做硬决策、只读、给可判定结论。
**pro 通道复用同一个会话**（不许每问新建子智能体）。

## 命令

```
门禁 tools\gates.ps1   单测 cargo test --workspace --all-targets
静态 cargo clippy --workspace --all-targets -- -D warnings
外壳 node tools\uitest.mjs   界面 node tools\uitest-window.mjs
索引 jingwei index   单文件 jingwei file <路径> [--json]
交接 jingwei handover   解释 jingwei explain <名|symbol:id>
真源 node tools\collect_facts.mjs   性能 node tools\perf.mjs
打包 pack.ps1 → dist\<日期>\   包冒烟 node tools\smoke-pack.mjs --gui
```

## 读什么

`docs/MANIFEST-摘要.md`（不知读哪份先看它）· `docs/design/界面约定.md`（加界面前）·
`docs/reference/失效模式清单.md`（我犯过的错）· `docs/reference/债表.md`（下一个做哪条）。

## 省流

一件事一次会话；命令必须超时；跑界面前后查 `jingwei-gui` 残留=0；
重活前后 `node tools\health.mjs --once`，编译 `CARGO_BUILD_JOBS=4`。
上下文**轮数 ≥12 或剩余 <40%** 就重整，保留四样：目标与 todo / 已定裁决与未决问题 / 真源与门禁状态 / 数据契约。

**人的话优先于本文件。**
