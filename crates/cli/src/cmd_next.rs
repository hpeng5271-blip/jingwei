//! `jingwei next`：**一屏回答"现在该做什么"**（2026-10-21）。
//!
//! 为什么要有它：状态散在四处——学习进度在本地 JSON、待重编在 `claims` 图里、
//! 候选在 `_inbox/`、文本态在条目里。每天第一句问的就是"今天干什么"，却要敲四条命令。
//!
//! **它只汇总已有的判断**：不新增任何判定逻辑，每行都能用括号里那条命令复跑核对。
//! 因此它不是"新能力"，是**入口**——这也是它不该改变任何状态的原因（纯读）。

use std::path::Path;

use crate::cmd_learn::{load_progress, plan, progress_path};

pub fn cmd_next(root: &Path, json: bool) -> i32 {
    // ① 学：本地进度 + 各主题的路径（与 `learn <主题>` 同一个 plan()）
    let done = load_progress(&progress_path());
    let entries = crate::cmd_knowledge::load_entries(root);
    let mut topics: Vec<String> = Vec::new();
    for e in &entries {
        let Some(raw) = e.fields.get("topic") else {
            continue;
        };
        for t in raw.split([',', '，', '、', ';', '；']) {
            let t = t.trim();
            if !t.is_empty() && !topics.iter().any(|x| x == t) {
                topics.push(t.to_string());
            }
        }
    }
    topics.sort();
    let mut learn_next: Option<String> = None;
    let mut learned_total = 0usize;
    let mut steps_total = 0usize;
    for t in &topics {
        let Ok(p) = plan(root, t) else {
            continue;
        };
        for (i, s) in p.iter().enumerate() {
            steps_total += 1;
            if done.contains(&s.entry) {
                learned_total += 1;
            } else if learn_next.is_none() {
                learn_next = Some(format!("{t} 第 {}/{} 步：{}", i + 1, p.len(), s.claim));
            }
        }
    }

    // ② 回收 / ③ 候选 / ④ 文本态（都只读）
    let store = crate::cmd_knowledge::open_graph(root);
    let stale = store
        .as_ref()
        .map(|s| s.stale_claims().map(|v| v.len()).unwrap_or(0))
        .unwrap_or(0);
    let inbox = std::fs::read_dir(root.join("docs").join("knowledge").join("_inbox"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    let unmapped = entries
        .iter()
        .filter(|e| e.symbols.is_empty() && crate::cmd_knowledge::why_rejected(root, e).is_none())
        .count();

    if json {
        println!(
            "{}",
            serde_json::json!({
                "learn": {"learned": learned_total, "total": steps_total, "next": learn_next},
                "stale": stale,
                "inbox": inbox,
                "text_only": unmapped,
            })
        );
        return 0;
    }

    println!("现在该做什么（每行括号里是对应的命令，可复跑核对）：");
    match &learn_next {
        Some(n) => println!("  学　{n}（jingwei learn <主题>；--mark <序号> 打勾）"),
        None if steps_total == 0 => println!("  学　查不了：docs/knowledge/ 里还没有带 topic 的条目"),
        None => println!(
            "  学　全部学完（{learned_total}/{steps_total}）（jingwei progress 看明细）"
        ),
    }
    if stale > 0 {
        println!("  收　待重编 {stale} 条（jingwei knowledge stale → fix）");
    }
    if inbox > 0 {
        println!("  编　待确认候选 {inbox} 条（jingwei knowledge add <名字>，先补 check）");
    }
    if unmapped > 0 {
        println!("  挂　文本态 {unmapped} 条没挂符号（只能按主题学，不能按符号反查）");
    }
    if stale == 0 && inbox == 0 && unmapped == 0 {
        println!("  干净：没有待回收、没有待确认、没有文本态");
    }
    // 项目侧下一步：与 `handover` ⑤ 同一来源（guide 的 nextStep），不另算一套。
    let facts = jingwei_engine::detect::probe::make_facts(root, None);
    let g = jingwei_engine::understand::guide::make_guide(&facts, &[]).to_json();
    match g["nextStep"].as_str() {
        Some(s) if !s.trim().is_empty() && s != "查不了" => {
            println!("  做　{s}（jingwei handover 看完整一页）");
        }
        _ => println!("  做　查不了（这个项目没法给出下一步；jingwei handover 看细节）"),
    }
    0
}
