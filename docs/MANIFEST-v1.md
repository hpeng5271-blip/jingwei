# 基线包 MANIFEST v1.1（2026-10-20 更新）

> 目的：**从零到能跑**。任何一台机器拿到这个包 + Rust 工具链，按 §3 的命令就能复现核心能力。
> 本文件由 pro 决策层定清单、flash 执行核对。

## 1. 必须有

| # | 内容 | 为什么 |
|---|---|---|
| 1 | `crates/{model,engine,cli,app}` + `src-tauri/` + `ui/` + `Cargo.toml`/`Cargo.lock` | 全部源码（`ui/` 无打包器，静态嵌入） |
| 2 | `tools/gates.ps1`、`gate.mjs`、`uitest.mjs`、`uitest-window.mjs`、`collect_facts.mjs`、`check_facts.mjs` | 门禁全套 |
| 3 | `tools/gates.json` + `tools/facts.json` | 门禁清单与数字真源 |
| 4 | `AGENTS.md` / `框架.md` / `SOP.md` / `README.md` / `使用说明.md` | 入口与方法论 |
| 5 | `docs/decisions/`（README + D-01…D-04 + PRO-GOAL） | 决策记录协议（D-03 增量扫、D-04 跨文件限定名） |
| 6 | `docs/reference/借鉴-设计细节.md` | 学习产出（salsa 复用等） |
| 7 | `docs/process/研究对象基线.md` | 两个复杂项目的选型 + 复现命令 + 实测数字 |
| 8 | `tools/llm/`（digest / slice / caps / ledger / limits） | 降本增效的输入治理 |
| 9 | `tools/smoke-pack.mjs` + `tools/check-scripts.mjs` | **验"包真能跑"** 与"工具脚本语法别坏"（新增门禁规则） |
| 10 | `tools/watch.mjs` / `tools/scan-changed.mjs`（Node 兜底） | 增量扫：journal 空就明说"查不了" |

## 2. 不许有

| 内容 | 原因 |
|---|---|
| `dist/*.zip`、`dist/_old/` | 构建产物，包内重新打 |
| `target/`、`node_modules/` | 体积与可复现性 |
| `.jingwei/`（运行时图库） | 本机状态，不是源码 |
| 第三方项目源码（`D:\research\jingwei-objects` 474MB） | 太大；只引用 + 复现命令 |
| `__pycache__`、`.env`、`secrets.env` | 垃圾与机密 |
| `docs/archive/` 之外的史料 | 已归档，不再进主链 |

## 3. 从零到跑（4 条命令）

```powershell
cargo build --release                     # 或 cargo test --workspace --all-targets
powershell -File tools\gates.ps1          # 门禁全套，结尾有表
target\release\jingwei.exe index          # 建图谱（当前目录）
target\release\jingwei.exe positions      # 位置回读（0 错才算健康）
```

## 4. 研究对象（只引用，不复刻）

见 `docs/process/研究对象基线.md`：`tauri`（Rust，329 文件 / 5,916 符号 / 3.2s）与
`flask`（Python，83 文件 / 1,580 符号 / 231ms；依赖边 183 条）。

## 5. 版本与产物

- 版本：`0.3.0`（三处一致：workspace `Cargo.toml` / `package.json` / `src-tauri/tauri.conf.json`）
- 产物：`dist/jingwei-0.3.0-win-x64.zip`
- 校验：`node tools/check-pack.mjs`（打包冒烟）

## 5. 0.3.0 包里到底有什么（2026-10-20 06:12 核对）

| 内容 | 数量/大小 | 说明 |
|---|---|---|
| `jingwei.exe` / `jingwei-gui.exe` | 2 个 | CLI + 界面 |
| `演示项目/` | 3 个（hello_python / hello_rust / messy_service） | 打开就能看效果 |
| `docs-shots/` | 7 张 PNG | 真窗口截图，不是假图 |
| `使用说明.md` / `演示脚本.md` / `介绍.md` / `先看这个.txt` | 4 份 | 从"怎么用"到"为什么这么设计" |
| 总大小 | **6.06 MB** | zip（解压后约 15 MB） |

## 6. 怎么验这份清单没吹牛

```powershell
node tools\smoke-pack.mjs --gui   # 解压真包 → index + positions + rename + GUI
```

它做四件事：解压到临时目录、对演示项目跑 `index`（要出"个文件 ·"那行）、
跑 `positions`（对不上必须为 0）、拷一份 demo 跑 `rename --write`（要"回读通过"）、
起 GUI 三秒不退。**任何一步不对就退出码非 0。**