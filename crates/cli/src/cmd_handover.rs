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
    let lives = store.as_ref().and_then(|s| query::file_life_counts(s).ok()).unwrap_or_default();
    let decs = jingwei_engine::understand::decisions::recent(root, 3);
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
                "fileLife": lives,
                "decisions": decs.iter().map(|d| serde_json::json!({"id": d.id, "title": d.title, "why": d.why})).collect::<Vec<_>>(),
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
    if !lives.is_empty() {
        let get = |k: &str| lives.get(k).copied().unwrap_or(0);
        println!(
            "      文件状态：改 {} / 未变 {} / 删 {} / 未见 {}",
            get("modified"), get("unchanged"), get("deleted"), get("unseen")
        );
    }
    match counts {
        Some((f, s, c, r)) => println!("  ③ 规模：{f} 文件 / {s} 符号 / {c} 调用边 / {r} 引用边"),
        None => println!("  ③ 规模：查不了（图谱还没建，先 `jingwei index`）"),
    }
    // **知识层**（2026-10-20，pro 任务卡 G）：接手人要看到"这个项目积累了多少主张、多少挂上了代码"。
    // 没有主张就整行不打印——空着比编一句"0 条"更能说明"这里还什么都没积累"。
    if let Some(n) = knowledge_line(root) {
        println!("{n}");
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
    if decs.is_empty() {
        println!("  ⑦ 关键决策与理由：查不了（docs/decisions/ 里没有记录）");
    } else {
        println!("  ⑦ 关键决策与理由（know-why）：");
        for d in &decs {
            match &d.why {
                Some(w) => println!("      {} {} —— {}", d.id, d.title, w),
                None => println!("      {} {} —— 查不了（记录里没写理由）", d.id, d.title),
            }
        }
    }
    println!();
    0
}

/// 知识层那一行：主张数 + 挂上代码的条数。
///
/// 为什么要它：接手人真正怕的是"这套东西只剩代码，脑子里那部分没了"。
/// 主张是**知识衰减**的对冲物——不显示，等于不知道有没有积累。
/// 没有主张就返回 None（整行不打印）：空着比编一句"0 条"更能说明"这里还什么都没积累"。
fn knowledge_line(root: &Path) -> Option<String> {
    let s = query::open_for_query(root).ok()?;
    let n = s.claims_count().ok()?;
    if n == 0 {
        return None;
    }
    let linked = s.claims_linked_count().unwrap_or(0);
    Some(format!(
        "  ③b 知识层：{n} 条主张 / {linked} 条已挂符号（明细 jingwei knowledge check）"
    ))
}
