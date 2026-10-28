use clap::{Parser, Subcommand};
use datacore::ids::{change_row, evidence_id, judgment_row, source_id};
use datacore::model::{record_verdict, Evidence, Judgment, Polarity, Source, Verdict};
use datacore::query;
use datacore::store::Store;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod edges_resolve;
mod normalize;
// 日志模块给 jw 用；jingwei-gui 那个 bin 复用本文件但不打日志，
// 所以对 "没人用" 的警告整体豁免（不是藏问题：模块本身就只服务 CLI）。
#[allow(dead_code)]
mod jwlog;
// 归一化层（工序 ⑤）：这三个名字原先散在本文件里，现在统一在 normalize.rs
use normalize::path_of as subject_path;
use normalize::scope as scope_of;
use engine::view::card_value;
use engine::view::{clip_display, day_str, render_card_text, short_commit};
use normalize::subject as resolve_subject;

#[derive(Parser)]
#[command(name = "jw", version, about = "jingwei core v0.1")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// 调试日志（stderr；stdout 仍是纯数据）
    #[arg(long, global = true, hide = true)]
    debug: bool,
}

/// 解析规则版本：改动符号/边的语义时 +1，老库会被强制重建。
pub use datacore::model::PARSE_RULES_VERSION;

#[derive(Subcommand)]
enum Cmd {
    /// 在仓库里初始化 .jingwei/jingwei.db
    Init {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 读 git 历史，落快照/改动/判断
    Ingest {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value_t = 200)]
        limit: usize,
        /// 忽略快照断点，重写已有提交（修复历史数据的 blob 等）
        #[arg(long)]
        force: bool,
    },
    /// 采集 Markdown 文档，落 doc 判断
    Docs {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    /// 解析 Rust 源码，落 symbol 判断
    Parse {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 0 = 全量（默认；2000 是静默截断，pro 已否）
        #[arg(long, default_value_t = 0)]
        limit: usize,
        /// 打每文件耗时 p50/p95/top10
        #[arg(long)]
        timing: bool,
        /// 只打印 walk 到的源文件（排障用，不写库）
        #[arg(long)]
        debug_walk: bool,
    },
    /// 问：这个路径被谁改过（held / refuted / uncertain）
Why {
        path: String,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 内部/调试：按**完整 subject** 取结论。对外请用 card/explain（选择器→视图）
Ask {
        subject: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 回看：只看该时刻之前的判断（unix 秒）
        #[arg(long)]
        as_of: Option<i64>,
        /// 输出 JSON（含 scope.env，测试与前端用）
        #[arg(long)]
        json: bool,
    },
    /// 讲清一个路径现在什么状态（状态 + 改动 + 文档 + 符号）
Explain {
        path: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 输出 JSON（给编辑器/脚本用）
        #[arg(long)]
        json: bool,
    },
    /// 项目概览与演进：有多少东西、按天怎么长起来的
    Project {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 项目时间线：按天看提交/新增/改动/删除与累计文件
    Timeline {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 输出 JSON（与 card --json 同一套版本约定）
        #[arg(long)]
        json: bool,
        /// 顺带给改动最多的前 N 个文件
        #[arg(long)]
        top: Option<usize>,
    },
    /// 记忆折叠（默认只读计划）：同向连续段 → 留首尾 + gist
    Compact {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 真正执行删除（未启用，等 gist 落点裁决）
#[arg(long)]
        apply: bool,
    },
    /// 回到上次停留的位置（跨次可用）
Resume {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 取消钉住（pin 槽只有一个）
    Unpin {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 关系：列一个文件出发的边（mod / use / impl）
    Edges {
        path: Option<String>,
        root: Option<PathBuf>,
        /// 只看某一类：mod / use / impl
        #[arg(long)]
        kind: Option<String>,
        /// 解析规则版本：改动符号/边的语义时 +1，老库会被强制重建。

/// 解析率统计（use 可解析 X/Y · ext Z · 未解析 ? N）
        #[arg(long)]
        stats: bool,
        /// 机器可读（配合 --stats）
        #[arg(long)]
        json: bool,
        /// 反查：谁调用了这个 subject（call 边，incoming calls）
        #[arg(long)]
        to: Option<String>,
    },
    /// 覆盖：改动过的 .rs 里哪些没有符号判断（回答"0 符号"到底为什么）
    Coverage {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 列多少条（默认 20）
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// 输出 JSON（UI 契约：lang/total/parsed/unparsed/by_ext）
        #[arg(long)]
        json: bool,
    },
    /// 采构建：Cargo.toml/lock → 依赖判断
    Deps {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 长驻服务：stdin/stdout NDJSON（v1.5 订阅的接口）
    Serve {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 关闭订阅轮询（默认开）
        #[arg(long)]
        no_subscribe: bool,
    },
    /// 增量刷新：一次跑 docs + parse + ingest（编辑器保存后调它）
    Refresh {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    /// 单文件结构卡片：状态 + 符号结构 + 最近改动 + 依据
    Card {
        path: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 输出 JSON：卡片与 UI 层的数据契约
        #[arg(long)]
        json: bool,
        /// 观测态：按天改动历史
        #[arg(long)]
        history: bool,
        /// 卡片态：history / def / dups（文本与 --json 同一份数据）
        #[arg(long)]
        state: Option<String>,
        /// 批量：再取几张卡（一次进程取 N 张）
        #[arg(long = "also")]
        also: Vec<String>,
        /// 钉住这张卡（pin 槽只有一个，新的会替换旧的）
        #[arg(long)]
        pin: bool,
    },
    /// 扫描并列出冲突
Conflicts {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 重验一条判断：符号/文件还在 → held；不在了 → refuted（写新判断 + 旧判断过期）
    Verify {
        subject: String,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 把一条判断标为过期（默认最新那条；--id 定点）
Expire {
        subject: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 过期时刻（unix 秒），默认现在
#[arg(long)]
        at: Option<i64>,
        /// 指定判断 id（必须属于该 subject）
#[arg(long)]
        id: Option<String>,
    },
    /// 人工裁决一对冲突：resolve <subject> --verdict held|refuted --by <人> --why <一句话>
    Resolve {
        subject: String,
        #[arg(long)]
        verdict: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        why: String,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 按 name 找符号 / 文件 / 文档 / 决策
    Find {
        term: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        /// 只看某一类：symbol / change / doc / decision / hypothesis
        #[arg(long)]
        kind: Option<String>,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
    },
    /// 按 subject 子串搜索（find 的别名，保留旧习惯）
    Search {
        term: String,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 登记候选：alt-add <候选 subject> <决策 subject>
    AltAdd {
        child: String,
        parent: String,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 列出某决策的候选：alt-list <决策 subject>
    AltList {
        parent: String,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 导入人工记录（`D-*.md`）：目录或单文件
    Record {
        path: PathBuf,
        /// 只检查记录写法（替代项有没有明确结论），不改库
        #[arg(long)]
        lint: bool,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 统计
    Stat {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 被 size 政策跳过的文件（>2MB），与 control 的 skipped 对照
    Skipped {
        #[arg(long)]
        json: bool,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 改动最多的文件（dashboard 用；timeline --top N 的别名）
    #[command(hide = true)]
    Top {
        #[arg(long, default_value_t = 10)]
        files: usize,
        #[arg(long)]
        json: bool,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 跨仓对比（v0）：两个库只读，按依赖名比版本
    Cross {
        /// 仓 A 根
        a: PathBuf,
        /// 仓 B 根
        b: PathBuf,
        /// 比依赖（当前唯一模式）
        #[arg(long)]
        dep: bool,
        /// 只看某个生态：cargo / npm / py（默认全部）
        #[arg(long)]
        eco: Option<String>,
        /// 比"同内容文件"（按 git blob，不含路径）
        #[arg(long)]
        files: bool,
        /// 把 0 字节文件也算进"同内容"（默认排除）
        #[arg(long)]
        include_empty: bool,
        #[arg(long)]
        json: bool,
    },
    /// 原始层叶子名（按文件）：对照组不变量用
    RawLeaves {
        #[arg(long)]
        json: bool,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 原始层计数：use / mod / impl 块（对照组用，不做解析）
    RawStats {
        #[arg(long)]
        json: bool,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 记忆折叠：只折「已过期 + 同 verdict + n≥8 + 90 天」的中间段（dry-run 默认）
    Fold {
        #[arg(long, default_value = "90d")]
        older_than: String,
        /// 真写（默认只打印计划）
        #[arg(long)]
        apply: bool,
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 校验不变量
Check {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// 本地工作台：HTTP + bridge（默认不开浏览器；--open 才开，防止自动化弹窗）
    Web {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value_t = 7777)]
        port: u16,
        #[arg(long)]
        open: bool,
    },
    /// LSP 信任门（用户级信任库；不写仓库）
    Trust {
        /// status | grant | revoke
        #[arg(default_value = "status")]
        action: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// 打印语言能力表（capability negotiation 的外部契约）
    Capability {
        #[arg(long)]
        json: bool,
    },
    /// 影响面：改一个函数/文件，谁受影响、哪些测试要跑（反向 call 闭包，只读）
    Impact {
        /// 目标：`symbol:<path>#<fn>` 或文件路径（文件 = 该文件所有 fn）
        target: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// 哪个测试是这个符号的例子（静态可达，不等于运行覆盖）
    TestsFor {
        /// 目标：`symbol:<path>#<fn>`
        target: String,
        /// 反向 BFS 深度（默认 3）
        #[arg(long, default_value_t = 3)]
        depth: i64,
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// 研究引用：给出可粘贴、可复现到具体提交的引用行（permalink 语义）
    Cite {
        /// subject：`symbol:<path>#<name>` 或 `file:<path>`
        subject: String,
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

/// 双击（无参数）时给人看的一行提示；CLI 没有参数就没有可做的事。
fn no_arg_hint() -> &'static str {
    "经纬是命令行工具：双击 start.cmd 打开工作台；命令行用 jw --help / jw web <repo>"
}

/// 只有在"无参数 + 交互式终端"时才 pause（管道/CI 不阻塞）。
fn should_pause(argc: usize, is_tty: bool) -> bool {
    argc == 1 && is_tty
}

/// `jw lsp-check` 的手写入口（不经 clap）。参数：
/// `--json`、`--root <dir>`、`--limit <n>`、`--debug`，其余按路径收。
#[allow(dead_code)] // gui bin 复用本文件但不走这些入口
fn lsp_check_entry() -> ExitCode {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut root: Option<PathBuf> = None;
    let mut json = false;
    let mut limit = 200usize;
    let mut debug = false;
    let mut it = std::env::args().skip(2).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--debug" => debug = true,
            "--root" => {
                if let Some(v) = it.next() {
                    root = Some(PathBuf::from(v));
                }
            }
            "--limit" => {
                if let Some(v) = it.next() {
                    if let Ok(n) = v.parse::<usize>() {
                        limit = n;
                    }
                }
            }
            s if s.starts_with("--") => {
                eprintln!("未知参数：{s}");
                return ExitCode::from(3);
            }
            s => paths.push(PathBuf::from(s)),
        }
    }
    jwlog::init(debug);
    match cmd_lsp_check(&paths, root.as_ref(), json, limit) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(3)
        }
    }
}

fn main() -> ExitCode {
    // `--version` 要能"照着讲"：semver + 派生规则版本一起给（clap 只给 semver）
    if let Some(a) = std::env::args().nth(1) {
        if a == "--version" || a == "-V" {
            println!("jw {} (rules {})", env!("CARGO_PKG_VERSION"), PARSE_RULES_VERSION);
            return ExitCode::SUCCESS;
        }
    }
    // `lsp-check` 走**手写分发**而不是 clap：这个子命令在 clap derive 上会让
    // 帮助渲染栈溢出（实测 `jw --help` 直接 stack overflow），绕开它最省事、也最可回滚。
    if let Some(a) = std::env::args().nth(1) {
        if a == "lsp-check" {
            return lsp_check_entry();
        }
    }
    use std::io::IsTerminal;
    let argv: Vec<String> = std::env::args().collect();
    if should_pause(argv.len(), std::io::stdin().is_terminal()) {
        println!("{}", no_arg_hint());
        println!("按回车退出…");
        let mut _line = String::new();
        let _ = std::io::stdin().read_line(&mut _line);
        return ExitCode::SUCCESS;
    }
    let cli = Cli::parse();
    jwlog::init(cli.debug);
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            let msg = e.to_string();
            if msg.starts_with("no_db") {
                // 只读命令在无库目录：明确说"没库"，绝不建库（exit 2）
                println!("{msg}");
                return ExitCode::from(2);
            }
            eprintln!("error: {e}");
            ExitCode::from(3)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.cmd {
        Cmd::Init { root } => {
            let store = Store::open(&root)?;
            println!("initialized {} at {}", store.repo(), store.db_path().display());
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Ingest { root, limit, force } => cmd_ingest(&root, limit, false, force),
        Cmd::Docs { root, limit } => cmd_docs(&root, limit, false),
        Cmd::Parse { root, limit, timing, debug_walk } => {
            if debug_walk {
                for p in collect::parse::walk_paths(&root) {
                    println!("{p}");
                }
                Ok(ExitCode::SUCCESS)
            } else {
                cmd_parse(&root, limit, false, timing)
            }
        }
        Cmd::Why { path, root } => cmd_why(&root, &path),
        Cmd::Ask { subject, root, as_of, json } => cmd_ask(&root, &subject, as_of, json),
        Cmd::Explain { path, root, json } => cmd_explain(&root, &path, json),
        Cmd::Project { root } => cmd_project(&root),
        Cmd::Timeline { root, json, top } => cmd_timeline(&root, json, top),
        Cmd::Compact { root, apply } => cmd_compact(&root, apply),
        Cmd::Card {
            path,
            root,
            json,
            history,
            state,
            also,
            pin,
        } => cmd_card(
            &root,
            &path,
            &also,
            json,
            history || state.as_deref() == Some("history"),
            pin,
            true,
            state.as_deref(),
        ),
        Cmd::Resume { root } => cmd_resume(&root),
        Cmd::Unpin { root } => cmd_unpin(&root),
        Cmd::Deps { root } => cmd_deps(&root),
        Cmd::Coverage { root, limit, json } => cmd_coverage(&root, limit, json),
        Cmd::Edges {
            path,
            root,
            kind,
            stats,
            json,
            to,
        } => {
            // `edges --stats <repo>` / `edges --to <subject> <repo>`：**只有一个**位置参数时它是 root；
            // 同时给了 path 和 root 就按原样用（否则会把 path 当 root）。
            let one_arg_is_root = (stats || to.is_some()) && path.is_some() && root.is_none();
            let (p, r) = if one_arg_is_root {
                (None, Some(PathBuf::from(path.clone().unwrap_or_default())))
            } else {
                (path.clone(), root.clone())
            };
            cmd_edges(
                &r.unwrap_or_else(|| PathBuf::from(".")),
                p.as_deref().unwrap_or("."),
                kind,
                stats,
                json,
                to,
            )
        }
        Cmd::Serve { root, no_subscribe } => cmd_serve(&root, no_subscribe),
        Cmd::Refresh { root, limit } => cmd_refresh(&root, limit, false),
        Cmd::Conflicts { root } => cmd_conflicts(&root),
        Cmd::Verify { subject, root } => cmd_verify(&root, &subject),
        Cmd::Expire {
            subject,
            root,
            at,
            id,
        } => cmd_expire(&root, &subject, at, id),
        Cmd::Resolve {
            subject,
            verdict,
            by,
            why,
            root,
        } => cmd_resolve(&root, &subject, &verdict, &by, &why),
        Cmd::Search { term, root } => cmd_find(&root, &term, None, false),
        Cmd::Find { term, root, kind, json } => cmd_find(&root, &term, kind, json),
        Cmd::AltAdd { child, parent, root } => cmd_alt_add(&root, &child, &parent),
        Cmd::AltList { parent, root } => cmd_alt_list(&root, &parent),
        Cmd::Record { path, root, lint } => cmd_record(&root, &path, lint),
        Cmd::Stat { root } => cmd_stat(&root),
        Cmd::Skipped { json, root } => {
            let mut out = Vec::new();
            collect::parse::walk_paths(&root)
                .into_iter()
                .filter(|p| {
                    std::fs::metadata(root.join(p)).map(|m| m.len()).unwrap_or(0) > 2 * 1024 * 1024
                })
                .for_each(|p| out.push(p));
            if json {
                println!("{}", serde_json::to_string(&out)?);
            } else {
                for p in out {
                    println!("{p}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Top { files, json, root } => {
            let store = Store::open(&root)?;
            let rows = store.top_files(files)?;
            if json {
                println!("{}", serde_json::to_string(&rows.iter().map(|(p, n)| serde_json::json!({"path": p, "n": n})).collect::<Vec<_>>())?);
            } else {
                for (p, n) in rows {
                    println!("{n}\t{p}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Cross { a, b, dep, eco, files, include_empty, json } => {
            if files {
                cmd_cross_files(&a, &b, include_empty, json)
            } else {
                cmd_cross(&a, &b, dep, eco.as_deref(), json)
            }
        }
        Cmd::RawLeaves { json, root } => {
            let m = collect::parse::raw_use_leaves(&root);
            if json {
                println!("{}", serde_json::to_string(&m)?);
            } else {
                for (f, ls) in m.iter().take(10) {
                    let pairs: Vec<String> = ls.iter().map(|(l, n)| format!("{l}@{n}")).collect();
                    println!("{f}  {}", pairs.join(","));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::RawStats { json, root } => {
            let (u, m, i) = collect::parse::raw_totals(&root);
            let py = collect::parse::raw_py_counts(&root);
            let ts = collect::parse::raw_ts_counts(&root);
            if json {
                println!("{}", serde_json::json!({"use": u, "mod": m, "impl": i, "py": py, "ts": ts}));
            } else {
                println!("use {u} · mod {m} · impl {i}");
                let g = |k: &str| py.get(k).copied().unwrap_or(0);
                println!(
                    "py func {} · method {} · class {} · import {}",
                    g("func"), g("method"), g("class"), g("import")
                );
                let h = |k: &str| ts.get(k).copied().unwrap_or(0);
                println!(
                    "ts func {} · class {} · interface {} · type {} · import {}",
                    h("func"), h("class"), h("interface"), h("type"), h("import")
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Fold {
            older_than,
            apply,
            root,
        } => cmd_fold(&root, &older_than, apply),
        Cmd::Check { root } => cmd_check(&root),
        Cmd::Web { root, port, open } => cmd_web(&root, port, open),
        Cmd::Trust { action, root, json } => cmd_trust(&action, &root, json),
        Cmd::Capability { json } => cmd_capability(json),
        Cmd::Cite { subject, root, json } => cmd_cite(&root, &subject, json),
        Cmd::Impact { target, root, json } => cmd_impact(&root, &target, json),
        Cmd::TestsFor {
            target,
            depth,
            root,
            json,
        } => cmd_tests_for(&root, &target, depth, json),

    }
}

fn cmd_ingest(root: &PathBuf, limit: usize, quiet: bool, force: bool) -> anyhow::Result<ExitCode> {
    if !collect::git::is_repo(root) {
        if !quiet {
            eprintln!("not a git repository: {}", root.display());
        }
        return Ok(ExitCode::from(2));
    }
    let store = Store::open(root)?;
    // 浅克隆必须当场说：否则 timeline "已全量扫描" 会骗人
    if collect::git::is_shallow(root) && !quiet {
        println!("注意：浅克隆，历史不完整（只有克隆点之后；用完整 clone 或 --unshallow）");
    }
    // --limit 0 = 全量（timeline 要"从零"必须能拿全历史）
    // --limit 0 = 全量，但超大历史（>5000 提交）会几十小时：显式降级并说明
    let total_for_cap = collect::git::total_commits(root);
    let limit = match (limit, total_for_cap) {
        (0, Some(t)) if t > 5000 => {
            if !quiet {
                println!("注意：历史 {t} 个提交，全量太重，本次只扫最近 5000（用 --limit 指定更多）");
            }
            5000
        }
        (0, _) => 1_000_000,
        (n, _) => n,
    };

    let commits = collect::git::log(root, limit).map_err(|e| anyhow::anyhow!(e))?;
    let repo = store.repo().to_string();
    let mut touched = 0usize;
    let mut written: Vec<String> = Vec::new();
    // 断点续跑：已经入库的提交跳过（snapshot 表就是进度）
    let done = if force { Default::default() } else { store.snapshot_hashes()? };

    store.begin()?;
    let mut since_flush = 0usize;
    for c in &commits {
        if done.contains(&c.hash) {
            continue;
        }
        store.put_snapshot(&c.hash, c.at)?;
        // source 一个提交一个：原来每文件 REPLACE 同一行（k8s 单提交 400+ 次）
        let src = Source {
            id: source_id(&repo, "git", c.at),
            instrument: "git".into(),
            instrument_version: None,
            env: format!("repo={repo}"),
            time: c.at,
        };
        store.put_source(&src)?;
        for f in &c.files {
            let ch = change_row(
                &c.hash,
                &f.path,
                &f.status,
                &c.author,
                c.at,
                &c.subject,
                f.old_path.clone(),
                f.blob.clone(),
            );
            store.put_change(&ch)?;
            touched += 1;

            let subject = format!("file:{}", f.path);
            written.push(subject.clone());
            let mut scope = scope_of(vec![f.path.clone()], "file", &repo);
            // 内容身份：rename 后 blob 不变 → entity 不变（历史不断链）
if let Some(blob) = &f.blob {
                scope.env.insert("entity".into(), format!("b:{blob}"));
            }
            if let Some(old) = &f.old_path {
                scope.env.insert("old_path".into(), old.clone());
            }
            let j = judgment_row(&repo, &subject, "change", Verdict::Held, c.at, 1, 1, scope);
            store.put_judgment(&j)?;

            let ev = Evidence {
                id: evidence_id(&j.id, &c.hash, "supports"),
                judgment_id: j.id.clone(),
                premise: format!("commit:{}", c.hash),
                polarity: Polarity::Supports,
                source_id: src.id.clone(),
            };
            store.put_evidence(&ev)?;
        }
        since_flush += 1;
        // 每 50 个提交分批提交并记进度：中断后能从 snapshot 表续跑
        if since_flush % 50 == 0 {
            store.commit()?;
            store.set_meta("ingest_done_commits", &(done.len() + since_flush).to_string())?;
            store.begin()?;
        }
    }

    store.set_meta("scanned_commits", &commits.len().to_string())?;
    store.set_meta("last_ingest_at", &datacore::store::now_secs().to_string())?;
    store.note_write(&written)?;
    store.commit()?;

    if !quiet {
        println!(
            "ingested {} commits, {} file-changes into {}",
            commits.len(),
            touched,
            store.db_path().display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_docs(root: &PathBuf, limit: usize, quiet: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    let repo = store.repo().to_string();
    let scan_at = datacore::store::now_secs();
    let docs = collect::docs::scan(root, limit);

    // source 一次扫描一个：time 取 max(mtime)，重跑同 mtime 同 id，不膨胀
    let src_time = docs
        .iter()
        .map(|d| d.mtime)
        .filter(|m| *m > 0)
        .max()
        .unwrap_or(scan_at);
    let src = Source {
        id: source_id(&repo, "docs", src_time),
        instrument: "docs".into(),
        instrument_version: None,
        env: format!("repo={repo}"),
        time: src_time,
    };
    store.put_source(&src)?;

    let mut written: Vec<String> = Vec::new();
    for d in &docs {
        let subject = format!("doc:{}", d.path);
        written.push(subject.clone());
        // at = 文件 mtime：同一文件重跑同 id（幂等），改文件才产生新判断
        let at = if d.mtime > 0 { d.mtime } else { scan_at };
        let mut scope = scope_of(vec![d.path.clone()], "file", &repo);
        // 身份解析：同内容已见过 → 复用同一 entity 与首次路径（改名/移动不换身份）
let (entity, first_path) = match store.find_entity_by_content(&d.content_hash)? {
            Some((e, fp)) => (e, fp),
            None => (d.entity.clone(), d.path.clone()),
        };
        scope.env.insert("entity".into(), entity);
        scope.env.insert("entity_first_path".into(), first_path);
        scope.env.insert("entity_content".into(), d.content_hash.clone());
        let j = judgment_row(&repo, &subject, "doc", Verdict::Held, at, 1, 1, scope);
        store.put_judgment(&j)?;
        let ev = Evidence {
            id: evidence_id(&j.id, &d.path, "supports"),
            judgment_id: j.id.clone(),
            premise: format!("doc:{}:{}", d.path, d.title),
            polarity: Polarity::Supports,
            source_id: src.id.clone(),
        };
        store.put_evidence(&ev)?;
        store.expire_older_than(&subject, Some(&j.id), at)?;
    }
    store.note_write(&written)?;
    if !quiet {
        println!("ingested {} docs into {}", docs.len(), store.db_path().display());
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_parse(root: &PathBuf, limit: usize, quiet: bool, timing: bool) -> anyhow::Result<ExitCode> {
    let limit = if limit == 0 { usize::MAX } else { limit };
    let root2 = root.clone();
    // AST 递归在大仓上会打爆默认 1MB 栈：整个分批解析放进 128MB 栈的线程里，
    // 线程自己开库（Connection 不能跨线程共享）。
    let summary = std::thread::Builder::new()
        .stack_size(128 * 1024 * 1024)
        .spawn(move || -> anyhow::Result<(usize, usize, usize, usize, usize, usize, usize, std::collections::HashMap<String, usize>)> {
            let store = Store::open(&root2)?;
            let repo = store.repo().to_string();
            let scan_at = datacore::store::now_secs();
            let src_id = source_id(&repo, "tree-sitter", scan_at);
            store.put_source(&Source {
                id: src_id.clone(),
                instrument: "tree-sitter".into(),
                instrument_version: Some("rust".into()),
                env: format!("repo={repo}"),
                time: scan_at,
            })?;

            // 断点续跑：已有符号的文件跳过；但边层/解析规则是后加的，老库要整树补一次
            let edge_v = store.get_meta("edge_resolve_v")?.unwrap_or_default();
            let rules_v = store.get_meta("parse_rules_version")?.unwrap_or_default();
            let rules_old = rules_v != PARSE_RULES_VERSION;
            if rules_old {
                if !quiet {
                    println!(
                        "重建：rules v{}→v{}",
                        if rules_v.is_empty() { "0" } else { &rules_v },
                        PARSE_RULES_VERSION
                    );
                }
                store.set_meta("parse_rules_version", PARSE_RULES_VERSION)?;
            }
            let full = rules_old || store.edge_count()? == 0 || edge_v != "17";
            let skip = if full {
                std::collections::HashSet::new()
            } else {
                store.symbol_paths()?
            };
            if full {
                // 全量重解析：边是派生数据，先清空（否则旧 dst 会和新 dst 并存）
                store.clear_edges()?;
            }

            let mut n = 0usize;
            let mut e_mod = 0usize;
            let mut e_use = 0usize;
            let mut e_impl = 0usize;
            let mut e_import = 0usize;
            let mut written: Vec<String> = Vec::new();
            let mut err: Option<anyhow::Error> = None;
            let mut files = 0usize;

            store.begin()?;
            let report = collect::parse::scan_batched_any(root2.as_path(), limit, &skip, |_rel, _abs, syms, edges| {
                let mut step = || -> anyhow::Result<()> {
                    for sym in &syms {
                        let subject = format!("symbol:{}#{}", sym.path, sym.name);
                        let at = if sym.mtime > 0 { sym.mtime } else { scan_at };
                        let mut scope = scope_of(vec![sym.path.clone()], "symbol", &repo);
                        let ext = sym.path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
                        let lang = collect::parse::lang_for_ext(ext).unwrap_or("rust");
                        scope.env.insert("lang".into(), lang.into());
                        // 符号身份与文件内容解耦：同路径同名同 kind 跨内容版本是同一实体
                        let entity =
                            collect::hash::symbol_entity(&sym.path, &sym.name, &sym.kind);
                        let first_path = sym.path.clone();
                        scope.env.insert("entity".into(), entity);
                        scope.env.insert("entity_first_path".into(), first_path);
                        scope.env.insert("entity_content".into(), sym.content_hash.clone());
                        let j =
                            judgment_row(&repo, &subject, "symbol", Verdict::Held, at, 1, 1, scope);
                        store.put_judgment(&j)?;
                        // 同一 (subject, at) 的行若已被标过期，DO NOTHING 会吞掉新写入 → 永久哑火
                        if store.judgment_expired(&j.id)? {
                            store.put_judgment_force(&j)?;
                        }
                        let ev = Evidence {
                            id: evidence_id(
                                &j.id,
                                &format!("{}:{}:{}", sym.path, sym.line, at),
                                "supports",
                            ),
                            judgment_id: j.id.clone(),
                            premise: format!("def:{}:{}:{}:{}", sym.path, sym.line, sym.kind, at),
                            polarity: Polarity::Supports,
                            source_id: src_id.clone(),
                        };
                        store.put_evidence(&ev)?;
                        store.expire_older_than(&subject, Some(&j.id), at)?;
                        written.push(subject);
                        n += 1;
                    }
                    for e in &edges {
                        store.put_edge(&e.src, &e.dst, &e.kind, scan_at, &src_id, e.leaf.as_deref(), e.line)?;
                        match e.kind.as_str() {
                            "mod" => e_mod += 1,
                            "use" => e_use += 1,
                            "impl" => e_impl += 1,
                            "import" => e_import += 1,
                            _ => {}
                        }
                    }
                    files += 1;
                    if files % 200 == 0 {
                        store.commit()?;
                        store.set_meta("parse_done_files", &files.to_string())?;
                        store.begin()?;
                    }
                    Ok(())
                };
                if let Err(e) = step() {
                    err = Some(e);
                    return false;
                }
                true
            });
            if let Some(e) = err {
                return Err(e);
            }
            if !quiet && report.remaining_after_limit > 0 {
                println!(
                    "注意：达到 --limit，剩余 {} 个文件未解析（要全量请用 --limit 0）",
                    report.remaining_after_limit
                );
            }
            if !quiet && !report.skipped.is_empty() {
                println!("跳过超大文件 {} 个（>2MB，generated）", report.skipped.len());
                for (rel, size) in report.skipped.iter().take(20) {
                    println!("  {rel}  {}KB", size / 1024);
                }
                if report.skipped.len() > 20 {
                    println!("  …还有 {} 个", report.skipped.len() - 20);
                }
            }
            if !quiet && timing {
                let mut ms: Vec<u128> = report.timings.iter().map(|(_, t)| *t).collect();
                ms.sort();
                let p = |q: f64| -> u128 {
                    if ms.is_empty() { 0 } else { ms[((ms.len() as f64 - 1.0) * q) as usize] }
                };
                println!(
                    "timing：文件 {}（跳过超大 {}）· p50 {}ms · p95 {}ms · max {}ms",
                    ms.len(),
                    report.skipped.len(),
                    p(0.50),
                    p(0.95),
                    ms.last().copied().unwrap_or(0)
                );
                let mut top = report.timings.clone();
                top.sort_by(|a, b| b.1.cmp(&a.1));
                for (rel, t) in top.iter().take(10) {
                    println!("  top {t}ms  {rel}");
                }
            }
            let (rewritten, left) = edges_resolve::resolve_all(&store, root2.as_path())?;
            store.set_meta("edge_resolve_v", "17")?;
            store.note_write(&written)?;
            store.commit()?;
            Ok((n, e_mod, e_use, e_impl, e_import, rewritten, left, report.seen))
        })
        .map_err(|e| anyhow::anyhow!(e))?
        .join()
        .map_err(|_| anyhow::anyhow!("parse thread panicked"))??;

    if !quiet {
        let (n, e_mod, e_use, e_impl, e_import, rewritten, left, seen) = summary;
        let store = Store::open(root)?;
        // 分母用"树里见到的文件数"；改动文件数会小于树，出现 >100% 的假覆盖
        let rs: i64 = match seen.get("rs").copied() {
            Some(n) => n as i64,
            None => store
                .changed_files_by_ext()?
                .into_iter()
                .find(|(e, _)| e == "rs")
                .map(|(_, n)| n)
                .unwrap_or(0),
        };
        let with_syms = store.symbol_paths_lang("rust")?;
        let pct = if rs > 0 { with_syms * 100 / rs } else { 100 };
        println!("parsed {} symbols into {}", n, store.db_path().display());
        println!("覆盖：rust {rs} 文件 / {with_syms} 有符号（{pct}%）");
        let go: i64 = seen.get("go").copied().unwrap_or(0) as i64;
        if go > 0 {
            let go_syms = store.symbol_paths_lang("go")?;
            let go_pct = go_syms * 100 / go;
            println!("覆盖 go {go_syms}/{go}（{go_pct}%）");
        }
        println!("边：mod {e_mod} · use {e_use} · impl {e_impl} · import {e_import}");
        println!("解析 pass：重写 {rewritten} 行，仍 ? {left} 条");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_why(root: &PathBuf, path: &str) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let subject = format!("file:{path}");
    let ans = query::ask_with(&store, &subject, None, &collect::capability::LangCaps)?;

    // 没有判断（含过期/冲突）→ 与 ask 完全同口径，再补改动明细
    let no_judgment = ans.main.is_none()
        && matches!(ans.outcome, datacore::compose::Outcome::Uncertain("no_data"));
    if !no_judgment {
        let code = render_answer(&subject, &ans);
        let rows = store.changes_for(path)?;
        if rows.is_empty() {
            println!("  改动 0 次（没有 git 记录触及这个路径）");
        } else {
            println!("  改动 {} 次：", rows.len());
            for r in rows.iter().take(10) {
                println!(
                    "  {} {} {} <{}> {}",
                    &r.commit_hash[..8.min(r.commit_hash.len())],
                    r.status,
                    r.at,
                    r.author,
                    r.subject
                );
            }
        }
        return Ok(ExitCode::from(code));
    }

    // 有改动记录但没有判断 → 保守 held，并提示补判断
let rows = store.changes_for(path)?;
    if !rows.is_empty() {
        println!(
            "held: {path} 被 {} 次改动触及（还没有判断，建议先 ingest 补齐）",
            rows.len()
        );
        return Ok(ExitCode::SUCCESS);
    }

    // 都没有 → 三态里能说清的那几种
if !collect::git::is_repo(root) {
        println!("uncertain: 不是 git 仓库（no_git）");
        return Ok(ExitCode::from(2));
    }
    if collect::git::is_shallow(root) {
        println!("uncertain: 浅克隆，历史不完整（shallow）");
        return Ok(ExitCode::from(2));
    }
    let scanned = store
        .get_meta("scanned_commits")?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if scanned == 0 {
        println!("uncertain: 还没 ingest（not_ingested）");
        return Ok(ExitCode::from(2));
    }

    println!("refuted: 在已扫描的 {scanned} 个提交里，没有改动触及 {path}");
    Ok(ExitCode::from(1))
}

/// 打印一次回答（首行三态 + 主判断 + 依据），返回退出码。
/// `ask` 与 `why` 共用：两个命令的输出/退出码不许再漂移。
fn render_answer(subject: &str, ans: &datacore::query::Answer) -> u8 {
    match &ans.outcome {
        datacore::compose::Outcome::Held => println!("held: {subject}"),
        datacore::compose::Outcome::Refuted => println!("refuted: {subject}"),
        datacore::compose::Outcome::Uncertain(r) => {
            // 四类原因各有自己的标签，别都叫 uncertain（否则分不清“没数据”和“没过闸”）
let label = match *r {
                "no_data" | "expired" | "conflict" | "coverage_short" => *r,
                _ => "uncertain",
            };
            if label == "uncertain" {
                println!("uncertain: {subject}（{r}）");
            } else {
                println!("{label}: {subject}");
            }
        }
    }
    if let Some(j) = &ans.main {
        println!(
            "  judgment {} kind={} verdict={} coverage={}/{}",
            j.id,
            j.kind,
            j.verdict.as_str(),
            j.coverage_read,
            j.coverage_total
        );
        let gran = if j.scope.granularity.is_empty() {
            "-"
        } else {
            j.scope.granularity.as_str()
        };
        println!(
            "  scope={gran} objects={} env={}",
            j.scope.objects.len(),
            j.scope.env.len()
        );
    }
    for e in &ans.evidence {
        println!("  <- [{}] {} ({})", e.polarity, e.premise, e.instrument);
    }
    match ans.outcome {
        datacore::compose::Outcome::Held => 0,
        datacore::compose::Outcome::Refuted => 1,
        datacore::compose::Outcome::Uncertain(_) => 2,
    }
}

fn cmd_ask(root: &PathBuf, subject: &str, as_of: Option<i64>, json: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    // ask 不猜：只答给定的 subject。猜路径→前缀是记录过的错误（会配错对象）
let ans = query::ask_with(&store, subject, as_of, &collect::capability::LangCaps)?;
    // 能力闸：file:/symbol: 指向的语言没有符号能力 → 符号类问题落 uncertain(no_capability)；
    // 若已有判断（例如 file: 的存在性 = held），只在后面加一行能力说明——不拿能力冒充命题真值。
    let cap_path = subject
        .strip_prefix("file:")
        .or_else(|| subject.strip_prefix("symbol:").map(|s| s.split('#').next().unwrap_or(s)));
    let cap_note: Option<String> = cap_path
        .filter(|p| {
            let ext = collect::capability::ext_of_path(p);
            // 只对"登记过但没能力"的语言开口；md/toml 这类不是源码语言，仍走 no_data
            collect::capability::capability_for_ext(ext).is_some()
                && !collect::capability::supports(ext, collect::capability::Feature::Symbol)
        })
        .map(collect::capability::symbols_note);
    let no_cap = ans.main.is_none() && cap_note.is_some();
    if json {
        let m = ans.main.as_ref();
        let reason = if m.is_some() {
            None
        } else if no_cap {
            Some("no_capability")
        } else {
            Some("no_data")
        };
        let capability = cap_path.and_then(|p| {
            let ext = collect::capability::ext_of_path(p);
            collect::capability::capability_for_ext(ext).map(|c| {
                serde_json::json!({
                    "lang": c.lang,
                    "symbol": c.symbol,
                    "use": c.use_edge,
                    "import": c.import_edge,
                    "note": collect::capability::symbols_note(p),
                })
            })
        });
        let v = serde_json::json!({
            "v": 1,
            "subject": subject,
            "verdict": m.map(|j| j.verdict.as_str().to_string()),
            "at": m.map(|j| j.at),
            "expires_at": m.and_then(|j| j.expires_at),
            "reason": reason,
            "capability": capability,
            "scope": m.map(|j| serde_json::to_value(&j.scope).unwrap_or_default()),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        let code = if m.is_some() {
            0
        } else if no_cap {
            2
        } else {
            3
        };
        return Ok(ExitCode::from(code));
    }
    if no_cap {
        let note = cap_note.unwrap_or_default();
        println!("uncertain: {subject}（no_capability：{note}）");
        return Ok(ExitCode::from(2));
    }
    let code = render_answer(subject, &ans);
    if let (Some(note), true) = (&cap_note, ans.main.is_some()) {
        println!("  能力    {note}（符号数不作数）");
    }
    if ans.main.is_none()
        && matches!(ans.outcome, datacore::compose::Outcome::Uncertain("no_data"))
    {
        let cands = store.subject_candidates(subject)?;
        if !cands.is_empty() {
            println!("  候选 subject（用完整形式再问）：{}", cands.join(" / "));
        } else {
            println!("  查不了：库里没有这个 subject，也没有可对应的 file:/doc:/symbol: 形式");
        }
    }
    Ok(ExitCode::from(code))
}

/// 改动次数前 n 名的作者。
fn top_authors(rows: &[datacore::store::ChangeRow], n: usize) -> Vec<(String, usize)> {
    let mut m: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in rows {
        *m.entry(r.author.clone()).or_default() += 1;
    }
    let mut v: Vec<(String, usize)> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// 一屏讲清一个路径：状态 / 改动 / 谁改过 / 文档 / 符号。`--json` 给编辑器用。
fn cmd_explain(root: &PathBuf, input: &str, json: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let subject = resolve_subject(&store, input);
    let path = subject_path(&subject).to_string();
    let ans = query::ask_with(&store, &subject, None, &collect::capability::LangCaps)?;

    let code = match &ans.outcome {
        datacore::compose::Outcome::Held => 0u8,
        datacore::compose::Outcome::Refuted => 1u8,
        datacore::compose::Outcome::Uncertain(_) => 2u8,
    };
    let state = match &ans.outcome {
        datacore::compose::Outcome::Held => "held".to_string(),
        datacore::compose::Outcome::Refuted => "refuted".to_string(),
        datacore::compose::Outcome::Uncertain(r) => format!("uncertain({r})"),
    };

    let doc = store.find_judgment(&format!("doc:{path}"))?;
    let syms = store.count_symbols_for_path(&path)?;
    // 能力维度：能力缺失 ≠ 没符号（symbols_status 只对文件卡有意义）
    let sym_status: Option<&'static str> = if path.is_empty() || path.starts_with("dep:") {
        None
    } else {
        Some(collect::capability::symbols_status(&path, syms))
    };

    // 家谱：移动过的文件把旧路径的改动也算进来
    let lineage = store.lineage_paths(&path, 20)?;
    let lineage_changes = store.changes_for_paths(&lineage)?;
    let rows = store.changes_for(&path)?;
    let rows = store.changes_for(&path)?;
    let rows = store.changes_for(&path)?;
    let authors = top_authors(&rows, 2);

    if json {
        let v = serde_json::json!({
            "input": input,
            "subject": subject,
            "path": path,
            "state": state,
            "exit": code,
            "judgment": ans.main.as_ref().map(|j| serde_json::json!({
                "id": j.id,
                "kind": j.kind,
                "verdict": j.verdict.as_str(),
                "coverage_read": j.coverage_read,
                "coverage_total": j.coverage_total,
                "at": j.at,
            })),
            "evidence": ans.evidence.iter().map(|e| serde_json::json!({
                "polarity": e.polarity,
                "premise": e.premise,
                "instrument": e.instrument,
            })).collect::<Vec<_>>(),
            "changes": lineage_changes,
            "lineage": lineage,
            "authors": authors.iter().map(|(a, n)| serde_json::json!({"name": a, "n": n})).collect::<Vec<_>>(),
            "recent": rows.iter().take(3).map(|r| serde_json::json!({
                "commit": r.commit_hash,
                "status": r.status,
                "at": r.at,
                "author": r.author,
                "subject": r.subject,
            })).collect::<Vec<_>>(),
            "doc": doc.as_ref().map(|j| serde_json::json!({
                "subject": j.subject,
                "verdict": j.verdict.as_str(),
            })),
            "symbols": syms,
            "symbols_status": sym_status,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(ExitCode::from(code));
    }


    println!("{path}");
    println!("  状态    {state} · [{subject}]");

    if rows.is_empty() {
        println!("  改动    0 次");
    } else {
        println!("  改动    {} 次", rows.len());
        if let Some((who, n)) = authors.first() {
            println!("  作者    {who} {n} 次");
        }
        println!("  最近：");
        for r in rows.iter().take(3) {
            println!(
                "    {} {} {} <{}> {}",
                &r.commit_hash[..8.min(r.commit_hash.len())],
                r.status,
                r.at,
                r.author,
                r.subject
            );
        }
    }

    match &doc {
        Some(j) => println!("  文档    {}（{}）", j.verdict.as_str(), j.subject),
        None => println!("  文档    无"),
    }
    println!("  符号    {syms} 个");

    Ok(ExitCode::from(code))
}

fn cmd_conflicts(root: &PathBuf) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let n = store.scan_conflicts(datacore::store::now_secs())?;
    let list = store.list_conflicts()?;
    let resolved = store.resolved_conflict_count()?;
    println!(
        "scanned, found {n} conflict pair(s), {} unresolved, {resolved} resolved",
        list.len()
    );
    for (a, b) in list.iter().take(10) {
        let sa = store.judgment_subject(a)?.unwrap_or_else(|| a.clone());
        let sb = store.judgment_subject(b)?.unwrap_or_else(|| b.clone());
        println!("  {sa} <-> {sb} [{} / {}]", short_id(a), short_id(b));
    }
    Ok(ExitCode::SUCCESS)
}

fn short_id(id: &str) -> &str {
    let start = id.len().saturating_sub(8);
    if id.is_char_boundary(start) {
        &id[start..]
    } else {
        id
    }
}

/// 按名字找东西：符号带行号（从 `def:<path>:<line>:<kind>:<at>` 依据解析），其余按 kind 分组。
fn find_json(store: &Store, term: &str, kind: Option<&str>) -> anyhow::Result<serde_json::Value> {
    let rows = store.find(term, kind)?;
    let hits: Vec<serde_json::Value> = rows
        .iter()
        .map(|(subject, k, verdict, premise)| {
            let mut h = serde_json::json!({
                "subject": subject,
                "kind": k,
                "verdict": verdict,
            });
            if k == "symbol" {
                let (path, line) = def_location(premise);
                h["path"] = serde_json::json!(path);
                h["line"] = serde_json::json!(line);
                h["name"] = serde_json::json!(
                    subject.split_once('#').map(|(_, n)| n).unwrap_or(subject)
                );
            }
            h
        })
        .collect();
    Ok(serde_json::json!({
        "v": 1,
        "term": term,
        "kind": kind,
        "total": hits.len(),
        "hits": hits,
    }))
}

fn cmd_find(root: &PathBuf, term: &str, kind: Option<String>, json: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let rows = store.find(term, kind.as_deref())?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&find_json(&store, term, kind.as_deref())?)?
        );
        return Ok(ExitCode::from(if rows.is_empty() { 2 } else { 0 }));
    }
    if rows.is_empty() {
        println!("no match for {term}");
        return Ok(ExitCode::from(2));
    }
    for (subject, k, verdict, premise) in &rows {
        match k.as_str() {
            "symbol" => {
                let (path, line) = def_location(premise);
                let name = subject.split_once('#').map(|(_, n)| n).unwrap_or(subject);
                println!("{k:<9} {verdict:<10} {path}:{line}  {name}");
            }
            _ => println!("{k:<9} {verdict:<10} {subject}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// 从 `def:<path>:<line>:<kind>:<at>` 里取 (path, line)。
fn def_location(premise: &str) -> (String, String) {
    let Some(rest) = premise.strip_prefix("def:") else {
        return (String::new(), String::from("?"));
    };
    let mut it = rest.rsplitn(3, ':');
    let _at = it.next();
    let _kind = it.next();
    match it.next() {
        Some(path_line) => match path_line.rsplit_once(':') {
            Some((p, l)) => (p.to_string(), l.to_string()),
            None => (path_line.to_string(), String::from("?")),
        },
        None => (rest.to_string(), String::from("?")),
    }
}

/// 重验是**写一条新判断**（at=now），把该 subject 的旧判断全部过期。
///
/// 不再原地改 verdict：那会把历史抹掉（E3：contradicts 挂在自带 supports 的同一判断上，ask 永远 conflict）。
fn cmd_verify(root: &PathBuf, subject: &str) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    let repo = store.repo().to_string();
    let now = datacore::store::now_secs();
    let Some(old) = store.find_judgment(subject)? else {
        println!("uncertain: {subject}（no_data）");
        return Ok(ExitCode::from(2));
    };

    // judgment_id 只到秒：同秒再写会撞 id、被 put_judgment 的 DO NOTHING 静默吞掉
    let at = store.max_at(subject)?.map_or(now, |m| now.max(m + 1));

    let ok = if let Some(rest) = subject.strip_prefix("symbol:") {
        let (path, name) = rest
            .split_once('#')
            .map(|(p, n)| (p.to_string(), n.to_string()))
            .unwrap_or((rest.to_string(), String::new()));
        let p = root.join(&path);
        if !p.exists() {
            false
        } else {
            collect::parse::file_symbols(&p, &path)
                .iter()
                .any(|s| s.name == name)
        }
    } else if let Some(rest) = subject.strip_prefix("file:") {
        root.join(rest).exists()
    } else if let Some(rest) = subject.strip_prefix("doc:") {
        root.join(rest).exists()
    } else {
        // 不认识的 subject 不能当不存在处理，否则会给 decision:/issue: 写一条 refuted
        anyhow::bail!("not verifiable: {subject}（只支持 symbol:/file:/doc:）");
    };

    let v = if ok { Verdict::Held } else { Verdict::Refuted };
    let j = judgment_row(&repo, subject, &old.kind, v, at, 1, 1, old.scope.clone());
    store.put_judgment(&j)?;

    let src = Source {
        id: source_id(&repo, "verify", at),
        instrument: "verify".into(),
        instrument_version: None,
        env: format!("repo={repo}"),
        time: at,
    };
    store.put_source(&src)?;

    let premise = format!("recheck:{at}:{subject}");
    let ev = Evidence {
        id: evidence_id(&j.id, &premise, if ok { "supports" } else { "contradicts" }),
        judgment_id: j.id.clone(),
        premise,
        polarity: if ok {
            Polarity::Supports
        } else {
            Polarity::Contradicts
        },
        source_id: src.id.clone(),
    };
    store.put_evidence(&ev)?;

    // 过期时刻 = 新观测的 at：DDL 要求 expires_at >= 该判断的 at，at 可能被推到
    // 现在之后（躲同秒 id 撞车）；代价是那一秒内旧判断仍活着，见 B3.1 记录。
    let n = store.expire_older_than(subject, Some(&j.id), at)?;
    store.note_write(&[subject.to_string()])?;
    println!("{}: {subject}（新判断 at={at}；旧判断过期 {n} 条）", v.as_str());
    Ok(ExitCode::from(if ok { 0 } else { 1 }))
}

fn cmd_expire(
    root: &PathBuf,
    subject: &str,
    at: Option<i64>,
    id: Option<String>,
) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    let Some(j) = store.find_judgment(subject)? else {
        println!("uncertain: {subject}（no_data）");
        return Ok(ExitCode::from(2));
    };
    let ts = at.unwrap_or_else(|| {
        // DDL 要求 expires_at >= at：新观测可能把 at 推到 now+1（躲同秒 id 撞车），
        // 所以默认过期时刻取 max(now, 该判断的 at)，而不是裸 now。
        datacore::store::now_secs().max(j.at)
    });

    // --id 定点：只许标该 subject 下的判断
    let target = match &id {
        None => j.id.clone(),
        Some(want) => {
            let hit = store
                .judgments_for(subject)?
                .into_iter()
                .find(|x| &x.id == want);
            match hit {
                Some(x) => x.id,
                None => anyhow::bail!("judgment id 不属于 {subject}: {want}"),
            }
        }
    };
    store.set_expiry(&target, ts)?;
    store.note_write(&[subject.to_string()])?;
    println!("expired: {subject} at {ts}");
    Ok(ExitCode::SUCCESS)
}

/// 人工裁决一对冲突：写新判断 → 过期旧的 → 冲突清账。
///
/// 机器不许调用（`--by` 必填、记进依据），不做自动仲裁。
fn cmd_resolve(
    root: &PathBuf,
    subject: &str,
    verdict_str: &str,
    by: &str,
    why: &str,
) -> anyhow::Result<ExitCode> {
    if by.trim().is_empty() {
        anyhow::bail!("--by 必填（谁做的裁决）");
    }
    if why.trim().is_empty() {
        anyhow::bail!("--why 必填（一句话理由）");
    }
    let v = match verdict_str {
        "held" => Verdict::Held,
        "refuted" => Verdict::Refuted,
        other => anyhow::bail!("--verdict 只能是 held / refuted，收到 {other}"),
    };

    let store = Store::open(root)?;
    let repo = store.repo().to_string();
    let now = datacore::store::now_secs();

    match store.resolve_conflict(&repo, subject, v, by, why, now)? {
        None => {
            println!("no_conflict: {subject}");
            Ok(ExitCode::from(2))
        }
        Some((_judgment_id, n)) => {
            store.note_write(&[subject.to_string()])?;
            println!(
                "resolved: {subject} -> {} by {by}（旧判断过期 {n} 条）",
                v.as_str()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn cmd_alt_add(root: &PathBuf, child: &str, parent: &str) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    let c = store
        .find_judgment(child)?
        .ok_or_else(|| anyhow::anyhow!("找不到候选：{child}"))?;
    let p = store
        .find_judgment(parent)?
        .ok_or_else(|| anyhow::anyhow!("找不到决策：{parent}"))?;
    store.put_alternative(&c.id, &p.id)?;
    println!("linked: {} -> {}", c.id, p.id);
    Ok(ExitCode::SUCCESS)
}

fn cmd_alt_list(root: &PathBuf, parent: &str) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let Some(p) = store.find_judgment(parent)? else {
        println!("uncertain: {parent}（no_data）");
        return Ok(ExitCode::from(2));
    };
    let alts = store.list_alternatives(&p.id)?;
    if alts.is_empty() {
        println!("no alternatives for {parent}");
        return Ok(ExitCode::from(2));
    }
    for a in alts {
        let subject = store.judgment_subject(&a)?.unwrap_or_else(|| a.clone());
        println!("candidate {subject}");
    }
    Ok(ExitCode::SUCCESS)
}

/// held / refuted 要求“看全了”：一次状态观测 = 1/1；uncertain 还没观测 = 0/0。
fn coverage_of(verdict: Verdict) -> (i64, i64) {
    match verdict {
        Verdict::Uncertain => (0, 0),
        _ => (1, 1),
    }
}

/// 记录名里的日期：`D-20261021-05…` → 2026-10-21 00:00 UTC 的 unix 秒。
///
/// 不用时钟：重跑同一批记录不产生新判断（E2 幂等教训）。
fn record_at(rid: &str) -> Option<i64> {
    let rest = rid.strip_prefix("D-")?;
    let digits: String = rest.chars().take(8).collect();
    if digits.len() != 8 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let y: i64 = digits[0..4].parse().ok()?;
    let m: i64 = digits[4..6].parse().ok()?;
    let d: i64 = digits[6..8].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400)
}

/// Howard Hinnant 的 days_from_civil：1970-01-01 起的天数（纯整数，不依赖时区）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 一条替代项自己的三态：被否 → refuted，选中 → held，没写结论 → uncertain。
fn alt_verdict(text: &str) -> Verdict {
    if ["否决", "放弃", "不采用", "不选", "不做", "拒绝"]
        .iter()
        .any(|k| text.contains(k))
    {
        Verdict::Refuted
    } else if ["选它", "选择", "采纳", "采用", "落定"]
        .iter()
        .any(|k| text.contains(k))
    {
        Verdict::Held
    } else {
        Verdict::Uncertain
    }
}

/// 人工记录导入：每个 `D-*.md` 生成一条 decision 判断；显式替代项生成 hypothesis 判断 + 候选关系。
///
/// 依据只存**指针**（`record:<stem>`），不抄正文；scope.objects 只放磁盘上真有的路径。
fn cmd_record(root: &PathBuf, path: &PathBuf, lint: bool) -> anyhow::Result<ExitCode> {
    if lint {
        // 只认显式结论：替代项文本里没有 选它/否决 这类词，就警告（不改库、exit 0）
        let marks = ["选它", "选择", "采纳", "采用", "落定", "否决", "放弃", "不采用", "不选", "不做", "拒绝"];
        let records = if path.is_dir() {
            collect::record::scan(path)
        } else {
            let text = std::fs::read_to_string(path)?;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("record").to_string();
            vec![collect::record::parse_markdown(&text, &stem)]
        };
        let mut warned = 0usize;
        for r in &records {
            if r.alts.is_empty() {
                continue;
            }
            let any_marked = r.alts.iter().any(|a| marks.iter().any(|m| a.text.contains(m)));
            if !any_marked {
                // 短 id：D-YYYYMMDD-NN（去掉后面的标题后缀）
                let short: String = r.id.splitn(4, '-').take(3).collect::<Vec<_>>().join("-");
                println!("{short}：散文式选项，无结论标注");
                warned += 1;
            }
        }
        println!("lint：{} 条记录，{} 条无结论标注（只是写法警告，exit 0）", records.len(), warned);
        return Ok(ExitCode::SUCCESS);
    }
    let store = Store::open(root)?;
    let repo = store.repo().to_string();

    let records = if path.is_dir() {
        collect::record::scan(path)
    } else {
        let text = std::fs::read_to_string(path)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("record")
            .to_string();
        vec![collect::record::parse_markdown(&text, &stem)]
    };

    let mut n_dec = 0usize;
    let mut n_hyp = 0usize;
    let mut written: Vec<String> = Vec::new();
    for r in &records {
        let at = record_at(&r.id)
            .ok_or_else(|| anyhow::anyhow!("记录名里没有 D-YYYYMMDD 日期：{}", r.id))?;
        let verdict = record_verdict(&r.status_raw);
        let (cr, ct) = coverage_of(verdict);

        let objects: Vec<String> = r
            .paths
            .iter()
            .filter(|p| root.join(p.as_str()).is_file())
            .cloned()
            .collect();
        let mut scope = scope_of(objects, "decision", &repo);
        scope.env.insert("source".into(), "record".into());

        let subject = format!("decision:{}#{}", repo, r.id);
        written.push(subject.clone());
        let j = judgment_row(&repo, &subject, "decision", verdict, at, cr, ct, scope);
        // 记录是外部真源：改了文件重导必须能更新（force=UPDATE，不删行不级联）
        store.put_judgment_force(&j)?;

        // source 按记录日期一个：11 条记录 / 5 个日期 → 重跑不增
        let src = Source {
            id: source_id(&repo, "record", at),
            instrument: "record".into(),
            instrument_version: None,
            env: format!("repo={repo}"),
            time: at,
        };
        store.put_source(&src)?;

        let premise = format!("record:{}", r.id);
        let ev = Evidence {
            id: evidence_id(&j.id, &premise, "supports"),
            judgment_id: j.id.clone(),
            premise,
            polarity: Polarity::Supports,
            source_id: src.id.clone(),
        };
        store.put_evidence(&ev)?;
        n_dec += 1;

        for alt in &r.alts {
            let hv = alt_verdict(&alt.text);
            let (hcr, hct) = coverage_of(hv);
            let hsubj = format!("hypothesis:{}#{}/{}", repo, r.id, alt.index);
            let mut hscope = scope_of(Vec::new(), "decision", &repo);
            hscope.env.insert("source".into(), "record".into());
            let hj = judgment_row(&repo, &hsubj, "hypothesis", hv, at, hcr, hct, hscope);
            store.put_judgment_force(&hj)?;

            // 指针不带 verdict：verdict 属于 judgment，写进指针会让关键词规则一改
            // 就生成新 evidence id、旧行留成陈货。
let hp = format!("record:{}:alt#{}", r.id, alt.index);
            let hev = Evidence {
                id: evidence_id(&hj.id, &hp, "supports"),
                judgment_id: hj.id.clone(),
                premise: hp,
                polarity: Polarity::Supports,
                source_id: src.id.clone(),
            };
            store.put_evidence(&hev)?;
            store.put_alternative(&hj.id, &j.id)?;
            n_hyp += 1;
        }
    }

    // 对账用一行（只打印，不进模型）：raw=原始出现，unique=全局去重，existing=磁盘上真有
let raw: usize = records.iter().map(|r| r.path_mentions).sum();
    let mut all_paths: Vec<String> = Vec::new();
    for r in &records {
        all_paths.extend(r.paths.iter().cloned());
    }
    all_paths.sort();
    all_paths.dedup();
    let unique = all_paths.len();
    let existing = all_paths
        .iter()
        .filter(|p| root.join(p.as_str()).is_file())
        .count();
    let alts: usize = records.iter().map(|r| r.alts.len()).sum();

    store.note_write(&written)?;
    println!(
        "imported {} records ({} decisions, {} hypotheses) into {}",
        records.len(),
        n_dec,
        n_hyp,
        store.db_path().display()
    );
    println!(
        "paths: raw={raw} unique={unique} existing={existing}; alts={alts}; decisions={n_dec} hypotheses={n_hyp}"
    );
    Ok(ExitCode::SUCCESS)
}

/// 项目概览 + 演进时间线：把「这东西是怎么长起来的」讲清楚。
fn cmd_project(root: &PathBuf) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let repo = store.repo().to_string();
    let files = store.distinct_change_paths()?;
    let changes = store.count("change")?;
    let commits = store.count("snapshot")?;
    let kinds = store.kind_counts()?;
    let get = |k: &str| {
        kinds
            .iter()
            .find(|(kind, _)| kind == k)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    };

    println!("项目  {repo}");
    println!("  文件    {files} 个（git 碰过）");
    println!("  改动    {changes} 次 / {commits} 个提交");
    if let Some((a, b)) = store.change_span()? {
        println!("  区间    {} 到 {}", day_str(a), day_str(b));
    }
    println!("  文档    {}", get("doc"));
    println!("  符号    {}", get("symbol"));
    println!("  决策    {}（假设 {}）", get("decision"), get("hypothesis"));

    let days = store.changes_per_day()?;
    if !days.is_empty() {
        let max = days.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1);
        println!("\n  按天演进（提交改动数）");
        for (day, n) in &days {
            let bar = (n * 24 / max).max(1) as usize;
            println!(
                "    {}  {:<24} {}",
                day_str(*day),
                "█".repeat(bar),
                n
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// gitlink 数：`.gitmodules` 里每个 `path = ...` 算一个（路径无内容，不算普通文件）。
fn gitlink_count(root: &Path) -> i64 {
    collect::git::ls_tree_gitlinks(root) as i64
}

/// 项目时间线：按天看它怎么长起来。`--limit 0` 全量 ingest 后才是真历史。
fn cmd_timeline(root: &PathBuf, json: bool, top_n: Option<usize>) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let repo = store.repo().to_string();
    let scanned = store.count("snapshot")?;
    let total = collect::git::total_commits(root);

    if json {
        // 与文本同口径，只是提前组装成一份数据
        let files = store.distinct_change_paths()?;
        let historical = files;
        let current = collect::git::ls_tree_files(root).map(|n| n as i64).unwrap_or(historical);
        let file_stats = (historical, current, (historical - current).max(0));
        let gitlinks = gitlink_count(root);
        let changes = store.count("change")?;
        let kinds = store.kind_counts()?;
        let get = |k: &str| {
            kinds
                .iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, n)| *n)
                .unwrap_or(0)
        };
        let mut days: std::collections::BTreeMap<i64, (i64, i64, i64, i64, i64)> =
            std::collections::BTreeMap::new();
        for (d, a, m, x) in store.change_days()? {
            let e = days.entry(d).or_default();
            e.1 = a;
            e.2 = m;
            e.3 = x;
        }
        for (d, c) in store.commit_days()? {
            days.entry(d).or_default().0 = c;
        }
        for (d, n) in store.file_first_days()? {
            days.entry(d).or_default().4 = n;
        }
        let mut cum = 0i64;
        let day_rows: Vec<serde_json::Value> = days
            .iter()
            .map(|(d, (c, a, m, x, newf))| {
                cum += newf;
                serde_json::json!({
                    "day": day_str(*d),
                    "commits": c,
                    "added": a,
                    "modified": m,
                    "deleted": x,
                    "cumulative_files": cum,
                })
            })
            .collect();
        let dec: Vec<serde_json::Value> = store
            .decisions_by_day()?
            .into_iter()
            .map(|(d, n)| serde_json::json!({"day": day_str(d), "n": n}))
            .collect();
        let deps: std::collections::BTreeMap<String, i64> =
            store.dep_counts()?.into_iter().collect();
        let v = serde_json::json!({
            "v": 1,
            "repo": repo,
            "commits_total": total,
            "scanned": scanned,
            "changes": changes,
            "files": files,
            "top": top_n.map(|n| store.top_files(n)).transpose()?.unwrap_or_default()
                .iter().map(|(p, c)| serde_json::json!({"path": p, "n": c})).collect::<Vec<_>>(),
            "file_counts": {
                "historical": file_stats.0,
                "current": file_stats.1,
                "deleted": file_stats.2,
                "gitlinks": gitlinks,
            },
            "docs": get("doc"),
            "symbols": get("symbol"),
            "decisions": get("decision"),
            "deps": deps,
            "days": day_rows,
            "decisions_by_day": dec,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(ExitCode::SUCCESS);
    }


    let shallow = collect::git::is_shallow(root);
    println!("项目  {repo}");
    match total {
        Some(t) if shallow => println!(
            "  仓库    {t} 个提交（浅克隆：只有克隆点之后的历史，已扫描 {scanned}）",
        ),
        Some(t) if (t as i64) > scanned => println!(
            "  仓库    {t} 个提交（已扫描 {scanned} —— 历史不完整，用 `ingest --limit 0` 补全）",
        ),
        Some(t) => println!("  仓库    {t} 个提交（已全量扫描）"),
        None => println!("  仓库    不是 git 仓库（只展示已采集部分）"),
    }

    let files = store.distinct_change_paths()?;
    let changes = store.count("change")?;
    let kinds = store.kind_counts()?;
    let get = |k: &str| {
        kinds
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    };
    println!("  改动    {changes} 次 / {files} 文件");
    {
        let h = files;
        let cur = collect::git::ls_tree_files(root).map(|n| n as i64).unwrap_or(h);
        let del = (h - cur).max(0);
        let gl = gitlink_count(root);
        println!(
            "  文件    {h} 历史 · {cur} 当前 · {del} 已删{}",
            if gl > 0 {
                format!("（gitlink {gl}）")
            } else {
                String::new()
            }
        );
    }
    println!(
        "  内容    文档 {} · 符号 {} · 决策 {}（快照）",
        get("doc"),
        get("symbol"),
        get("decision")
    );
    let deps = store.dep_counts()?;
    if !deps.is_empty() {
        let n = |k: &str| {
            deps.iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, n)| *n)
                .unwrap_or(0)
        };
        let total: i64 = deps.iter().map(|(_, n)| n).sum();
        println!(
            "  构建    依赖 {total}（直接 {} · 开发 {} · 构建 {}）",
            n("normal"),
            n("dev"),
            n("build")
        );
    }

    // 语言覆盖：只解析 rust，其他语言的符号是"没解析"，不是"没有符号"
    let exts = store.changed_files_by_ext()?;
    if !exts.is_empty() {
        let rs = exts
            .iter()
            .find(|(e, _)| e == "rs")
            .map(|(_, n)| *n)
            .unwrap_or(0);
        let other: i64 = exts.iter().filter(|(e, _)| e != "rs").map(|(_, n)| n).sum();
        let top: Vec<String> = exts
            .iter()
            .filter(|(e, _)| e != "rs")
            .take(3)
            .map(|(e, n)| format!("{e} {n}"))
            .collect();
        let with_syms = store.symbol_path_count()?;
        println!(
            "{}",
            clip_display(
                &format!(
                    "  语言    rust {rs} 文件（有符号 {with_syms}） / 其他 {other} 未解析{}",
                    if top.is_empty() {
                        String::new()
                    } else {
                        format!("（{}）", top.join(" · "))
                    }
                ),
                48
            )
        );
    }

    let mut days: std::collections::BTreeMap<i64, (i64, i64, i64, i64, i64)> =
        std::collections::BTreeMap::new();
    for (d, a, m, x) in store.change_days()? {
        let e = days.entry(d).or_default();
        e.1 = a;
        e.2 = m;
        e.3 = x;
    }
    for (d, c) in store.commit_days()? {
        days.entry(d).or_default().0 = c;
    }
    for (d, n) in store.file_first_days()? {
        days.entry(d).or_default().4 = n;
    }

    if !days.is_empty() {
        // 一屏：最多看最近 8 天；新文件列并入"累计"（累计 = 每天新文件之和）
        let all: Vec<(&i64, &(i64, i64, i64, i64, i64))> = days.iter().collect();
        let start = all.len().saturating_sub(8);
        println!("\n  日期         提交  新增  改动  删除  累计");
        let mut cum = 0i64;
        for (d, (c, a, m, x, newf)) in &all[..start] {
            let _ = (d, c, a, m, x);
            cum += newf; // 被截掉的早期天也要计入累计
        }
        for (d, (c, a, m, x, newf)) in &all[start..] {
            cum += newf;
            println!(
                "  {:<10}  {:>4}  {:>4}  {:>4}  {:>4}  {:>5}",
                day_str(**d),
                c,
                a,
                m,
                x,
                cum
            );
        }
    }

    let dec = store.decisions_by_day()?;
    if !dec.is_empty() {
        let total: i64 = dec.iter().map(|(_, n)| n).sum();
        let first = dec.first().map(|(d, _)| day_str(*d)).unwrap_or_default();
        let last = dec.last().map(|(d, _)| day_str(*d)).unwrap_or_default();
        println!(
            "\n  决策    {total} 次（{first} → {last}，{} 天）",
            dec.len()
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// 单文件结构卡片（只读，v1 不画图——没有 edge 数据，画了就是假图）。
fn cmd_card(
    root: &PathBuf,
    input: &str,
    also: &[String],
    json: bool,
    history: bool,
    pin: bool,
    // 从 pin 恢复时 = false，别覆盖 preview 槽
    remember: bool,
    state: Option<&str>,
) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let mut inputs: Vec<String> = vec![input.to_string()];
    inputs.extend(also.iter().cloned());

    let mut vals = Vec::new();
    let mut code = 0u8;
    for i in &inputs {
        let t_card = std::time::Instant::now();
        let (c, v) = card_value(&store, root, i, history, state)?;
        if jwlog::on(jwlog::Level::Debug) {
            // 命中/落空：`skip` 字段有值 = 没给出卡（三态里的"查不了/没有"），否则就是命中了。
            let hit = v.get("skip").is_none() && v.get("state").and_then(|s| s.as_str()) != Some("no");
            let ms = t_card.elapsed().as_millis().to_string();
            jwlog::emit(
                jwlog::Level::Debug,
                if hit { "card.hit" } else { "card.miss" },
                &[("subject", i), ("ms", &ms)],
            );
        }
        code = code.max(c);
        vals.push(v);
    }

    if json {
        // 单张保持对象（向后兼容），多张输出数组（批量：一次进程取 N 张卡）
let out = if vals.len() == 1 {
            vals.remove(0)
        } else {
            serde_json::Value::Array(vals)
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for (i, v) in vals.iter().enumerate() {
            if i > 0 {
                println!();
            }
            print!("{}", render_card_text(v));
        }
    }
    // 两个槽：preview（1 个，复用同一位）+ pinned（≤1）。last_focus 只记位置。
if remember && inputs.len() == 1 {
        let _ = store.set_meta("ui.preview", input);
        let _ = store.set_meta("ui.last_focus", input);
        if pin {
            let _ = store.set_meta("ui.pinned", input);
        }
    }
    Ok(ExitCode::from(code))
}

/// 回到上次停留点：只记最后位置，不记历史选择。
/// 回到上次位置：pin 槽优先，其次 preview（最近看过），最后 last_focus。
fn cmd_resume(root: &PathBuf) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    let get = |k: &str| -> Option<String> {
        store
            .get_meta(k)
            .ok()
            .flatten()
            .filter(|s: &String| !s.is_empty())
    };
    let (slot, input) = if let Some(p) = get("ui.pinned") {
        ("pinned", p)
    } else if let Some(p) = get("ui.preview").or_else(|| get("ui.last_focus")) {
        ("preview", p)
    } else {
        println!("还没有停留点（先跑一次 card <路径>）");
        return Ok(ExitCode::from(2));
    };
    drop(store);
    println!("[{slot}] {input}");
    // 从 pin 恢复不覆盖 preview（否则 unpin 后回不到刚才浏览的位置）
    cmd_card(root, &input, &[], false, false, false, slot == "preview", None)
}

/// 取消钉住（pin 槽只留一个）。
fn cmd_unpin(root: &PathBuf) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    store.set_meta("ui.pinned", "")?;
    println!("已取消钉住");
    Ok(ExitCode::SUCCESS)
}


/// 从 subject 串取"名字"（symbol/test）。
fn typed_name(s: &str) -> Option<String> {
    match datacore::core::Subject::parse(s).ok()? {
        datacore::core::Subject::Symbol { name, .. } | datacore::core::Subject::Test { name, .. } => {
            Some(name)
        }
        _ => None,
    }
}

fn cmd_edges(
    root: &PathBuf,
    path: &str,
    kind: Option<String>,
    stats: bool,
    json: bool,
    to: Option<String>,
) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    // 反查 incoming calls：谁调用了这个 subject
    if let Some(subject) = to {
        let known = store.find_judgment(&subject)?.is_some();
        let rows = store.call_edges_to(&subject)?;
        if json {
            let callers: Vec<serde_json::Value> = rows
                .iter()
                .map(|(src, line)| {
                    let file = datacore::core::typed_path(src).unwrap_or_else(|| src.clone());
                    serde_json::json!({"from": src, "file": file, "line": line})
                })
                .collect();
            let v = serde_json::json!({"v": 1, "to": subject, "subject_known": known,
                                       "total": callers.len(), "callers": callers});
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else {
            println!("{subject}  被调用 {} 次", rows.len());
            for (src, line) in rows.iter().take(30) {
                let file = datacore::core::typed_path(src).unwrap_or_else(|| src.clone());
                println!("  {file}:{line}");
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    if stats {
        let (internal, ext, unresolved, total) = store.edge_stats()?;
        let pct = if total > 0 { internal * 100 / total } else { 0 };
        let (impl_in, impl_ext, impl_q) = store.impl_stats()?;
        let (call_total, call_un) = store.edge_kind_stats("call")?;
        let (test_total, _test_un) = store.edge_kind_stats("test")?;
        let (macro_total, _) = store.edge_kind_stats("test_macro")?;
        if json {
            let top_use = store.top_use_files(3)?;
            let v = serde_json::json!({
                "v": 1,
                "use": {"internal": internal, "ext": ext, "unresolved": unresolved, "total": total},
                "impl": {"internal": impl_in, "ext": impl_ext, "unquestioned": impl_q},
                "call": {"total": call_total, "resolved": call_total - call_un, "unresolved": call_un},
                "test": {"standard": test_total, "macro": macro_total, "total": test_total + macro_total},
                "top_use": top_use.iter().map(|(p, n)| serde_json::json!({"path": p, "n": n})).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&v)?);
            return Ok(ExitCode::SUCCESS);
        }
        println!("internal {internal}/{total}（{pct}%）");
        println!("impl internal {impl_in} · ext {impl_ext} · ? {impl_q}");
        println!("ext 外部 {ext}");
        println!("未解析 ? {unresolved}");
        println!(
            "call {call_total}（已解析 {} · 未解析 {call_un}）",
            call_total - call_un
        );
        println!("test {}（standard {test_total} · macro {macro_total}）", test_total + macro_total);
        return Ok(ExitCode::SUCCESS);
    }
    let mut rows = store.edges_located(path)?;
    if let Some(k) = &kind {
        rows.retain(|(_, _, kk, _, _)| kk == k);
    }
    if rows.is_empty() {
        println!("no edges for {path}");
        return Ok(ExitCode::from(2));
    }
    // 位置投影（多态）：use/mod/import 指源文件里的位置；impl 指声明所在文件里的位置；
    // test 的 src 是 `test:<file>#<fn>`，位置就在它的 dst（所在文件）里。
    let loc = |src: &str, k: &str, leaf: &Option<String>, line: Option<i64>| -> Option<(String, i64)> {
        let line = line?;
        let file = match k {
            "impl" => leaf.clone().unwrap_or_else(|| src.to_string()),
            "test" | "test_macro" => src.to_string(),
            _ => src.to_string(),
        };
        Some((file, line))
    };
    if json {
        let edges: Vec<serde_json::Value> = rows
            .iter()
            .map(|(from, dst, k, leaf, line)| {
                let l = loc(path, k, leaf, *line);
                let mut e = serde_json::json!({
                    "kind": k,
                    "dst": dst,
                    "leaf": leaf,
                    "loc": l.map(|(f, n)| serde_json::json!({"file": f, "line": n})),
                });
                // call/test 的 src 是合成 subject，但 caller / 测试函数名对消费者有用
                if k == "call" || k == "test" || k == "test_macro" {
                    e["from"] = serde_json::json!(from);
                }
                e
            })
            .collect();
        let mut v = serde_json::json!({"v": 1, "path": path, "total": edges.len(), "edges": edges});
        // `?use` 行带原因码（IDE 要能分清缺文件/遮蔽/glob/属性路径）
        if let Some(r) = gap_reasons(root, &store, path) {
            v["unresolved_reasons"] = r;
        }
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(ExitCode::SUCCESS);
    }
    println!("{path}  边 {} 条", rows.len());
    for (from, dst, k, leaf, line) in rows.iter().take(30) {
        let caller = typed_name(from).unwrap_or_default();
        match loc(path, k, leaf, *line) {
            Some((f, n)) => {
                if k == "test" || k == "test_macro" {
                    // 测试行显示测试函数名（dst 就是本文件，不重复打印）
                    println!("  {k:<10} {}  @{f}:{n}", leaf.clone().unwrap_or_default());
                } else if k == "call" {
                    println!(
                        "  {k:<5} {caller} → {:<28}  @{f}:{n}",
                        leaf.clone().unwrap_or_default()
                    );
                } else {
                    println!("  {k:<5} {dst}  @{f}:{n}");
                }
            }
            None => println!("  {k:<5} {dst}"),
        }
    }
    if rows.len() > 30 {
        println!("  …还有 {} 条", rows.len() - 30);
    }
    Ok(ExitCode::SUCCESS)
}

/// 覆盖：改动过的 .rs 里哪些没有符号判断——"0 符号"要么补映射，要么给出清单，不许口头带过。
/// 一条未解析 .rs 的原因（reason 只取四个值；db_mismatch 是 bug 信号，不是装饰）。
fn coverage_reason(root: &Path, p: &str) -> (&'static str, Option<serde_json::Value>) {
    let abs = root.join(p);
    if !abs.is_file() {
        return ("deleted", None);
    }
    let f = collect::parse::facts(&abs);
    if f.mapped > 0 {
        return ("db_mismatch", Some(serde_json::json!({"mapped": f.mapped})));
    }
    if f.empty {
        return ("empty", None);
    }
    if f.macros > 0 {
        return (
            "macro_unexpanded",
            Some(serde_json::json!({"macro_calls": f.macros})),
        );
    }
    ("no_mapped_item", None)
}

fn cmd_coverage(root: &PathBuf, limit: usize, json: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let exts = store.changed_files_by_ext()?;
    let rs = exts
        .iter()
        .find(|(e, _)| e == "rs")
        .map(|(_, n)| *n)
        .unwrap_or(0);
    let py_files = exts
        .iter()
        .find(|(e, _)| e == "py")
        .map(|(_, n)| *n)
        .unwrap_or(0);
    let py_parsed = store.symbol_paths_lang("python")?;
    let ts_files = exts
        .iter()
        .find(|(e, _)| e == "ts" || e == "tsx")
        .map(|(_, n)| *n)
        .unwrap_or(0);
    let ts_parsed = store.symbol_paths_lang("typescript")?;
    let with_syms = store.symbol_path_count()?;
    let (total, list) = store.unparsed_rust(limit)?;

    if json {
        let unparsed: Vec<serde_json::Value> = list
            .iter()
            .map(|p| {
                let (reason, evidence) = coverage_reason(root, p);
                let mut e = serde_json::json!({"path": p, "reason": reason});
                if let Some(ev) = evidence {
                    e["evidence"] = ev;
                }
                e
            })
            .collect();
        let by_ext: std::collections::BTreeMap<String, i64> = exts.into_iter().collect();
        let tests_std = store.tests_by_file("test")?;
        let tests_macro = store.tests_by_file("test_macro")?;
        let tests_total: i64 = tests_std.iter().map(|(_, n)| *n).sum::<i64>()
            + tests_macro.iter().map(|(_, n)| *n).sum::<i64>();
        let gap_table = collect::gap::GapTable::new(root);
        let mut gap_reasons: std::collections::BTreeMap<&'static str, i64> = Default::default();
        let mut gap_total = 0i64;
        for (src, dst, line) in store.unresolved_use_edges()? {
            *gap_reasons.entry(gap_table.reason(&src, &dst, line).as_str()).or_default() += 1;
            gap_total += 1;
        }
        let v = serde_json::json!({
            "v": 1,
            "lang": "rust",
            "total": rs,
            "parsed": with_syms,
            "py": {"total": py_files, "parsed": py_parsed},
            "ts": {"total": ts_files, "parsed": ts_parsed},
            "unparsed": unparsed,
            "by_ext": by_ext,
            "unresolved_use": {"total": gap_total, "reasons": gap_reasons},
            "tests": {"total": tests_total,
                      "standard": tests_std.iter().map(|(_, n)| *n).sum::<i64>(),
                      "macro": tests_macro.iter().map(|(_, n)| *n).sum::<i64>(),
                      "by_file": tests_std.iter().cloned().collect::<std::collections::BTreeMap<_,_>>(),
                      "macro_by_file": tests_macro.iter().cloned().collect::<std::collections::BTreeMap<_,_>>()},
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(ExitCode::SUCCESS);
    }

    println!("rust {rs} 文件 / {with_syms} 有符号 / {total} 未解析");
    if total == 0 {
        println!("ok：所有改动过的 .rs 都有符号判断");
        // 不提前 return：后面还要给 Go 的未解析文件一个说法
    }
    for p in &list {
        let (reason, evidence) = coverage_reason(root, p);
        let why = match reason {
            "deleted" => "文件已不存在（只出现在历史里）".to_string(),
            "empty" => "空文件/无项".to_string(),
            "macro_unexpanded" => format!(
                "有宏调用 {} 处，token 树未展开",
                evidence.as_ref().and_then(|e| e["macro_calls"].as_i64()).unwrap_or(0)
            ),
            "db_mismatch" => format!(
                "与库不一致（AST 里有 {} 个可映射项）",
                evidence.as_ref().and_then(|e| e["mapped"].as_i64()).unwrap_or(0)
            ),
            _ => "只有 use/mod/impl".to_string(),
        };
        println!("  {p}  —— {why}");
    }
    if (list.len() as i64) < total {
        println!("  …还有 {} 条", total - list.len() as i64);
    }

    // Go：树里的 .go 减去已有符号，按原因分组（各类之和 = 未解析数，没有"其他"）
    let all = collect::parse::source_files(root);
    let syms = store.symbol_paths()?;
    let unparsed_go: Vec<String> = all
        .into_iter()
        .filter(|p| p.ends_with(".go") && !syms.contains(p))
        .collect();
    if !unparsed_go.is_empty() {
        let reasons = collect::parse::go_reasons(root, &unparsed_go);
        let mut by: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        for (p, r) in reasons {
            by.entry(r).or_default().push(p);
        }
        let parts: Vec<String> = by.iter().map(|(k, v)| format!("{k} {}", v.len())).collect();
        println!("go 未解析 {} 个（{}）", unparsed_go.len(), parts.join(" · "));
        let mut shown = 0usize;
        for (reason, paths) in &by {
            for p in paths.iter().take(3) {
                if shown >= 20 {
                    break;
                }
                println!("  {p}  —— {reason}");
                shown += 1;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// 采构建：读 Cargo.toml + Cargo.lock，落 dep:<name> 判断（版本/类型进 scope.env）。
fn cmd_deps(root: &PathBuf) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let repo = store.repo().to_string();
    let has_cargo = root.join("Cargo.toml").is_file();
    let has_npm = root.join("package.json").is_file();
    if !has_cargo && has_npm {
        return cmd_deps_npm(&store, root, &repo);
    }
    if !has_cargo && !has_npm {
        let py = collect::py::scan(root);
        if !py.is_empty() {
            return cmd_deps_py(&store, root, &repo);
        }
        if root.join("pyproject.toml").is_file() {
            println!("pyproject 未采集（只认 requirements.txt，不解析 pyproject.toml）");
            return Ok(ExitCode::from(2));
        }
    }
    let scan = match collect::cargo::scan(root) {
        Ok(s) => s,
        Err(_) => {
            // 非 Rust 仓库要明说"没采"，不许打"依赖 0"假装完整
            if root.join("package.json").is_file() {
                println!("无 Cargo.toml（npm 依赖未采集）");
            } else if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
                println!("无 Cargo.toml（python 依赖未采集）");
            } else if root.join("go.mod").is_file() {
                println!("无 Cargo.toml（go 依赖未采集）");
            } else if root.join("pom.xml").is_file() || root.join("build.gradle").is_file() {
                println!("无 Cargo.toml（jvm 依赖未采集）");
            } else {
                println!("没采到：这个目录没有 Cargo.toml（或读不了）");
            }
            return Ok(ExitCode::from(2));
        }
    };

    let at = datacore::store::now_secs();
    let src = Source {
        id: source_id(&repo, "cargo", at),
        instrument: "cargo".into(),
        instrument_version: None,
        env: format!("repo={repo}"),
        time: at,
    };
    store.put_source(&src)?;

    let mut n = 0usize;
    let mut written: Vec<String> = Vec::new();
    for d in &scan.deps {
        // 同名依赖可能同时出现在 normal 与 dev/build：subject 必须带类型，否则会撞 id
        let subject = if d.kind == collect::cargo::DepKind::Normal {
            format!("dep:{}", d.name)
        } else {
            format!("dep:{}@{}", d.name, d.kind.as_str())
        };
        written.push(subject.clone());
        let mut scope = scope_of(vec!["Cargo.toml".into()], "module", &repo);
        if let Some(v) = &d.version {
            scope.env.insert("version".into(), v.clone());
        }
        if let Some(v) = &d.resolved {
            scope.env.insert("resolved".into(), v.clone());
        }
        scope.env.insert("dep_kind".into(), d.kind.as_str().into());
        let j = judgment_row(&repo, &subject, "dependency", Verdict::Held, at, 1, 1, scope);
        store.put_judgment(&j)?;
        store.expire_older_than(&subject, Some(&j.id), at)?;

        let premise = format!(
            "dep:{}:{}:{}:{}",
            d.name,
            d.version.clone().unwrap_or_default(),
            d.resolved.clone().unwrap_or_default(),
            d.kind.as_str()
        );
        let ev = Evidence {
            id: evidence_id(&j.id, &premise, "supports"),
            judgment_id: j.id.clone(),
            premise,
            polarity: Polarity::Supports,
            source_id: src.id.clone(),
        };
        store.put_evidence(&ev)?;
        n += 1;
    }
    // 已删掉的依赖：重跑时把不再出现的 live 依赖过期（否则卡片会一直"记住"旧依赖）
    for subj in store.live_dependency_subjects()? {
        if !written.contains(&subj) {
            if let Some(j) = store.find_judgment(&subj)? {
                store.set_expiry(&j.id, at)?;
            }
        }
    }
    if n > 0 {
        store.note_write(&written)?;
    }

    use collect::cargo::DepKind;
    let count = |k: DepKind| scan.deps.iter().filter(|d| d.kind == k).count();
    println!(
        "deps {n}（直接 {} · 开发 {} · 构建 {} · 工作区成员 {} · lock {} 包）",
        count(DepKind::Normal),
        count(DepKind::Dev),
        count(DepKind::Build),
        scan.members.len(),
        scan.lock_packages
    );
    if scan.workspace_decls > 0 {
        println!(
            "workspace 声明 {} 条（候选版本，不计入依赖）",
            scan.workspace_decls
        );
    }
    // 混合仓库：只采了 Cargo，别的包管理器要明说"没采"，不许暗示"依赖完整"
    if root.join("package.json").is_file() {
        let (n_npm, npm_normal, npm_dev, npm_unknown) = write_npm_deps(&store, root, &repo)?;
        println!(
            "npm deps {n_npm}（直接 {npm_normal} · 开发 {npm_dev} · 版本未知 {npm_unknown}）"
        );
        let n_py = write_py_deps(&store, root, &repo)?;
        if n_py > 0 {
            println!("py deps {n_py}（只认 requirements.txt）");
        }
    }
    if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
        println!("python  未采集（deps 只支持 Cargo）");
    }
    Ok(ExitCode::SUCCESS)
}

/// 增量刷新：编辑器保存后调它。三条流入各跑一遍（都有 mtime 幂等），最后报总 seq。
fn cmd_refresh(root: &PathBuf, limit: usize, quiet: bool) -> anyhow::Result<ExitCode> {
    // 三个子命令各自开库、各自 note_write；这里只负责串起来
    let _ = cmd_docs(root, limit, quiet)?;
    let _ = cmd_parse(root, limit.saturating_mul(10), quiet, false)?;
    let _ = cmd_ingest(root, limit, quiet, false)?;
    let store = Store::open(root)?;
    let seq = store
        .get_meta("write_seq")?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if !quiet {
        println!("refresh done（write_seq {seq}）");
    }
    Ok(ExitCode::SUCCESS)
}

/// 长驻服务（v1.5 订阅的接口先打通）：stdin/stdout NDJSON，一行一请求/一响应。
/// 只做 ping/stat/quit——不推送、不订阅；客户端比较 `write_seq` 自己决定要不要重查。
fn cmd_serve(root: &PathBuf, no_subscribe: bool) -> anyhow::Result<ExitCode> {
    use std::io::{BufRead, Write};
    let store = Store::open(root)?;

    // 空闲折叠：起一个后台线程，空闲 >= JW_IDLE_COMPACT_SECS（默认 300，0=关）就跑一次 compact。
    // 它只写库（fold + note_write），不往 stdout 打任何东西——不会污染 NDJSON。
    let idle_secs: i64 = std::env::var("JW_IDLE_COMPACT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let last_active = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(
        datacore::store::now_secs(),
    ));
    if idle_secs > 0 {
        let la = last_active.clone();
        let root2 = root.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let now = datacore::store::now_secs();
            if now - la.load(std::sync::atomic::Ordering::Relaxed) < idle_secs {
                continue;
            }
            la.store(now, std::sync::atomic::Ordering::Relaxed);
            let Ok(st) = Store::open(&root2) else { continue };
            let Ok(plans) = st.fold_plans(now, 8, 90 * 86_400) else {
                continue;
            };
            let mut written = Vec::new();
            for (subject, verdict, entity, _n, t0, t1) in &plans {
                if st.apply_fold(subject, verdict, entity.as_deref(), *t0, *t1).is_ok() {
                    written.push(subject.clone());
                }
            }
            if !written.is_empty() {
                let _ = st.note_write(&written);
            }
            let _ = st.set_meta("last_compact_at", &now.to_string());
        });
    }
    let touch = |la: &std::sync::Arc<std::sync::atomic::AtomicI64>| {
        la.store(
            datacore::store::now_secs(),
            std::sync::atomic::Ordering::Relaxed,
        );
    };
    let seq = || -> i64 {
        store
            .get_meta("write_seq")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };
    // ---- 协议 v1：hello / seq / replay / ack ----
    const PROTO: u64 = 1;
    const RING_CAP: usize = 1024;
    const REPLAY_CAP: usize = 256;
    const FEATURES: &[&str] = &[
        "hello", "replay", "ack", "subscribe", "watch", "stat", "find", "card", "edges",
        "ask", "cite", "impact", "tests_for", "coverage", "capability", "edge_stats",
        "open", "pin", "focus", "refresh", "quit",
    ];
    static OUT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let ring: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<(i64, serde_json::Value)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::with_capacity(RING_CAP)));
    // 外部进程（jw parse 等）写库也要变成事件：500ms 轮询 write_seq，变化即入环 + 推 db_changed
    {
        let ring2 = ring.clone();
        let root3 = root.clone();
        let mut seen = seq();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let Ok(st) = Store::open(&root3) else { continue };
            let cur = st
                .get_meta("write_seq")
                .ok()
                .flatten()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            if cur <= seen {
                continue;
            }
            {
                let mut r = ring2.lock().unwrap();
                for s in (seen + 1)..=cur {
                    r.push_back((s, serde_json::json!({"seq": s, "method": "db_changed", "type": "event"})));
                }
                while r.len() > RING_CAP {
                    r.pop_front();
                }
            }
            let _g = OUT_LOCK.lock().unwrap();
            let mut o = std::io::stdout();
            let _ = writeln!(
                o,
                "{}",
                serde_json::json!({"v": 1, "type": "event", "event": "db_changed", "seq": cur, "write_seq": cur})
            );
            let _ = o.flush();
            seen = cur;
        });
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    {
        let _g = OUT_LOCK.lock().unwrap();
        writeln!(
            out,
            "{}",
            serde_json::json!({"v": 1, "type": "hello", "proto": PROTO, "features": FEATURES,
                               "rules": PARSE_RULES_VERSION, "write_seq": seq()})
        )?;
        out.flush()?;
    }
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        touch(&last_active);
        let req: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(out, "{}", serde_json::json!({"v": 1, "ok": false, "error": format!("bad_json: {e}")}))?;
                out.flush()?;
                continue;
            }
        };
        let id = req["id"].clone();
        let method = req["method"].as_str().unwrap_or("");
        // 客户端可以声明期望的协议版本；更高版本直接拒（不猜语义）
        if let Some(v) = req["v"].as_u64() {
            if v > 1 {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"v": 1, "id": id, "ok": false, "error": format!("unsupported_version: {v}")})
                )?;
                out.flush()?;
                continue;
            }
        }
        let seq_before = seq();
        let t0 = std::time::Instant::now();
        let resp = match method {
            "hello" => {
                let mut degraded: Vec<String> = Vec::new();
                if let Some(want) = req["features"].as_array() {
                    for w in want {
                        if let Some(s) = w.as_str() {
                            if !FEATURES.contains(&s) {
                                degraded.push(s.to_string());
                            }
                        }
                    }
                }
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "proto": PROTO, "rules": PARSE_RULES_VERSION, "features": FEATURES,
                    "degraded": degraded, "write_seq": seq(),
                }})
            }
            "events_since" => {
                let since = req["since"].as_i64().unwrap_or(0);
                let r = ring.lock().unwrap();
                let oldest = r.front().map(|(s, _)| *s);
                let mut events: Vec<serde_json::Value> = r
                    .iter()
                    .filter(|(s, _)| *s > since)
                    .map(|(_, e)| e.clone())
                    .collect();
                let truncated = events.len() > REPLAY_CAP;
                if truncated {
                    events.truncate(REPLAY_CAP);
                }
                // since=0 是"首次订阅"：先给快照语义，不报 gap；
                // 截断/回放窗口跨过 ring 都必须显式 gap=true，不许只标 truncated。
                let gap = if since == 0 {
                    false
                } else {
                    truncated || matches!(oldest, Some(o) if since + 1 < o)
                };
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "events": events, "gap": gap, "oldest_seq": oldest,
                    "truncated": truncated, "write_seq": seq(),
                }})
            }
            "ack" => {
                let upto = req["seq"].as_i64().unwrap_or(0);
                let mut dropped = 0usize;
                let mut rg = ring.lock().unwrap();
                while let Some((s, _)) = rg.front() {
                    if *s <= upto {
                        rg.pop_front();
                        dropped += 1;
                    } else {
                        break;
                    }
                }
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "acked": upto, "dropped": dropped, "write_seq": seq(),
                }})
            }
            "ping" => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"pong": true, "write_seq": seq()}}),
            "stat" => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                "judgment": store.count("judgment")?,
                "evidence": store.count("evidence")?,
                "change": store.count("change")?,
                "write_seq": seq(),
            }}),
            // 长轮询：等 write_seq 涨过 since，超时就回 changed:false
            // 取卡：一条连接里取 N 张，UI 不再每张卡起一个进程
            "card" => match engine::Engine::open(&root) {
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
                Ok(eng) => {
                    let input = req["path"].as_str().unwrap_or("");
                    let history = req["history"].as_bool().unwrap_or(false);
                    let state = req["state"].as_str();
                    // 一次取多张：path + also[]（和 CLI 的 --also 对齐）
                    let mut inputs: Vec<String> = vec![input.to_string()];
                    if let Some(also) = req["also"].as_array() {
                        for v in also {
                            if let Some(s) = v.as_str() {
                                inputs.push(s.to_string());
                            }
                        }
                    }
                    let mut cards = Vec::new();
                    let mut code = 0u8;
                    let mut err: Option<String> = None;
                    for i in &inputs {
                        match eng.card(i, history, state) {
                            Ok((c, v)) => {
                                code = code.max(c);
                                cards.push(v);
                            }
                            Err(e) => {
                                err = Some(e.to_string());
                                break;
                            }
                        }
                    }
                    if let Some(m) = err {
                        serde_json::json!({"v": 1, "id": id, "ok": false, "error": m})
                    } else if cards.len() == 1 {
                        let one = cards.remove(0);
                        serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"exit": code, "card": one}})
                    } else {
                        serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"exit": code, "cards": cards}})
                    }
                }
            },
            // 编辑器保存后的单通道入口：一条连接里就跑完三条流入
            // 边层消费端：与 `jw edges` 同构（kind/dst）
            "edges" => {
                let path = req["path"].as_str().unwrap_or("");
                match engine::Engine::open(&root).and_then(|e| e.edges(path)) {
                    Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                    Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
                }
            }
            // 与 `jw edges --stats` 同构的解析率三行
            // 三个槽动作：open（浏览，替换 preview）/ pin（固化 ≤1）/ focus（读或写 last_focus）
            "open" => {
                let path = req["path"].as_str().unwrap_or("");
                store.set_meta("ui.preview", path)?;
                store.set_meta("ui.last_focus", path)?;
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"slot": "preview", "path": path}})
            }
            "pin" => {
                let path = req["path"].as_str().unwrap_or("");
                store.set_meta("ui.pinned", path)?;
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"slot": "pinned", "path": path}})
            }
            "focus" => {
                if let Some(path) = req["path"].as_str() {
                    if !path.is_empty() {
                        store.set_meta("ui.last_focus", path)?;
                    }
                }
                let get = |k: &str| store.get_meta(k).ok().flatten().unwrap_or_default();
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "preview": get("ui.preview"),
                    "pinned": get("ui.pinned"),
                    "last_focus": get("ui.last_focus"),
                }})
            }
            "find" => {
                let term = req["term"].as_str().unwrap_or("");
                let kind = req["kind"].as_str();
                let v = find_json(&store, term, kind)?;
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": v})
            }
            "ask" => {
                let subject = req["subject"].as_str().unwrap_or("");
                let as_of = req["as_of"].as_i64();
                let ans = query::ask_with(&store, subject, as_of, &collect::capability::LangCaps)?;
                let reason = match &ans.outcome {
                    datacore::compose::Outcome::Held => "held",
                    datacore::compose::Outcome::Refuted => "refuted",
                    datacore::compose::Outcome::Uncertain(r) => r,
                };
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "subject": subject,
                    "reason": reason,
                    "masked": ans.masked,
                    "verdict": ans.main.as_ref().map(|j| j.verdict.as_str()),
                    "at": ans.main.as_ref().map(|j| j.at),
                    "expires_at": ans.main.as_ref().and_then(|j| j.expires_at),
                }})
            }
            "cite" => {
                let subject = req["subject"].as_str().unwrap_or("");
                let e = engine::Engine::open(&root).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let (v, code) = e.cite(subject).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                serde_json::json!({"v": 1, "id": id, "ok": code == 0, "result": v})
            }
            "coverage" => {
                let tests_std = store.tests_by_file("test")?;
                let tests_macro = store.tests_by_file("test_macro")?;
                let gap_table = collect::gap::GapTable::new(&root);
                let mut reasons: std::collections::BTreeMap<&'static str, i64> = Default::default();
                for (src, dst, line) in store.unresolved_use_edges()? {
                    *reasons.entry(gap_table.reason(&src, &dst, line).as_str()).or_default() += 1;
                }
                let (call_total, call_un) = store.edge_kind_stats("call")?;
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "tests": {
                        "standard": tests_std.iter().map(|(_, n)| *n).sum::<i64>(),
                        "macro": tests_macro.iter().map(|(_, n)| *n).sum::<i64>(),
                    },
                    "call": {"total": call_total, "resolved": call_total - call_un, "unresolved": call_un},
                    "unresolved_use": reasons,
                }})
            }
            "impact" => {
                let target = req["target"].as_str().unwrap_or("");
                let e = engine::Engine::open(root).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let (v, code) = e.impact(target).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                serde_json::json!({"v": 1, "id": id, "ok": code == 0, "result": v})
            }
            "tests_for" => {
                let target = req["target"].as_str().unwrap_or("");
                let depth = req["depth"].as_i64().unwrap_or(3);
                let e = engine::Engine::open(root).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let v = e.tests_for(target, depth).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let code = if v.get("reason").is_some() { 2 } else { 0 };
                serde_json::json!({"v": 1, "id": id, "ok": code == 0, "result": v})
            }
            "capability" => {
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": capability_payload()})
            }
            "file" => {
                let path = req["path"].as_str().unwrap_or("");
                let start = req["start"].as_i64().unwrap_or(1).max(1) as usize;
                let want = req["end"].as_i64().unwrap_or(400).max(1) as usize;
                let abs = root.join(path);
                match std::fs::read_to_string(&abs) {
                    Ok(text) => {
                        let all: Vec<&str> = text.lines().collect();
                        let total = all.len();
                        let slice: Vec<String> = all
                            .iter()
                            .skip(start - 1)
                            .take(want.min(2000))
                            .map(|l| l.to_string())
                            .collect();
                        serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "path": path, "start": start, "total": total,
                            "lines": slice, "truncated": start - 1 + want < total,
                            "source": "fs",
                        }})
                    }
                    Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
                }
            }
            "problems" => {
                // 诊断只来自 fixture（diagnostics.json），来源显式标 db/fixture，不假装有 LSP
                let path = req["path"].as_str().unwrap_or("");
                let fixture = root.join("diagnostics.json");
                let mut items: Vec<serde_json::Value> = Vec::new();
                if let Ok(text) = std::fs::read_to_string(&fixture) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(arr) = v["diagnostics"][path].as_array() {
                            for d in arr {
                                items.push(serde_json::json!({
                                    "line": d["line"], "severity": d["severity"],
                                    "message": d["message"], "source": "db", "origin": "fixture",
                                }));
                            }
                        }
                    }
                }
                let mut err = 0; let mut warn = 0; let mut info = 0;
                for it in &items {
                    match it["severity"].as_str().unwrap_or("info") {
                        "error" => err += 1,
                        "warning" => warn += 1,
                        _ => info += 1,
                    }
                }
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "path": path, "items": items, "source": "db", "origin": "fixture",
                    "counts": {"error": err, "warning": warn, "info": info},
                    "note": "fixture 诊断；LSP 未接入",
                }})
            }
            "lsp_status" => {
                let trusted = trust_is_trusted(&root);
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "source": "lsp", "capability": "status", "at": datacore::store::now_secs(),
                    "seq": seq(), "trusted": trusted, "path": trust_key(&root),
                    "store": jw_user_dir().join("trust.json").to_string_lossy(),
                    "server": "fixture", "note": "未接真 server；未信任不 spawn",
                }})
            }
            "lsp_trust" => {
                let action = req["action"].as_str().unwrap_or("status");
                let trusted = match action {
                    "grant" => trust_set(&root, true)?,
                    "revoke" => trust_set(&root, false)?,
                    _ => trust_is_trusted(&root),
                };
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "source": "lsp", "capability": "trust", "at": datacore::store::now_secs(),
                    "seq": seq(), "trusted": trusted, "action": action,
                }})
            }
            "lsp_spawn" => {
                // G1：未信任 → 0 spawn，明确 reason（不是静默失败）
                if !trust_is_trusted(&root) {
                    serde_json::json!({"v": 1, "id": id, "ok": false, "result": {
                        "source": "lsp", "capability": "spawn", "at": datacore::store::now_secs(),
                        "seq": seq(), "reason": "untrusted_workspace", "spawned": false,
                        "note": "启动 LSP 会执行仓库的 build.rs / proc-macro；先显式信任",
                    }})
                } else {
                    let marker = root.join(".jingwei").join("lsp-marker");
                    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("..").join("..").join("tools").join("web").join("fixtures").join("lsp-echo.mjs");
                    let node = std::env::var("NODE").unwrap_or_else(|_| "node".to_string());
                    // 无 shell + env 白名单 + spawn 前校验可执行文件存在
                    if !script.is_file() {
                        serde_json::json!({"v": 1, "id": id, "ok": false, "result": {
                            "source": "lsp", "capability": "spawn", "reason": "server_down",
                            "seq": seq(), "spawned": false,
                        }})
                    } else {
                        let mut cmd = std::process::Command::new(&node);
                        cmd.arg(&script).arg(&marker);
                        cmd.env_clear();
                        for k in ["PATH", "SystemRoot", "TEMP", "TMP", "HOME", "USERPROFILE"] {
                            if let Ok(v) = std::env::var(k) { cmd.env(k, v); }
                        }
                        cmd.stdin(std::process::Stdio::piped())
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::null());
                        match cmd.spawn() {
                            Ok(mut child) => {
                                // fixture 启动即发 initialize 回包：带超时读一行，避免挂死
                                let pid = child.id();
                                let (tx, rx) = std::sync::mpsc::channel();
                                if let Some(mut so) = child.stdout.take() {
                                    std::thread::spawn(move || {
                                        use std::io::Read;
                                        let mut buf = [0u8; 4096];
                                        let n = so.read(&mut buf).unwrap_or(0);
                                        let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                                    });
                                }
                                let ok = matches!(
                                    rx.recv_timeout(std::time::Duration::from_secs(3)),
                                    Ok(s) if s.contains("capabilities") || s.contains("serverInfo")
                                );
                                if ok {
                                    let _ = child.kill();
                                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                                        "source": "lsp", "capability": "spawn", "at": datacore::store::now_secs(),
                                        "seq": seq(), "spawned": true, "pid": pid, "initialized": true,
                                        "marker": marker.to_string_lossy(),
                                    }})
                                } else {
                                    let _ = child.kill();
                                    serde_json::json!({"v": 1, "id": id, "ok": false, "result": {
                                        "source": "lsp", "capability": "spawn", "at": datacore::store::now_secs(),
                                        "seq": seq(), "spawned": true, "initialized": false,
                                        "reason": "server_down",
                                    }})
                                }
                            }
                            Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "result": {
                                "source": "lsp", "capability": "spawn", "reason": "server_down",
                                "error": e.to_string(), "seq": seq(), "spawned": false,
                            }}),
                        }
                    }
                }
            }
            "lsp_diagnostics" => {
                let file = req["path"].as_str().unwrap_or("src/lib.rs").to_string();
                let mode = req["mode"].as_str().unwrap_or("push").to_string();
                let force_down = req["force_down"].as_bool().unwrap_or(false);
                let db_items = |file: &str| -> Vec<serde_json::Value> {
                    let mut out = Vec::new();
                    if let Ok(text) = std::fs::read_to_string(root.join("diagnostics.json")) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(arr) = v["diagnostics"][file].as_array() {
                                for d in arr {
                                    out.push(serde_json::json!({
                                        "file": file, "line": d["line"], "col": 1,
                                        "severity": d["severity"], "code": "",
                                        "message": d["message"], "source": "db",
                                    }));
                                }
                            }
                        }
                    }
                    out
                };
                if !trust_is_trusted(&root) {
                    serde_json::json!({"v": 1, "id": id, "ok": false, "result": {
                        "source": "lsp", "capability": "diagnostics", "at": datacore::store::now_secs(),
                        "seq": seq(), "reason": "untrusted_workspace", "items": [], "degraded": false,
                    }})
                } else if force_down {
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                        "source": "db", "capability": "diagnostics", "at": datacore::store::now_secs(),
                        "seq": seq(), "reason": "server_down", "degraded": true, "items": db_items(&file),
                        "note": "LSP 不可用：已降级到 db/fixture，自研索引仍可用",
                    }})
                } else {
                    let marker = root.join(".jingwei").join("lsp-marker");
                    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("..").join("..").join("tools").join("web").join("fixtures").join("lsp-echo.mjs");
                    let node = std::env::var("NODE").unwrap_or_else(|_| "node".to_string());
                    if !script.is_file() {
                        serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "source": "db", "capability": "diagnostics", "reason": "server_down",
                            "degraded": true, "items": db_items(&file), "seq": seq(),
                        }})
                    } else {
                        let mut cmd = std::process::Command::new(&node);
                        cmd.arg(&script).arg(&marker).arg(&file);
                        cmd.env_clear();
                        for k in ["PATH", "SystemRoot", "TEMP", "TMP", "HOME", "USERPROFILE"] {
                            if let Ok(v) = std::env::var(k) { cmd.env(k, v); }
                        }
                        cmd.stdin(std::process::Stdio::piped())
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::null());
                        match cmd.spawn() {
                            Ok(mut child) => {
                                let pid = child.id();
                                if mode == "pull" {
                                    if let Some(si) = child.stdin.as_mut() {
                                        use std::io::Write;
                                        let body = serde_json::json!({"jsonrpc":"2.0","id":9,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":format!("file:///{file}")}}}).to_string();
                                        let _ = si.write_all(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes());
                                        let _ = si.flush();
                                    }
                                }
                                let (tx, rx) = std::sync::mpsc::channel();
                                if let Some(mut so) = child.stdout.take() {
                                    std::thread::spawn(move || {
                                        use std::io::Read;
                                        let mut buf = [0u8; 8192];
                                        loop {
                                            match so.read(&mut buf) {
                                                Ok(0) => break,
                                                Ok(n) => { if tx.send(String::from_utf8_lossy(&buf[..n]).to_string()).is_err() { break; } }
                                                Err(_) => break,
                                            }
                                        }
                                    });
                                }
                                let mut raw = String::new();
                                let mut found: Option<Vec<serde_json::Value>> = None;
                                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                                while found.is_none() && std::time::Instant::now() < deadline {
                                    if let Ok(chunk) = rx.recv_timeout(std::time::Duration::from_millis(300)) {
                                        raw.push_str(&chunk);
                                        let frames = lsp_frames(&raw);
                                        let diags = if mode == "pull" {
                                            frames.iter().find_map(|f| f["result"]["items"].as_array()).cloned()
                                        } else {
                                            frames.iter().find_map(|f| f["params"]["diagnostics"].as_array()).cloned()
                                        };
                                        if let Some(d) = diags {
                                            found = Some(map_lsp_diagnostics(&file, &d));
                                        }
                                    }
                                }
                                #[cfg(windows)]
                                {
                                    let _ = std::process::Command::new("taskkill")
                                        .args(["/PID", &pid.to_string(), "/T", "/F"])
                                        .stdout(std::process::Stdio::null())
                                        .stderr(std::process::Stdio::null())
                                        .status();
                                }
                                let _ = child.kill();
                                match found {
                                    Some(items) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                                        "source": "lsp", "capability": "diagnostics", "at": datacore::store::now_secs(),
                                        "seq": seq(), "mode": mode, "degraded": false, "items": items,
                                    }}),
                                    None => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                                        "source": "db", "capability": "diagnostics", "reason": "server_down",
                                        "degraded": true, "items": db_items(&file), "seq": seq(),
                                        "note": "LSP 超时：已降级到 db/fixture",
                                    }}),
                                }
                            }
                            Err(e) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                                "source": "db", "capability": "diagnostics", "reason": "server_down",
                                "degraded": true, "items": db_items(&file), "error": e.to_string(), "seq": seq(),
                            }}),
                        }
                    }
                }
            }
            "lsp_ra_info" => {
                let ra = find_rust_analyzer();
                let version = ra.as_ref().and_then(|b| ra_version(b));
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "source": "lsp", "capability": "ra_info", "at": datacore::store::now_secs(),
                    "seq": seq(), "found": ra.is_some(), "version": version,
                    "path": ra.map(|p| p.to_string_lossy().to_string()),
                }})
            }
            "lsp_real" => {
                let file = req["path"].as_str().unwrap_or("src/lib.rs").to_string();
                if !trust_is_trusted(&root) {
                    serde_json::json!({"v": 1, "id": id, "ok": false, "result": {
                        "source": "lsp", "capability": "ra_diagnostics", "reason": "untrusted_workspace",
                        "seq": seq(), "items": [], "skip": null,
                    }})
                } else if req["force_down"].as_bool().unwrap_or(false) {
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                        "source": "db", "capability": "ra_diagnostics", "skip": "server_down",
                        "degraded": true, "items": [], "seq": seq(),
                    }})
                } else {
                    let session_out = ra_session(&root, |s| {
                        let pull = s.caps["capabilities"]["diagnosticProvider"].is_object();
                        (pull, s.diagnostics(&root, &file))
                    });
                    match session_out {
                        Ok((pull, Some(items))) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "source": "lsp", "capability": "ra_diagnostics", "at": datacore::store::now_secs(),
                            "seq": seq(), "provider": "rust-analyzer", "skip": null,
                            "items": items, "degraded": false,
                            "capabilities_used": if pull { "pull" } else { "push" },
                        }}),
                        Ok((_, None)) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "source": "lsp", "capability": "ra_diagnostics", "skip": "server_down",
                            "degraded": true, "items": [], "seq": seq(), "note": "超时：自研索引仍可用",
                        }}),
                        Err(skip) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "source": "lsp", "capability": "ra_diagnostics", "skip": skip,
                            "degraded": true, "items": [], "seq": seq(),
                        }}),
                    }
                }
            }
            "lsp_hover" => {
                let file = req["path"].as_str().unwrap_or("src/lib.rs").to_string();
                let line1 = req["line"].as_i64().unwrap_or(1);
                let col1 = req["col"].as_i64().unwrap_or(1);
                let force_down = req["force_down"].as_bool().unwrap_or(false);
                let fallback = |reason: &str| {
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                        "source": "db", "capability": "hover", "skip": reason, "degraded": true,
                        "type": null, "doc": null, "note": "类型未知：未信任/无 server/超时都不猜",
                        "line": line1, "col": col1, "lsp_line": ui_to_lsp(line1), "lsp_char": ui_to_lsp(col1),
                        "seq": seq(), "at": datacore::store::now_secs(),
                    }})
                };
                if !trust_is_trusted(&root) {
                    fallback("untrusted_workspace")
                } else if force_down {
                    fallback("server_down")
                } else {
                    match ra_session(&root, |s| {
                        let supported = s.caps["capabilities"]["hoverProvider"].as_bool().unwrap_or(false)
                            || s.caps["capabilities"]["hoverProvider"].is_object();
                        (supported, s.hover(&root, &file, line1, col1))
                    }) {
                        Err(skip) => fallback(&skip),
                        Ok((false, _)) => fallback("no_capability"),
                        Ok((true, None)) => fallback("server_down"),
                        Ok((true, Some((frame, _)))) => {
                            let contents = &frame["result"]["contents"];
                            let (ty, doc) = hover_parts(contents);
                            match ty {
                                Some(ty) if !ty.trim().is_empty() => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                                    "source": "lsp", "capability": "hover", "at": datacore::store::now_secs(),
                                    "seq": seq(), "skip": null, "degraded": false,
                                    "type": ty, "doc": doc, "provider": "rust-analyzer",
                                    "capabilities_used": "hover",
                                    "line": line1, "col": col1,
                                    "lsp_line": ui_to_lsp(line1), "lsp_char": ui_to_lsp(col1),
                                    "raw_kind": contents.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
                                }}),
                                _ => fallback("no_hover"),
                            }
                        }
                    }
                }
            }
            "lsp_def" => {
                let file = req["path"].as_str().unwrap_or("src/lib.rs").to_string();
                let line1 = req["line"].as_i64().unwrap_or(1);
                let col1 = req["col"].as_i64().unwrap_or(1);
                let force_down = req["force_down"].as_bool().unwrap_or(false);
                let db_fallback = |reason: &str| {
                    let mut targets = Vec::new();
                    let defs = match store.symbol_defs(&file) {
                        Ok(d) => d,
                        Err(e) => {
                            return serde_json::json!({"v": 1, "id": id, "ok": false,
                                "error": format!("db: {e}")})
                        }
                    };
                    for (name, _kind, line) in defs {
                        if line == line1 {
                                targets.push(serde_json::json!({
                                    "file": file, "line": line, "col": 1,
                                    "lsp_line": ui_to_lsp(line), "lsp_char": 0, "name": name,
                                }));
                            }
                        }
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                        "source": "db", "capability": "definition", "precision": "low",
                        "skip": reason, "degraded": true, "targets": targets,
                        "line": line1, "col": col1, "lsp_line": ui_to_lsp(line1), "lsp_char": ui_to_lsp(col1),
                        "seq": seq(), "at": datacore::store::now_secs(),
                        "note": "自研索引兜底（低精度）；未信任/无 server/超时都不猜",
                    }})
                };
                if !trust_is_trusted(&root) {
                    db_fallback("untrusted_workspace")
                } else if force_down {
                    db_fallback("server_down")
                } else {
                    let out = ra_session(&root, |s| {
                        let supported = s.caps["capabilities"]["definitionProvider"].as_bool().unwrap_or(false)
                            || s.caps["capabilities"]["definitionProvider"].is_object();
                        (supported, s.definition(&root, &file, line1, col1))
                    });
                    match out {
                        Err(skip) => db_fallback(&skip),
                        Ok((false, _)) => db_fallback("no_capability"),
                        Ok((true, Some(targets))) if !targets.is_empty() => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "source": "lsp", "capability": "definition", "precision": "high",
                            "skip": null, "degraded": false, "targets": targets,
                            "provider": "rust-analyzer", "capabilities_used": "definition",
                            "line": line1, "col": col1, "lsp_line": ui_to_lsp(line1), "lsp_char": ui_to_lsp(col1),
                            "seq": seq(), "at": datacore::store::now_secs(),
                        }}),
                        _ => db_fallback("no_definition"),
                    }
                }
            }
            "lsp_ref" => {
                let file = req["path"].as_str().unwrap_or("src/lib.rs").to_string();
                let line1 = req["line"].as_i64().unwrap_or(1);
                let col1 = req["col"].as_i64().unwrap_or(1);
                let include_decl = req["include_declaration"].as_bool().unwrap_or(true);
                let force_down = req["force_down"].as_bool().unwrap_or(false);
                let db_fallback = |reason: &str| {
                    // 自研兜底：同名定义的 call 调用者边（只读），并显式给覆盖率
                    let mut targets = Vec::new();
                    let mut name = String::new();
                    let defs = match store.symbol_defs(&file) {
                        Ok(d) => d,
                        Err(e) => {
                            return serde_json::json!({"v": 1, "id": id, "ok": false,
                                "error": format!("db: {e}")})
                        }
                    };
                    for (n, _k, l) in defs {
                        if l == line1 { name = n; break; }
                    }
                    let (call_total, call_un) = match store.edge_kind_stats("call") {
                        Ok(v) => v,
                        Err(e) => {
                            return serde_json::json!({"v": 1, "id": id, "ok": false,
                                "error": format!("db: {e}")})
                        }
                    };
                    if !name.is_empty() {
                        let subj = format!("symbol:{file}#{name}");
                        let rows = match store.call_edges_to(&subj) {
                            Ok(r) => r,
                            Err(e) => {
                                return serde_json::json!({"v": 1, "id": id, "ok": false,
                                    "error": format!("db: {e}")})
                            }
                        };
                        for (src, line) in rows {
                                if let Some(p) = datacore::core::typed_path(&src) {
                                    if let Some(nm) = typed_name(&src) {
                                        let _ = nm;
                                        targets.push(serde_json::json!({"file": p, "line": line, "col": 1, "name": name}));
                                    }
                                }
                            }
                    }
                    targets.sort_by_key(|t| (t["file"].as_str().unwrap_or("").to_string(), t["line"].as_i64().unwrap_or(0), t["col"].as_i64().unwrap_or(0)));
                    targets.dedup_by(|a, b| a["file"] == b["file"] && a["line"] == b["line"] && a["col"] == b["col"]);
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                        "source": "db", "capability": "references", "precision": "low",
                        "skip": reason, "degraded": true, "targets": targets, "total": targets.len(),
                        "coverage": {"resolved": call_total - call_un, "unresolved": call_un},
                        "include_declaration": include_decl,
                        "line": line1, "col": col1,
                        "seq": seq(), "at": datacore::store::now_secs(),
                        "note": "自研 use/call 兜底；无结果不等于没有引用",
                    }})
                };
                if !trust_is_trusted(&root) {
                    db_fallback("untrusted_workspace")
                } else if force_down {
                    db_fallback("server_down")
                } else {
                    let out = ra_session(&root, |s| {
                        let supported = s.caps["capabilities"]["referencesProvider"].as_bool().unwrap_or(false)
                            || s.caps["capabilities"]["referencesProvider"].is_object();
                        (supported, s.references(&root, &file, line1, col1, include_decl))
                    });
                    match out {
                        Err(skip) => db_fallback(&skip),
                        Ok((false, _)) => db_fallback("no_capability"),
                        Ok((true, Some(mut targets))) => {
                            targets.sort_by_key(|t| (t["file"].as_str().unwrap_or("").to_string(), t["line"].as_i64().unwrap_or(0), t["col"].as_i64().unwrap_or(0)));
                            targets.dedup_by(|a, b| a["file"] == b["file"] && a["line"] == b["line"] && a["col"] == b["col"]);
                            let total = targets.len();
                            serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                                "source": "lsp", "capability": "references", "precision": "high",
                                "skip": null, "degraded": false, "targets": targets, "total": total,
                                "provider": "rust-analyzer", "capabilities_used": "references",
                                "include_declaration": include_decl,
                                "line": line1, "col": col1,
                                "seq": seq(), "at": datacore::store::now_secs(),
                            }})
                        }
                        _ => db_fallback("no_reference"),
                    }
                }
            }
            "lsp_hints" => {
                let file = req["path"].as_str().unwrap_or("src/lib.rs").to_string();
                let start = req["start"].as_i64().unwrap_or(1).max(1);
                let end = req["end"].as_i64().unwrap_or(start + 80).max(start);
                let force_down = req["force_down"].as_bool().unwrap_or(false);
                let fallback = |reason: &str| {
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                        "source": "db", "capability": "inlay_hint", "skip": reason, "degraded": true,
                        "hints": [], "total": 0, "truncated": false, "badge": "db",
                        "start": start, "end": end, "seq": seq(), "at": datacore::store::now_secs(),
                        "note": "不显示类型 hint；未信任/无 server/超时都不猜",
                    }})
                };
                if !trust_is_trusted(&root) {
                    fallback("untrusted_workspace")
                } else if force_down {
                    fallback("server_down")
                } else {
                    let out = ra_session(&root, |s| {
                        let supported = s.caps["capabilities"]["inlayHintProvider"].is_object()
                            || s.caps["capabilities"]["inlayHintProvider"].as_bool().unwrap_or(false);
                        (supported, s.inlay_hints(&root, &file, start, end))
                    });
                    match out {
                        Err(skip) => fallback(&skip),
                        Ok((false, _)) => fallback("no_capability"),
                        Ok((true, Some((hints, truncated)))) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "source": "lsp", "capability": "inlay_hint", "skip": null, "degraded": false,
                            "hints": hints, "total": hints.len(), "truncated": truncated, "badge": "lsp",
                            "provider": "rust-analyzer", "capabilities_used": "inlayHint",
                            "start": start, "end": end, "seq": seq(), "at": datacore::store::now_secs(),
                        }}),
                        _ => fallback("server_down"),
                    }
                }
            }
            "lsp_actions" => {
                let file = req["path"].as_str().unwrap_or("src/lib.rs").to_string();
                let line1 = req["line"].as_i64().unwrap_or(1);
                let col1 = req["col"].as_i64().unwrap_or(1);
                let message = req["message"].as_str().unwrap_or("").to_string();
                let severity = req["severity"].as_i64().unwrap_or(1);
                let force_down = req["force_down"].as_bool().unwrap_or(false);
                let fallback = |reason: &str| {
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                        "source": "db", "capability": "code_action", "skip": reason, "degraded": true,
                        "actions": [], "total": 0, "applied": 0, "badge": "db",
                        "line": line1, "col": col1, "seq": seq(), "at": datacore::store::now_secs(),
                        "note": "只读展示；不 apply 任何 edit",
                    }})
                };
                if !trust_is_trusted(&root) {
                    fallback("untrusted_workspace")
                } else if force_down {
                    fallback("server_down")
                } else {
                    let out = ra_session(&root, |s| {
                        let supported = s.caps["capabilities"]["codeActionProvider"].is_object()
                            || s.caps["capabilities"]["codeActionProvider"].as_bool().unwrap_or(false);
                        (supported, s.code_actions(&root, &file, line1, col1, &message, severity))
                    });
                    match out {
                        Err(skip) => fallback(&skip),
                        Ok((false, _)) => fallback("no_capability"),
                        Ok((true, Some(actions))) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                            "source": "lsp", "capability": "code_action", "skip": null, "degraded": false,
                            "actions": actions, "total": actions.len(), "applied": 0, "badge": "lsp",
                            "provider": "rust-analyzer", "capabilities_used": "codeAction",
                            "line": line1, "col": col1, "seq": seq(), "at": datacore::store::now_secs(),
                        }}),
                        _ => fallback("no_action"),
                    }
                }
            }
            "lsp_stop" => {
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "source": "lsp", "capability": "stop", "at": datacore::store::now_secs(),
                    "seq": seq(), "stopped": true,
                }})
            }
            "edge_stats" => {
                let (internal, ext, unresolved, total) = store.edge_stats()?;
                let (impl_in, impl_ext, impl_q) = store.impl_stats()?;
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "internal": internal,
                    "ext": ext,
                    "unresolved": unresolved,
                    "total": total,
                    "impl_internal": impl_in,
                    "impl_ext": impl_ext,
                    "impl_question": impl_q,
                }})
            }
            "refresh" => {
                let limit = req["limit"].as_u64().unwrap_or(200) as usize;
                cmd_refresh(root, limit, true)?;
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"write_seq": seq()}})
            }
            "watch" => {
                let since = req["since"].as_i64().unwrap_or(0);
                let timeout_ms = req["timeout_ms"].as_i64().unwrap_or(5000).clamp(50, 60_000);
                let start = std::time::Instant::now();
                let (cur, changed) = loop {
                    let cur = seq();
                    if cur > since {
                        break (cur, true);
                    }
                    if start.elapsed().as_millis() as i64 >= timeout_ms {
                        break (cur, false);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                };
                let (events, mut gap) = if changed {
                    store.watch_events(since)?
                } else {
                    (Vec::new(), false)
                };
                if changed && events.is_empty() {
                    gap = true; // 日志没了（比如刚升级）→ 让客户端全量重取
                }
                let events: Vec<serde_json::Value> = events
                    .into_iter()
                    .map(|(s, subs)| serde_json::json!({"seq": s, "subjects": subs}))
                    .collect();
                serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                    "write_seq": cur,
                    "changed": changed,
                    "gap": gap,
                    "events": events,
                }})
            }
            // 订阅（最小版）：长轮询等 write_seq 前进，返回期间的变更事件；溢出发 gap
            "subscribe" => {
                if no_subscribe {
                    serde_json::json!({"v": 1, "id": id, "ok": false, "error": "subscribe disabled"})
                } else {
                    let since = req["since"].as_i64().unwrap_or(0);
                    let timeout_ms =
                        req["timeout_ms"].as_i64().unwrap_or(1000).clamp(50, 5_000);
                    let start = std::time::Instant::now();
                    loop {
                        let cur = seq();
                        if cur > since || start.elapsed().as_millis() as i64 >= timeout_ms {
                            let (events, gap) = store.watch_events(since)?;
                            let evs: Vec<serde_json::Value> = events
                                .into_iter()
                                .map(|(s2, subs)| {
                                    serde_json::json!({"seq": s2, "subjects": subs})
                                })
                                .collect();
                            break serde_json::json!({"v": 1, "id": id, "ok": true, "result": {
                                "write_seq": cur, "gap": gap, "events": evs,
                            }});
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }
            "stats" => match engine::Engine::open(&root).and_then(|e| e.stats()) {
                Ok(mut v) => {
                    if let Some(o) = v.as_object_mut() {
                        o.insert("seq".into(), serde_json::json!(seq()));
                        o.insert("at".into(), serde_json::json!(datacore::store::now_secs()));
                    }
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": v})
                }
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "env_probe" => match engine::Engine::open(&root).and_then(|e| e.env_probe()) {
                Ok(v) => {
                    if jwlog::on(jwlog::Level::Debug) {
                        let n = v["tools"].as_array().map(|a| a.len()).unwrap_or(0).to_string();
                        let ms = t0.elapsed().as_millis().to_string();
                        jwlog::emit(jwlog::Level::Debug, "env.probe", &[("tools", &n), ("ms", &ms)]);
                    }
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": v})
                }
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "inspect" => match engine::Engine::open(&root).and_then(|e| e.inspect()) {
                Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "source_files" => match engine::Engine::open(&root).and_then(|e| {
                let limit = req["limit"].as_u64().unwrap_or(2000) as usize;
                e.source_files(limit)
            }) {
                Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "clean_scan" => match engine::Engine::open(&root).and_then(|e| e.clean_scan()) {
                Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "notes" => match engine::Engine::open(&root).and_then(|e| e.notes()) {
                Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "run_plan" => match engine::Engine::open(&root).and_then(|e| e.run_plan()) {
                Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "projects" => match engine::Engine::open(&root).and_then(|e| e.projects()) {
                Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "scan" => {
                let parent = req["parent"].as_str().unwrap_or("");
                if jwlog::on(jwlog::Level::Debug) {
                    jwlog::emit(jwlog::Level::Debug, "scan.start", &[("parent", parent)]);
                }
                let t_scan = std::time::Instant::now();
                let r = engine::Engine::open(&root).and_then(|e| {
                    let depth = req["depth"].as_u64().unwrap_or(3) as u32;
                    let limit = req["limit"].as_u64().unwrap_or(500) as usize;
                    e.scan(parent, depth, limit)
                });
                match r {
                    Ok(v) => {
                        if jwlog::on(jwlog::Level::Debug) {
                            let n = v["items"].as_array().map(|a| a.len()).unwrap_or(0).to_string();
                            let dirs = v["scanned_dirs"].as_u64().unwrap_or(0).to_string();
                            let ms = t_scan.elapsed().as_millis().to_string();
                            jwlog::emit(
                                jwlog::Level::Debug,
                                "scan.end",
                                &[("n", &n), ("dirs", &dirs), ("ms", &ms)],
                            );
                        }
                        serde_json::json!({"v": 1, "id": id, "ok": true, "result": v})
                    }
                    Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
                }
            }
            "machine" => serde_json::json!({"v": 1, "id": id, "ok": true, "result": engine::Engine::machine()}),
            "machine_hw" => serde_json::json!({"v": 1, "id": id, "ok": true, "result": engine::Engine::machine_hw()}),

            "check" => match engine::Engine::open(&root).and_then(|e| e.check()) {
                // T2.4：与 facade 同源（12 项）；旧 4 项 stub 声称 files/symbols 却恒 0，已废弃
                Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
            },
            "files" => {
                let rel = req["path"].as_str().unwrap_or(".");
                match engine::Engine::open(&root).and_then(|e| e.files(rel)) {
                    Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                    Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
                }
            }
            "read_file" => {
                let rel = req["path"].as_str().unwrap_or("");
                match engine::Engine::open(&root).and_then(|e| e.read_file(rel)) {
                    Ok(v) => serde_json::json!({"v": 1, "id": id, "ok": true, "result": v}),
                    Err(e) => serde_json::json!({"v": 1, "id": id, "ok": false, "error": e.to_string()}),
                }
            }
            "cli" => {
                let args: Vec<String> = req["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
                let sub = args.first().cloned().unwrap_or_default();
                if sub == "check" {
                    serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"source": "db", "ok": true}})
                } else {
                    serde_json::json!({"v": 1, "id": id, "ok": false, "error": format!("cli not mapped: {sub}")})
                }
            }
            "cancel" => serde_json::json!({"v": 1, "id": id, "ok": true, "result": {"cancelled": true}}),
            "quit" => break,
            other => serde_json::json!({"v": 1, "id": id, "ok": false, "error": format!("unknown method: {other}")}),
        };
        if jwlog::on(jwlog::Level::Debug) {
            let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let ms = t0.elapsed().as_millis().to_string();
            jwlog::emit(
                jwlog::Level::Debug,
                "serve.call",
                &[("method", method), ("ms", &ms), ("ok", if ok { "1" } else { "0" })],
            );
        }
        let seq_after = seq();
        if seq_after > seq_before {
            let mut r = ring.lock().unwrap();
            for s in (seq_before + 1)..=seq_after {
                r.push_back((
                    s,
                    serde_json::json!({"seq": s, "method": method, "type": "write"}),
                ));
            }
            while r.len() > RING_CAP {
                r.pop_front();
            }
        }
        {
            let _g = OUT_LOCK.lock().unwrap();
            writeln!(out, "{resp}")?;
            out.flush()?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// 记忆折叠（只读规划）：同 subject、同 verdict 的连续段，段首过期 ≥90 天、条数 ≥8 才可折。
/// 留段首 + 段尾，中间观测删掉换一条 gist——转折点与未过期判断永不动。
fn cmd_compact(root: &PathBuf, apply: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    let plans = store.fold_plans(datacore::store::now_secs(), 8, 90 * 86_400)?;

    println!("可折叠段：{}", plans.len());
    let mut removed = 0usize;
    for (subject, verdict, entity, n, t0, t1) in &plans {
        println!(
            "  {subject} ×{n}（{} → {}，{verdict}{}）",
            day_str(*t0),
            day_str(*t1),
            entity
                .as_deref()
                .map(|e| format!("，{e}"))
                .unwrap_or_default()
        );
        removed += n.saturating_sub(2);
    }
    println!(
        "预计：删 {removed} 行中间观测，保留每段首尾，另立 gist {} 条",
        plans.len()
    );

    if apply {
        let mut removed = 0usize;
        let mut written: Vec<String> = Vec::new();
        for (subject, verdict, entity, _n, t0, t1) in &plans {
            removed += store.apply_fold(subject, verdict, entity.as_deref(), *t0, *t1)?;
            written.push(subject.clone());
        }
        let a = store.false_coverage()?;
        let b = store.refuted_without_full_coverage()?;
        let c = store.orphan_evidence()?;
        let d = store.orphan_alternative()?;
        if removed > 0 {
            store.note_write(&written)?;
        }
        println!("已折叠：删除 {removed} 行中间观测");
        println!(
            "check: false_coverage={a} refuted_without_full={b} orphan_evidence={c} orphan_alternative={d}"
        );
        return Ok(ExitCode::from(if a == 0 && b == 0 && c == 0 && d == 0 {
            0
        } else {
            1
        }));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_stat(root: &PathBuf) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    println!("repo        {}", store.repo());
    println!("db          {}", store.db_path().display());
    for t in ["judgment", "evidence", "source", "change", "snapshot", "conflict", "edge"] {
        println!("{t:<12}{}", store.count(t)?);
    }
    println!("false_cov   {}", store.false_coverage()?);
    // kind 分布：一眼看出有哪些判断
    for (kind, n) in store.kind_counts()? {
        println!("kind {kind:<11}{n}");
    }
    println!("alt         {}", store.count("alternative_of")?);
    let deps = store.dep_counts()?;
    if !deps.is_empty() {
        let line = deps
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect::<Vec<_>>()
            .join(" · ");
        println!("依赖(当前)  {line}");
    }
    // 订阅接口（v1.5）：客户端比较 write_seq 就知道库变没变
    if let Some(seq) = store.get_meta("write_seq")? {
        let at = store
            .get_meta("last_write_at")?
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let n = store.get_meta("last_write_subjects")?.unwrap_or_default();
        println!("write_seq   {seq}（{} · 影响 {n} 条）", day_str(at));
    }
    if let Some(at) = store
        .get_meta("last_compact_at")?
        .and_then(|s| s.parse::<i64>().ok())
    {
        println!("last_compact {}（serve 空闲时检查过折叠）", day_str(at));
    }
    Ok(ExitCode::SUCCESS)
}

/// 把 "90d" / "24h" / "30m" 解析成秒。
fn parse_age(s: &str) -> anyhow::Result<i64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let n: i64 = num.parse().map_err(|_| anyhow::anyhow!("看不懂的时间：{s}"))?;
    let secs = match unit {
        "d" => 86_400,
        "h" => 3_600,
        "m" => 60,
        "s" => 1,
        _ => return Err(anyhow::anyhow!("单位只支持 d/h/m/s：{s}")),
    };
    Ok(n * secs)
}


/// 手写 semver 解析：支持 "1"、"1.2"、"1.2.3"、前缀 ^~=、后缀 -pre/+build；`*`/git/path → None。
fn semver_of(v: &str) -> Option<(u64, u64, u64)> {
    let t = v.trim().trim_start_matches(|c: char| c == '^' || c == '~' || c == '=' || c == '>');
    let t = t.trim();
    if t.is_empty() || t == "*" || t.contains("git") || t.contains("path") || t.contains('*') {
        return None;
    }
    let core = t.split('-').next().unwrap_or(t).split('+').next().unwrap_or(t);
    let mut it = core.split('.');
    let major: u64 = it.next()?.parse().ok()?;
    let minor: u64 = it.next().unwrap_or("0").parse().ok()?;
    let patch: u64 = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// 漂移等级：0=同 major（含完全相同）/1=minor 漂移/2=major 漂移/3=未知。
fn drift_rank(a: Option<(u64, u64, u64)>, b: Option<(u64, u64, u64)>) -> u8 {
    match (a, b) {
        (Some(a), Some(b)) => {
            if a == b {
                0
            } else if a.0 != b.0 {
                2
            } else if a.0 == 0 {
                // 0.x：按 minor 比，0.8 vs 0.9 算 major-drift
                if a.1 != b.1 {
                    2
                } else {
                    1
                }
            } else if a.1 != b.1 {
                1
            } else {
                0
            }
        }
        _ => 3,
    }
}

fn drift_label(r: u8) -> &'static str {
    match r {
        0 => "same-major",
        1 => "minor-drift",
        2 => "major-drift",
        _ => "unknown",
    }
}

/// Python 依赖采集（只认 requirements.txt）：subject `dep:py:<name>`。
fn write_py_deps(store: &Store, root: &PathBuf, repo: &str) -> anyhow::Result<usize> {
    let deps = collect::py::scan(root);
    let at = datacore::store::now_secs();
    let src = Source {
        id: source_id(repo, "py", at),
        instrument: "py".into(),
        instrument_version: None,
        env: format!("repo={repo}"),
        time: at,
    };
    store.put_source(&src)?;
    let mut written = Vec::new();
    for d in &deps {
        let subject = format!("dep:py:{}", d.name);
        written.push(subject.clone());
        let mut scope = scope_of(vec!["requirements.txt".into()], "module", repo);
        scope.env.insert("ecosystem".into(), "py".into());
        if let Some(v) = &d.version {
            scope.env.insert("version".into(), v.clone());
        }
        scope.env.insert("dep_kind".into(), "normal".into());
        let j = judgment_row(repo, &subject, "dependency", Verdict::Held, at, 1, 1, scope);
        store.put_judgment(&j)?;
        store.expire_older_than(&subject, Some(&j.id), at)?;
        let premise = format!(
            "dep:py:{}:{}",
            d.name,
            d.version.clone().unwrap_or_default()
        );
        store.put_evidence(&Evidence {
            id: evidence_id(&j.id, &premise, "supports"),
            judgment_id: j.id.clone(),
            premise,
            polarity: Polarity::Supports,
            source_id: src.id.clone(),
        })?;
    }
    for subj in store.live_dependency_subjects()? {
        if subj.starts_with("dep:py:") && !written.contains(&subj) {
            if let Some(j) = store.find_judgment(&subj)? {
                store.set_expiry(&j.id, at)?;
            }
        }
    }
    if !written.is_empty() {
        store.note_write(&written)?;
    }
    Ok(deps.len())
}

fn cmd_deps_py(store: &Store, root: &PathBuf, repo: &str) -> anyhow::Result<ExitCode> {
    let n = write_py_deps(store, root, repo)?;
    println!("py deps {n}（只认 requirements.txt）");
    if root.join("pyproject.toml").is_file() {
        println!("pyproject 未采集（不解析 pyproject.toml）");
    }
    Ok(ExitCode::SUCCESS)
}

/// npm 依赖采集（无 Cargo 时走这里）：subject `dep:npm:<name>`。
fn write_npm_deps(
    store: &Store,
    root: &PathBuf,
    repo: &str,
) -> anyhow::Result<(usize, usize, usize, usize)> {
    let deps = collect::npm::scan(root).map_err(|e| anyhow::anyhow!(e))?;
    let at = datacore::store::now_secs();
    let src = Source {
        id: source_id(repo, "npm", at),
        instrument: "npm".into(),
        instrument_version: None,
        env: format!("repo={repo}"),
        time: at,
    };
    store.put_source(&src)?;
    let mut written = Vec::new();
    let mut unknown = 0usize;
    let mut n = 0usize;
    let mut normal = 0usize;
    let mut dev = 0usize;
    for d in &deps {
        let subject = if d.kind == "normal" {
            format!("dep:npm:{}", d.name)
        } else {
            format!("dep:npm:{}@{}", d.name, d.kind)
        };
        written.push(subject.clone());
        let mut scope = scope_of(vec!["package.json".into()], "module", repo);
        scope.env.insert("ecosystem".into(), "npm".into());
        if let Some(v) = &d.version {
            scope.env.insert("version".into(), v.clone());
        } else {
            unknown += 1;
        }
        if let Some(v) = &d.resolved {
            scope.env.insert("resolved".into(), v.clone());
        }
        scope.env.insert("dep_kind".into(), d.kind.clone());
        let j = judgment_row(repo, &subject, "dependency", Verdict::Held, at, 1, 1, scope);
        store.put_judgment(&j)?;
        store.expire_older_than(&subject, Some(&j.id), at)?;
        let premise = format!(
            "dep:npm:{}:{}:{}:{}",
            d.name,
            d.version.clone().unwrap_or_default(),
            d.resolved.clone().unwrap_or_default(),
            d.kind
        );
        store.put_evidence(&Evidence {
            id: evidence_id(&j.id, &premise, "supports"),
            judgment_id: j.id.clone(),
            premise,
            polarity: Polarity::Supports,
            source_id: src.id.clone(),
        })?;
        n += 1;
        if d.kind == "normal" {
            normal += 1;
        } else {
            dev += 1;
        }
    }
    for subj in store.live_dependency_subjects()? {
        if subj.starts_with("dep:npm:") && !written.contains(&subj) {
            if let Some(j) = store.find_judgment(&subj)? {
                store.set_expiry(&j.id, at)?;
            }
        }
    }
    if n > 0 {
        store.note_write(&written)?;
    }
    Ok((n, normal, dev, unknown))
}

fn cmd_deps_npm(store: &Store, root: &PathBuf, repo: &str) -> anyhow::Result<ExitCode> {
    let (n, normal, dev, unknown) = write_npm_deps(store, root, repo)?;
    println!("npm deps {n}（直接 {normal} · 开发 {dev} · 版本未知 {unknown}）");
    let n_py = write_py_deps(store, root, repo)?;
    if n_py > 0 {
        println!("py deps {n_py}（只认 requirements*.txt）");
    } else if root.join("pyproject.toml").is_file() {
        println!("pyproject 未采集（不解析 pyproject.toml）");
    }
    Ok(ExitCode::SUCCESS)
}

/// 跨仓同内容文件：按 git blob（内容寻址）分组；不含路径、不建表。
fn cmd_cross_files(a: &PathBuf, b: &PathBuf, include_empty: bool, json: bool) -> anyhow::Result<ExitCode> {
    let sa = Store::open(a)?;
    let sb = Store::open(b)?;
    let ma = sa.latest_blobs()?;
    let mb = sb.latest_blobs()?;
    let mut by_blob_a: std::collections::HashMap<String, Vec<String>> = Default::default();
    for (path, blob) in ma {
        by_blob_a.entry(blob).or_default().push(path);
    }
    let mut by_blob_b: std::collections::HashMap<String, Vec<String>> = Default::default();
    for (path, blob) in mb {
        by_blob_b.entry(blob).or_default().push(path);
    }
    // git 的空 blob（0 字节内容的 SHA-1）：默认不算"重复代码"
    const EMPTY_BLOB: &str = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    let mut pairs = Vec::new();
    let mut excluded_empty = 0usize;
    for (blob, pa) in &by_blob_a {
        if let Some(pb) = by_blob_b.get(blob) {
            if blob == EMPTY_BLOB && !include_empty {
                excluded_empty += 1;
                continue;
            }
            pairs.push((blob.clone(), pa[0].clone(), pb[0].clone()));
        }
    }
    let no_blob_a = sa.count("change").unwrap_or(0);
    let _ = no_blob_a;
    let no_blob = sa.pending_blob_paths()? + sb.pending_blob_paths()?;
    pairs.sort();
    if json {
        let v = serde_json::json!({
            "v": 1,
            "a": a.to_string_lossy(),
            "b": b.to_string_lossy(),
            "count": pairs.len(),
            "excluded_empty": excluded_empty,
            "no_blob": no_blob,
            "same_content": pairs.iter().map(|(h, pa, pb)| serde_json::json!({
                "hash": h, "a": pa, "b": pb,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "相同内容 {} 个（按 git blob；SHA-1 碰撞在 10^6 级可忽略）· 排除空文件 {excluded_empty} · 无 blob 文件 {no_blob}",
        pairs.len()
    );
    for (h, pa, pb) in pairs.iter().take(20) {
        println!("{}  {pa} ↔ {pb}", &h[..8.min(h.len())]);
    }
    if pairs.len() > 20 {
        println!("…还有 {} 对", pairs.len() - 20);
    }
    Ok(ExitCode::SUCCESS)
}

/// 跨仓 v0：两个库各开只读连接，按依赖名比版本（版本来自 dep 判断的 env）。
fn cmd_cross(a: &PathBuf, b: &PathBuf, dep: bool, eco: Option<&str>, json: bool) -> anyhow::Result<ExitCode> {
    if !dep {
        println!("cross 目前只支持 --dep");
        return Ok(ExitCode::from(2));
    }
    let sa = Store::open_existing(a)?;
    let sb = Store::open_existing(b)?;
    let ma: std::collections::BTreeMap<String, (Option<String>, Option<String>)> = sa
        .dep_versions()?
        .into_iter()
        .map(|(n, d, r)| (n, (d, r)))
        .collect();
    let mb: std::collections::BTreeMap<String, (Option<String>, Option<String>)> = sb
        .dep_versions()?
        .into_iter()
        .map(|(n, d, r)| (n, (d, r)))
        .collect();

    let eco_of = |k: &str| -> &'static str {
        if k.starts_with("npm:") {
            "npm"
        } else if k.starts_with("py:") {
            "py"
        } else {
            "cargo"
        }
    };
    let keep = |k: &str| match eco {
        None | Some("all") => true,
        Some(want) => eco_of(k) == want,
    };
    let mut shared = 0usize;
    let mut diff = 0usize;
    let mut counts = [0usize; 4];
    let mut rows = Vec::new();
    for (name, (da, ra)) in ma.iter().filter(|(k, _)| keep(k)) {
        if let Some((db, rb)) = mb.get(name) {
            shared += 1;
            let va = da.clone().or_else(|| ra.clone());
            let vb = db.clone().or_else(|| rb.clone());
            let is_diff = va != vb;
            if is_diff {
                diff += 1;
            }
            let rank = drift_rank(semver_of(va.as_deref().unwrap_or("?")), semver_of(vb.as_deref().unwrap_or("?")));
            counts[rank as usize] += 1;
            rows.push((name.clone(), va, vb, is_diff, rank));
        }
    }
    // 排序：major → minor → same-major → unknown（unknown 不猜，排最后）
    let key = |r: u8| if r == 3 { 4 } else { 2 - r };
    rows.sort_by(|a, b| key(a.4).cmp(&key(b.4)).then_with(|| a.0.cmp(&b.0)));

    if json {
        let v = serde_json::json!({
            "v": 1,
            "a": a.to_string_lossy(),
            "b": b.to_string_lossy(),
            "shared": shared,
            "diff": diff,
            "eco": eco.unwrap_or("all"),
            "drift": {
                "same_major": counts[0], "major": counts[2], "minor": counts[1], "unknown": counts[3],
            },
            "rows": rows.iter().map(|(n, va, vb, d, r)| serde_json::json!({
                "name": n,
                "a": va.clone().unwrap_or_else(|| "?".into()),
                "b": vb.clone().unwrap_or_else(|| "?".into()),
                "diff": d,
                "drift": drift_label(*r),
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(ExitCode::SUCCESS);
    }

    println!(
        "共有依赖 {shared} · 版本不同 {diff} · same-major {} / minor-drift {} / major-drift {} / unknown {}",
        counts[0], counts[1], counts[2], counts[3]
    );
    for (name, va, vb, _d, r) in rows {
        let fa = va.unwrap_or_else(|| "?".into());
        let fb = vb.unwrap_or_else(|| "?".into());
        println!("{name}  A={fa} / B={fb}  [{}]", drift_label(r));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_fold(root: &PathBuf, older_than: &str, apply: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open(root)?;
    let age = parse_age(older_than)?;
    let now = datacore::store::now_secs();
    let plans = store.fold_plans(now, 8, age)?;
    let segs = plans.len();
    let rows: usize = plans.iter().map(|p| p.3).sum();
    if plans.is_empty() {
        println!("没有可折叠的段（规则：已过期 + 同 verdict + n≥8）");
        return Ok(ExitCode::SUCCESS);
    }
    if !apply {
        println!("dry-run：可折 {segs} 段 / 涉及 {rows} 行 / 预计新增 gist {segs} 行");
        for (subject, verdict, _e, n, t0, t1) in plans.iter().take(10) {
            println!("  {subject}  {verdict}  n={n}  {t0}..{t1}");
        }
        if plans.len() > 10 {
            println!("  …还有 {} 段", plans.len() - 10);
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut deleted = 0usize;
    let mut gists = 0usize;
    store.begin()?;
    for (subject, verdict, entity, _n, t0, t1) in &plans {
        let d = store.apply_fold(subject, verdict, entity.as_deref(), *t0, *t1)?;
        if d > 0 {
            deleted += d;
            gists += 1;
        }
    }
    store.commit()?;
    println!("已折 {gists} 段 / 删除 {deleted} 行 / 新增 gist {gists} 行");
    Ok(ExitCode::SUCCESS)
}

/// 某文件的 `?use` 原因码台账（`{code: n}`）；没有 `?use` 返回 None。

fn gap_reasons(root: &PathBuf, store: &Store, src: &str) -> Option<serde_json::Value> {
    let rows = store.unresolved_use_edges().ok()?;
    let table = collect::gap::GapTable::new(root);
    let mut m: std::collections::BTreeMap<&'static str, i64> = Default::default();
    for (s, dst, line) in rows.iter().filter(|(s, _, _)| s == src) {
        *m.entry(table.reason(s, dst, *line).as_str()).or_default() += 1;
    }
    if m.is_empty() {
        None
    } else {
        Some(serde_json::json!(m))
    }
}

fn cmd_cite(root: &PathBuf, subject: &str, json: bool) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let e = engine::Engine::open(root).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let (v, code) = e.cite(subject).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else if let Some(rows) = v["rows"].as_array() {
        let verdict = rows
            .first()
            .and_then(|r| r["verdict"].as_str())
            .unwrap_or("");
        for r in rows {
            println!(
                "{}:{}  {}  {verdict}  {}",
                r["file"].as_str().unwrap_or(""),
                r["line"].as_i64().unwrap_or(0),
                short_commit(r["commit"].as_str().unwrap_or("")),
                r["evidence_kind"].as_str().unwrap_or("")
            );
        }
    } else {
        println!(
            "unresolved: {subject}（{}）",
            v["reason"].as_str().unwrap_or("?")
        );
    }
    Ok(ExitCode::from(code))
}

fn cmd_impact(root: &PathBuf, target: &str, json: bool) -> anyhow::Result<ExitCode> {
    let e = engine::Engine::open(root).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let (v, code) = e.impact(target).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else if v.get("reason").is_some() {
        println!("unresolved: {target}（{}）", v["reason"].as_str().unwrap_or("?"));
    } else {
        let targets = v["targets"].as_array().cloned().unwrap_or_default();
        let direct = v["direct"].as_array().cloned().unwrap_or_default();
        let trans = v["transitive"].as_array().cloned().unwrap_or_default();
        let items = v["tests"]["items"].as_array().cloned().unwrap_or_default();
        println!("{target}");
        println!("  目标 {} 个", targets.len());
        println!("  直接调用者 {} 个", direct.len());
        for r in direct.iter().take(10) {
            println!("    {}  (line {})", r["from"].as_str().unwrap_or(""), r["line"]);
        }
        println!("  传递调用者（depth≤2）{} 个", trans.len());
        for r in trans.iter().take(10) {
            println!("    {}  (line {})  via {}", r["from"].as_str().unwrap_or(""), r["line"], r["via"].as_str().unwrap_or(""));
        }
        println!(
            "  受影响文件 {} 个 · 其中的测试 {} 条（file 级，不承诺覆盖该符号）",
            v["tests"]["files"].as_array().map(|a| a.len()).unwrap_or(0),
            items.len()
        );
        println!("  导入该文件的边 {} 条", v["importers"].as_array().map(|a| a.len()).unwrap_or(0));
        println!(
            "  完整度：call 已解析 {} · 未解析 {}",
            v["completeness"]["call_resolved"], v["completeness"]["call_unresolved"]
        );
    }
    Ok(ExitCode::from(code))
}

fn capability_payload() -> serde_json::Value {
    let rows = collect::capability::table();
    let langs: Vec<serde_json::Value> = rows
        .iter()
        .map(|(ext, c)| {
            serde_json::json!({
                "ext": ext,
                "lang": c.lang,
                "symbol": c.symbol,
                "use": c.use_edge,
                "import": c.import_edge,
            })
        })
        .collect();
    serde_json::json!({"v": 1, "features": ["symbol", "use", "import"], "langs": langs})
}

fn cmd_tests_for(
    root: &PathBuf,
    target: &str,
    depth: i64,
    json: bool,
) -> anyhow::Result<ExitCode> {
    let e = engine::Engine::open(root).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let v = e.tests_for(target, depth).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let code = if v.get("reason").is_some() { 2 } else { 0 };
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else if v.get("reason").is_some() {
        println!(
            "unresolved: {target}（{}，要 symbol:<path>#<fn>）",
            v["reason"].as_str().unwrap_or("?")
        );
    } else {
        let hits = v["tests"].as_array().cloned().unwrap_or_default();
        println!(
            "{}  可能相关的测试 {} 个（静态可达，不等于运行覆盖）",
            v["target"].as_str().unwrap_or(target),
            hits.len()
        );
        for h in &hits {
            println!(
                "  {}:{}  {}  depth={} via={}",
                h["file"].as_str().unwrap_or(""),
                h["line"],
                h["name"].as_str().unwrap_or(""),
                h["depth"],
                h["via"].as_str().unwrap_or("-")
            );
        }
        println!(
            "  完整度：call 已解析 {} · 未解析 {} · depth≤{} · 全库测试 {}",
            v["completeness"]["call_resolved"],
            v["completeness"]["call_unresolved"],
            v["completeness"]["depth_cap"],
            v["completeness"]["tests_total"]
        );
    }
    Ok(ExitCode::from(code))
}

fn cmd_capability(json: bool) -> anyhow::Result<ExitCode> {
    let rows = collect::capability::table();
    if json {
        println!("{}", serde_json::to_string_pretty(&capability_payload())?);
    } else {
        for (ext, c) in rows {
            println!(
                "{ext:<4} {:<12} symbol={} use={} import={}",
                c.lang,
                c.symbol as u8,
                c.use_edge as u8,
                c.import_edge as u8
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// 用户级信任库目录：%APPDATA%/jw（非 Windows 用 ~/.config/jw）。
/// **绝不写进仓库**——仓库不能自证可信。
fn jw_user_dir() -> std::path::PathBuf {
    if let Ok(a) = std::env::var("APPDATA") {
        return std::path::PathBuf::from(a).join("jw");
    }
    if let Ok(h) = std::env::var("HOME") {
        return std::path::PathBuf::from(h).join(".config").join("jw");
    }
    std::env::temp_dir().join("jw-user")
}

fn trust_key(root: &std::path::Path) -> String {
    std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn trust_load() -> serde_json::Map<String, serde_json::Value> {
    let p = jw_user_dir().join("trust.json");
    let Ok(text) = std::fs::read_to_string(p) else {
        return serde_json::Map::new();
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

fn trust_is_trusted(root: &std::path::Path) -> bool {
    trust_load()
        .get(&trust_key(root))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 显式信任/撤销 + 一行审计（audit.log）。
fn trust_set(root: &std::path::Path, trusted: bool) -> anyhow::Result<bool> {
    let dir = jw_user_dir();
    std::fs::create_dir_all(&dir)?;
    let mut map = trust_load();
    let key = trust_key(root);
    if trusted {
        map.insert(key.clone(), serde_json::Value::Bool(true));
    } else {
        map.remove(&key);
    }
    let body = serde_json::Value::Object(map);
    std::fs::write(dir.join("trust.json"), serde_json::to_string_pretty(&body)?)?;
    let line = format!(
        "{}\t{}\t{}\n",
        datacore::store::now_secs(),
        if trusted { "grant" } else { "revoke" },
        key
    );
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("audit.log"))?;
    f.write_all(line.as_bytes())?;
    Ok(trusted)
}

/// file:// URI → repo 相对路径（剥 `\\?\` 前缀、URL decode；不依赖 cwd）。
fn uri_to_rel(root: &std::path::Path, uri: &str) -> Option<String> {
    let mut s = uri.strip_prefix("file://")?.to_string();
    // percent-decode（%20 等）
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    let mut p = out.replace('\\', "/");
    if let Some(rest) = p.strip_prefix("//?/") { p = rest.to_string(); }
    if let Some(rest) = p.strip_prefix('/') { p = rest.to_string(); }
    let root_norm = std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    let root_norm = root_norm.strip_prefix("//?/").unwrap_or(&root_norm).to_string();
    let root_norm = root_norm.trim_start_matches('/');
    if let Some(rest) = p.strip_prefix(&format!("{root_norm}/")) {
        return Some(rest.to_string());
    }
    // 大小写不敏感兜底（Windows 盘符）
    let lp = p.to_lowercase();
    let lr = root_norm.to_lowercase();
    if let Some(rest) = lp.strip_prefix(&format!("{lr}/")) {
        return Some(p[p.len() - rest.len()..].to_string());
    }
    None
}

/// definition 结果 → targets（Location | LocationLink[]）：file/line/col 全 1-based。
fn map_definition(root: &std::path::Path, result: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let items: Vec<&serde_json::Value> = if let Some(a) = result.as_array() {
        a.iter().collect()
    } else if result.is_object() {
        vec![result]
    } else {
        Vec::new()
    };
    for it in items {
        let uri = it["uri"].as_str().or(it["targetUri"].as_str()).unwrap_or("");
        let range = if it["range"].is_object() { &it["range"] } else { &it["targetSelectionRange"] };
        let line = range["start"]["line"].as_i64().unwrap_or(0);
        let ch = range["start"]["character"].as_i64().unwrap_or(0);
        if let Some(rel) = uri_to_rel(root, uri) {
            out.push(serde_json::json!({
                "file": rel, "line": lsp_to_ui(line), "col": lsp_to_ui(ch),
                "lsp_line": line, "lsp_char": ch,
            }));
        }
    }
    out
}

/// UI(1-based) ↔ LSP(0-based) 的单向映射（hover 位置必须双向可核）。
fn ui_to_lsp(n: i64) -> i64 { if n <= 1 { 0 } else { n - 1 } }
fn lsp_to_ui(n: i64) -> i64 { n + 1 }

/// 长驻 RA 会话：hover 与 diagnostics 复用同一个进程（不新起第二个）。
struct RaSession {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    rx: std::sync::mpsc::Receiver<String>,
    buf: String,
    caps: serde_json::Value,
    pid: u32,
    opened: std::collections::HashSet<String>,
    next_id: i64,
}

static RA_SESSION: std::sync::Mutex<Option<RaSession>> = std::sync::Mutex::new(None);

impl RaSession {
    fn send(&mut self, v: &serde_json::Value) {
        use std::io::Write;
        let body = v.to_string();
        let _ = self
            .stdin
            .write_all(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes());
        let _ = self.stdin.flush();
    }

    fn pump_until(&mut self, deadline: std::time::Instant, mut want: impl FnMut(&serde_json::Value) -> bool) -> Option<serde_json::Value> {
        while std::time::Instant::now() < deadline {
            let frames = lsp_frames(&self.buf);
            for f in &frames {
                if want(f) {
                    return Some(f.clone());
                }
            }
            match self.rx.recv_timeout(std::time::Duration::from_millis(250)) {
                Ok(chunk) => self.buf.push_str(&chunk),
                Err(_) => continue,
            }
        }
        None
    }

    fn kill_tree(&mut self) {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &self.pid.to_string(), "/T", "/F"])
                .output();
        }
        let _ = self.child.kill();
    }

    fn open(&mut self, root: &std::path::Path, file: &str) {
        if self.opened.contains(file) {
            return;
        }
        let text = match std::fs::read_to_string(root.join(file)) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("jw: didOpen 跳过（读不到 {file}: {e}）");
                return;
            }
        };
        let abs = std::fs::canonicalize(root.join(file)).unwrap_or_else(|_| root.join(file));
        self.send(&serde_json::json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri": file_uri(&abs), "languageId":"rust", "version":1, "text": text}
        }}));
        self.opened.insert(file.to_string());
    }

    fn hover(&mut self, root: &std::path::Path, file: &str, line1: i64, col1: i64) -> Option<(serde_json::Value, i64)> {
        self.open(root, file);
        let abs = std::fs::canonicalize(root.join(file)).unwrap_or_else(|_| root.join(file));
        let pos = serde_json::json!({"line": ui_to_lsp(line1), "character": ui_to_lsp(col1)});
        // RA 在索引完成前会对 hover 回 null：短轮询重试（最多 ~5s），不把"还没算完"当成"没有"
        for _ in 0..20 {
            self.next_id += 1;
            let id = self.next_id;
            self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":"textDocument/hover","params":{
                "textDocument":{"uri": file_uri(&abs)}, "position": pos
            }}));
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
            if let Some(f) = self.pump_until(deadline, |f| f["id"].as_i64() == Some(id)) {
                if !f["result"].is_null() {
                    return Some((f, id));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        None
    }

    fn code_actions(&mut self, root: &std::path::Path, file: &str, line1: i64, col1: i64, message: &str, severity: i64) -> Option<Vec<serde_json::Value>> {
        self.open(root, file);
        let abs = std::fs::canonicalize(root.join(file)).unwrap_or_else(|_| root.join(file));
        let l = ui_to_lsp(line1);
        let c0 = ui_to_lsp(col1);
        let range = serde_json::json!({
            "start": {"line": l, "character": c0},
            "end": {"line": l, "character": c0 + 1}
        });
        let diag = serde_json::json!({
            "range": range, "severity": severity, "source": "rust-analyzer",
            "message": if message.is_empty() { "diagnostic" } else { message }
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            self.next_id += 1;
            let id = self.next_id;
            self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":"textDocument/codeAction","params":{
                "textDocument":{"uri": file_uri(&abs)}, "range": range,
                "context":{"diagnostics":[diag]}
            }}));
            let step = std::time::Instant::now() + std::time::Duration::from_millis(600);
            if let Some(f) = self.pump_until(step, |f| f["id"].as_i64() == Some(id)) {
                let raw = f["result"].as_array().cloned().unwrap_or_default();
                if !raw.is_empty() {
                    let mut out = Vec::new();
                    for a in raw.iter().take(50) {
                        out.push(serde_json::json!({
                            "title": a["title"].as_str().unwrap_or(""),
                            "kind": a["kind"].as_str().unwrap_or(""),
                            "source": "lsp",
                            "is_preferred": a["isPreferred"].as_bool().unwrap_or(false),
                            "has_edit": a["edit"].is_object(),
                        }));
                    }
                    return Some(out);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        None
    }

    fn inlay_hints(&mut self, root: &std::path::Path, file: &str, start1: i64, end1: i64) -> Option<(Vec<serde_json::Value>, bool)> {
        self.open(root, file);
        let abs = std::fs::canonicalize(root.join(file)).unwrap_or_else(|_| root.join(file));
        // 可视区末端可能超出文件末尾：必须夹到真实行数，否则 RA 回 error 而不是空数组
        let total_lines = std::fs::read_to_string(root.join(file))
            .map(|s| s.lines().count().max(1) as i64)
            .unwrap_or(1);
        let end_clamped = end1.clamp(start1, total_lines.max(start1));
        let range = serde_json::json!({
            "start": {"line": ui_to_lsp(start1), "character": 0},
            "end": {"line": ui_to_lsp(end_clamped), "character": 0}
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            self.next_id += 1;
            let id = self.next_id;
            self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":"textDocument/inlayHint","params":{
                "textDocument":{"uri": file_uri(&abs)}, "range": range
            }}));
            let step = std::time::Instant::now() + std::time::Duration::from_millis(600);
            if let Some(f) = self.pump_until(step, |f| f["id"].as_i64() == Some(id)) {
                let raw = f["result"].as_array().cloned().unwrap_or_default();
                // 索引未完成时先回空数组：继续等（最多 8s），不把"还没算完"当"没有 hint"
                if raw.is_empty() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    continue;
                }
                let truncated = raw.len() > 200;
                let mut out = Vec::new();
                for h in raw.iter().take(200) {
                    let line = h["position"]["line"].as_i64().unwrap_or(0);
                    let ch = h["position"]["character"].as_i64().unwrap_or(0);
                    let label = if let Some(s) = h["label"].as_str() {
                        s.to_string()
                    } else if let Some(parts) = h["label"].as_array() {
                        parts.iter().filter_map(|p| p["value"].as_str()).collect::<Vec<_>>().join("")
                    } else {
                        String::new()
                    };
                    out.push(serde_json::json!({
                        "file": file, "line": lsp_to_ui(line), "col": lsp_to_ui(ch),
                        "label": label, "kind": h["kind"].as_i64().unwrap_or(1),
                        "source": "lsp",
                    }));
                }
                out.sort_by_key(|x| (x["line"].as_i64().unwrap_or(0), x["col"].as_i64().unwrap_or(0)));
                return Some((out, truncated));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        None
    }

    fn references(&mut self, root: &std::path::Path, file: &str, line1: i64, col1: i64, include_decl: bool) -> Option<Vec<serde_json::Value>> {
        self.open(root, file);
        let abs = std::fs::canonicalize(root.join(file)).unwrap_or_else(|_| root.join(file));
        let pos = serde_json::json!({"line": ui_to_lsp(line1), "character": ui_to_lsp(col1)});
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            self.next_id += 1;
            let id = self.next_id;
            self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":"textDocument/references","params":{
                "textDocument":{"uri": file_uri(&abs)}, "position": pos,
                "context":{"includeDeclaration": include_decl}
            }}));
            let step = std::time::Instant::now() + std::time::Duration::from_millis(600);
            if let Some(f) = self.pump_until(step, |f| f["id"].as_i64() == Some(id)) {
                let targets = map_definition(root, &f["result"]);
                if !targets.is_empty() {
                    return Some(targets);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        None
    }

    fn definition(&mut self, root: &std::path::Path, file: &str, line1: i64, col1: i64) -> Option<Vec<serde_json::Value>> {
        self.open(root, file);
        let abs = std::fs::canonicalize(root.join(file)).unwrap_or_else(|_| root.join(file));
        let pos = serde_json::json!({"line": ui_to_lsp(line1), "character": ui_to_lsp(col1)});
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            self.next_id += 1;
            let id = self.next_id;
            self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":"textDocument/definition","params":{
                "textDocument":{"uri": file_uri(&abs)}, "position": pos
            }}));
            let step = std::time::Instant::now() + std::time::Duration::from_millis(500);
            if let Some(f) = self.pump_until(step, |f| f["id"].as_i64() == Some(id)) {
                let targets = map_definition(root, &f["result"]);
                if !targets.is_empty() {
                    return Some(targets);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        None
    }

    fn diagnostics(&mut self, root: &std::path::Path, file: &str) -> Option<Vec<serde_json::Value>> {
        self.open(root, file);
        let abs = std::fs::canonicalize(root.join(file)).unwrap_or_else(|_| root.join(file));
        let uri = file_uri(&abs);
        let pull = self.caps["capabilities"]["diagnosticProvider"].is_object();
        if pull {
            // 索引未完成时 pull 会先回空数组：短轮询直到有诊断（最多 ~5s），不把"还没算完"当"无诊断"
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            let mut last: Vec<serde_json::Value> = Vec::new();
            while std::time::Instant::now() < deadline {
                self.next_id += 1;
                let id = self.next_id;
                self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":"textDocument/diagnostic","params":{
                    "textDocument":{"uri": uri}
                }}));
                let step = std::time::Instant::now() + std::time::Duration::from_millis(500);
                if let Some(f) = self.pump_until(step, |f| f["id"].as_i64() == Some(id)) {
                    if let Some(items) = f["result"]["items"].as_array() {
                        last = items.clone();
                        if !items.is_empty() {
                            break;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            return Some(map_lsp_diagnostics(file, &last));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let f = self.pump_until(deadline, |f| {
            f["method"].as_str() == Some("textDocument/publishDiagnostics")
                && f["params"]["uri"].as_str().map(|u| u.ends_with(file)).unwrap_or(false)
        })?;
        let items = f["params"]["diagnostics"].as_array()?.clone();
        Some(map_lsp_diagnostics(file, &items))
    }
}

/// 复用/启动长驻会话；RA 不存在时返回 Err("no_server")，声明不支持时由调用方判 skip。
fn ra_session<R>(root: &std::path::Path, f: impl FnOnce(&mut RaSession) -> R) -> Result<R, String> {
    let mut guard = RA_SESSION.lock().unwrap();
    let dead = match guard.as_mut() {
        Some(s) => matches!(s.child.try_wait(), Ok(Some(_))) || s.child.try_wait().is_err(),
        None => true,
    };
    if dead {
        let Some(ra) = find_rust_analyzer() else { return Err("no_server".into()) };
        let root_uri = file_uri(&std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()));
        let mut cmd = std::process::Command::new(&ra);
        cmd.current_dir(root);
        cmd.env_clear();
        for k in ["PATH", "SystemRoot", "TEMP", "TMP", "HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "RUSTUP_HOME", "CARGO_HOME"] {
            if let Ok(v) = std::env::var(k) { cmd.env(k, v); }
        }
        if std::env::var("HOME").is_err() {
            if let Ok(up) = std::env::var("USERPROFILE") { cmd.env("HOME", up); }
        }
        if let Ok(up) = std::env::var("USERPROFILE") {
            if std::env::var("RUSTUP_HOME").is_err() { cmd.env("RUSTUP_HOME", std::path::Path::new(&up).join(".rustup")); }
            if std::env::var("CARGO_HOME").is_err() { cmd.env("CARGO_HOME", std::path::Path::new(&up).join(".cargo")); }
        }
        cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        engine::set_lsp_warming(true);
        let mut child = cmd.spawn().map_err(|e| format!("server_down: {e}"))?;
        let pid = child.id();
        let stdin = child.stdin.take().ok_or("server_down: no stdin")?;
        let stdout = child.stdout.take().ok_or("server_down: no stdout")?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut so = stdout;
            let mut b = [0u8; 8192];
            loop {
                match so.read(&mut b) {
                    Ok(0) => break,
                    Ok(n) => { if tx.send(String::from_utf8_lossy(&b[..n]).to_string()).is_err() { break; } }
                    Err(_) => break,
                }
            }
        });
        let mut s = RaSession { child, stdin, rx, buf: String::new(), caps: serde_json::json!({}), pid, opened: Default::default(), next_id: 100 };
        s.send(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "processId": std::process::id(), "rootUri": root_uri,
            "capabilities": {"workspace":{"workspaceFolders":true, "configuration":true},
                             "textDocument":{"synchronization":{"didOpen":true},
                                             "hover":{"contentFormat":["markdown","plaintext"]},
                                             "inlayHint":{"dynamicRegistration":false,
                                                          "resolveSupport":{"properties":["tooltip","textEdits","label","label.tooltip","label.location","label.command"]}},
                                             "codeAction":{"codeActionLiteralSupport":{"codeActionKind":{"valueSet":["quickfix","refactor","refactor.extract","refactor.inline","source","source.fixAll"]}}},
                                             "publishDiagnostics":{}}},
            "workspaceFolders":[{"uri": root_uri, "name":"jw"}],
        }}));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let Some(res) = s.pump_until(deadline, |f| f["id"].as_i64() == Some(1)) else {
            let _ = s.child.kill();
            return Err("server_down".into());
        };
        s.caps = res["result"].clone();
        engine::set_lsp_warming(false);
        s.send(&serde_json::json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
        *guard = Some(s);
    }
    Ok(f(&mut guard.as_mut().unwrap()))
}

/// hover 的 markdown → (type, doc)：RA 用 ```lang ... ``` + `---` + 文档。
fn hover_parts(contents: &serde_json::Value) -> (Option<String>, Option<String>) {
    let text = if let Some(s) = contents.as_str() {
        s.to_string()
    } else if let Some(v) = contents["value"].as_str() {
        v.to_string()
    } else if let Some(arr) = contents.as_array() {
        arr.iter().filter_map(|x| x["value"].as_str().or(x.as_str())).collect::<Vec<_>>().join("\n")
    } else {
        String::new()
    };
    if text.trim().is_empty() {
        return (None, None);
    }
    let mut ty = None;
    let mut doc = None;
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        if let Some(end) = after.find("```") {
            let body = &after[..end];
            let body = body.splitn(2, '\n').nth(1).unwrap_or(body).trim();
            if !body.is_empty() { ty = Some(body.to_string()); }
            let rest = after[end + 3..].trim();
            if !rest.is_empty() { doc = Some(rest.trim_start_matches("---").trim().to_string()); }
        }
    }
    if ty.is_none() {
        ty = Some(text.lines().next().unwrap_or("").trim().to_string());
    }
    (ty, doc)
}

/// 从 LSP 字节流里抽出所有 JSON 消息（Content-Length framing）。
/// rust-analyzer 发现链：JW_RA_PATH → 用户配置 lsp.json → PATH。
/// 绝对路径 → file:/// URI（去掉 Windows canonicalize 的 `\\?\` 前缀）。
fn file_uri(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_string();
    let s = s.strip_prefix("//./").unwrap_or(&s).to_string();
    format!("file:///{}", s.trim_start_matches('/'))
}

fn find_rust_analyzer() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("JW_RA_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let cfg = jw_user_dir().join("lsp.json");
    if let Ok(text) = std::fs::read_to_string(&cfg) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(p) = v["rust_analyzer"].as_str() {
                let pb = PathBuf::from(p);
                if pb.is_file() {
                    return Some(pb);
                }
            }
        }
    }
    let exe = if cfg!(windows) { "rust-analyzer.exe" } else { "rust-analyzer" };
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn ra_version(bin: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn lsp_frames(raw: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(i) = rest.find("\r\n\r\n") {
        let head = &rest[..i];
        let Some(len_pos) = head.to_ascii_lowercase().find("content-length:") else {
            rest = &rest[i + 4..];
            continue;
        };
        let len: usize = head[len_pos + 15..]
            .trim()
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let start = i + 4;
        if rest.len() < start + len {
            break;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&rest[start..start + len]) {
            out.push(v);
        }
        rest = &rest[start + len..];
    }
    out
}

/// LSP diagnostic → 我们的 item：file:line:col + severity + code，来源固定 lsp。
fn map_lsp_diagnostics(file: &str, diags: &[serde_json::Value]) -> Vec<serde_json::Value> {
    diags
        .iter()
        .map(|d| {
            let line = d["range"]["start"]["line"].as_i64().unwrap_or(0) + 1;
            let col = d["range"]["start"]["character"].as_i64().unwrap_or(0) + 1;
            let sev = match d["severity"].as_i64().unwrap_or(3) {
                1 => "error",
                2 => "warning",
                _ => "info",
            };
            serde_json::json!({
                "file": file, "line": line, "col": col, "severity": sev,
                "code": d["code"].as_str().unwrap_or(""), "message": d["message"].as_str().unwrap_or(""),
                "source": "lsp",
            })
        })
        .collect()
}

/// `jw web`：一个进程搞定 HTTP + 桥 + 浏览器（不依赖 node）。
fn cmd_web(root: &PathBuf, port: u16, open: bool) -> anyhow::Result<ExitCode> {
    serve_web(root, port, open, None)
}

/// HTTP + bridge + 浏览器；GUI bin 复用同一份实现（端口 0 = 自动分配）。
pub fn serve_web(
    root: &PathBuf,
    port: u16,
    open: bool,
    ready: Option<std::sync::mpsc::Sender<u16>>,
) -> anyhow::Result<ExitCode> {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Condvar, Mutex};

    let exe = std::env::current_exe()?;
    let mut child = std::process::Command::new(&exe)
        .arg("serve")
        .arg(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let child_in = Arc::new(Mutex::new(child.stdin.take().expect("serve stdin")));
    let child_out = child.stdout.take().expect("serve stdout");

    // 回包按 id 收集（serve 的 hello/event 帧没有 id，忽略）
    let pending: Arc<(Mutex<std::collections::HashMap<i64, serde_json::Value>>, Condvar)> =
        Arc::new((Mutex::new(Default::default()), Condvar::new()));
    {
        let pending = pending.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(child_out);
            let mut line = String::new();
            loop {
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                            if let Some(id) = v["id"].as_i64() {
                                let (m, cv) = &*pending;
                                m.lock().unwrap().insert(id, v);
                                cv.notify_all();
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    let ui_dir = {
        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let dir = exe.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        let a = dir.join("web").join("ui");
        if a.join("index.html").exists() { a } else { dir.join("..").join("..").join("tools").join("web").join("ui") }
    };
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
        anyhow::anyhow!(
            "启动失败：{e}（端口被占用就换 --port 7778；若提示缺 DLL，请安装 VC++ 2015–2022 运行库）"
        )
    })?;
    let index = include_str!("../../../tools/web/index.html");
    let repo = root.to_string_lossy().to_string();
    let actual = listener.local_addr()?.port();
    if let Some(tx) = ready { let _ = tx.send(actual); }
    let url = format!("http://127.0.0.1:{actual}/");
    println!("经纬 web: {url}");
    println!("仓库: {repo}");
    println!("关掉这个窗口（或 Ctrl+C）即停止。");
    if open {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "", &url])
                .spawn();
        }
    }

    let seq = Arc::new(std::sync::atomic::AtomicI64::new(0));
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        let pending = pending.clone();
        let child_in = child_in.clone();
        let seq = seq.clone();
        let repo = repo.clone();
        let ui_dir = ui_dir.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            let n = match s.read(&mut buf) { Ok(0) | Err(_) => return, Ok(n) => n };
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let mut it = req.lines();
            let first = it.next().unwrap_or("");
            let mut content_len = 0usize;
            for line in it {
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_len = v.trim().parse().unwrap_or(0);
                }
            }
            // 直接按 \r\n\r\n 找 body 起点，不要用 lines() 偏移累加（会差字节）
            let body_start = req.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
            let body = if body_start > 0 && body_start <= req.len() {
                &req[body_start..req.len().min(body_start + content_len)]
            } else { "" };
            let (status, ctype, payload) = if first.starts_with("GET / ") || first.starts_with("GET /index.html") {
                ("200 OK", "text/html; charset=utf-8", index.to_string())
            } else if first.starts_with("GET /ui/") {
                let rel = first.split_whitespace().nth(1).unwrap_or("/ui/index.html").trim_start_matches("/ui/");
                let file = ui_dir.join(rel);
                let ctype = if rel.ends_with(".css") { "text/css; charset=utf-8" }
                    else if rel.ends_with(".js") { "application/javascript; charset=utf-8" }
                    else { "text/html; charset=utf-8" };
                match std::fs::read_to_string(&file) {
                    Ok(s) => ("200 OK", ctype, s),
                    Err(_) => ("404 Not Found", "text/plain", "ui asset not found".to_string()),
                }
            } else if first.starts_with("GET /health") {
                ("200 OK", "application/json", serde_json::json!({"ok": true, "repo": repo}).to_string())
            } else if first.starts_with("POST /rpc") {
                let Ok(mut v) = serde_json::from_str::<serde_json::Value>(body) else {
                    let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n");
                    return;
                };
                let action = v["action"].as_str().unwrap_or("").to_string();
                let id = seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let obj = v.as_object_mut().unwrap();
                obj.remove("action");
                v["id"] = serde_json::json!(id);
                v["method"] = serde_json::json!(action);
                {
                    let mut ci = child_in.lock().unwrap();
                    let _ = writeln!(ci, "{v}");
                    let _ = ci.flush();
                }
                let (m, cv) = &*pending;
                let mut g = m.lock().unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                let resp = loop {
                    if let Some(r) = g.remove(&id) { break Some(r); }
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    if left.is_zero() { break None; }
                    let (ng, _t) = cv.wait_timeout(g, left.min(std::time::Duration::from_millis(300))).unwrap();
                    g = ng;
                };
                match resp {
                    Some(r) => ("200 OK", "application/json", r.to_string()),
                    None => ("504 Gateway Timeout", "application/json", "{\"ok\":false,\"error\":\"timeout\"}".to_string()),
                }
            } else {
                ("404 Not Found", "text/plain", "not found".to_string())
            };
            let _ = write!(s, "HTTP/1.1 {status}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}", payload.as_bytes().len());
            let _ = s.flush();
        });
    }
    if let Ok(mut g) = RA_SESSION.lock() {
        if let Some(mut s) = g.take() {
            s.kill_tree();
        }
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    }
    let _ = child.kill();
    Ok(ExitCode::SUCCESS)
}

fn cmd_trust(action: &str, root: &PathBuf, json: bool) -> anyhow::Result<ExitCode> {
    let trusted = match action {
        "grant" => trust_set(root, true)?,
        "revoke" => trust_set(root, false)?,
        _ => trust_is_trusted(root),
    };
    let v = serde_json::json!({
        "v": 1, "path": trust_key(root), "trusted": trusted,
        "store": jw_user_dir().join("trust.json").to_string_lossy(),
        "note": "启动 LSP 会执行仓库的 build.rs / proc-macro；只在你信任的仓库上开启",
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!(
            "{}  trusted={}   store={}",
            v["path"].as_str().unwrap_or(""),
            trusted,
            v["store"].as_str().unwrap_or("")
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// D7：用 rust-analyzer 审查一批文件。**有 error → 退出码 1；查不了 → 2**（三态分开）。
#[allow(dead_code)] // gui bin 复用本文件但不走这些入口
fn cmd_lsp_check(
    paths: &[PathBuf],
    root: Option<&PathBuf>,
    json: bool,
    limit: usize,
) -> anyhow::Result<ExitCode> {
    let mut files: Vec<PathBuf> = Vec::new();
    for p in paths {
        if p.is_dir() {
            collect_rs_files(p, &mut files, limit);
        } else if p.is_file() {
            files.push(p.clone());
        }
        if files.len() >= limit {
            files.truncate(limit);
            break;
        }
    }
    if files.is_empty() {
        println!("没有找到要查的 .rs 文件");
        return Ok(ExitCode::from(2));
    }
    let ws = root
        .cloned()
        .or_else(|| lsp::workspace_root(&files[0]))
        .unwrap_or_else(|| PathBuf::from("."));
    let t0 = std::time::Instant::now();
    let rep = lsp::check(&ws, &files, lsp::timeout_from_env())
        .map_err(|e| anyhow::anyhow!("lsp: {e}"))?;
    let ms = t0.elapsed().as_millis().to_string();
    if jwlog::on(jwlog::Level::Debug) {
        jwlog::emit(
            jwlog::Level::Debug,
            "lsp.check",
            &[
                ("files", &files.len().to_string()),
                ("errors", &rep.errors.to_string()),
                ("warnings", &rep.warnings.to_string()),
                ("ms", &ms),
            ],
        );
    }
    if json {
        let out = serde_json::json!({
            "workspace": ws.to_string_lossy(),
            "files": files.len(),
            "errors": rep.errors,
            "warnings": rep.warnings,
            "note": rep.note,
            "diagnostics": rep.diagnostics.iter().map(|d| serde_json::json!({
                "file": d.file, "line": d.line, "col": d.col,
                "severity": d.severity, "message": d.message,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for d in &rep.diagnostics {
            println!("{}:{}:{}: {} {}", d.file, d.line, d.col, d.severity, d.message);
        }
        println!(
            "\n{} 个文件：{} error / {} warning{}",
            files.len(),
            rep.errors,
            rep.warnings,
            rep.note.as_deref().map(|n| format!("（{n}）")).unwrap_or_default()
        );
    }
    if rep.errors > 0 {
        return Ok(ExitCode::from(1));
    }
    if rep.diagnostics.is_empty() && rep.note.is_some() {
        // 一条诊断都没有 + 有 note = 没查到（不等于没问题）
        return Ok(ExitCode::from(2));
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(dead_code)] // gui bin 复用本文件但不走这些入口
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>, limit: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= limit {
            return;
        }
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if matches!(name.as_str(), "target" | ".git" | ".jingwei" | "node_modules" | "dist") {
                continue;
            }
            collect_rs_files(&p, out, limit);
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

fn cmd_check(root: &PathBuf) -> anyhow::Result<ExitCode> {
    let store = Store::open_existing(root)?;
    let a = store.false_coverage()?;
    let b = store.refuted_without_full_coverage()?;
    let c = store.orphan_evidence()?;
    let d = store.orphan_alternative()?;
    let e = store.unregistered_kinds()?;
    let f = store.dangling_sources()?;
    let g = if store.write_log_ok()? { 0 } else { 1 };
    let h = store.edge_empty()?;
    let rules_now = store.get_meta("parse_rules_version")?.unwrap_or_default();
    let i = if rules_now == PARSE_RULES_VERSION { 0 } else { 1 };
    // 第 10/11 项：能力核对。声明了"需要某能力"的判断/边落在不支持该能力的扩展名上 = 撒谎。
    let caps = collect::capability::LangCaps;
    let mut cap_bad = 0i64;
    for (kind, _) in store.kind_counts()? {
        let Some(f) = datacore::capability::feature_required(&kind) else {
            continue;
        };
        for subject in store.subjects_by_kind(&kind)? {
            if let Some(path) = datacore::capability::path_of_subject(&subject) {
                if !datacore::capability::Caps::supports(&caps, path, f) {
                    cap_bad += 1;
                }
            }
        }
    }
    let mut edge_cap_bad = 0i64;
    for (kind, src, leaf) in store.edges_brief()? {
        let Some(f) = collect::capability::feature_of_kind(&kind) else {
            continue;
        };
        // impl 的文件在 leaf；test 的 src 是 `test:<file>#<fn>`；call 的 src 是 `symbol:<file>#<fn>`
        let test_file;
        let file: &str = if kind == "impl" {
            leaf.as_deref().unwrap_or(&src)
        } else if kind == "test" || kind == "test_macro" {
            test_file = src
                .strip_prefix("test:")
                .and_then(|r| r.split_once('#'))
                .map(|(p, _)| p.to_string())
                .unwrap_or_else(|| src.clone());
            &test_file
        } else if kind == "call" {
            test_file = datacore::core::typed_path(&src).unwrap_or_else(|| src.clone());
            &test_file
        } else {
            &src
        };
        if !datacore::capability::Caps::supports(&caps, file, f) {
            edge_cap_bad += 1;
        }
    }
    println!("false_coverage={a} refuted_without_full={b} orphan_evidence={c} orphan_alternative={d}");
    println!("unregistered_kind={e} dangling_source={f} write_log_bad={g} edge_empty={h} rules_version_bad={i}");
    let lsp_in_history: i64 = store.count_lsp_evidence()?;
    println!("capability_violation={cap_bad} edge_capability_violation={edge_cap_bad}");
    println!("lsp_in_history={lsp_in_history}");
    if a == 0
        && b == 0
        && c == 0
        && d == 0
        && e == 0
        && f == 0
        && g == 0
        && h == 0
        && i == 0
        && cap_bad == 0
        && edge_cap_bad == 0
    {
        println!("ok");
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

#[allow(dead_code)]
fn _typecheck(j: Judgment) -> String {
    j.id
}

#[cfg(test)]
mod cli_ux_tests {
    use super::{no_arg_hint, should_pause};

    #[test]
    fn no_arg_hint_is_actionable() {
        let h = no_arg_hint();
        assert!(h.contains("start.cmd"), "提示必须指到 start.cmd：{h}");
        assert!(h.contains("jw web"), "提示必须指到 jw web：{h}");
    }

    #[test]
    fn pause_only_without_args_and_tty() {
        assert!(should_pause(1, true));
        assert!(!should_pause(1, false));
        assert!(!should_pause(2, true));
    }
}

#[cfg(test)]
mod pos_map_tests {
    use super::{lsp_to_ui, ui_to_lsp};

    #[test]
    fn ui_lsp_position_roundtrip() {
        for ui in 1..=50i64 {
            assert_eq!(lsp_to_ui(ui_to_lsp(ui)), ui, "ui={ui}");
        }
        assert_eq!(ui_to_lsp(1), 0);
        assert_eq!(ui_to_lsp(0), 0, "0/负数按 0 处理，不 panic");
        assert_eq!(lsp_to_ui(0), 1);
    }
}
