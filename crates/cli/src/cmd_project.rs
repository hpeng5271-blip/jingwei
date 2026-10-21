//! 项目与环境那一摊：`new` / `env` / `envs` / `init` / `snap` / `note` / `notes`。
//!
//! 它们都围绕"这个项目自己的东西"：建它、绑解释器、给它补契约、
//! 记它的快照和想法。和 `doctor` 那种"查毛病"是两件事，所以分开放。
//!
//! 从 main.rs 拆出来的理由：门禁的行数上限（见《设计》§12.3）。

use super::{c, pad, wrap, BOLD, DIM, GREEN, MORE, RED, YELLOW};
use super::cmd_run::{cmd_task, guess_run};
use jingwei_engine::{envsetup, notes, probe, scaffold, snapshot, Contract};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::protocol::{envelope, Coverage, Evidence, State};


/// 一个 Python 都没探到时的三字段说明（2026-10-21）：**缺什么 / 为什么 / 怎么补**。
///
/// 这三样缺一，用户就只能看到"没有 Python"——而他真正需要知道的是"是我们没探到，还是他确实没装"，
/// 以及下一步该敲什么。
fn envs_unknown_summary() -> String {
    "暂无法探知：这台机器上没探到可用的 Python｜缺：可用的 Python 解释器｜为什么：PATH、注册表、常见安装目录与项目 venv 都没读到｜怎么补：装 uv 后跑 `uv python install 3.12`，或从 python.org 装一个".to_string()
}
pub(crate) fn cmd_envs(root: &Path, json: bool) -> i32 {
    let (contract, _) = Contract::load(root);
    let list = probe::find_pythons(root, contract.as_ref(), true);
    if json {
        // **协议**（2026-10-21）：以前这里直接打一个数组——空数组会被读成"这台机器没有 Python"，
        // 而真相往往是"几种探法都没读到"。三态 + 三字段（缺什么/为什么/怎么补）必须一起出。
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
        let found = !rows.is_empty();
        let state = if found { State::Yes } else { State::Unknown };
        let summary = if found {
            format!(
                "找到 {} 个可用的 Python（PATH / 注册表 / 常见安装目录 / venv）",
                rows.len()
            )
        } else {
            envs_unknown_summary()
        };
        let v = envelope(
            state,
            summary,
            vec![
                Evidence::command(
                    "jingwei envs",
                    "探法：PATH、注册表、常见安装目录、项目 venv；装法见 summary",
                ),
                Evidence::file("env.contract.json", "有契约时优先按契约里的解释器找"),
            ],
            // 探法有限：没找到不等于没有，所以永远标 partial。
            Coverage::partial(rows.len(), None, "探到的数目；探法有限，没找到 ≠ 没有"),
            serde_json::json!({"pythons": rows}),
        );
        crate::protocol::print(&v);
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

pub(crate) fn cmd_init(root: &Path, json: bool) -> i32 {
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_new(
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

pub(crate) fn cmd_env(root: &Path, rest: Vec<String>, venv_name: Option<String>, python_exe: Option<PathBuf>) -> i32 {
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

pub(crate) fn cmd_note(root: &Path, rest: Vec<String>, kind: Option<String>, purpose: Option<String>) -> i32 {
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

pub(crate) fn kind_list() -> String {
    notes::KINDS
        .iter()
        .map(|(k, l)| format!("{}={}", k, l))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn cmd_notes(root: &Path, json: bool) -> i32 {
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

pub(crate) fn cmd_snap(root: &Path, json: bool, diff_only: bool) -> i32 {
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
