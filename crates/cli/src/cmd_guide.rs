//! `jingwei guide [目录]` —— **一句话回答「现在能不能跑、卡在哪、下一步敲什么」**。
//!
//! ## 它和 `doctor` 的分工（这是这个文件存在的唯一理由）
//!
//! `doctor` 给的是**清单**：解释器、依赖、契约、环境变化，各报几条。
//! 那是"我知道的都告诉你"。
//!
//! 而人打开一个项目时问的是**一个问题**：「我要怎么把它跑起来？」
//! 一个列了 7 条提示却不说"先做哪个"的输出，等于把排序工作推回给用户——
//! 而排序正是工具该替他做的部分（设计 §8.1 第 ⑥ 条「一屏 1 结论」）。
//!
//! 所以这个命令的输出**刻意短**：项目是什么 / 能不能跑 / 卡在哪（第一条）/
//! 下一步敲什么。想看全部细节就 `jingwei doctor --more`。
//!
//! ## 三态落在这一层的形状
//!
//! 「能不能跑」有三个答案，界面上必须分得开：
//!   - **能跑** → 把命令给出来
//!   - **跑不起来** → 说清卡在哪，并给一条能敲的下一步
//!   - **还查不出来** → 明说"查不出来 + 缺什么"，**不许猜成前两种**
//!
//! 第三支是这个项目的招牌（四条线里"三态不许压扁"）。少它的话，
//! "你还没写契约"会被显示成"你的项目坏了"——那是把我们的空白说成用户的问题。

use super::{c, BOLD, CYAN, DIM, GREEN, RED, YELLOW};
use jingwei_engine::understand::guide::{self, Guide};
use jingwei_engine::Verdict;
use std::path::Path;

pub(crate) fn cmd_guide(root: &Path, json: bool) -> i32 {
    // 走的是**和 doctor 完全同一条取数路径**——两条路各算各的迟早会算得不一样
    // （这个坑在 diag 那一处刚踩过，不再踩第二次）。
    let prev = jingwei_engine::state::snapshot::load_latest(root).map(|(_, f)| f);
    let facts = jingwei_engine::detect::probe::make_facts(root, None);
    let g = guide::make_guide(&facts, &[]);

    if json {
        // **JSON 由 `Guide::to_json()` 给**（2026-10-16）——它与桌面界面同源。
        // 原来这段转换写在这里，而界面也要显示同一份指南：
        // 两处各写一遍迟早会算出两套说法（这个仓库为"两份清单各写各的"付过账）。
        println!(
            "{}",
            serde_json::to_string_pretty(&g.to_json()).unwrap_or_default()
        );
        // 退出码：能不能跑得起来。**"查不了"不当成失败**（那是我们的空白，不是他的错），
        // 但也不当成功——用 2，与"用法错"区分开。
        // **"不适用"（库）是 0**：那是确定地认出来了，不是查不了。
        return exit_code(&g);
    }

    render(&g, prev.is_some());
    exit_code(&g)
}

/// 三态 → 退出码。**抽出来是为了让"三支都要有"这件事有一个能被断言的地方。**
///
/// ## 第四态（不适用）退出码是 `0`（2026-10-17）
///
/// `介绍.md` 的约定：
///   0 = 成功（含"空结果"这种正常的空）
///   1 = 跑不起来（**他的问题**）
///   2 = 查不了（**我们的问题**）
///
/// **"不适用"一个都不是**：我们**确定地**认出了"这是个库，没有'跑'这回事"——
/// 那不是查不了（2 说"我们没查到"，是假话），也不是他的问题（让他补 `main`？）。
/// 它是**一次成功的判定**，所以是 `0`。
///
/// 实测依据：拿 8 个别人写的真 crate（anyhow / itoa / log / memchr / regex /
/// serde / clap / rand）跑，改之前全是"补上就能判断" + 退 2。
/// 一个脚本若按退出码决定要不要拦，会把每一个 Rust 库都拦下来。
fn exit_code(g: &Guide) -> i32 {
    match &g.ready {
        Verdict::Yes(_) => 0,
        Verdict::No => 1,
        Verdict::Unknown => match g.unknown_why {
            jingwei_engine::Unknown::NotApplicable => 0,
            _ => 2,
        },
    }
}

/// 渲染。**顺序就是"该看什么"的顺序**：结论 → 卡在哪 → 下一步 → 次要的。
fn render(g: &Guide, has_prev: bool) {
    println!();
    // ① 结论。一行说清，颜色按三态分——**"查不了"用黄色，不用红色**
    //    （红色是"出错了"，而"我们没查过"不是错）。
    let (mark, tone) = match &g.ready {
        Verdict::Yes(_) => ("✓", GREEN),
        Verdict::No => ("✗", RED),
        Verdict::Unknown => ("?", YELLOW),
    };
    let word = match &g.ready {
        Verdict::Yes(_) => "现在就能跑",
        Verdict::No => "现在跑不起来",
        // **"查不了"要说清是谁的**——两种情况的说法必须不同，
        // 因为用户能做的事完全不同（一种是"你补上"，一种是"不怪你"）。
        // 原来这一支只有一句"还查不出来能不能跑"，把那两件事混成了一件。
        Verdict::Unknown => {
            // **但还有一种情况：这个问题对它压根不适用**（2026-10-17）。
            //
            // 三次碰到的同一个洞：**空目录** / `site-packages` 里的库 /
            // **8 个别人写的真 crate**（`anyhow` `itoa` `log` `memchr` `regex`
            // `serde` `clap` `rand`）——8 个全是同一句「**补上就能判断**」。
            // 而它们是**库**：库没有"跑"这回事，让用户"补上"是让他去补一个 `main`。
            //
            // 现在这一支是**独立的一态**（`Unknown::NotApplicable`），不是绕过去的措辞。
            match g.unknown_why {
                jingwei_engine::Unknown::TheirGap => "还查不出来——缺一样你这边的东西",
                jingwei_engine::Unknown::OurGap => "还查不出来——是我们没查到，不怪你",
                jingwei_engine::Unknown::Unclear => "还查不出来能不能跑",
                // **不加"查不出来"三个字**：它不是查不出来，是**这个问题对它不成立**。
                jingwei_engine::Unknown::NotApplicable => "这个问题对它不适用",
            }
        }
    };
    println!("  {} {}", c(tone, mark), c(BOLD, word));
    println!("    {}", c(DIM, &g.what));
    // 未知时**多给一行"该做什么"**：只说"查不了"等于把问题丢回给用户。
    if matches!(g.ready, Verdict::Unknown) {
        // **优先用引擎给的那句话**（它知道场合），没有再退回类型的通用说法。
        // 原来这里直接读 `g.unknown_why.what_to_do()`，把 `Guide` 上那个
        // 更具体的 `what_to_do` **整个绕过去了**——`--json` 与文字两套话。
        let line = g
            .what_to_do
            .clone()
            .unwrap_or_else(|| g.unknown_why.what_to_do().to_string());
        println!("    {}", c(DIM, &line));
    }
    println!();

    // ② 卡在哪 —— **只列挡路的**，而且第一条就是"先做这个"
    if !g.blockers.is_empty() {
        println!("  {}", c(BOLD, "卡在哪"));
        for (i, b) in g.blockers.iter().enumerate() {
            let tag = if i == 0 { "先做这个" } else { "还有" };
            println!("    {} {}", c(CYAN, &format!("{}.", i + 1)), c(BOLD, &b.what)
                .to_string() + &format!("  {}", c(DIM, tag)));
            println!("       {}", b.why);
            if !b.fix.is_empty() {
                println!("       {} {}", c(DIM, "下一步"), c(GREEN, &b.fix));
            }
            if !b.verify.is_empty() {
                println!("       {} {}", c(DIM, "怎么确认"), c(DIM, &b.verify));
            }
        }
        println!();
    } else if !has_prev {
        // 没挡路的、也没跑过 —— 那"跑一次"就是唯一能给出确定答案的路。
        println!("  {}", c(BOLD, "下一步"));
        // **那句话只对"给出来的是命令"成立**（2026-10-17）。
        // 「不用做什么——这里不是一个项目」后面再缀"跑一次就知道"就是自相矛盾：
        // 它本来就不是个能跑的东西。所以按内容决定缀不缀。
        let is_command = g.what_to_do.is_none();
        if is_command {
            println!(
                "    {} {}",
                c(GREEN, &g.next_step),
                c(DIM, "（跑一次就知道它到底行不行）")
            );
        } else {
            println!("    {}", c(DIM, &g.next_step));
        }
        println!();
    }

    // ③ 能跑的话，把命令给出来（**这是这个命令存在的主要理由**）
    if let Verdict::Yes(cmd) = &g.ready {
        println!("  {}", c(BOLD, "怎么跑"));
        println!("    {}", c(GREEN, cmd));
        println!("    {}", c(DIM, "跑起来之后出了问题：`jingwei run` 会把报错挂到具体文件上"));
        println!();
    }

    // ④ 次要的：不挡路、但迟早咬一口。**与上面分开列**——
    //    混在一起就是没有排序，而排序是这一层要解决的问题。
    if !g.nice_to_have.is_empty() {
        println!("  {}", c(BOLD, "以后再说"));
        for n in &g.nice_to_have {
            println!("    {} {}", c(DIM, "·"), c(DIM, n));
        }
        println!();
    }

    // ⑤ 想看全部细节指个路（别让人以为"就这些"）
    println!("  {}", c(DIM, "看全部细节：jingwei doctor --more"));
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **退出码要分开四态**：能跑 0 / 跑不起来 1 / 查不了 2 / **不适用 0**。
    ///
    /// 少第三支的话，"你还没写契约"会在脚本里长得像"你的项目坏了"——
    /// 这与四条线里"没做不许长得像做成了"是同一条，只是方向反过来：
    /// **我们的空白不许长得像用户的问题。**
    ///
    /// 第四支（不适用）为什么要单独断言：它**该退 0**。
    /// 拿 8 个别人写的真 crate 实测过（anyhow / itoa / log / memchr / regex /
    /// serde / clap / rand），改之前它们全退 2——**每一个 Rust 库都被当成"查不了"**，
    /// 脚本会把它们全部拦下来。而"这是个库"是我们**确定地认出来**的，不是查不了。
    #[test]
    fn 三态各有各的退出码() {
        // `Guide` 没有 `Default`，字段会加——所以用 `..make_guide` 太贵，
        // 这里只填断言要用的那几个，其余给空值。**字段变了这里会编译报错**（那是对的）。
        let g = |ready: Verdict<String>, why: jingwei_engine::Unknown| Guide {
            what: String::new(),
            ready,
            blockers: vec![],
            next_step: String::new(),
            nice_to_have: vec![],
            run_command: None,
            unknown_why: why,
            what_to_do: None,
        };
        use jingwei_engine::Unknown;
        assert_eq!(
            exit_code(&g(Verdict::Yes("x".into()), Unknown::TheirGap)),
            0
        );
        assert_eq!(exit_code(&g(Verdict::No, Unknown::TheirGap)), 1);
        assert_eq!(exit_code(&g(Verdict::Unknown, Unknown::TheirGap)), 2);
        assert_eq!(exit_code(&g(Verdict::Unknown, Unknown::OurGap)), 2);
        assert_eq!(exit_code(&g(Verdict::Unknown, Unknown::Unclear)), 2);
        // **不适用 → 0**
        assert_eq!(
            exit_code(&g(Verdict::Unknown, Unknown::NotApplicable)),
            0,
            "「不适用」（库 / 不是一个项目）是**确定地认出来了**，不是查不了——\
             给它 2 会让每个 Rust 库都被脚本当成'失败'"
        );
    }
}
