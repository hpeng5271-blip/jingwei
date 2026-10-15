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
mod cmd_machine;
mod cmd_project;
mod cmd_run;

use cmd_clean::{cmd_clean, cmd_scan};
use cmd_doctor::{cmd_audit, cmd_changes, cmd_doctor, cmd_fix};
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
    match cmd.as_str() {
        "" | "help" => help(),
        "doctor" | "check" => cmd_doctor(&root, json, want_check),
        "new" => cmd_new(&root, json, rest, template, parent_dir, python_exe, want_venv, want_git, purpose, lang_name),
        "env" => cmd_env(&root, rest, venv_name, python_exe),
        "note" => cmd_note(&root, rest, kind, purpose),
        "notes" | "log" => cmd_notes(&root, json),
        "test" => cmd_task(&root, "test", None),
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
        other => {
            eprintln!("不认识子命令：{}", other);
            help()
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
fn help() -> i32 {
    println!(
        "{}  ·  项目环境体检 / 启动 / 清理\n\
         \n\
         用法：  jingwei <子命令> [选项]\n\
         \n\
         子命令\n\
         \x20 new <名字>   从模板建一个能立刻跑起来的项目\n\
         \x20 env         环境：status / init(建虚拟环境并绑进契约) / install / use\n\
         \x20 doctor      体检：解释器、依赖、契约、环境变化，给出问题清单\n\
         \x20 audit       项目体检：忽略规则、锁文件、密钥、路径、体积（--deep 才算体积）\n\
         \x20 fix         替你把体检里能安全代做的改掉（默认只预演，--yes 才动手）\n\
         \x20 whatschanged 从上次看过算起，这个项目被改了哪些地方\n\
         \x20 run [参数]  按契约启动项目：6 阶段进度 + 实时输出 + 就绪判定\n\
         \x20 test        跑契约里声明的 test 命令\n\
         \x20 note \"...\"  记一句想法；-k 选类别（goal/idea/plan/blocker/learned/decision/note）\n\
         \x20 notes       看这个项目的工作记录\n\
         \x20 scan        清理清单：哪些能删、哪些要管理员、哪些别碰\n\
         \x20 clean       执行清理（默认只预演，加 --yes 才动手）\n\
         \x20 snap        记录一份环境快照，并和上一份对比\n\
         \x20 envs        列出这台机器上所有能用的 Python\n\
         \x20 web         Node / 前端：包管理器、依赖装到哪去了、版本要求写在哪\n\
         \x20 init        给已有项目补一份 env.contract.json\n\
         \n\
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
    );
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
