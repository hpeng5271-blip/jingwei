//! jingwei 命令行外壳：所有逻辑都在 engine 里，这里只负责排版和参数。

use jingwei_engine::clean::{self, Action, Zone};
use jingwei_engine::{
    changes, envsetup, fix, launch, notes, probe, project, rules, scaffold, snapshot, Contract,
    Severity,
};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static COLOR: AtomicBool = AtomicBool::new(false);
static MORE: AtomicBool = AtomicBool::new(false);

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



/// 查这台机器装了什么。不需要项目，单独可用——
/// 「这机子什么配置」本身就是个常见问题，不该逼人先打开一个项目。
fn cmd_spec(json: bool) -> i32 {
    let facts = jingwei_engine::machine::spec_report();
    let notes = jingwei_engine::machine::spec_judgements(&facts);
    jingwei_engine::dlog::i(
        "6000",
        &format!("spec_facts={} judgements={}", facts.len(), notes.len()),
    );
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({ "facts": facts, "judgements": notes })
            )
            .unwrap_or_default()
        );
        return 0;
    }
    println!();
    println!("  {}  ·  这台机器", c(BOLD, "jingwei"));
    println!();
    for f in &facts {
        // 多来源印证的标 ✓；单来源不标；有分歧的标 ! 并把两边说法都摆出来
        let mark = if !f.agree {
            c(YELLOW, "!")
        } else if f.sources.len() > 1 {
            c(GREEN, "✓")
        } else {
            " ".into()
        };
        println!("  {} {} {}", mark, pad(&f.label, 12), c(BOLD, &f.value));
        if let Some(n) = &f.note {
            println!("      {}", c(DIM, n));
        }
        if !f.agree {
            for d in &f.detail {
                println!("      {} {}", c(DIM, "来源之间不一致："), d);
            }
        }
    }
    println!();
    println!("  {}", c(DIM, "✓ = 两个独立来源印证过；空着 = 只有一个来源拿得到"));
    if !notes.is_empty() {
        println!();
        println!("  {}", c(BOLD, "从这些数据能看出来的"));
        for n in &notes {
            println!("    · {}", n);
        }
    }
    println!();
    0
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

// ------------------------------------------------------------------ doctor

/// 这些是「机器现在的状态」，跟具体某个项目无关。
/// 每次体检都各刷一行，看的人很快就麻木了——收进一行里，想看再加 --more。
const MACHINE_NOTES: &[&str] = &["console-encoding", "path-python-is-alias"];

fn cmd_doctor(root: &Path, json: bool, want_check: bool) -> i32 {
    let prev = snapshot::load_latest(root).map(|(_, f)| f);
    let facts = probe::make_facts(root, None);
    let findings = rules::run_all(&facts, prev.as_ref());
    jingwei_engine::dlog::i(
        "1300",
        &format!(
            "root={} lang={} findings={} contract={} ms={}",
            root.display(),
            facts.lang.name(),
            findings.len(),
            facts.project.contract.is_some(),
            0
        ),
    );

    // 传递依赖图：命令行也要能看到「这个东西一共拉进来多少个包」
    let (edges, total) = jingwei_engine::deps::rust_tree(root);
    // 真实编译器诊断（--check 才跑：第一次要编译依赖，可能久）
    // 和图形界面共用同一个入口（diag::check_all）：两条路各算各的，
    // 迟早会算得不一样——这个坑刚踩过一次，不再踩第二次。
    let (diags, diag_notes) = if want_check {
        jingwei_engine::diag::check_all(root, std::time::Duration::from_secs(300))
    } else {
        (Vec::new(), Vec::new())
    };

    if json {
        let out = serde_json::json!({
            "facts": facts,
            "findings": findings,
            "diagnostics": diags,
            "diagnostics_notes": diag_notes,
            "deps_tree": {
                "edges": edges.iter().map(|e| serde_json::json!({"from": e.from, "to": e.to})).collect::<Vec<_>>(),
                "total": total,
            }
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return has_error(&findings);
    }

    println!();
    println!(
        "  {}  {}  ·  环境体检",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    println!("  {} {}", c(DIM, "时间"), facts.taken_at);
    println!();
    println!("  {}", c(BOLD, "概览"));
    for (k, v) in snapshot::summary_lines(&facts) {
        println!("    {} {}", c(DIM, &pad(&k, 10)), v);
    }

    // 默认只展开「这个项目的事」；机器层面的提示折成一行
    let (notes, shown): (Vec<&rules::Finding>, Vec<&rules::Finding>) = findings
        .iter()
        .partition(|f| MACHINE_NOTES.contains(&f.id.as_str()));
    // 项目卫生是另一条线：它不影响「跑不跑得起来」，所以默认也折起来，
    // 想看细节走 jingwei audit——那边的排版就是按它分组的。
    let (hygiene, shown): (Vec<&rules::Finding>, Vec<&rules::Finding>) =
        shown.into_iter().partition(|f| !f.group.is_empty());
    let shown = if MORE.load(Ordering::Relaxed) {
        findings.iter().collect()
    } else {
        shown
    };

    // 项目卫生不数进这一行：它有自己的折叠行，混在一起会让人以为
    // 「共 18 条」里有一半是挡住运行的问题。
    let chain: Vec<&rules::Finding> = findings.iter().filter(|f| f.group.is_empty()).collect();
    let (mut e, mut w, mut info) = (0, 0, 0);
    for f in &chain {
        match f.severity {
            Severity::Error => e += 1,
            Severity::Warn => w += 1,
            Severity::Info => info += 1,
        }
    }
    println!();
    if chain.is_empty() && hygiene.is_empty() {
        println!("  {} 没有发现问题。", c(GREEN, "✓"));
        println!(
            "    {}",
            c(
                DIM,
                "感觉一样要看一眼「概览」：环境类型和依赖是不是你以为的那样。"
            )
        );
        return 0;
    }

    println!(
        "  {} 共 {} 条（{} 错误 / {} 警告 / {} 提示）",
        c(BOLD, "问题"),
        chain.len(),
        e,
        w,
        info
    );
    for f in shown.iter() {
        let (mark, col) = match f.severity {
            Severity::Error => ("✗", RED),
            Severity::Warn => ("!", YELLOW),
            Severity::Info => ("i", CYAN),
        };
        println!();
        println!("  {} {} {}", c(col, mark), c(BOLD, &f.title), c(DIM, &f.id));
        if !f.evidence.is_empty() {
            println!("      {}", c(DIM, "证据"));
            for x in &f.evidence {
                println!("        · {}", x);
            }
        }
        if !f.verify.is_empty() {
            println!("      {}", c(DIM, "自己验证"));
            for x in &f.verify {
                println!("        $ {}", x);
            }
        }
        if !f.boundary.is_empty() {
            println!("      {} {}", c(DIM, "边界"), f.boundary);
        }
        if !f.next.is_empty() {
            println!("      {} {}", c(DIM, "下一步"), f.next);
        }
        if MORE.load(Ordering::Relaxed) && !f.excluded.is_empty() {
            println!("      {}", c(DIM, "可以排除的"));
            for x in &f.excluded {
                println!("        · {}", x);
            }
        }
    }
    println!();
    if !notes.is_empty() && !MORE.load(Ordering::Relaxed) {
        let ids: Vec<String> = notes
            .iter()
            .map(|f| format!("{}（{}）", f.title, f.id))
            .collect();
        println!(
            "  {} {} 条机器层面的提示，和这个项目无关：{}",
            c(DIM, "·"),
            notes.len(),
            ids.join("；")
        );
        println!("    {}", c(DIM, "想看细节加 --more"));
        println!();
    }
    if !hygiene.is_empty() && !MORE.load(Ordering::Relaxed) {
        let actionable = hygiene
            .iter()
            .filter(|f| f.severity != Severity::Info)
            .count();
        println!(
            "  {} {} 条项目卫生提示（忽略规则、锁文件、体积），其中 {} 条值得处理：jingwei audit 看全部",
            c(DIM, "·"),
            hygiene.len(),
            actionable
        );
        println!();
    }
    println!(
        "  {} 先处理错误，再看警告；提示不用管。",
        c(DIM, "顺序")
    );
    if e > 0 {
        1
    } else {
        0
    }
}

fn has_error(findings: &[rules::Finding]) -> i32 {
    if findings.iter().any(|f| f.severity == Severity::Error) {
        1
    } else {
        0
    }
}

// ------------------------------------------------------------------ audit

/// 一条结论的排版。顺序是刻意的：先给看到的事实，再给能自己验的办法，
/// 然后才是边界和建议。反过来写（先建议）会让人跳过证据直接照做。
fn print_finding_block(f: &rules::Finding) {
    let (mark, col) = match f.severity {
        Severity::Error => ("×", RED),
        Severity::Warn => ("!", YELLOW),
        Severity::Info => ("i", CYAN),
    };
    println!();
    println!("  {} {} {}", c(col, mark), c(BOLD, &f.title), c(DIM, &f.id));
    let block = |label: &str, lines: &[String]| {
        if lines.is_empty() {
            return;
        }
        println!("      {}", c(DIM, label));
        for x in lines {
            for (i, l) in x.lines().enumerate() {
                if i == 0 {
                    println!("        · {}", l);
                } else {
                    println!("          {}", l);
                }
            }
        }
    };
    block("证据", &f.evidence);
    if !f.verify.is_empty() {
        println!("      {}", c(DIM, "自己验证"));
        for x in &f.verify {
            println!("        $ {}", x);
        }
    }
    if !f.boundary.is_empty() {
        println!("      {} {}", c(DIM, "什么时候不用管"), f.boundary);
    }
    if !f.next.is_empty() {
        println!("      {}", c(DIM, "下一步"));
        for (i, l) in f.next.lines().enumerate() {
            if i == 0 {
                println!("        · {}", l);
            } else {
                println!("          {}", l);
            }
        }
    }
}

/// 项目体检：和语言无关的那一层——哪些文件不该进仓库、依赖记在哪、
/// 这个目录有多大。doctor 回答「跑不起来为什么」，它回答「跑得起来之后」。
fn cmd_audit(root: &Path, json: bool, deep: bool) -> i32 {
    if !root.is_dir() {
        eprintln!("这个目录不存在：{}", root.display());
        return 2;
    }
    let s = if deep {
        project::scan_deep(root)
    } else {
        project::scan(root)
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
        return i32::from(
            s.items
                .iter()
                .any(|f| f.severity == Severity::Error),
        );
    }

    let (e, w, info) = (
        s.items.iter().filter(|f| f.severity == Severity::Error).count(),
        s.items.iter().filter(|f| f.severity == Severity::Warn).count(),
        s.items.iter().filter(|f| f.severity == Severity::Info).count(),
    );

    println!();
    println!(
        "  {}  {}  ·  项目体检",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    // 细节先拼好再打印：把 format! 嵌进 println! 的参数里既难读，
    // 也是 clippy 明确会报的写法
    let scan_detail = format!(
        "{} 个文件{}{}{}",
        s.files_scanned,
        if s.langs.is_empty() {
            String::new()
        } else {
            format!("  语言 {}", s.langs.join(" / "))
        },
        if s.has_git { "  Git 仓库" } else { "  还不是 Git 仓库" },
        if deep { "  含体积统计" } else { "  未算体积（--deep）" }
    );
    println!("  {} {}", c(DIM, "扫描"), scan_detail);

    if !s.manifests.is_empty() {
        println!();
        println!("  {}", c(BOLD, "依赖清单"));
        for m in &s.manifests {
            println!(
                "    {} {:<6} {}",
                pad(&m.path, 22),
                c(DIM, &m.kind),
                if m.note.is_empty() { "（没读出内容）" } else { &m.note }
            );
        }
        if !s.lockfiles.is_empty() {
            println!(
                "    {} {}",
                c(DIM, "锁文件"),
                s.lockfiles.join("、")
            );
        }
    }

    if s.items.is_empty() {
        println!();
        println!("  {} 没有发现要处理的问题。", c(GREEN, "✓"));
        println!(
            "    {}",
            c(
                DIM,
                "看过的：忽略规则、依赖清单和锁文件、IDE 配置、路径长度、垃圾文件。"
            )
        );
        println!();
        return 0;
    }

    println!();
    println!(
        "  {} 共 {} 条（{} 要处理 / {} 注意 / {} 提示）",
        c(BOLD, "体检结果"),
        s.items.len(),
        e,
        w,
        info
    );
    for g in [
        project::G_IGNORE,
        project::G_DEPS,
        project::G_HABIT,
        project::G_SIZE,
    ] {
        let group: Vec<&rules::Finding> = s.items.iter().filter(|f| f.group == g).collect();
        if group.is_empty() {
            continue;
        }
        println!();
        println!("  {}  {}", c(BOLD, &format!("— {} —", g)), c(DIM, &format!("{} 条", group.len())));
        for f in group {
            print_finding_block(f);
        }
    }
    println!();
    if !deep {
        println!(
            "  {}",
            c(DIM, "体积和大文件要加 --deep 才算；平时不用。")
        );
    }
    println!("  {}", c(DIM, "先处理「×」，再看「!」，「i」放着不管也不会出事。"));
    if e > 0 {
        1
    } else {
        0
    }
}

// ------------------------------------------------------------------ snap

/// 这个项目刚才变了什么。报一次就记账，不重复念同一批改动。
fn cmd_changes(root: &Path, json: bool) -> i32 {
    if !root.is_dir() {
        eprintln!("这个目录不存在：{}", root.display());
        return 2;
    }
    let d = match changes::take(root) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("读不动项目状态：{}", e);
            return 1;
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&d).unwrap_or_default());
        return 0;
    }
    println!();
    println!(
        "  {}  {}  ·  这个项目变了什么",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    if !d.first_run && !d.from.is_empty() {
        println!("  {} 从 {} 算起", c(DIM, "对比"), d.from);
    }
    println!();
    println!("  {}", c(BOLD, &d.headline));
    for l in &d.lines {
        println!("    {} {}", c(DIM, "·"), l);
    }
    if d.lines.is_empty() {
        println!(
            "    {}",
            c(DIM, "文件没动过：源码、依赖清单、配置都和上次一样。")
        );
    }
    println!();
    if d.first_run {
        println!("  {}", c(DIM, "这是在记账，不是发现问题。下次再看就有对比了。"));
    } else {
        println!(
            "  {}",
            c(DIM, "改动列表在 .jingwei/watch.json，和笔记放一起；报过一次就不再重复。")
        );
    }
    0
}

/// 体检里能安全代做的那几件事。默认只预演——改别人仓库里的文件，
/// 先让他看清要加哪几行，比事后解释便宜得多。
fn cmd_fix(root: &Path, json: bool, yes: bool, only: &[String]) -> i32 {
    if !root.is_dir() {
        eprintln!("这个目录不存在：{}", root.display());
        return 2;
    }
    let s = project::scan(root);
    let plan = fix::plan(&s);

    if json {
        let out = if yes {
            match fix::apply(root, &s, only) {
                Ok(r) => serde_json::json!({ "dryRun": false, "report": r }),
                Err(e) => serde_json::json!({ "dryRun": false, "error": e.to_string() }),
            }
        } else {
            serde_json::json!({ "dryRun": true, "plan": plan })
        };
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    let wanted: Vec<fix::Fix> = if only.is_empty() {
        plan.fixes.clone()
    } else {
        plan.fixes
            .iter()
            .filter(|f| only.contains(&f.id))
            .cloned()
            .collect()
    };

    println!();
    println!(
        "  {}  {}  ·  {}",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION"),
        if yes { "动手改" } else { "预演" }
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    println!();

    if wanted.is_empty() {
        println!("  {} 没有工具该替你改的东西。", c(GREEN, "✓"));
    } else {
        println!("  {}", c(BOLD, "会做的事"));
        for f in &wanted {
            println!();
            println!("  {} {}", c(BOLD, &f.title), c(DIM, &f.id));
            println!("      改这个文件  {}", f.target);
            for l in &f.lines {
                println!("        + {}", l);
            }
            println!("      {} {}", c(DIM, "说明"), f.note);
        }
    }
    if !plan.skipped.is_empty() {
        println!();
        println!(
            "  {}",
            c(
                BOLD,
                "看得见但不代做的（工具不该越过你的决定）"
            )
        );
        for sk in &plan.skipped {
            if !only.is_empty() && !only.contains(&sk.id) {
                continue;
            }
            println!("    {} {}", c(DIM, &sk.id), sk.title);
            println!("        {}", c(DIM, &sk.reason));
        }
    }

    if !yes {
        println!();
        println!(
            "  {}",
            c(DIM, "这是预演。确认无误再加 --yes 动手；只想做其中一条用 --only <id>。")
        );
        return 0;
    }

    match fix::apply(root, &s, only) {
        Ok(r) => {
            println!();
            if r.nothing_to_do {
                println!("  {} 没有需要新增的行，文件没动。", c(GREEN, "✓"));
                return 0;
            }
            for l in &r.done {
                println!("  {} {}", c(GREEN, "✓"), l);
            }
            println!();
            println!(
                "  {}",
                c(DIM, "改动只在工作区里，git diff 能看到；不满意就 git checkout 那两个文件。")
            );
            0
        }
        Err(e) => {
            eprintln!("  改不动：{}", e);
            1
        }
    }
}

fn cmd_snap(root: &Path, json: bool, diff_only: bool) -> i32 {
    if diff_only {
        let Some((_, new)) = snapshot::load_latest(root) else {
            eprintln!("  还没有任何快照；先跑一次 jingwei snap");
            return 1;
        };
        let Some((_, old)) = snapshot::load_previous(root) else {
            eprintln!("  只有一份快照，没有可对比的对象");
            return 1;
        };
        let changes = snapshot::diff(&old, &new);
        if json {
            println!("{}", serde_json::to_string_pretty(&changes).unwrap_or_default());
            return 0;
        }
        println!(
            "\n  {}  {} → {}\n",
            c(BOLD, "环境变化"),
            old.taken_at,
            new.taken_at
        );
        if changes.is_empty() {
            println!("  {} 两处一模一样。", c(GREEN, "✓"));
        } else {
            for ch in &changes {
                println!("    · {}", ch.describe());
            }
        }
        println!();
        return 0;
    }

    let prev = snapshot::load_latest(root).map(|(_, f)| f);
    let facts = probe::make_facts(root, None);
    let saved = match snapshot::save(root, &facts) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("  保存快照失败：{}", e);
            return 1;
        }
    };
    let changes = prev
        .as_ref()
        .map(|p| snapshot::diff(p, &facts))
        .unwrap_or_default();

    if json {
        println!("{}", serde_json::to_string_pretty(&facts).unwrap_or_default());
        return 0;
    }

    println!("\n  {} {}", c(BOLD, "已记录快照"), saved.display());
    println!("  {}", c(DIM, "只记环境事实，不记 PATH 条目本身"));
    println!();
    for (k, v) in snapshot::summary_lines(&facts) {
        println!("    {} {}", c(DIM, &pad(&k, 10)), v);
    }
    println!();
    if prev.is_none() {
        println!("  {} 这是第一份，下次再记一份就能看差异了。", c(DIM, "·"));
    } else if changes.is_empty() {
        println!("  {} 和上一份完全相同。", c(GREEN, "✓"));
    } else {
        println!("  {} 和上一份比，变了 {} 处：", c(BOLD, "差异"), changes.len());
        let n = if MORE.load(Ordering::Relaxed) {
            changes.len()
        } else {
            changes.len().min(8)
        };
        for ch in changes.iter().take(n) {
            println!("    · {}", ch.describe());
        }
        if n < changes.len() {
            println!("    {} 还有 {} 条，加 --more 看全", c(DIM, "…"), changes.len() - n);
        }
    }
    println!();
    0
}

// ------------------------------------------------------------------ run

/// 跑完之后的「内存」一段。这是系统资源层唯一的出口，
/// 所以它只讲事实（峰值多少、有没有换页、当时谁占着），不讲建议。
fn print_memory(m: &jingwei_engine::mem::MemReport) {
    println!();
    // 采样跟不上就直说采不准。把「没采到」报成「它就是 0」，
    // 是最不该出现的那种错——它看起来像个结论。
    if !m.reliable {
        println!(
            "  {} 进程只活了 {} ms，比采样间隔还短，这次的内存数字不可信。",
            c(DIM, "内存"),
            m.duration_ms
        );
        println!(
            "      {}",
            c(DIM, "（工作集可能只是它刚启动时的空壳；想量得准，让程序至少跑半秒）")
        );
        return;
    }
    println!(
        "  {} 进程峰值 {} MB（私有提交 {} MB）· 当时系统 {:.1}/{:.1} GB",
        c(BOLD, "内存"),
        m.peak_working_mb,
        m.peak_commit_mb,
        m.sys_commit_used_mb as f64 / 1024.0,
        m.phys_total_mb as f64 / 1024.0
    );
    if m.paging {
        println!(
            "      {} 峰值时系统的已提交内存已经压过物理内存，多出来的部分被挪到了 pagefile\
             ——编译这类负载一进磁盘就慢一个数量级。这就是它慢的原因。",
            c(YELLOW, "换页")
        );
    } else if m.peak_working_mb > 0 && m.phys_avail_at_peak_mb < m.phys_total_mb / 8 {
        println!(
            "      {} 峰值时物理内存只剩 {} MB，已经贴着边了。这次还没换页，再加点东西就会。",
            c(YELLOW, "贴边"),
            m.phys_avail_at_peak_mb
        );
    }
    if m.page_faults > 0 {
        // 缺页速率才是能比较的量。写成「平均每 N 毫秒一次」在短任务上
        // 会算出 0——那种数字看着像 bug，其实是除法取整。
        let secs = (m.duration_ms as f64 / 1000.0).max(0.001);
        let per_sec = (m.page_faults as f64 / secs) as u64;
        println!(
            "      {}",
            c(
                DIM,
                &format!(
                    "累计缺页 {} 次，约每秒 {} 次。缺页多不等于有问题——它只说明程序一直在向系统要内存",
                    m.page_faults, per_sec
                )
            )
        );
    }
    if !m.others.is_empty() {
        let top: Vec<String> = m
            .others
            .iter()
            .take(5)
            .map(|p| format!("{} {}", p.name, jingwei_engine::clean::human_size(p.working_mb * 1048576)))
            .collect();
        println!("      当时还占着：{}", c(DIM, &top.join(" · ")));
    }
}

fn cmd_run(root: &Path, run_override: Option<String>, extra: Vec<String>) -> i32 {
    let opts = launch::LaunchOptions {
        root: root.to_path_buf(),
        run_override,
        extra_args: extra,
        timeout: Duration::from_secs(30),
        purpose: launch::Purpose::Run,
    };
    println!();
    let mut cb = |ev: launch::Event| match ev {
        launch::Event::Stage {
            index,
            total,
            title,
            status,
            detail,
            ms,
        } => {
            let (mark, word, col) = match status {
                launch::StageStatus::Ok => ("✓", "通过", GREEN),
                launch::StageStatus::Note => ("!", "注意", YELLOW),
                launch::StageStatus::Fail => ("✗", "失败", RED),
            };
            let head = format!("[{}/{}] {}", index, total, title);
            println!(
                "  {} {} {}{}",
                c(DIM, &pad(&head, 44)),
                c(col, &format!("{} {}", mark, word)),
                c(DIM, "· "),
                c(DIM, &format!("{}ms", ms))
            );
            if !detail.is_empty() {
                for line in wrap(&detail, 88) {
                    println!("        {}", c(DIM, &line));
                }
            }
        }
        launch::Event::Line { source, text } => {
            let (pre, col) = match source {
                launch::Source::Stdout => ("│", ""),
                launch::Source::Stderr => ("!", RED),
                launch::Source::Log => ("·", CYAN),
            };
            println!(
                "  {} {}",
                c(if col.is_empty() { DIM } else { col }, pre),
                c(col, &text)
            );
        }
        launch::Event::Ready { how, ms } => {
            println!();
            println!(
                "  {} {} {}",
                c(GREEN, "✓ 已就绪"),
                how,
                c(DIM, &format!("({}ms)", ms))
            );
            println!("  {}", c(DIM, "下面的输出会一直跟着；按 Ctrl+C 结束。"));
            println!();
        }
        launch::Event::Exited { code } => {
            println!();
            if code == 0 {
                println!("  {} 进程已退出（退出码 0）", c(GREEN, "✓"));
            } else {
                println!(
                    "  {} 进程已退出（退出码 {}）",
                    c(YELLOW, "!"),
                    code
                );
            }
        }
    };

    match launch::launch(&opts, &mut cb) {
        Ok(out) => {
            if out.ready.is_none() {
                println!();
                if out.exit_code != 0 {
                    // 进程自己挂了：病因在它打印的话里，跟 ready_when 没关系。
                    println!(
                        "  {} 进程异常退出（退出码 {}）",
                        c(RED, "✗"),
                        out.exit_code
                    );
                    match &out.diagnosis {
                        Some(d) => {
                            println!("  {} {}", c(BOLD, "症结"), d.crux);
                            println!("  {} {}", c(DIM, "下一步"), d.hint);
                        }
                        None => println!(
                            "  {}",
                            c(
                                DIM,
                                "真正的报错在上面标 ! 的行里——那是程序自己说的话，重点在那儿。"
                            )
                        ),
                    }
                } else {
                    // 进程好好结束了，只是契约没说清「什么算成功」
                    println!(
                        "  {} 进程正常结束了，但契约里没写清「什么算成功」，工具说不准这次成不成。",
                        c(YELLOW, "!")
                    );
                    println!(
                        "  {} 在 env.contract.json 的 ready_when 里声明：常驻服务写 {{\"port\": 端口}}，\
                         命令行工具写 {{\"log_match\": \"成功时才会出现的那句话\"}}",
                        c(DIM, "下一步"),
                    );
                }
            }
            if let Some(m) = &out.memory {
                print_memory(m);
                if !out.memory_delta.is_empty() {
                    println!("      {}", c(BOLD, "和上次比："));
                    for d in &out.memory_delta {
                        println!("        · {}", d);
                    }
                }
                println!();
            }
            if out.exit_code < 0 {
                1
            } else {
                out.exit_code
            }
        }
        Err(e) => {
            println!();
            println!("  {} {}", c(RED, "✗ 启动失败"), e);
            println!();
            1
        }
    }
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

fn cmd_scan(root: &Path, json: bool) -> i32 {
    let items = clean::scan(root);
    if json {
        println!("{}", serde_json::to_string_pretty(&items).unwrap_or_default());
        return 0;
    }
    render_scan(&items);
    0
}

/// 清单的阅读顺序：先动你自己的项目，再动工具缓存，最后才是系统。
const CAT_ORDER: &[&str] = &[
    "项目产物",
    "Python 缓存",
    "Node 缓存",
    "Go 缓存",
    "Rust 缓存",
    "JVM 缓存",
    "VS Code 缓存",
    "浏览器缓存",
    "系统",
];

fn cat_rank(name: &str) -> usize {
    CAT_ORDER
        .iter()
        .position(|x| *x == name)
        .unwrap_or(CAT_ORDER.len())
}

fn render_scan(items: &[clean::Item]) {
    let (a_bytes, a_n) = clean::total(items, Zone::A);
    let rec: Vec<&clean::Item> = items
        .iter()
        .filter(|i| i.zone == Zone::A && i.action == Action::Delete && i.recommended)
        .collect();
    let (rec_bytes, rec_n) = (
        rec.iter().map(|i| i.size_bytes).sum::<u64>(),
        rec.len(),
    );
    println!();
    println!("  {}  ·  清理清单", c(BOLD, "jingwei"));
    println!();
    println!("  {}", c(GREEN, "A 区 · 可清理（工具代删，默认走回收站）"));
    println!(
        "       {}",
        c(
            DIM,
            &format!(
                "可清 {} / {} 项；其中标 [推荐] 的 {} / {} 项",
                clean::human_size(a_bytes),
                a_n,
                clean::human_size(rec_bytes),
                rec_n
            )
        )
    );
    let mut sorted: Vec<&clean::Item> = items.iter().filter(|i| i.zone == Zone::A).collect();
    sorted.sort_by(|x, y| {
        (cat_rank(&x.category), std::cmp::Reverse(x.size_bytes))
            .cmp(&(cat_rank(&y.category), std::cmp::Reverse(y.size_bytes)))
    });
    let mut shown_cat = String::new();
    for it in sorted {
        if it.category != shown_cat {
            shown_cat = it.category.clone();
            println!("    {}", c(DIM, &format!("— {} —", shown_cat)));
        }
        let size = if it.size_bytes > 0 {
            format!("{:>10}", clean::human_size(it.size_bytes))
        } else {
            " ".repeat(10)
        };
        let tag = if it.action == Action::Command {
            c(CYAN, "[命令]")
        } else if it.recommended {
            c(GREEN, "[推荐]")
        } else {
            c(YELLOW, "[斟酌]")
        };
        println!("      {} {} {}", tag, pad(&it.label, 30), c(BOLD, &size));
        println!("        {}", c(DIM, &it.path));
        if !it.reason.is_empty() {
            println!("        可删 {}", it.reason);
        }
        println!("        代价 {}", it.consequence);
        if let Some(cmd) = &it.command {
            println!("        {} {}", c(CYAN, "命令"), c(BOLD, cmd));
        }
    }

    let b: Vec<&clean::Item> = items.iter().filter(|i| i.zone == Zone::B).collect();
    println!();
    println!("  {}", c(BOLD, "B 区 · 需要管理员（工具不代跑，只给你命令）"));
    for it in b {
        let size = if it.size_bytes > 0 {
            format!("  {}", clean::human_size(it.size_bytes))
        } else {
            String::new()
        };
        println!("    {}{}", c(BOLD, &it.label), c(DIM, &size));
        println!("      {}", c(DIM, &it.path));
        println!("      原因 {}", it.reason);
        println!("      代价 {}", it.consequence);
        if let Some(cmd) = &it.command {
            println!("      {} {}", c(CYAN, "命令"), c(BOLD, cmd));
        }
    }

    println!();
    println!("  {}", c(BOLD, "C 区 · 不要碰（这里只解释原因）"));
    for it in items.iter().filter(|i| i.zone == Zone::C) {
        println!("    {} {}", c(RED, "×"), c(BOLD, &it.label));
        println!("      {}", c(DIM, &it.path));
        for line in wrap(&it.consequence, 84) {
            println!("      {}", line);
        }
    }
    println!();
    println!("  {}", c(DIM, "A 区不是「都该删」：标 [斟酌] 的自己看一眼再决定。"));
    println!();
}

// ------------------------------------------------------------------ clean

fn cmd_clean(
    root: &Path,
    json: bool,
    yes: bool,
    permanent: bool,
    recommended_only: bool,
    only: &[String],
) -> i32 {
    let items = clean::scan(root);
    let selected: Vec<clean::Item> = items
        .iter()
        .filter(|i| i.action == Action::Delete)
        .filter(|i| !recommended_only || i.recommended)
        .filter(|i| {
            only.is_empty()
                || only.iter().any(|k| {
                    i.label.to_lowercase().contains(k)
                        || i.path.to_lowercase().contains(k)
                        || i.category.to_lowercase().contains(k)
                })
        })
        .cloned()
        .collect();

    let bytes: u64 = selected.iter().map(|i| i.size_bytes).sum();
    let files: usize = selected.iter().map(|i| i.files).sum();

    if !yes {
        println!();
        println!("  {}  ·  预演（什么都没删）", c(BOLD, "jingwei"));
        println!();
        for it in &selected {
            let tag = if it.recommended {
                c(GREEN, "[推荐]")
            } else {
                c(YELLOW, "[斟酌]")
            };
            println!(
                "    {} {} {}",
                tag,
                pad(&it.label, 30),
                c(BOLD, &clean::human_size(it.size_bytes))
            );
            println!("      {}", c(DIM, &it.path));
        }
        println!();
        println!(
            "  将要清理 {} 项 / {} 个文件 / {}",
            selected.len(),
            files,
            c(BOLD, &clean::human_size(bytes))
        );
        println!(
            "  {}",
            c(
                DIM,
                "上面这些都没删。确认后加 --yes 执行。\n    只清标了 [推荐] 的那几项：再加 --recommended。\n    默认进回收站（能还原）；想直接抹掉就加 --permanent。"
            )
        );
        println!();
        return 0;
    }

    let rep = clean::delete(&selected, permanent);
    if json {
        let out = serde_json::json!({
            "freed": rep.freed,
            "removed": rep.removed,
            "failed": rep.failed,
            "skipped": rep.skipped,
            "permanent": permanent,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return if rep.failed.is_empty() { 0 } else { 1 };
    }

    println!();
    println!(
        "  {} 清了 {} 项，释放 {}",
        c(GREEN, "✓"),
        rep.removed,
        c(BOLD, &clean::human_size(rep.freed))
    );
    println!(
        "  {}",
        if permanent {
            c(DIM, "永久删除，不可还原。")
        } else {
            c(DIM, "删掉的东西都在回收站里，可以还原。")
        }
    );
    if !rep.failed.is_empty() {
        println!();
        println!("  {} 有 {} 项没删掉：", c(YELLOW, "!"), rep.failed.len());
        for x in rep.failed.iter().take(12) {
            println!("    · {}", x);
        }
        println!(
            "      {}",
            c(DIM, "多半是被程序占用（关掉 VS Code / 浏览器 / 正在跑的 Python 再试）。")
        );
    }
    println!();
    if rep.failed.is_empty() {
        0
    } else {
        1
    }
}

// ------------------------------------------------------------------ envs

fn cmd_envs(root: &Path, json: bool) -> i32 {
    let (contract, _) = Contract::load(root);
    let list = probe::find_pythons(root, contract.as_ref(), true);
    if json {
        let rows: Vec<serde_json::Value> = list
            .iter()
            .map(|p| {
                serde_json::json!({
                    "path": p.path.to_string_lossy(),
                    "source": p.source,
                    "version": p.version,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return 0;
    }
    println!();
    if list.is_empty() {
        println!("  {} 一个可用的 Python 都没有。", c(RED, "✗"));
        println!("    {}", c(DIM, "装法：装 uv 后跑 uv python install 3.12，或从 python.org 装。"));
        println!();
        return 1;
    }
    println!("  {} 找到 {} 个：", c(BOLD, "Python"), list.len());
    println!();
    for p in &list {
        println!(
            "    {}  {}  {}",
            c(BOLD, &pad(p.version.clone().unwrap_or_else(|| "?".into()).as_str(), 10)),
            p.path.display(),
            c(DIM, &format!("[{}]", p.source))
        );
    }
    println!();
    println!(
        "  {}",
        c(
            DIM,
            "工具用第一个能真正跑起来的。\n  上面那些转发壳（应用商店别名、py 的转发程序）即使能跑也排在最后——\n  它们指向的解释器会被系统改掉，写进契约就是给以后埋雷。"
        )
    );
    println!();
    0
}

// ------------------------------------------------------------------ init

fn cmd_init(root: &Path, json: bool) -> i32 {
    if !root.is_dir() {
        eprintln!("  目录不存在：{}", root.display());
        return 1;
    }
    let existing = Contract::load(root);
    if existing.0.is_some() && !json {
        println!();
        println!(
            "  {} 已经有 env.contract.json 了，这次不覆盖。",
            c(YELLOW, "!")
        );
        println!("    先备份再删掉它，然后重新 init。");
        println!();
        return 1;
    }

    let py = probe::find_pythons(root, None, false);
    let first = py.first().map(|p| p.path.to_path_buf());
    let deps = Contract::declared_deps(root);
    let has_req = root.join("requirements.txt").is_file();
    let run = guess_run(root);

    let ct = Contract {
        schema_version: Some(1),
        name: Some(
            root.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名项目".into()),
        ),
        description: None,
        runtime: jingwei_engine::contract::Runtime {
            python: Some(">=3.10".into()),
            python_exe: first.as_ref().map(|p| p.to_string_lossy().to_string()),
        },
        deps: jingwei_engine::contract::Deps {
            required: deps,
            optional: Vec::new(),
        },
        commands: jingwei_engine::contract::Commands {
            install: if has_req {
                Some("python -m pip install -r requirements.txt".into())
            } else {
                None
            },
            build: Some("python -m compileall -q .".into()),
            run,
            test: None,
        },
        log_files: Vec::new(),
        ready_when: Some(jingwei_engine::contract::ReadyWhen {
            port: None,
            log_match: None,
            exit_code: Some(0),
            timeout_s: Some(60),
        }),
        manual: Vec::new(),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&ct).unwrap_or_default());
        return 0;
    }

    println!();
    match ct.save(root) {
        Ok(_) => println!("  {} 已生成 env.contract.json", c(GREEN, "✓")),
        Err(e) => {
            eprintln!("  写入失败：{}", e);
            return 1;
        }
    }
    println!("    {}", c(DIM, &root.join("env.contract.json").display().to_string()));
    println!();
    println!("  {} 你大概要改这几处：", c(BOLD, "下一步"));
    if ct.commands.run.is_none() {
        println!("    · run   还没猜出来，填成你平时敲的那句，例如 \"python main.py\"");
    }
    println!("    · ready_when  现在写的是「退出码 0」。如果它是常驻服务，改成 {{\"port\": 8000}} 或 {{\"log_match\": \"已监听\"}}");
    println!("    · deps.required  现在从 requirements.txt 读，没写的自己补");
    println!();
    println!("  {}", c(DIM, "改完跑 jingwei doctor 确认。"));
    println!();
    0
}

fn guess_run(root: &Path) -> Option<String> {
    let dir_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let cands = [
        "main.py",
        "app.py",
        "run.py",
        "cli.py",
        "start.py",
        "manage.py",
        "server.py",
    ];
    for n in cands {
        if root.join(n).is_file() {
            return Some(format!("python {}", n));
        }
    }
    let nested = format!("{}.py", dir_name);
    if root.join(&nested).is_file() {
        return Some(format!("python {}", nested));
    }
    // 退化：src 下唯一的 py
    let src = root.join("src");
    if let Ok(rd) = std::fs::read_dir(&src) {
        let mut hits: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".py") && !n.starts_with("__"))
            .collect();
        hits.sort();
        if hits.len() == 1 {
            return Some(format!("python src/{}", hits[0]));
        }
    }
    None
}

// ------------------------------------------------------------------ 控制台

// ------------------------------------------------------------------ 任务 / new / env / notes

/// 装依赖、跑测试这类「跑完就结束」的动作：同一套 6 阶段进度，措辞换掉。
fn run_task(root: &Path, command: String, extra: Vec<String>) -> i32 {
    let opts = launch::LaunchOptions {
        root: root.to_path_buf(),
        run_override: Some(command),
        extra_args: extra,
        timeout: Duration::from_secs(30),
        purpose: launch::Purpose::Task,
    };
    println!();
    let mut cb = |ev: launch::Event| match ev {
        launch::Event::Stage {
            index,
            total,
            title,
            status,
            detail,
            ms,
        } => {
            let (mark, word, col) = match status {
                launch::StageStatus::Ok => ("✓", "通过", GREEN),
                launch::StageStatus::Note => ("!", "注意", YELLOW),
                launch::StageStatus::Fail => ("✗", "失败", RED),
            };
            let head = format!("[{}/{}] {}", index, total, title);
            println!(
                "  {} {} {}{}",
                c(DIM, &pad(&head, 44)),
                c(col, &format!("{} {}", mark, word)),
                c(DIM, "· "),
                c(DIM, &format!("{}ms", ms))
            );
            if !detail.is_empty() {
                for line in wrap(&detail, 88) {
                    println!("        {}", c(DIM, &line));
                }
            }
        }
        launch::Event::Line { source, text } => {
            let (pre, col) = match source {
                launch::Source::Stdout => ("│", ""),
                launch::Source::Stderr => ("!", RED),
                launch::Source::Log => ("·", CYAN),
            };
            println!(
                "  {} {}",
                c(if col.is_empty() { DIM } else { col }, pre),
                c(col, &text)
            );
        }
        launch::Event::Ready { how, ms } => {
            println!("\n  {} {} {}", c(GREEN, "✓ 完成"), how, c(DIM, &format!("({}ms)", ms)));
        }
        launch::Event::Exited { code } => {
            println!();
            if code == 0 {
                println!("  {} 全部结束（退出码 0）", c(GREEN, "✓"));
            } else {
                println!("  {} 命令没跑成功（退出码 {}）", c(RED, "✗"), code);
                println!(
                    "    {}",
                    c(DIM, "看上面标 ! 的行：那是程序自己打印的原始报错，重点在那儿。")
                );
            }
        }
    };
    match launch::launch(&opts, &mut cb) {
        Ok(out) => {
            if out.exit_code < 0 {
                1
            } else {
                out.exit_code
            }
        }
        Err(e) => {
            println!("\n  {} {}\n", c(RED, "✗ 没跑起来"), e);
            1
        }
    }
}

fn cmd_task(root: &Path, which: &str, override_cmd: Option<String>) -> i32 {
    let (contract, err) = Contract::load(root);
    if let Some(e) = err {
        println!("\n  {} 契约读不了：{}\n", c(RED, "✗"), e);
        return 1;
    }
    let cmds = contract.as_ref().map(|x| x.commands.clone()).unwrap_or_default();
    let command = override_cmd.or_else(|| match which {
        "test" => cmds.test.clone(),
        "install" => cmds.install.clone(),
        _ => None,
    });
    let Some(command) = command else {
        println!();
        println!("  {} 契约里没有声明 {} 命令。", c(YELLOW, "!"), which);
        println!(
            "    {}",
            c(
                DIM,
                "在 env.contract.json 的 commands 里补一条，或直接：jingwei run --run \"<你的命令>\""
            )
        );
        println!();
        return 1;
    };
    run_task(root, command, Vec::new())
}

#[allow(clippy::too_many_arguments)]
fn cmd_new(
    root: &Path,
    json: bool,
    rest: Vec<String>,
    template: Option<String>,
    parent_dir: Option<PathBuf>,
    python_exe: Option<PathBuf>,
    want_venv: bool,
    want_git: bool,
    purpose: Option<String>,
    lang_name: Option<String>,
) -> i32 {
    let Some(raw) = rest.first() else {
        println!();
        println!("  用法： jingwei new <名字> [选项]");
        println!("    --template <模板>   {}", scaffold::template_help().replace('\n', "\n    "));
        println!("    --dir <父目录>      建在哪儿（默认当前目录）");
        println!("    --python <exe>      用哪个解释器");
        println!("    --purpose <一句>    为什么做它（会记成第一条记录）");
        println!("    --no-venv           不建虚拟环境");
        println!("    --git               顺手 git init");
        println!();
        return 2;
    };
    let name = scaffold::safe_name(raw);
    if name != *raw {
        println!("\n  {} 项目名换成了 {}（它要当包名用）\n", c(DIM, "·"), c(BOLD, &name));
    }
    let tpl = match template.as_deref() {
        None => scaffold::Template::Package,
        Some(s) => match scaffold::Template::parse(s) {
            Some(t) => t,
            None => {
                eprintln!("不认识的模板：{}", s);
                return 2;
            }
        },
    };
    let dir = parent_dir.unwrap_or_else(|| root.to_path_buf()).join(&name);
    let py = python_exe
        .clone()
        .or_else(|| probe::first_working(root, None, 3).map(|(p, _)| p));

    let opts = scaffold::NewOptions {
        dir: dir.clone(),
        name: name.clone(),
        template: tpl,
        lang: lang_name.clone().and_then(|l| scaffold::Lang::parse(&l)).unwrap_or(scaffold::Lang::Python),
        python_exe: py.clone(),
        purpose,
    };
    let rep = match scaffold::create(&opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\n  {} {}\n", c(RED, "✗ 建不了"), e);
            return 1;
        }
    };

    let mut venv_note: Option<String> = None;
    if want_venv {
        if let Some(p) = &py {
            match envsetup::create_venv(&dir, p, ".venv") {
                Ok(cr) => match envsetup::bind(&dir, &cr.python_exe) {
                    Ok(_) => {
                        venv_note = Some(format!(
                            "已建虚拟环境 .venv（{}），并把契约指向它",
                            cr.method.id()
                        ))
                    }
                    Err(e) => venv_note = Some(format!("虚拟环境建好了，但绑进契约失败：{}", e)),
                },
                Err(e) => venv_note = Some(format!("{}", e)),
            }
        } else {
            venv_note = Some("没找到可用解释器，虚拟环境先跳过".into());
        }
    }

    let mut git_note = None;
    if want_git {
        if let Some(git) = probe::which("git.exe") {
            let d = dir.to_string_lossy().to_string();
            match probe::run_cmd(&git, &["-C", &d, "init"], Duration::from_secs(60)) {
                Some((true, _)) => git_note = Some("git 仓库已初始化（还没提交）".to_string()),
                _ => git_note = Some("git init 没成功".to_string()),
            }
        } else {
            git_note = Some("没找到 git，跳过".into());
        }
    }

    if json {
        let out = serde_json::json!({
            "dir": dir.to_string_lossy(),
            "template": tpl.id(),
            "created": rep.created,
            "note": rep.note,
            "venv": venv_note,
            "git": git_note,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    println!();
    println!(
        "  {} 建好了 {}（模板 {}）",
        c(GREEN, "✓"),
        c(BOLD, &dir.display().to_string()),
        tpl.id()
    );
    for f in &rep.created {
        println!("      {}", c(DIM, f));
    }
    for n in &rep.note {
        println!("  {} {}", c(GREEN, "·"), n);
    }
    if let Some(v) = venv_note {
        println!("  {} {}", c(GREEN, "·"), v);
    }
    if let Some(g) = git_note {
        println!("  {} {}", c(GREEN, "·"), g);
    }
    println!();
    println!("  {}", c(BOLD, "下一步"));
    println!("      cd {}", dir.display());
    println!("      jingwei doctor     {}  ", c(DIM, "看看环境和契约对不对"));
    println!("      jingwei run        {}", c(DIM, "跑起来"));
    println!("      jingwei note \"...\"  {}", c(DIM, "想到什么随手记一句"));
    println!();
    0
}

fn cmd_env(root: &Path, rest: Vec<String>, venv_name: Option<String>, python_exe: Option<PathBuf>) -> i32 {
    let sub = rest.first().map(|s| s.as_str()).unwrap_or("status");
    let (contract, err) = Contract::load(root);
    if let Some(e) = err {
        println!("\n  {} 契约读不了：{}\n", c(RED, "✗"), e);
        return 1;
    }
    let bound = contract
        .as_ref()
        .and_then(|c| c.runtime.python_exe.clone())
        .map(PathBuf::from);

    match sub {
        "status" => {
            println!();
            println!("  {} {}", c(BOLD, "绑定解释器"), match &bound {
                Some(p) => p.display().to_string(),
                None => c(DIM, "（没有绑定，靠自动发现）").to_string(),
            });
            if let Some(p) = &bound {
                let st = envsetup::describe(p);
                println!("      版本 {} · {}", st.version, if st.in_venv { "在虚拟环境里" } else { "不在虚拟环境里" });
                if st.isolated {
                    println!("      {}", c(YELLOW, "注意：它是嵌入式/便携发行版（隔离模式）"));
                }
                if !st.has_pip {
                    println!("      {}", c(YELLOW, "注意：里面没有 pip，装依赖会失败"));
                }
            }
            let venvs = probe::scan_venvs(root);
            println!();
            println!("  {}", c(BOLD, "项目里的虚拟环境"));
            if venvs.is_empty() {
                println!("      {}", c(DIM, "（没有。jingwei env init 可以建一个）"));
            } else {
                for v in venvs {
                    let exe = envsetup::venv_exe(Path::new(&v));
                    let mark = if bound.as_deref() == Some(exe.as_path()) { " ← 契约指的就是它" } else { "" };
                    println!("      {}{}", v, c(GREEN, mark));
                }
            }
            let base = bound.clone().or_else(|| probe::first_working(root, contract.as_ref(), 3).map(|(p, _)| p));
            println!();
            println!("  {}", c(BOLD, "能怎么建虚拟环境"));
            match &base {
                Some(p) => {
                    let m = envsetup::method_for(p);
                    println!("      {}", m.id());
                    if m == envsetup::Method::None {
                        println!(
                            "      {}",
                            c(
                                DIM,
                                "原因：这台解释器里没有 venv / ensurepip（嵌入式发行版就是这样）；jingwei env init 会把出路说清楚"
                            )
                        );
                    }
                }
                None => println!("      {}", c(RED, "没找到可用解释器")),
            }
            println!();
            0
        }
        "init" => {
            let name = venv_name.unwrap_or_else(|| ".venv".into());
            let base = python_exe
                .or(bound)
                .or_else(|| probe::first_working(root, contract.as_ref(), 3).map(|(p, _)| p));
            let Some(base) = base else {
                println!("\n  {} 找不到可用解释器，先 jingwei envs 看一眼\n", c(RED, "✗"));
                return 1;
            };
            println!();
            println!("  用 {} 建 {}", c(DIM, &base.display().to_string()), c(BOLD, &name));
            match envsetup::create_venv(root, &base, &name) {
                Ok(cr) => {
                    println!("  {} {} 已就绪（{}）", c(GREEN, "✓"), cr.venv_dir.display(), cr.method.id());
                    match envsetup::bind(root, &cr.python_exe) {
                        Ok(_) => {
                            println!("  {} 契约已指向 {}", c(GREEN, "✓"), cr.python_exe.display());
                            println!(
                                "    {}",
                                c(DIM, "以后 jingwei run / doctor 都用它，不需要 activate，也不看 PATH。")
                            );
                        }
                        Err(e) => {
                            println!("  {} 绑进契约失败：{}", c(RED, "✗"), e);
                            return 1;
                        }
                    }
                    println!();
                    0
                }
                Err(e) => {
                    println!("  {} {}", c(RED, "✗"), e);
                    println!();
                    1
                }
            }
        }
        "use" => {
            let Some(target) = rest.get(1) else {
                println!("\n  用法： jingwei env use <python.exe 的路径>   或   jingwei env use auto\n");
                return 2;
            };
            if target == "auto" {
                let mut ct = contract.unwrap_or_default();
                ct.runtime.python_exe = None;
                match ct.save(root) {
                    Ok(_) => println!("\n  {} 已解除绑定，改回自动发现\n", c(GREEN, "✓")),
                    Err(e) => {
                        println!("\n  {} {}\n", c(RED, "✗"), e);
                        return 1;
                    }
                }
                return 0;
            }
            let p = PathBuf::from(target);
            match envsetup::bind(root, &p) {
                Ok(_) => {
                    let st = envsetup::describe(&p);
                    println!("\n  {} 契约已指向 {}\n", c(GREEN, "✓"), p.display());
                    if st.version.is_empty() {
                        println!(
                            "  {} 但它跑不起来——重新绑一个能用的\n",
                            c(YELLOW, "!")
                        );
                        return 1;
                    }
                    println!("    Python {}\n", st.version);
                    0
                }
                Err(e) => {
                    println!("\n  {} {}\n", c(RED, "✗"), e);
                    1
                }
            }
        }
        "install" => cmd_task(root, "install", None),
        other => {
            eprintln!("env 的子命令只有 status / init / install / use");
            eprintln!("（{} 不认识）", other);
            2
        }
    }
}

fn cmd_note(root: &Path, rest: Vec<String>, kind: Option<String>, purpose: Option<String>) -> i32 {
    let (kind_raw, text): (String, String) = match (purpose, kind) {
        (Some(p), None) => ("goal".to_string(), p),
        (_, k) => (k.unwrap_or_else(|| "note".to_string()), rest.join(" ")),
    };
    let kind = match kind_raw.as_str() {
        "goal" => "goal",
        "idea" => "idea",
        "plan" => "plan",
        "blocker" => "blocker",
        "learned" => "learned",
        "decision" => "decision",
        "note" => "note",
        other => {
            eprintln!("不认识的类别：{}。可用：{}", other, kind_list());
            return 2;
        }
    };
    let text = text.trim();
    if text.is_empty() {
        println!();
        println!("  记一句： jingwei note \"想到什么就写什么\"");
        println!("  分类：   jingwei note -k {} \"...\"", kind_list());
        println!();
        return 2;
    }
    let input = notes::NoteInput {
        slug: None,
        kind: kind.to_string(),
        title: String::new(),
        body: text.to_string(),
        tags: Vec::new(),
    };
    match notes::save(root, &input) {
        Ok(n) => {
            println!(
                "\n  {} [{}] {}\n  {}\n",
                c(GREEN, "记下了"),
                notes::kind_label(&n.kind),
                n.title,
                c(DIM, &format!("存成 {}", notes::path_of(root, &n.slug).display()))
            );
            0
        }
        Err(e) => {
            eprintln!("\n  {} {}\n", c(RED, "✗"), e);
            1
        }
    }
}

fn kind_list() -> String {
    notes::KINDS
        .iter()
        .map(|(k, l)| format!("{}={}", k, l))
        .collect::<Vec<_>>()
        .join(" ")
}

fn cmd_notes(root: &Path, json: bool) -> i32 {
    let migrated = notes::migrate_legacy(root);
    let all = notes::list(root);
    if json {
        println!("{}", serde_json::to_string_pretty(&all).unwrap_or_default());
        return 0;
    }
    println!();
    if all.is_empty() {
        println!("  这个项目还没有记录。");
        println!("    {}", c(DIM, "jingwei note \"为什么做它\" 开始"));
        println!();
        return 0;
    }
    let show = if MORE.load(Ordering::Relaxed) { all.len() } else { all.len().min(10) };
    let start = all.len() - show;
    println!(
        "  {} 共 {} 条{}",
        c(BOLD, "工作记录"),
        all.len(),
        if start > 0 {
            format!("，下面是最近 {} 条（--more 看全部）", show)
        } else {
            String::new()
        }
    );
    println!();
    if migrated > 0 {
        println!("  {}", c(DIM, &format!("（把 {} 条旧记录搬成了笔记）", migrated)));
    }
    for n in &all[start..] {
        let stamp = n.updated.get(..16).unwrap_or(&n.updated).replace('T', " ");
        println!(
            "  {}  {}",
            c(DIM, &stamp),
            c(BOLD, &format!("[{}]", notes::kind_label(&n.kind)))
        );
        println!("      {}", c(BOLD, &n.title));
        for line in wrap(&n.body, 80) {
            println!("      {}", line);
        }
        if !n.tags.is_empty() {
            println!("      {}", c(DIM, &format!("#{}", n.tags.join(" #"))));
        }
        println!();
    }
    0
}
#[cfg(windows)]
fn setup_console() {
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
fn setup_console() {
    COLOR.store(std::io::stdout().is_terminal(), Ordering::Relaxed);
}
