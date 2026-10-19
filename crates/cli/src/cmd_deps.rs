//! `jingwei deps`：**依赖对账**——清单声明的 vs 锁文件锁到的（2026-10-19）。
//!
//! 两边都是 cargo 自己的文件格式（外部真值），不经过我们的解析逻辑。

use std::path::Path;

pub fn cmd_deps(root: &Path, json: bool) -> i32 {
    let Some(rep) = jingwei_engine::understand::deps_check::check(root) else {
        eprintln!("读不到 Cargo.toml 或 Cargo.lock（不是 Rust 项目？）");
        return 2;
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&rep).unwrap_or_default());
    } else {
        println!("  依赖对账：{} 条声明", rep.checked());
        println!("    ✓ 锁里对得上：{}", rep.ok);
        println!("    × 锁里没有：{}", rep.missing);
        println!("    ? 判不了：{}（path/git/区间要求）", rep.unverifiable);
        for r in &rep.rows {
            if r.state != jingwei_engine::understand::deps_check::DepState::Ok {
                println!("    {} {} 声明 `{}` 锁定 {:?}", r.state.as_str(), r.name, r.declared, r.locked);
            }
        }
    }
    if rep.pass() { 0 } else { 1 }
}
