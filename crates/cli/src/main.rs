//! jingwei 命令行外壳：所有逻辑都在 engine 里，这里只负责排版和参数。

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static COLOR: AtomicBool = AtomicBool::new(false);
static MORE: AtomicBool = AtomicBool::new(false);

// 命令按**领域**分家（见《设计》§12.3 那条行数门禁逼出来的拆分）。
// 拆的边界不是"大小平均切"：同一个文件里的东西该回答同一类问题。
mod cmd_clean;
mod cmd_doctor;
mod cmd_deps;
mod cmd_discover;
mod cmd_explain;
mod cmd_handover;
mod cmd_knowledge;
mod cmd_learn;
mod cmd_symbol;
mod cmd_harvest;
mod cmd_kverify;
mod cmd_next;
mod cmd_impact;
mod cmd_positions;
mod cmd_rename;
mod cmd_watch;
mod cmd_graph;
mod cmd_lsp;
mod cmd_guide;
mod cmd_machine;
mod cmd_project;
mod cmd_run;
mod cmd_verify;
mod cmd_projects;

use cmd_clean::{cmd_clean, cmd_scan};
use cmd_doctor::{cmd_audit, cmd_changes, cmd_doctor, cmd_fix};
use cmd_graph::cmd_graph;
use cmd_guide::cmd_guide;
use cmd_machine::{cmd_spec, cmd_web};
use cmd_project::{
    cmd_env, cmd_envs, cmd_init, cmd_new, cmd_note, cmd_notes, cmd_snap, kind_list,
};
use cmd_run::{cmd_run, cmd_task};

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[90m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";

fn c(code: &str, s: &str) -> String {
    if COLOR.load(Ordering::Relaxed) {
        format!("{}{}{}", code, s, RESET)
    } else {
        s.to_string()
    }
}

/// 东亚字符按 2 列宽算，否则中文标题的补点会歪
fn width(s: &str) -> usize {
    s.chars()
        .map(|ch| if (ch as u32) > 0x1100 { 2 } else { 1 })
        .sum()
}

fn pad(s: &str, to: usize) -> String {
    let w = width(s);
    if w >= to {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(to - w))
    }
}

// ------------------------------------------------------------------ 入口

fn main() {
    setup_console();
    jingwei_engine::dlog::install_panic_hook();
    jingwei_engine::dlog::i("1000", "app=cli");
    let code = run();
    jingwei_engine::dlog::i("1001", &format!("exit={} buffered={}", code, jingwei_engine::dlog::buffered()));
    jingwei_engine::dlog::flush();
    std::process::exit(code);
}

fn run() -> i32 {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut cmd = String::new();
    let mut rest: Vec<String> = Vec::new();
    let mut json = false;
    let mut want_check = false;
    let mut yes = false;
    let mut permanent = false;
    let mut diff_only = false;
    let mut deep = false;
    let mut recommended_only = false;
    let mut only: Vec<String> = Vec::new();
    let mut run_override: Option<String> = None;
    let mut template: Option<String> = None;
    let mut parent_dir: Option<PathBuf> = None;
    let mut python_exe: Option<PathBuf> = None;
    let mut want_venv = true;
    let mut want_git = false;
    let mut venv_name: Option<String> = None;
    let mut purpose: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut lang_name: Option<String> = None;
    // `projects` 用：--depth N（往下几层）与 --all（连嵌套的一起给）
    let mut depth_arg: usize = 2;
    let mut show_all = false;
    // `rename` 用：--to <新名>（新名可能在全局解析阶段就被消费，所以放这里）
    let mut to_arg: Option<String> = None;
    // `watch` 用：--journal <路径>（同样会被全局解析器先吃掉）
    let mut journal_arg: Option<String> = None;
    let mut write_files = false;

    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        match a {
            "--root" | "-C" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => root = PathBuf::from(v),
                    None => {
                        eprintln!("--root 后面要跟路径");
                        return 2;
                    }
                }
            }
            "--run" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => run_override = Some(v.clone()),
                    None => {
                        eprintln!("--run 后面要跟命令");
                        return 2;
                    }
                }
            }
            "--json" => json = true,
            // 顺带跑一次真实的编译器检查（Rust 项目才有）
            "--check" => want_check = true,
            "--yes" | "-y" => yes = true,
            "--permanent" => permanent = true,
            "--diff" => diff_only = true,
            "--deep" => deep = true,
            // **knowledge verify 用**：允许在脏工作区跑（会检测命令有没有动工作区）
            "--allow-dirty" => rest.push("--allow-dirty".to_string()),
            // **impact --file 用**：按文件问影响面
            "--file" => rest.push("--file".to_string()),
            // **harvest --from-git 用**：从提交记录发现候选（顶层 `--git` 是另一个开关，别混）
            "--from-git" => rest.push("--from-git".to_string()),
            "--all" => show_all = true,
            "--depth" => {
                i += 1;
                match argv.get(i).and_then(|v| v.parse::<usize>().ok()) {
                    Some(n) => depth_arg = n.clamp(1, 6),
                    None => {
                        eprintln!("--depth 后面要跟一个 1..6 的数字");
                        return 2;
                    }
                }
            }
            "--to" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => to_arg = Some(v.clone()),
                    None => {
                        eprintln!("--to 后面要跟新名字");
                        return 2;
                    }
                }
            }
            "--write" => write_files = true,
            "--journal" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => journal_arg = Some(v.clone()),
                    None => {
                        eprintln!("--journal 后面要跟路径");
                        return 2;
                    }
                }
            }
            "--more" => MORE.store(true, Ordering::Relaxed),
            "--recommended" => recommended_only = true,
            "--template" | "-t" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => template = Some(v.clone()),
                    None => {
                        eprintln!("--template 后面要跟模板名");
                        return 2;
                    }
                }
            }
            "--dir" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => parent_dir = Some(PathBuf::from(v)),
                    None => {
                        eprintln!("--dir 后面要跟路径");
                        return 2;
                    }
                }
            }
            "--python" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => python_exe = Some(PathBuf::from(v)),
                    None => {
                        eprintln!("--python 后面要跟解释器路径");
                        return 2;
                    }
                }
            }
            "--name" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => venv_name = Some(v.clone()),
                    None => {
                        eprintln!("--name 后面要跟虚拟环境目录名");
                        return 2;
                    }
                }
            }
            "--venv" => want_venv = true,
            "--no-venv" => want_venv = false,
            "--git" => want_git = true,
            "--purpose" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => purpose = Some(v.clone()),
                    None => {
                        eprintln!("--purpose 后面要跟一句「为什么做它」");
                        return 2;
                    }
                }
            }
            "--lang" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => lang_name = Some(v.clone()),
                    None => {
                        eprintln!("--lang 后面要跟语言（python / rust / node / java / cpp）");
                        return 2;
                    }
                }
            }
            "--kind" | "-k" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => kind = Some(v.clone()),
                    None => {
                        eprintln!("--kind 后面要跟类别（{}）", kind_list());
                        return 2;
                    }
                }
            }
            "--only" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => only.push(v.to_lowercase()),
                    None => {
                        eprintln!("--only 后面要跟关键词");
                        return 2;
                    }
                }
            }
            // **扫盘的同源检测**（2026-10-19）：只对同名目录算 Jaccard，代价可控
            "--similar" => rest.push("--similar".to_string()),
            // **knowledge fix 用**：--symbol <新符号|symbol:id>（带值，原样透传给子命令）
            "--symbol" => {
                rest.push("--symbol".to_string());
                i += 1;
                match argv.get(i) {
                    Some(v) => rest.push(v.clone()),
                    None => {
                        eprintln!("--symbol 后面要跟符号（名字或 symbol:id）");
                        return 2;
                    }
                }
            }
            // **learn --mark 用**：--mark <步骤id>（带值，原样透传）
            "--mark" => {
                rest.push("--mark".to_string());
                i += 1;
                match argv.get(i) {
                    Some(v) => rest.push(v.clone()),
                    None => {
                        eprintln!("--mark 后面要跟步骤 id（learn 列表里那一条的条目名）");
                        return 2;
                    }
                }
            }
            // **为什么没扫到**：扫完按原因统计被跳过的目录
            "--why" => rest.push("--why".to_string()),
            "--help" | "-h" => return help(),
            "--version" | "-V" => {
                println!("jingwei {}", env!("CARGO_PKG_VERSION"));
                return 0;
            }
            "--" => {
                rest.extend(argv[i + 1..].iter().cloned());
                break;
            }
            _ if a.starts_with('-') => {
                eprintln!("不认识的参数：{}（用 jingwei --help 看用法）", a);
                return 2;
            }
            _ => {
                if cmd.is_empty() {
                    cmd = a.to_string();
                } else {
                    rest.push(a.to_string());
                }
            }
        }
        i += 1;
    }

    let root = strip_verbatim(root.canonicalize().unwrap_or(root));

    // **前置条件：这个目录得真的存在**（2026-10-17）。
    //
    // ## 抓到的现象（拿边界输入跑出来的）
    //
    // 对一个**拼错的路径**，三个出口给了三个不同答案：
    //   `guide`  → 「补上就能判断」（退出码 **2**）
    //   `doctor` → 「问题 共 3 条（0 错误）…… 这个项目还没有契约」（退出码 **0**！）
    //   `audit`  → 又是另一种（退出码 2）
    //
    // **`doctor` 退出 0 是最坏的**：脚本靠退出码决定要不要拦下来，
    // 而它对一个不存在的目录说"一切正常"。
    //
    // ## 为什么归 `1` 而不归 `2`
    //
    // `介绍.md` 的约定：**`1` = 跑不起来（他的问题）**、`2` = 查不了（我们的问题）。
    // 路径拼错是**他的问题**——不是我们读不到（那才是 2）。
    // 所以是 `1`，**不需要发明第四种退出码**。
    //
    // ## 为什么只对一部分命令检查
    //
    // `new` 是**创建**项目的（目录本来就该不存在）；
    // `scan` / `clean` / `spec` / `envs` 是**机器级**的，`--root` 对它们没意义。
    // 别的命令都要读一个已存在的项目——对它们说"目录不存在"**有信息量**，
    // 而继续往下跑只会给出**看起来正常、其实全是空的**结论。
    let needs_existing = matches!(
        cmd.as_str(),
        "guide"
            | "howto"
            | "inspect"
            | "doctor"
            | "check"
            | "audit"
            | "fix"
            | "whatschanged"
            | "run"
            | "test"
            | "env"
            | "note"
            | "notes"
            | "log"
            | "web"
            | "snap"
            | "graph"
            | "index"
            | "init"
    );
    if needs_existing && !root.is_dir() {
        let why = format!("这个目录不存在：{}", root.display());
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "state": "no",
                    "reason": why,
                    "why": "their_gap",
                    "fix": "确认路径拼写；或者在项目列表里点一个"
                })
            );
        } else {
            println!();
            println!("  {} {}", c(RED, "✗"), why);
            println!("    {}", c(DIM, "确认路径拼写；或者在项目列表里点一个"));
            println!();
        }
        // 退出码 1：**他的问题**（路径不对），不是"我们查不了"（那是 2）
        return 1;
    }

    match cmd.as_str() {
        "" | "help" => help(),
        // `inspect` 是 `使用说明.md` 承诺过的入口（"看项目：结论、待处理项"），
        // 而它**从来没在分派表里出现过**——按说明书照做会撞上"不认识子命令"。
        // 现在把它接成 doctor 的同一个入口：**同一个内核，两个名字**。
        "inspect" | "doctor" | "check" => cmd_doctor(&root, json, want_check),
        "guide" | "howto" => cmd_guide(&root, json),
        "new" => cmd_new(&root, json, rest, template, parent_dir, python_exe, want_venv, want_git, purpose, lang_name),
        "env" => cmd_env(&root, rest, venv_name, python_exe),
        "note" => cmd_note(&root, rest, kind, purpose),
        "notes" | "log" => cmd_notes(&root, json),
        "test" => cmd_task(&root, "test", None),
        "verify" | "build" => cmd_verify::cmd_verify(&root, json),
        // **位置回读**：用原文当裁判核图谱（2026-10-19）
        // **依赖对账**：清单 vs 锁文件（2026-10-19）
        "deps" => cmd_deps::cmd_deps(&root, json),
        "positions" => cmd_positions::cmd_positions(&root, json),
        // **知识条目严筛**（2026-10-20，pro 任务卡 A）：双头漏斗最细的那道颈
        "knowledge" => cmd_knowledge::cmd_knowledge(&root, &rest, json),
        // **学习路径出口**（2026-10-20，pro 任务卡 B）：知识条目按 prereq 排成能照着走的顺序
        "learn" => cmd_learn::cmd_learn(&root, &rest, json),
        // **学习进度**（2026-10-20，pro 任务卡 F）：同一个出口，进度落在本地状态里
        "progress" => cmd_learn::cmd_progress(&root, &rest, json),
        // **一屏"现在该做什么"**（2026-10-21）：把学/收/编/挂四处状态汇总成入口（纯读，不改任何状态）
        "next" => cmd_next::cmd_next(&root, json),
        // **改动影响面**（2026-10-21）：往上游看谁会受影响——改代码前真正要问的那句
        "impact" => cmd_impact::cmd_impact(&root, &rest, json),
        // **符号 → 条目 反查**（2026-10-20，pro 任务卡 C）：可操作图谱的第一个查询原语
        "symbol" => cmd_symbol::cmd_symbol(&root, &rest, json),
        // **采集半边**（2026-10-20，pro 任务卡 D）：从散文里抽逐字候选 → _inbox 待确认
        "harvest" => cmd_harvest::cmd_harvest(&root, &rest, json),
    "watch" => {
        let roots: Vec<String> = rest
            .iter()
            .filter(|a| !a.starts_with("--"))
            .cloned()
            .collect();
        let journal = journal_arg.clone();
        cmd_watch::cmd_watch(&root, &roots, journal.as_deref())
    }
    "handover" => cmd_handover::cmd_handover(&root, json),
    "explain" | "why" => {
        let Some(who) = rest.first() else {
            eprintln!("用法：jingwei explain <名字|symbol:id>");
            return 2;
        };
        cmd_explain::cmd_explain(&root, who, json)
    }
    "rename" => {
        let name = rest.first().map(|s| s.as_str()).unwrap_or("");
        let to = to_arg.clone().unwrap_or_default();
        if name.is_empty() || to.is_empty() {
            eprintln!("用法：jingwei rename <符号名或id> --to <新名> [--write]");
            2
        } else {
            cmd_rename::cmd_rename(&root, name, &to, write_files, json)
        }
    }
        "projects" | "list" => cmd_projects::cmd_projects(&root, depth_arg, show_all, json),
        "snap" | "snapshot" => cmd_snap(&root, json, diff_only),
        "run" | "start" => cmd_run(&root, run_override, rest),
        "scan" | "clean-list" => cmd_scan(&root, json),
        "audit" | "health" => cmd_audit(&root, json, deep),
        "fix" => cmd_fix(&root, json, yes, &only),
        "whatschanged" | "changed" => cmd_changes(&root, json),
        "clean" => cmd_clean(&root, json, yes, permanent, recommended_only, &only),
        "envs" | "python" => cmd_envs(&root, json),
        "spec" | "machine" => cmd_spec(json),
        "web" | "node" => cmd_web(&root, json),
        "init" => cmd_init(&root, json),
        "graph" => cmd_graph(&root, json, &rest),
        // **扫盘找项目**：可扫多个根、边扫边出、有终态（与界面同一套引擎）。
        "discover" => {
            // **深度要用全局解析出来的 `depth_arg`**（2026-10-19 抓到）：
            // `--depth` 在全局参数解析里就被消费了，rest 里根本没有它，
            // 于是本地解析永远 fallback 到默认值 —— 整盘扫时看起来"扫不深"。
            let mut roots: Vec<std::path::PathBuf> = Vec::new();
            for a in &rest {
                if !a.starts_with("--") {
                    roots.push(std::path::PathBuf::from(a));
                }
            }
            let want_similar = rest.iter().any(|a| a == "--similar");
            let want_why = rest.iter().any(|a| a == "--why");
            cmd_discover::cmd_discover(roots, depth_arg, json, want_similar, want_why, journal_arg.as_deref().map(std::path::Path::new))
        }

        // **`jingwei index` = 把项目带到"图谱可用"的状态。**
        // 与界面「索引一次」同一个内核入口（`graph::index::index_source`）。
        //
        // 为什么它必须存在：**要人开窗手点的操作，就等于不能被执行**。
        // 原来 `graph index` 是"故意没做"的（理由是"两处实现迟早不一致"），
        // 而这里是一处实现两条外壳——不冲突，而缺它的代价是每次看图都要手点。
        "index" | "index-graph" => cmd_graph(&root, json, &["index".to_string()]),
        // **最小 LSP**：编辑器通过 stdio 说话，只做"跳到定义"（读图谱，不重新解析）。
        "lsp" => cmd_lsp::cmd_lsp(&root),
        other => {
            eprintln!("不认识子命令：{}", other);
            help();
            // **返回非 0。**
            //
            // 这原来是 `help()`——而 `help()` 返回 0，于是**"不认识这个命令"被报成了成功**。
            // 脚本里 `jingwei inspect && next_step` 会继续往下走，
            // 而用户以为自己刚跑完一次体检。这与四条线里"三态不许压扁"是同一条：
            // **"没做"不许长得像"做成了"。**
            1
        }
    }
}

/// canonicalize 在 Windows 上会给出 \\?\D:\... 这种前缀，给人看的路径要还原
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() > 2 && b[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    p
}
/// **子命令表：唯一真源。**
///
/// `--help` 显示什么、测试允许什么，都从这一份走。
///
/// ## 为什么要有它（2026-10-16 抓到的两处真缺陷）
///
/// 1. **`--help` 漏了四个主命令**：`inspect` / `graph` / `spec` / `machine` 都能跑，
///    却不在 `--help` 里——而 `使用说明.md` 是准的。**两份清单各写各的，必然漂移。**
/// 2. 更要紧的是 `使用说明.md` 承诺的 `jingwei inspect [目录]`（"看项目：结论、待处理项"）
///    **在 CLI 里根本不存在**：跑它会打印"不认识子命令：inspect"。
///    一条被文档承诺、却被命令行拒绝的入口——用户按说明书照做就撞墙。
///
/// 所以把清单收成一份，再让 `--help` 与它**同源**，并由测试盯着两边不许分家。
///
/// 每项是 `(名字, 别名, 说明)`。名字是**主名**（`--help` 里显示的那个），
/// 别名只记在这里、供分派用。
/// **主路径 8 个**（2026-10-21）：只在这 8 个里选，其余是"按需用"。
/// 判据来自审计：36 个入口 = 没有默认动作；主路径固定后，`--help` 的排序就是优先级。
pub(crate) const MAIN_VERBS: &[&str] = &[
    "next", "explain", "impact", "positions", "handover", "index", "graph", "knowledge",
];

pub(crate) const COMMANDS: &[(&str, &[&str], &str)] = &[
    ("new", &[], "从模板建一个能立刻跑起来的项目"),
    (
        "guide",
        &["howto"],
        "**一句话回答「现在能不能跑、卡在哪、下一步敲什么」**——比 doctor 短，因为它会排序",
    ),
    ("env", &[], "环境：status / init(建虚拟环境并绑进契约) / install / use"),
    ("doctor", &["check"], "体检：解释器、依赖、契约、环境变化，给出问题清单"),
    ("inspect", &[], "看这个项目：结论、待处理项（与 doctor 同一个入口）"),
    (
        "audit",
        &["health"],
        "项目体检：忽略规则、锁文件、密钥、路径、体积（--deep 才算体积）",
    ),
    ("fix", &[], "替你把体检里能安全代做的改掉（默认只预演，--yes 才动手）"),
    (
        "whatschanged",
        &["changed"],
        "从上次看过算起，这个项目被改了哪些地方",
    ),
    ("run", &["start"], "按契约启动项目：6 阶段进度 + 实时输出 + 就绪判定"),
    ("test", &[], "跑契约里声明的 test 命令"),
    (
        "projects",
        &["list"],
        "**列出一个目录下的项目**（走目录索引；--all 连嵌套的一起给，--depth 控制往下几层）",
    ),
    (
        "verify",
        &["build"],
        "**跑一次真实构建**，回答这台机器够不够用：通过 / 没过 / 没有构建这一步 / 我们没验成",
    ),
    (
        "note",
        &[],
        "记一句想法；-k 选类别（goal/idea/plan/blocker/learned/decision/note）",
    ),
    ("notes", &["log"], "看这个项目的工作记录"),
    ("scan", &["clean-list"], "清理清单：哪些能删、哪些要管理员、哪些别碰"),
    ("clean", &[], "执行清理（默认只预演，加 --yes 才动手）"),
    ("snap", &["snapshot"], "记录一份环境快照，并和上一份对比"),
    ("envs", &["python"], "列出这台机器上所有能用的 Python"),
    (
        "web",
        &["node"],
        "Node / 前端：包管理器、依赖装到哪去了、版本要求写在哪",
    ),
    ("init", &[], "给已有项目补一份 env.contract.json"),
    (
        "discover",
        &[],
        "**扫盘找项目**：可扫多个根、边扫边出、有终态（--depth N，默认 4）",
    ),
    (
        "lsp",
        &[],
        "**最小 LSP（stdio）**：给编辑器用，只做「跳到定义」——位置来自图谱，查不到就 null",
    ),
    ("spec", &["machine"], "这台机器：CPU / 内存 / 磁盘 / 显卡 / 系统设置"),
    ("graph", &[], "运行历史：跑过几次、各出了什么问题、挂在哪"),
    // 这几个一直是能跑的命令，却**从没进过帮助表**（2026-10-20 发现）：
    // 用户敲 `jingwei --help` 看不到 handover / explain / positions / rename / watch，
    // 只能从说明书或记忆里找。帮助表是"有哪些命令"的唯一真源，漏了就等于没这命令。
    ("handover", &[], "接手人一页纸：能不能跑 / 规模 / 热点 / 下一步 / 决策理由 / 文件状态"),
    ("explain", &["why"], "这个符号是什么、定义在哪、被谁调 / 被引用几处"),
    (
        "impact",
        &[],
        "**改动影响面**：往上游看谁会受影响（静态 `calls` 边逐层，`--depth 1..4`）",
    ),
    ("positions", &[], "图谱里的行号对不对（用原文回读，0 错才可信）"),
    ("rename", &[], "改名：判不准就不发编辑；`--write` 才落盘，写完回读"),
    ("watch", &[], "盯文件变化，写成 journal，给 `discover --journal` 用"),
    (
        "knowledge",
        &[],
        "**知识层**：check 严筛 / add 人确认 / sync 进图 / stale+fix 回收 / verify 跑判据",
    ),
    (
        "learn",
        &[],
        "**学习路径**：按条目的 prereq 排出「先学什么、凭什么」——顺序不是文件名；`--symbol` 从代码出发",
    ),
    (
        "progress",
        &[],
        "**学到哪了**：已学 N/总 M、逐条 ○/●（进度存本地，不进仓库、不进任何门禁）",
    ),
    (
        "next",
        &[],
        "**一屏「现在该做什么」**：学（第一步没学的）/ 收（待重编）/ 编（待确认候选）/ 挂（文本态）",
    ),
    (
        "symbol",
        &[],
        "**反查**：这个符号被哪几条知识条目解释过（条目引用了查不到的符号 → 标「重编」）",
    ),
    (
        "harvest",
        &[],
        "**采集半边**：从散文里抽「逐字候选」到 docs/knowledge/_inbox/，人确认后才入条目",
    ),
    (
        "index",
        &[],
        "**建/更新图谱**——之后图谱页与 `jingwei graph` 就有东西了（与界面「索引一次」同一个内核入口）",
    ),
];

/// 把帮助文本渲染成字符串——`help()` 与测试都走它。
///
/// **为什么不让 `help()` 直接 `println!`**：那样测试只能去抓 stdout，
/// 而这条测试要断言的是"每个子命令都出现在帮助里"。拆出可测的纯函数，
/// 判据才能真的落在**渲染出来的那一份**上，而不是落在"我以为它会渲染什么"上。
fn render_help() -> String {
    render_help_with(MORE.load(std::sync::atomic::Ordering::Relaxed))
}

/// **默认只列主路径 8 个**（2026-10-21）；`jingwei --more --help` 才列全部。
/// 两部分都从 `COMMANDS` 生成——**一个都不删**，只是默认不展示。
fn render_help_with(all: bool) -> String {
    let mut subs = String::new();
    subs.push_str("  【主路径】\n");
    // **按 MAIN_VERBS 的顺序列**（2026-10-21）：主路径的意义是"从哪开始"，
    // 所以第一个必须是 `next`——而不是 `COMMANDS` 里的书写顺序（那第一个是 graph）。
    for want in MAIN_VERBS {
        if let Some((name, _aliases, desc)) = COMMANDS.iter().find(|(n, _, _)| n == want) {
            subs.push_str(&format!("  {:<12} {}\n", name, desc));
        }
    }
    if all {
        subs.push_str("\n  【其余·按需用】\n");
        for (name, _aliases, desc) in COMMANDS {
            if !MAIN_VERBS.contains(name) {
                subs.push_str(&format!("  {:<12} {}\n", name, desc));
            }
        }
    } else {
        let hidden = COMMANDS.len() - MAIN_VERBS.len();
        subs.push_str(&format!("\n  其余 {hidden} 个按需用：jingwei --more --help\n"));
    }
    format!(
        "{}  ·  项目状态解释器（主路径 8 个，其余按需）\n\
         \n\
         用法：  jingwei <子命令> [选项]\n\
         \n\
         子命令\n\
         {subs}\
         选项\n\
         \x20 -C, --root <目录>   指定项目根（默认当前目录）\n\
         \x20 --template <模板>   new 用：script / package / cli\n\
         \x20 --dir <父目录>      new 用：建在哪儿\n\
         \x20 --purpose <一句>    new 用：为什么做它，会记成第一条记录\n\
         \x20 --python <exe>      指定解释器\n\
         \x20 --no-venv           new 用：不建虚拟环境\n\
         \x20 --git               new 用：顺手 git init\n\
         \x20 --json              输出机器可读的 JSON\n\
         \x20 --deep              audit 用：额外统计产物体积和大文件\n\
         \x20 --more              展开更多细节\n\
         \x20 --run <命令>        覆盖契约里的 run 命令\n\
         \x20 --recommended       清理时只选「推荐清理」的项\n\
         \x20 --only <关键词>     只处理标签/路径/分类里含该关键词的项（可重复）\n\
         \x20 --permanent         永久删除（不加则走回收站）\n\
         \x20 -y, --yes           跳过预演，直接执行清理",
        c(BOLD, "jingwei")
    )
}

fn help() -> i32 {
    println!("{}", render_help());
    0
}

fn wrap(s: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = if (ch as u32) > 0x1100 { 2 } else { 1 };
        if w + cw > cols {
            out.push(std::mem::take(&mut cur));
            w = 0;
        }
        cur.push(ch);
        w += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ------------------------------------------------------------------ scan


#[cfg(windows)]
pub(crate) fn setup_console() {
    extern "system" {
        fn SetConsoleOutputCP(cp: u32) -> i32;
        fn SetConsoleCP(cp: u32) -> i32;
        fn GetStdHandle(n: u32) -> *mut core::ffi::c_void;
        fn GetConsoleMode(h: *mut core::ffi::c_void, m: *mut u32) -> i32;
        fn SetConsoleMode(h: *mut core::ffi::c_void, m: u32) -> i32;
    }
    // SAFETY: 全是值入参。`GetStdHandle` 返回的伪句柄由系统拥有、无需（也不该）关闭；
    // 句柄取值失败时下面的 `GetConsoleMode` 会返回 0，跳过后续设置。
    // 没有控制台（输出被重定向）时这些调用会失败，失败即放弃——不改动任何东西。
    unsafe {
        // 不切成 UTF-8 的话，中文在 GBK 控制台上会变成乱码
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);
        let h = GetStdHandle(0xFFFF_FFF5); // STD_OUTPUT_HANDLE(-11)
        let mut mode = 0u32;
        if GetConsoleMode(h, &mut mode) != 0 {
            SetConsoleMode(h, mode | 0x0004); // ENABLE_VIRTUAL_TERMINAL_PROCESSING
        }
    }
    COLOR.store(std::io::stdout().is_terminal(), Ordering::Relaxed);
}

#[cfg(not(windows))]
pub(crate) fn setup_console() {
    COLOR.store(std::io::stdout().is_terminal(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {

    /// **覆盖度不够时，必须提醒"角色判断会偏多"**（2026-10-18，fd 实测换的）。
    ///
    /// 真数据：24 个 .rs、120 条 calls 里跨文件只接上 27 条、141 条拿不准，
    /// 结果判出 **5 个"入口"**，只有 `main.rs` 是真的。
    /// 所以这条判据是：**拿不准比连上还多 → 就要说那句话**；够就不说（不吵）。
    #[test]
    fn 覆盖度不够时要提醒角色判断会偏多() {
        // 拿不准 > 连上 → 必须提醒
        let tip = cmd_graph::coverage_caveat_for_test(32, 141);
        assert!(tip.is_some(), "拿不准 141 比连上 32 多，必须提醒");
        let t = tip.unwrap();
        assert!(t.contains("偏多"), "要说出后果：{t}");
        assert!(t.contains("141"), "要把数说出来：{t}");

        // 连上更多 → 不许吵
        assert!(cmd_graph::coverage_caveat_for_test(200, 10).is_none());
        // 一样多 → 不吵（边界：等于不算不够）
        assert!(cmd_graph::coverage_caveat_for_test(50, 50).is_none());
    }
    use super::*;

    /// **子命令层也要"认不出就报错"，不许静默当成默认行为。**
    ///
    /// 这条是拿一个真缺陷换的（2026-10-16，第 27 轮目标盘点时抓到）：
    /// `cmd_graph` 里"空子命令"（列运行历史）和"认不出的子命令"**共用同一个 `_ =>` 兜底**，
    /// 于是 `jingwei graph index`——**账本里写着"故意没做"的那条命令**——
    /// 会打印运行历史并**返回 0**。脚本里 `jingwei graph index && 下一步`
    /// 就这么安心往下走了。
    ///
    /// 这与 `main.rs` 顶层分派第 19 轮修的是同一条线（**没做不许长得像做成了**），
    /// 那次只修了外面那一层。**同一个形状漏在里面一层，隔了八轮才被发现。**
    ///
    /// `cmd_env` 里就是对的（`other => { eprintln!(...); 2 }`），所以这条测试盯的是
    /// "别把 `cmd_graph` 改回去"——判据是那条兜底臂必须**显式叫 `other`**，而且**返回非 0**。
    ///
    /// **这条测试的第一版判据写宽了**：它只在 `match sub {` 之后找 `_ =>`，
    /// 而那段里还有别的 `_ =>`（`serde_json` 的 `unwrap_or_default()` 之类），
    /// 于是**对着已经修好的代码报红**。改成盯 `other =>` 这个显式名字 + 它的返回码——
    /// **判据要么精确，要么会被人当成噪声绕过去。**
    #[test]
    fn graph_认不出的子命令不许静默成功() {
        let src = include_str!("cmd_graph.rs");
        // ① 空子命令那条臂必须在（默认行为），否则"默认"和"认不出"又会被同一段代码接住
        assert!(
            src.contains("\"\" =>"),
            "cmd_graph 里该有一条专门处理空子命令的臂——\
             否则「只看运行历史」这个默认行为和「认不出的子命令」又会被同一段代码接住"
        );
        // ② 认不出的那条臂必须显式叫 other，并且返回非 0
        let tail = src
            .split("match sub {")
            .nth(1)
            .expect("找不到 `match sub {`——cmd_graph 的形状变了，同步改这条测试");
        let other_at = tail.find("other =>").expect(
            "cmd_graph 的 `match sub` 里没有 `other =>` 兜底臂。\
             认不出的子命令会被静默吞掉（`jingwei graph index` 返回 0）——\
             用户以为跑成功了。请补上显式的 `other => { ...; 2 }`。",
        );
        let arm = &tail[other_at..(other_at + 400).min(tail.len())];
        assert!(
            arm.contains("2"),
            "`cmd_graph` 里认不出子命令的那条臂没有返回非 0——\
             与 main.rs 顶层分派第 19 轮修的是同一条线：**没做不许长得像做成了**。\narm = {arm}"
        );
    }

    /// **`--help` 与分派表不许分家。**
    ///
    /// 这条是拿两处真缺陷换的（2026-10-16）：
    ///   1. `--help` 漏了 `inspect` / `graph` / `spec` / `machine` 四个能跑的主命令；
    ///   2. `使用说明.md` 承诺的 `jingwei inspect` **在 CLI 里根本不存在**，
    ///      跑它会打印"不认识子命令"——用户按说明书照做就撞墙。
    ///
    /// 判据分两层，缺一层就会漏：
    ///   - 第一层：表里每个名字都要出现在 `--help` 里（防"加了命令没写说明"）；
    ///   - 第二层：表里每个名字都要**真的接上分派**——做法是把本文件读出来，
    ///     找 `match cmd.as_str()` 那一段里有没有这个字符串字面量。
    ///     只查 `--help` 是不够的：那样"只加说明、没接线"照样能过。
    #[test]
    fn 每个子命令都出现在帮助里而且真的接上了分派() {
        let src = include_str!("main.rs");
        let dispatch = src
            .split("match cmd.as_str()")
            .nth(1)
            .and_then(|s| s.split("other =>").next())
            .expect("找不到 match cmd.as_str() 那一段——分派表的形状变了，得同步改这条测试");

        // 把 `--help` 真的渲染一遍来断言——**不靠"这句断言恒真"蒙过去**。
        // （第一版这里写过一条 `COMMANDS.iter().any(...)` 的占位断言，它永远为真；
        //  那正是这个仓库在防的"恒真断言"，当场删了。）
        // 2026-10-21：默认 `--help` 只列主路径，全表要看 `--more`。这里断言**全表那一份**。
        let rendered = render_help_with(true);
        for (name, aliases, desc) in COMMANDS {
            assert!(
                !desc.is_empty(),
                "子命令 {name} 没有说明；`--help` 是要给人看的"
            );
            // 第一层：`--help` 渲染出来里得有它
            assert!(
                rendered.contains(&format!("\n  {name}")),
                "子命令 {name} 没出现在 `--help` 里——用户看不到它"
            );
            // 第二层：**主名必须自己出现在分派里**。
            //
            // 这里原来写的是"主名**或任一别名**命中即可"——**那是个口子**
            // （2026-10-16 外部对照复核指出的）：把 `"spec" | "machine"` 改成只剩
            // `"machine"`，判据照样绿，而 `--help` 打印的是**主名** `spec`
            // （`render_help` 忽略别名），用户看到 `spec` 却敲不动它。
            // 主名才是对外承诺的那个名字，所以必须单独钉。
            assert!(
                dispatch.contains(&format!("\"{name}\"")),
                "子命令 {name} 是 `--help` 里对外承诺的名字，却**没接上分派**——\
                 用户会看到它，然后撞上「不认识子命令」"
            );
            // 别名（如果有）也各钉一次：别名是"同一个内核的另一个名字"，同样不能是死的
            for a in aliases.iter() {
                assert!(
                    dispatch.contains(&format!("\"{a}\"")),
                    "子命令 {name} 的别名 {a} 没接上分派——写了个敲不动的名字"
                );
            }
        }
    }

    /// **分派表里不许有"被注释掉的臂"。**
    ///
    /// 这是上一个口子的另一面（2026-10-16 外部对照复核指出）：上面的判据是在
    /// `match cmd.as_str()` 那一段里做**子串**匹配，而注释也在那一段里。
    /// 把 `"graph" => cmd_graph(...)` 整行注释掉，`"graph"` 这个字符串还在，
    /// **判据照样绿，而命令已经死了**。所以要把注释行排除掉再找。
    #[test]
    fn 分派里不许有被注释掉的臂() {
        let src = include_str!("main.rs");
        let dispatch = src
            .split("match cmd.as_str()")
            .nth(1)
            .and_then(|s| s.split("other =>").next())
            .expect("找不到 match cmd.as_str() 那一段");
        // 去掉行注释：`// ...` 到行尾（这几条臂都是单行的，不需要处理块注释）
        let live: String = dispatch
            .lines()
            .map(|l| match l.find("//") {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for (name, aliases, _) in COMMANDS {
            for w in std::iter::once(*name).chain(aliases.iter().copied()) {
                let lit = format!("\"{w}\"");
                if dispatch.contains(&lit) && !live.contains(&lit) {
                    panic!(
                        "子命令 {w} 的名字只出现在**注释里**——那条臂被注释掉了，\
                         命令已经死了，而字符串还在，所以旧的子串判据查不出来"
                    );
                }
            }
        }
    }

    /// **不允许有名字写重**（主名与别名都不许撞）。
    ///
    /// 撞了的时候，`match` 的先后顺序会决定谁生效——那种"看着有两个入口、
    /// 其实只有一个能用"的坑，比缺一个命令还难查。
    #[test]
    fn 主名与别名都不许重复() {
        let mut seen: Vec<&str> = Vec::new();
        for (name, aliases, _) in COMMANDS {
            for w in std::iter::once(*name).chain(aliases.iter().copied()) {
                assert!(!seen.contains(&w), "子命令名 {w} 出现了两次");
                seen.push(w);
            }
        }
    }

    /// `inspect` 必须在表里——它是 `使用说明.md` 承诺过的入口。
    ///
    /// 单拎出来钉住，是因为它正是上面说的第 2 处缺陷：**文档承诺了、命令行不认**。
    /// 光靠"表与分派一致"是查不出这件事的（两边都不含它也一样一致）。
    #[test]
    fn 说明书承诺过的_inspect_必须在表里() {
        assert!(
            COMMANDS.iter().any(|(n, _, _)| *n == "inspect"),
            "`使用说明.md` 写着 `jingwei inspect [目录]`——表里没有它，说明书就在骗人"
        );
    }

    /// **打不开图谱库时，不许一律说成"还没建过图谱"**（2026-10-16）。
    ///
    /// 原来的代码是 `Err(_)` + 一句话——把**库在、打不开**（被别的进程占着 /
    /// 文件坏了 / 权限不对）也说成"你还没建过图谱"。用户于是去点「索引一次」，
    /// **而那个动作解决不了他的问题**：真正的错因被那句话盖住了。
    /// 这是三态压扁的典型形态：**"我们读不到"伪装成了"你没有"。**
    ///
    /// ## 为什么用扫源码，而不是造一个打不开的库
    ///
    /// 试过了：SQLite 很宽容——**垃圾内容它能打开**（错误延后到查询时才出），
    /// 而把库的位置放成**目录**时 `db.is_file()` 是 false，走的是 `no` 那条路。
    /// 也就是说**"库在、但 `open_for_query` 失败"这个组合在磁盘上很难自然造出来**。
    /// 而这条判据要守的恰恰是那一段——所以像上面那条一样扫源码：
    /// **判据要么能构造，要么就精确地盯住那段代码。**
    ///
    /// 三个断言分开钉，因为它们会各自坏：
    ///   ① 必须**先看磁盘上有没有那个库**（那是事实，不是猜）；
    ///   ② 库里没有 → 才允许说"还没建过图谱"；
    ///   ③ 库在 → 必须是 `unknown` 而不是 `no`（**不许把我们的失败说成用户的空白**）。
    #[test]
    fn 打不开图谱库时不许一律说成还没建过() {
        let src = include_str!("cmd_graph.rs");
        assert!(
            src.contains("db.is_file()"),
            "必须先按'磁盘上有没有那个库'分流——那是事实，不是猜"
        );
        assert!(
            src.contains(r#""state": "unknown""#),
            "库在、读不到必须是 unknown；一律说成 no 就是把我们的失败说成用户的空白"
        );
        assert!(
            src.contains(r#""why": "our_gap""#),
            "unknown 那一支要带上归属（our_gap），别让外壳猜"
        );
        // 而且它**不许**被算成"跑不起来"（1）——"查不了"与"跑不起来"是两件事。
        assert!(
            src.contains("return 2;"),
            "读不到库时退出码该是 2（查不了），不是 1（跑不起来）"
        );
    }
}






