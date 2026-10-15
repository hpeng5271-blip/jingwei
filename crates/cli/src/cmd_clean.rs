//! 清理那一摊：`scan`（哪些能删）和 `clean`（动手）。
//!
//! 两件事必须挨着放：`scan` 的输出就是 `clean` 的输入，
//! 中间那层"分类 + 危险度 + 数量"的排版是同一套说法。
//! 分开写的话，会出现"列出来的和删掉的不是同一批"这种最坏的情况。
//!
//! 从 main.rs 拆出来的理由：门禁的行数上限（见《设计》§12.3）。

use super::{c, pad, wrap, BOLD, CYAN, DIM, GREEN, RED, YELLOW};
use jingwei_engine::clean::{self, Action, Zone};
use std::path::Path;

pub(crate) fn cmd_scan(root: &Path, json: bool) -> i32 {
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

pub(crate) fn cat_rank(name: &str) -> usize {
    CAT_ORDER
        .iter()
        .position(|x| *x == name)
        .unwrap_or(CAT_ORDER.len())
}

pub(crate) fn render_scan(items: &[clean::Item]) {
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

pub(crate) fn cmd_clean(
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

