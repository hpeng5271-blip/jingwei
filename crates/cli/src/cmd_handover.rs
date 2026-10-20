//! `jingwei handover`（2026-10-20）：**给接手人的一页纸**。
//!
//! 来自外部观点的吸收（原子能的"知识衰减定律"）：交接的真正成本在"人脑子里的那部分"。
//! 这个命令只做一件事——把**已经有来源的事实**按"接班先问什么"的顺序摆出来。
//!
//! **三态**：图谱没建 → "规模/入口 查不了（先 index）"，但"能不能跑"仍能说；
//! 项目里没有欠账记录 → 明说"这个项目没有欠账记录"，不编。

use std::path::Path;

use jingwei_engine::graph::query;
use jingwei_engine::understand::guide;

pub fn cmd_handover(root: &Path, json: bool) -> i32 {
    let facts = jingwei_engine::detect::probe::make_facts(root, None);
    let g = guide::make_guide(&facts, &[]);
    let gj = g.to_json();

    let store = query::open_for_query(root).ok();
    let counts = store.as_ref().and_then(|s| query::basic_counts(s).ok());
    let tops = store
        .as_ref()
        .and_then(|s| query::top_incoming_files(s, 5).ok())
        .unwrap_or_default();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "what": gj["what"], "ready": gj["ready"], "headline": gj["headline"],
                "blockers": gj["blockers"], "nextStep": gj["nextStep"],
                "counts": counts.map(|(f, s, c, r)| serde_json::json!({"files": f, "symbols": s, "calls": c, "references": r})),
                "entrances": tops.iter().map(|(f, n)| serde_json::json!({"file": f, "incomingCalls": n})).collect::<Vec<_>>(),
            }))
            .unwrap_or_default()
        );
        return 0;
    }

    println!();
    println!("  ① 是什么：{}", gj["what"].as_str().unwrap_or("查不了"));
    println!("  ② 能不能跑：{}", gj["headline"].as_str().unwrap_or("查不了"));
    if let Some(bs) = gj["blockers"].as_array() {
        for (i, b) in bs.iter().take(3).enumerate() {
            println!(
                "     挡路 {}：{} —— {}",
                i + 1,
                b["what"].as_str().unwrap_or(""),
                b["why"].as_str().unwrap_or("")
            );
        }
    }
    match counts {
        Some((f, s, c, r)) => println!("  ③ 规模：{f} 文件 / {s} 符号 / {c} 调用边 / {r} 引用边"),
        None => println!("  ③ 规模：查不了（图谱还没建，先 `jingwei index`）"),
    }
    if tops.is_empty() {
        println!("  ④ 热点：查不了（没有调用边，或还没索引）");
    } else {
        println!("  ④ 热点（被调用最多的文件；多半是公共底层，不一定是入口）：");
        for (f, n) in &tops {
            println!("      {f}  （被调用 {n} 条）");
        }
    }
    println!("  ⑤ 下一步：{}", gj["nextStep"].as_str().unwrap_or("查不了"));
    println!("  ⑥ 欠什么：查不了（这个项目里没有欠账记录；欠账表只在经纬自己的仓库里）");
    println!();
    0
}
