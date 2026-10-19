# 基线包 MANIFEST v1（2026-10-19）

> 目的：**从零到能跑**。任何一台机器拿到这个包 + Rust 工具链，按 §3 的命令就能复现核心能力。
> 本文件由 pro 决策层定清单、flash 执行核对。

## 1. 必须有

| # | 内容 | 为什么 |
|---|---|---|
| 1 | `crates/{model,engine,cli,app}` + `src-tauri/` + `ui/` + `Cargo.toml`/`Cargo.lock` | 全部源码（`ui/` 无打包器，静态嵌入） |
| 2 | `tools/gates.ps1`、`gate.mjs`、`uitest.mjs`、`uitest-window.mjs`、`collect_facts.mjs`、`check_facts.mjs` | 门禁全套 |
| 3 | `tools/gates.json` + `tools/facts.json` | 门禁清单与数字真源 |
| 4 | `AGENTS.md` / `框架.md` / `SOP.md` / `README.md` / `使用说明.md` | 入口与方法论 |
| 5 | `docs/decisions/`（README + D-01 + PRO-GOAL） | 决策记录协议 |
| 6 | `docs/reference/借鉴-设计细节.md` | 学习产出（salsa 复用等） |
| 7 | `docs/process/研究对象基线.md` | 两个复杂项目的选型 + 复现命令 + 实测数字 |
| 8 | `tools/llm/`（digest / slice / caps / ledger / limits） | 降本增效的输入治理 |

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
