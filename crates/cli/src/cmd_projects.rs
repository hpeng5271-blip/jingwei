//! `jingwei projects`：**列出一个目录下的项目**（`框架.md` §4.9 的第一个真实消费点）。
//!
//! ## 为什么它必须存在
//!
//! "找项目"原来**只有桌面界面的出口**（Tauri 的 `list_projects`）。后果两条：
//! 1. **脚本化不了**——CI 里没法验"这个父目录里应该出现 3 个项目"；
//! 2. 采集那套改造**没有 CLI 级验收口**，只能靠 GUI 或单元测试。
//!
//! ## 它走的是索引，不是重新遍历一遍判一遍
//!
//! `dirs_index::build()` 先建一张**目录表**（每行：mtime / 有哪些标记文件 / 被哪条规则跳过），
//! 然后"项目"与"最外层"都是**对表的查询**。所以：
//! - `--all` 能看见嵌套项目（monorepo 里的包），而默认口径与界面一致（遇项目就不再往里算）；
//! - 想说"为什么某个目录没被算成项目"，表里有行、有原因；
//! - 判定规则将来要改，**不用重新遍历**。
//!
//! ## 三态在这里的落法
//!
//! - 一个项目都没有、但目录**读得出来** → 说"没找到项目"，退出码 **0**（这是结论）；
//! - 目录**不存在 / 读不出来** → 说"读不到"，退出码 **2**（这是我们的失败，不是"你没有"）。

use std::path::Path;

use jingwei_engine::understand::dirs_index;
use super::{c, BOLD, DIM, GREEN, YELLOW};

pub fn cmd_projects(root: &Path, depth: usize, all: bool, json: bool) -> i32 {
    if !root.is_dir() {
        eprintln!("读不到这个目录：{}", root.display());
        eprintln!("  {}", c(DIM, "（这是我们的失败，不是「你没有项目」——退出码 2）"));
        return 2;
    }

    let t = std::time::Instant::now();
    // **走带缓存的那条**：遍历照走（诚实边界见 `dirs_index` 的注释），
    // 但每个目录的"事实"能复用——`--json` 里把复用了多少报出来。
    let (idx, reuse) = dirs_index::build_cached(root, depth, None);
    let found: Vec<String> = if all {
        idx.all_projects()
    } else {
        idx.top_level_projects()
    }
    .iter()
    .map(|p| p.to_string_lossy().to_string())
    .collect();
    let ms = t.elapsed().as_millis() as u64;

    if json {
        let v = serde_json::json!({
            "root": root.to_string_lossy(),
            "depth": depth,
            "all": all,
            "projects": found,
            "visited": idx.visited,
            "skipped": idx.skipped_count(),
            "facts_reused": reuse.facts_reused,
            "facts_read": reuse.facts_read,
            "ms": ms,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return 0;
    }

    println!();
    if found.is_empty() {
        println!("  {} 没找到项目（深度 {}，看了 {} 个目录）", c(YELLOW, "·"), depth, idx.visited);
        println!(
            "    {}",
            c(DIM, "判定依据是清单文件 / .git / env.contract.json；换个更外层的目录，或加 --depth。")
        );
    } else {
        println!(
            "  {} 找到 {} 个项目（深度 {}，看了 {} 个目录，{}ms）",
            c(GREEN, "✓"),
            c(BOLD, &found.len().to_string()),
            depth,
            idx.visited,
            ms
        );
        for p in &found {
            // 相对路径更好读：绝对路径往往前 40 个字符都一样
            let shown = match std::path::Path::new(p).strip_prefix(root) {
                Ok(rel) => rel.to_string_lossy().to_string(),
                Err(_) => p.clone(),
            };
            println!("    {}", shown);
        }
    }
    // **复用多少也要报出来**：这是"索引有没有起作用"的唯一证据。
    if reuse.facts_reused > 0 {
        println!(
            "    {}",
            c(DIM, &format!("（{} 个目录的事实来自缓存，{} 个重新读过）", reuse.facts_reused, reuse.facts_read))
        );
    }
    // **跳过多少要报出来**：不报的话，"没找到"会被读成"这里没有"。
    if idx.skipped_count() > 0 {
        println!(
            "    {}",
            c(DIM, &format!("（跳过了 {} 个目录：node_modules / target / 以 . 开头、系统目录等）", idx.skipped_count()))
        );
    }
    println!();
    0
}
