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
    // 分级提醒：一手不到一半时，说明大半结论是转写/二手——学的时候要打折。
    let mut passed = 0usize;
    let mut first_hand = 0usize;
    for e in &entries {
        if crate::cmd_knowledge::why_rejected(root, e).is_some() {
            continue;
        }
        passed += 1;
        if e.fields.get("level").map(|l| l == "一手").unwrap_or(false) {
            first_hand += 1;
        }
    }
    let weak = passed > 0 && first_hand * 2 < passed;

    if json {
        // **统一输出协议**（2026-10-21）：结论 + 三态 + 依据 + 覆盖度。
        //
        // 为什么这四样必须一起出：GUI 概览卡、编辑器状态栏、报告窗都渲染同一份数据；
        // 只给结论不给依据/覆盖度，用户就没法判断"这话能信到什么程度"——那不是信息，是噪声。
        //
        // `state` 回答的是"这个项目的结论能不能下"：
        //   unknown = 没有可判断的对象（无知识条目、无图谱）
        //   no      = 有待处理项（待重编/待确认/文本态没挂符号），现在下结论会误导
        //   yes     = 可以下结论
        let state = if store.is_none() && entries.is_empty() {
            "unknown"
        } else if stale > 0 || inbox > 0 || unmapped > 0 {
            "no"
        } else {
            "yes"
        };
        let summary = if state == "unknown" {
            "查不了：这个项目还没有可判断的东西（无知识条目、无图谱）".to_string()
        } else if state == "no" {
            format!("先处理 {stale} 条待重编 / {inbox} 条待确认 / {unmapped} 条文本态，再下结论")
        } else if let Some(n) = &learn_next {
            format!("可以下结论；下一步：{n}")
        } else if steps_total == 0 {
            "可以下结论；还没有带 topic 的学习条目".to_string()
        } else {
            "可以下结论；学习已全部完成".to_string()
        };
        // 依据：每条都是**能自己复跑**的东西（文件/命令/库），不是"我觉得"。
        let mut evidence = Vec::new();
        evidence.push(serde_json::json!({
            "kind": if store.is_some() { "graph" } else { "command" },
            "ref": if store.is_some() {
                format!("{}/.jingwei/graph.db", root.display())
            } else {
                "jingwei index".to_string()
            },
            "note": if store.is_some() { "图谱已建，符号层可用" } else { "图谱未建，符号层查不了" },
        }));
        if !entries.is_empty() {
            evidence.push(serde_json::json!({
                "kind": "file",
                "ref": "docs/knowledge/",
                "note": format!("知识条目 {} 条（全部读过）", entries.len()),
            }));
        }
        println!(
            "{}",
            serde_json::json!({
                "state": state,
                "summary": summary,
                "evidence": evidence,
                // 覆盖度：read 是"这次真的读了几个条目"，total 是全量（知识目录我们是全读的）；
                // 图谱没建就是 partial——**不许把"没读"报成 0**。
                "coverage": {
                    "read": entries.len(),
                    "total": entries.len(),
                    "partial": store.is_none(),
                    "note": if store.is_some() {
                        "知识层全读；图谱已建".to_string()
                    } else {
                        "知识层全读；图谱未建（先 jingwei index）".to_string()
                    },
                },
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
    if weak {
        println!(
            "  弱　一手 {first_hand}/{passed} 不到一半：转写/二手的结论当「线索」看，别当判据（jingwei knowledge check 看分级）"
        );
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
    // **第三层：依据与边界**（2026-10-21）。前两层的每一行都带命令（= 你自己能验），
    // 这一层回答"这些结论是从哪出来的、什么时候不该信"。三处同源：都从上面那几个数来，不另算。
    println!(
        "  依据：知识 {} 条（一手 {} 条）· 图谱{}",
        passed,
        first_hand,
        if store.is_some() {
            "已建"
        } else {
            "还没建（先 jingwei index）"
        }
    );
    println!(
        "  边界：只看静态事实——动态派发/泛型派发不连，所以「查不了」≠「没有」；这几行不是全部，只是当前最该做的"
    );
    0
}


