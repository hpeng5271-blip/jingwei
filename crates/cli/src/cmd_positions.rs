//! `jingwei positions`：**用原文当裁判**核一遍图谱里的行号（2026-10-19）。
//!
//! 与 `jingwei verify` 的区别：那个是"跑构建验证能不能跑"，
//! 这个是"回去读文件，看图谱记的位置对不对"——**用文件当裁判，不自证**。

use std::path::Path;

pub fn cmd_positions(root: &Path, json: bool) -> i32 {
    let db = root.join(".jingwei").join("graph.db");
    let store = match jingwei_engine::graph::store::Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("打不开图谱（先跑一次 jingwei index）：{e}");
            return 2;
        }
    };
    let rep = jingwei_engine::graph::verify::check_positions(root, &store);
    let deps = jingwei_engine::graph::verify::dep_edges_of(&store);
    if json {
        println!("{}", serde_json::to_string_pretty(&rep).unwrap_or_default());
    } else {
        println!("  位置回读：核了 {} 个符号", rep.checked());
        println!("    ✓ 对得上：{}", rep.ok);
        println!("    × 对不上：{}", rep.bad);
        println!("    ? 查不了：{}（文件缺失 / 行号越界，不算错）", rep.unknown);
        println!("    ~ 位置不准：{}（名字在文件里、不在那一行——不判错）", rep.imprecise);
        for s in &rep.samples {
            println!("    × {s}");
        }
        println!("  依赖边（File→Dependency）：{} 条", deps.len());
        for (f, d) in deps.iter().take(10) {
            println!("    {f} → {d}");
        }
    }
    if rep.pass() { 0 } else { 1 }
}
