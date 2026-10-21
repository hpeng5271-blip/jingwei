//! `jingwei positions`：**用原文当裁判**核一遍图谱里的行号（2026-10-19）。
//!
//! 与 `jingwei verify` 的区别：那个是"跑构建验证能不能跑"，
//! 这个是"回去读文件，看图谱记的位置对不对"——**用文件当裁判，不自证**。

use std::path::Path;

use crate::protocol::{envelope, print, Coverage, Evidence, State};

pub fn cmd_positions(root: &Path, json: bool) -> i32 {
    let db = root.join(".jingwei").join("graph.db");
    // **只读打开**（2026-10-20 修）：以前用 `Store::open`，没索引过的项目会被**建出一个空库**，
    // 然后打印「✓ 对得上 0 / × 对不上 0」并 exit=0——而"对不上必须是 0"正是验收判据，
    // 于是**没索引的项目凭空通过**。查不了就是查不了，不许算通过。
    if !db.exists() {
        return 查不了(
            json,
            "查不了：还没索引（先 jingwei index）——位置回读没有可核的对象",
            1,
        );
    }
    let store = match jingwei_engine::graph::query::open_for_query(root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("打不开图谱（先跑一次 jingwei index）：{e}");
            return 2;
        }
    };
    if store.symbol_count().unwrap_or(0) == 0 {
        return 查不了(
            json,
            "查不了：图谱里 0 个符号（还没索引完？）——位置回读没有可核的对象",
            1,
        );
    }
    let rep = jingwei_engine::graph::verify::check_positions(root, &store);
    let deps = jingwei_engine::graph::verify::dep_edges_of(&store);
    if json {
        let total = store.symbol_count().unwrap_or(0);
        let state = if rep.pass() { State::Yes } else { State::No };
        let summary = format!(
            "核了 {} 个符号：✓{} / ×{} / ?{}（查不了） / ~{}（位置不准）",
            rep.checked(),
            rep.ok,
            rep.bad,
            rep.unknown,
            rep.imprecise
        );
        let v = envelope(
            state,
            summary,
            vec![
                Evidence::graph(root, "位置回读的裁判是图库里的行号"),
                Evidence::command(
                    "jingwei positions",
                    "回读源文件原文核行号；文件缺失/越界算查不了，不算错",
                ),
            ],
            Coverage::full(rep.checked(), &format!("核了 {} 个符号（共 {total} 个）", rep.checked())),
            serde_json::json!({
                "positions": rep,
                "dep_edges": deps.len(),
            }),
        );
        print(&v);
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

/// 「查不了」也必须走协议：`--json` 的用户拿到的是**结构**，不是一句中文。
///
/// 这条以前是 bug：没索引时 `positions --json` 打的是纯文本，脚本按 JSON 解析直接炸，
/// 而人看到的却是"查不了"——**两种出口两种说法**，正是这套协议要消灭的东西。
fn 查不了(json: bool, why: &str, code: i32) -> i32 {
    if json {
        let v = envelope(
            State::Unknown,
            why,
            vec![
                Evidence::command("jingwei index", "先把图谱建起来，才有可核的对象"),
                Evidence::file(".jingwei/graph.db", "库不存在或没有符号"),
            ],
            Coverage::partial(0, None, "没有可核的对象（不是 0 个错，是没核）"),
            serde_json::json!({}),
        );
        print(&v);
    } else {
        println!("{why}");
    }
    code
}
