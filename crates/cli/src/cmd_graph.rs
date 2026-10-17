//! 图谱那一摊（命令行版）：`graph` 看运行历史，`graph runs <id>` 看某一次的明细。
//!
//! ## 为什么只做"读"，不做索引
//!
//! 索引（`graph index`）在引擎里已经有现成的实现（`jingwei_engine::graph::index`，
//! 桌面外壳的 `graph_index` 就是调它）。这里先不重复一份——**同一件事两处实现，迟早不一致**。
//! 命令行现在能做的是**把图谱里已有的东西问出来**，那正是 M5 缺的那后半截。
//!
//! ## 和桌面外壳的关系
//!
//! 两边走的是**同一个查询层**（`jingwei_engine::graph::query`），所以答案必然一致。
//! 这正是这个项目"一个内核、两份外壳"那句话要兑现的地方：
//! 不是"两处各写一遍排版"，而是"同一个答案，两种排版"。
//!
//! ## 三态在这里也得看得见
//!
//! 每条诊断的落点是三态：有文件 / 这条问题本来没说是哪个文件 / 说了但那个文件没索引过。
//! 命令行不画颜色块，所以用**三种不同的措辞**分开——文字界面同样不许把后两种混成一句。

use super::{c, pad, BOLD, CYAN, DIM, GREEN, RED, YELLOW};
use jingwei_engine::graph::{query, store::Store};
use jingwei_engine::Verdict;
use serde_json::json;
use std::path::Path;

/// `jingwei graph [runs <id>] [--json]`
pub(crate) fn cmd_graph(root: &Path, json: bool, rest: &[String]) -> i32 {
    let sub = rest.first().map(|s| s.as_str()).unwrap_or("");
    let run_id = rest.get(1).map(|s| s.as_str());

    // `jingwei index` 是**同一个内核入口**的另一条外壳（界面「索引一次」走 `graph_index`，
    // 两边都调 `graph::index::index_rust`）。
    //
    // ## 为什么补这一条（2026-10-16；当初"故意没做"的理由已经不成立）
    //
    // 原来记的理由是「引擎里已有实现，CLI 再写一遍迟早不一致」。
    // **那条理由说的是"两处实现"，而这里是一处实现两条外壳**——不冲突。
    // 而缺它的代价是实打实的：**每次看图都要开窗手点「索引一次」**。
    // 工具的价值在于"能被执行"，而"要人开窗手点"就等于**不能被执行**。
    //
    // 判据：**一条命令能不能把项目带到"图谱可用"的状态**——能，它就该存在。
    if matches!(sub, "index" | "build" | "scan") {
        return cmd_index(root, json);
    }

    let store: Store = match query::open_for_query(root) {
        Ok(s) => s,
        Err(e) => {
            // **`Err` 有两种，不能一律说成"还没建过图谱"**（2026-10-16 修）。
            //
            // 原来的代码是 `Err(_)` + 一句话：把**库在、打不开**（被别的进程占着 /
            // 文件坏了 / 权限不对）也说成"你还没建过图谱"。用户于是去点「索引一次」，
            // 而那个动作**解决不了他的问题**——真正的错因被这句话盖住了。
            //
            // 判据是**磁盘上有没有那个库**（事实，不是猜）：
            //   - 没有 → `no`：他还没建过。点一下就有。**他的空白。**
            //   - 有   → `unknown`：库在，是我们读不到。**我们的失败**，
            //            而且要把原始错误带出来（不然没法查）。
            let db = jingwei_engine::graph_db_path(root);
            if !db.is_file() {
                if json {
                    println!("{}", json!({ "state": "no", "reason": "这个项目还没建过图谱" }));
                } else {
                    println!();
                    println!("  {} 这个项目还没建过图谱。", c(DIM, "·"));
                    println!(
                        "    {}",
                        c(DIM, "在桌面界面里打开它 → 「图谱」页 → 点「索引一次」。")
                    );
                    println!();
                }
                return 0;
            }
            if json {
                println!(
                    "{}",
                    json!({
                        "state": "unknown",
                        "why": "our_gap",
                        "reason": format!("图谱库在，但读不到：{e}"),
                        "whatToDo": jingwei_engine::Unknown::OurGap.what_to_do(),
                    })
                );
            } else {
                println!();
                println!("  {} 图谱库在，但读不到。", c(YELLOW, "?"));
                println!("    {} {}", c(DIM, "原因："), e);
                println!(
                    "    {}",
                    c(DIM, "这不是你少做了什么——多半是另一个进程正占着它，或者文件坏了。")
                );
                println!();
            }
            // **退出码 2**：与"跑不起来"(1) 和"用法错"分开，也和 `no` 的 0 分开——
            // "查不了"不该被脚本当成"没有"。
            return 2;
        }
    };

    match sub {
        // 某一次的明细
        "runs" | "run" | "log" => {
            let Some(id) = run_id else {
                eprintln!("用法：jingwei graph runs <运行 id>（id 从 `jingwei graph` 里看）");
                return 2;
            };
            match query::answer_run(&store, id) {
                Ok(None) => {
                    // 查不到就如实说"没有这次运行"——**不返回一个空答案**。
                    if json {
                        println!("{}", json!({ "state": "no", "reason": "图谱里没有这次运行" }));
                    } else {
                        println!();
                        println!("  {} 图谱里没有这次运行：{}", c(YELLOW, "!"), id);
                        println!("    {}", c(DIM, "运行 id 用 `jingwei graph` 看。"));
                        println!();
                    }
                    1
                }
                Ok(Some(a)) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "runId": a.run_id,
                                "command": a.command,
                                "exitCode": a.exit_code,
                                "at": a.at,
                                "diagnostics": a.diagnostics.iter().map(|d| json!({
                                    "title": d.title,
                                    "file": match &d.file {
                                        Verdict::Yes(id) => json!({ "state": "yes", "data": id }),
                                        // `No` = 它本来就没说位置 → **用户的空白**（这条问题就是没落点）
                                        Verdict::No => json!({
                                            "state": "no",
                                            "reason": "这条问题没有说是哪个文件",
                                            "why": "their_gap",
                                        }),
                                        // `Unknown` = 它说了、我们没索引 → **我们的失败**。
                                        // 归属由引擎带出来（`d.unknown_why`），**不在这里猜**：
                                        // 外壳自己猜"Unknown 是谁的"正是 32 处没接上的原因。
                                        Verdict::Unknown => json!({
                                            "state": "unknown",
                                            "reason": "它说了是哪个文件，但那个文件不在图谱里",
                                            "why": "our_gap",
                                            "whatToDo": d.unknown_why.what_to_do(),
                                        }),
                                    },
                                })).collect::<Vec<_>>(),
                            }))
                            .unwrap_or_default()
                        );
                        return 0;
                    }
                    print_answer(&a);
                    0
                }
                Err(e) => {
                    eprintln!("查这次运行失败：{e}");
                    1
                }
            }
        }
        // 默认（**只对空子命令**）：规模 + 最近运行
        "" => match query::recent_runs(&store, 20) {
            Ok(list) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "runs": list.iter().map(|r| json!({
                                "runId": r.run_id,
                                "command": r.command,
                                "exitCode": r.exit_code,
                                "at": r.at,
                                "diagnostics": r.diagnostics,
                            })).collect::<Vec<_>>(),
                        }))
                        .unwrap_or_default()
                    );
                    return 0;
                }
                print_runs(&list);
                0
            }
            Err(e) => {
                eprintln!("列运行记录失败：{e}");
                1
            }
        },
        // **认不出的子命令：报错、返回非 0。**
        //
        // 这里原来和空子命令**共用同一个 `_ =>` 兜底**，于是
        // `jingwei graph index`（账本里写着"故意没做"的那条）会**打印运行历史并返回 0**——
        // 脚本里 `jingwei graph index && 下一步` 就这么安心往下走了。
        // 与第 19 轮修顶层分派时是同一条线：**没做不许长得像做成了**
        // （四条线里"三态不许压扁"的外壳版）。那次只修了外面那一层，这条漏了。
        other => {
            eprintln!("不认识 graph 的子命令：{other}");
            eprintln!("用法：jingwei graph [runs <id>] [--json]");
            eprintln!("  （索引那一条故意没做：引擎里有实现，CLI 再写一遍迟早不一致；");
            eprintln!("    要索引请在桌面界面里打开项目 → 「图谱」页 → 点「索引一次」）");
            2
        },
    }
}

/// 最近运行那张表。**一屏看得完**：一行一次，两列对齐。
fn print_runs(list: &[query::RunSummary]) {
    println!();
    println!("  {}  ·  运行历史", c(BOLD, "jingwei"));
    let bad = list.iter().filter(|r| r.exit_code != 0).count();
    println!(
        "       {}",
        c(
            DIM,
            &format!(
                "共 {} 次；其中失败 {} 次。点开某一次的明细：jingwei graph runs <id>",
                list.len(),
                bad
            )
        )
    );
    println!();
    if list.is_empty() {
        println!("  {} 还没有跑过任何东西。", c(DIM, "·"));
        println!(
            "    {}",
            c(DIM, "跑一次就有了：jingwei run（或在界面里点「运行」）。")
        );
        println!();
        return;
    }
    for r in list {
        let code = if r.exit_code == 0 {
            c(GREEN, &format!("{:>3}", r.exit_code))
        } else {
            c(RED, &format!("{:>3}", r.exit_code))
        };
        let when = ms_to_local(r.at);
        let n = if r.diagnostics == 0 {
            c(DIM, "无问题").to_string()
        } else {
            format!("{} 条问题", r.diagnostics)
        };
        // 命令可能很长：截到 46 列，超出用省略号。**不换行**——一屏一行才好扫。
        let cmd = truncate(&r.command, 46);
        println!(
            "  {} {}  {}  {}",
            c(DIM, &when),
            code,
            c(BOLD, &pad(&cmd, 46)),
            c(CYAN, &n)
        );
        println!("       {}", c(DIM, &r.run_id));
    }
    println!();
}

/// 某一次运行的明细。
fn print_answer(a: &query::RunAnswer) {
    println!();
    println!("  {}  ·  这次运行", c(BOLD, "jingwei"));
    println!("  {} {}", c(DIM, "命令"), a.command);
    println!(
        "  {} {}  {}",
        c(DIM, "退出码"),
        if a.exit_code == 0 {
            c(GREEN, "0（成功）")
        } else {
            c(RED, &format!("{}（失败）", a.exit_code))
        },
        c(DIM, &ms_to_local(a.at))
    );
    println!();
    if a.diagnostics.is_empty() {
        println!("  {} 这次运行没有留下问题。", c(GREEN, "✓"));
        println!();
        return;
    }
    println!("  {} 这次运行留下 {} 条问题：", c(BOLD, "问题"), a.diagnostics.len());
    println!();
    for d in &a.diagnostics {
        // 三态三种措辞，**不许混**：这就是这套系统最核心的那条线在终端里的样子。
        // `unknown` 那一支在 2026-10-16 之前是**死代码**——查询层从来没产出过它。
        // 现在产出之后，措辞要**带上差哪一步**（是哪个文件没索引到），
        // 否则用户知道"查不了"却不知道该做什么。
        let (mark, where_) = match &d.file {
            Verdict::Yes(id) => (c(CYAN, "→"), c(CYAN, id.trim_start_matches("file:"))),
            Verdict::No => (c(DIM, "·"), c(DIM, "（这条问题没有说是哪个文件）")),
            Verdict::Unknown => {
                let who = d.stated_file.as_deref().unwrap_or("某个文件");
                // **带上"该谁做什么"**：只说"没索引过"，用户不知道那是不是他该管的事。
                // 这一句从引擎的归属来（`d.unknown_why`），不是这里编的。
                (
                    c(YELLOW, "?"),
                    c(
                        YELLOW,
                        &format!(
                            "（它说是 {who}，但那个文件不在图谱里——它还没被索引过。{}）",
                            d.unknown_why.what_to_do()
                        ),
                    ),
                )
            }
        };
        println!("  {} {}", mark, d.title);
        println!("      {}", where_);
    }
    println!();
    println!(
        "  {}",
        c(DIM, "挂到文件上的那几条，就是在问「改了这里会影响谁」时用得上的入口。")
    );
    println!();
}

/// 毫秒时间戳 → `MM-DD HH:MM`（本地时区）。算不出来就返回一个占位，
/// **不假装知道时间**——空字符串比假时间诚实。
fn ms_to_local(ms: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    if ms <= 0 {
        return String::new();
    }
    let secs = (ms / 1000) as u64;
    // 用 SystemTime 做时区换算需要第三方库；这里只做 UTC 偏移的近似，
    // 不引依赖。差 8 小时好过引一个库——但**必须写明这一点**，否则读的人会以为它是本地时间。
    let t = UNIX_EPOCH + Duration::from_secs(secs + 8 * 3600);
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (days, rem) = (d / 86400, d % 86400);
    let (h, m) = (rem / 3600, (rem % 3600) / 60);
    // 1970-01-01 起的天数 → 月日（只做展示，不值得为它引 chrono）
    let (mo, day) = civil_from_days(days as i64);
    format!("{mo:02}-{day:02} {h:02}:{m:02}")
}

/// 天数（1970-01-01 起）→ (月, 日)。Howard Hinnant 那套 civil_from_days 的简化版。
fn civil_from_days(z: i64) -> (i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let _ = y;
    (m, d)
}

/// `jingwei index` —— **把项目带到"图谱可用"的状态**。
///
/// 与界面「索引一次」**同一个内核入口**（`graph::index::index_rust`），
/// 所以两条路的结果必然一致——这正是"一个内核、两份外壳"该有的样子。
///
/// 报的东西分三层，因为它们说的是**完全不同的三件事**：
///   ① 扫了多少文件 / 抽出多少符号 —— "读进去多少"
///   ② 连上多少 / 拿不准多少 / 外部多少 —— "理解了多少"
///      （只报一个"关系总数"的话，看的人分不清"图很全"和"图漏了一半"）
///   ③ 写了 use 却没接上、名字在但命名空间不符 —— "**我们自己漏了多少**"
///      （这两句是回答"那堆'外部'里混没混着我们自己漏接的"）
fn cmd_index(root: &Path, json: bool) -> i32 {
    let db = jingwei_engine::graph_db_path(root);
    if let Some(d) = db.parent() {
        if let Err(e) = std::fs::create_dir_all(d) {
            eprintln!("建不了数据目录 {}：{e}", d.display());
            return 1;
        }
    }
    let t0 = std::time::Instant::now();
    let mut store = match Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("打开图谱库失败：{e}");
            return 1;
        }
    };
    let rep = match jingwei_engine::graph::index::index_rust(root, &mut store) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("索引失败：{e}");
            return 1;
        }
    };    // **扫到 0 个文件时报清楚，并且必须非 0 退出。**
    //
    // 这是 2026-10-16 实测抓出来的：`jingwei index --root D:\projects\jingwei`
    // 报「**0 个文件 · 0 个符号**」而**退出码 0**——看起来像"索引成功，项目是空的"。
    //
    // ⚠️ **判据必须用 `scanned`，不是 `files`**（2026-10-16 第二次修）：
    // `files` 是"这次处理了几个"，全部没变时它**就是 0**——那是**正常**的增量运行。
    // 我第一版用 `files == 0` 判断，于是"什么都没改"被报成「这个目录里没有可索引的源文件」。
    // **两个数混用会让"正常的空"和"真的空"分不开**——这正是三态那条铁律在命令行的落法。
    if rep.scanned == 0 {
        let shown = root.display();
        let exists = root.is_dir();
        eprintln!(
            "扫到 0 个文件——这个目录{}：{shown}",
            if exists { "里没有可索引的源文件（.rs/.py）" } else { "不存在" },
        );
        eprintln!("  　提示：`--root` 之后跟的是**项目目录**；用 `jingwei scan` 可以看到它扫到了什么。");
        return 1;
    }
    let ms = t0.elapsed().as_millis() as u64;

    if json {
        // **形状由 `IndexReport::to_json()` 给**（2026-10-16）——与桌面界面同源。
        // 原来这里与 `src-tauri/src/graph.rs` 各拼一份，而它们已经漂了。
        let mut v = rep.to_json();
        if let Some(o) = v.as_object_mut() {
            o.insert("ms".into(), json!(ms));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&v).unwrap_or_default()
        );
        return 0;
    }

    println!();
    println!("  {} 图谱已建好", c(GREEN, "✓"));
    // **"扫到多少"与"处理了多少"必须分开说。**
    //
    // 2026-10-16 修：原来只报 `rep.files`，而它是"**这次真正解析+写库**的文件数"——
    // 没变的被三级检测跳过了。于是**一次增量运行会显示「1 个文件 · 10 个符号」**，
    // 看起来像"这个项目只有一个文件"。实测两条外壳因此差 7 倍：
    // `jingwei index` 报 1，而 `export_graph`（临时空库、全部处理）报 91。
    // **那不是 bug，是增量在正常工作——但报告把它说成了项目规模。**
    if rep.skipped_unchanged + rep.skipped_same_content > 0 {
        println!(
            "    这次处理了 {} 个文件（扫到 {} 个，其余 {} 个没变、跳过了）· {} 个符号 · {} 条调用（{}ms）",
            rep.files,
            rep.scanned,
            rep.skipped_unchanged + rep.skipped_same_content,
            rep.symbols,
            rep.calls,
            ms
        );
    } else {
        println!(
            "    {} 个文件 · {} 个符号 · {} 条调用（{}ms）",
            rep.files, rep.symbols, rep.calls, ms
        );
    }
    println!("    {}", c(DIM, "理解了多少："));
    println!(
        "      连上 {} 条 · {} 拿不准（不猜）· {} 外部（不归我们管）",
        rep.resolved,
        rep.ambiguous.len(),
        rep.external
    );
    // **"我们自己漏了多少"要单独说**——"外部 4579 个"这个数里混着
    // "标准库"与"我们没接上"，不分开报的话图是不是真的全，判断不了。
    let leak = rep.unmapped_imports.len() + rep.namespace_mismatch.len();
    if leak > 0 {
        println!(
            "      其中**我们自己漏的**：写了 use 却没接上 {} 处 · 名字在但命名空间不符 {} 处",
            rep.unmapped_imports.len(),
            rep.namespace_mismatch.len()
        );
    }
    if rep.truncated {
        println!("    {} 撞到文件数上限，这次没索引完", c(YELLOW, "!"));
    }
    println!("    {}", c(DIM, "看图：打开桌面界面 → 「图谱」页（或 jingwei graph）"));
    println!();
    0
}

/// 按**字符**截断（不是字节）：中文按字算，一列一个字宽，短了补省略号。
fn truncate(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}
