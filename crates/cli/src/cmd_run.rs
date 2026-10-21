//! 跑起来那一摊：`run`（按契约启动 + 6 阶段进度）、`test`。
//!
//! 阶段进度的措辞、内存占用那一行、就绪判定后的收尾都在这里——
//! 它们是同一件事的几个侧面：**让用户看见"现在到哪一步了"**。
//!
//! 从 main.rs 拆出来的理由：门禁的行数上限（见《设计》§12.3）。

use super::{c, pad, wrap, BOLD, CYAN, DIM, GREEN, RED, YELLOW};
use jingwei_engine::{launch, Contract};
use std::path::Path;
use std::time::Duration;

pub(crate) fn print_memory(m: &jingwei_engine::mem::MemReport) {
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

pub(crate) fn cmd_run(root: &Path, run_override: Option<String>, extra: Vec<String>) -> i32 {
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
                    // **先给"缺哪个件"**（如果有）：它比"错在哪一行"更靠前——
                    // 缺件时用户去改代码是白费力气。两个都来自 `Outcome`，
                    // **外壳不许自己再扫一遍输出**（那会出现两个消费点各说各的）。
                    if let Some(m) = &out.missing_tool {
                        println!(
                            "  {} 这台机器缺 {}",
                            c(YELLOW, "!"),
                            c(BOLD, &m.what)
                        );
                        println!("    {} {}", c(DIM, "为什么"), m.why);
                        println!("    {} {}", c(DIM, "怎么装"), m.shape);
                        println!("    {} {}", c(DIM, "怎么自己验"), m.verify);
                        println!();
                    }
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

pub(crate) fn guess_run(root: &Path) -> Option<String> {
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
/// 跑一个"任务"（test / install 这类）。
///
/// `command` 是 `Option`：契约里没写这条命令时，**别在这里就判死**——
/// Node 项目的命令来自 `package.json` 的 scripts，那是下一层（`launch`）的活儿。
/// 在这一层拦住的话，一个正常的 Vue 项目会收到"契约里没声明 test 命令"，
/// 而它压根不该有契约。
pub(crate) fn run_task(
    root: &Path,
    command: Option<String>,
    extra: Vec<String>,
    purpose: launch::Purpose,
) -> i32 {
    let opts = launch::LaunchOptions {
        root: root.to_path_buf(),
        run_override: command,
        extra_args: extra,
        timeout: Duration::from_secs(30),
        purpose,
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

pub(crate) fn cmd_task(root: &Path, which: &str, override_cmd: Option<String>) -> i32 {
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
    // **Node 项目不走这一段**：它的 test / dev / build 都写在 package.json 的
    // scripts 里，由 `launch` 那层去挑。在这里报"契约里没声明"是答非所问——
    // 一个 Vue 项目本来就不该有 env.contract.json。
    // **网络闸门**（2026-10-21 第二轮）：装依赖会碰仓库。白名单（pypi / npm / crates / maven）之外
    // 一律**先拒后跑**；允许只给本次会话 / 24 小时（见 sandbox_allow），可随时收回。
    if let Some(cmd) = command.as_deref() {
        let looks_install = cmd.contains(" install") || cmd.contains("--index-url") || cmd.contains("--registry");
        if looks_install {
            let allow = crate::sandbox_allow::allow_path();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Err(host) = crate::sandbox_allow::net_gate(&allow, cmd, now) {
                println!();
                println!(
                    "  {} 暂无法探知：这条安装命令要访问 {host}，它不在白名单里｜缺：该仓库域名的允许｜为什么：网络默认拒绝，只放行包仓库（pypi / npm / crates / maven）｜怎么补：确认可信后把它加进允许表（24 小时），或去掉命令里的自定义仓库地址、改用默认源",
                    c(RED, "✗")
                );
                println!();
                return 2;
            }
        }
    }
    let node_project = jingwei_engine::web::is_node_project(root);
    if command.is_none() && !node_project {
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
    }
    // 用途要传下去：装依赖和跑测试是**两件不同的事**，
    // 在 Node 项目上它们分别落到 `{包管理器} install` 和 `{包管理器} run test`。
    let purpose = match which {
        "install" => launch::Purpose::Install,
        "build" => launch::Purpose::Build,
        _ => launch::Purpose::Task,
    };
    run_task(root, command, Vec::new(), purpose)
}
