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

use crate::protocol::{envelope, Coverage, Evidence, State};

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
        // 统一协议：接手人一页纸也必须能回答"能不能下结论、凭什么、覆盖多少"。
        let state = if counts.is_none() {
            State::Unknown
        } else if gj["ready"].as_bool().unwrap_or(false) {
            State::Yes
        } else {
            State::No
        };
        let summary = gj["headline"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "查不了：还没法给出能不能跑的结论".to_string());
        let mut evidence = vec![if store.is_some() {
            Evidence::graph(root, "规模/热点/文件状态都来自图库")
        } else {
            Evidence::command("jingwei index", "图谱没建，规模与热点查不了")
        }];
        if !decs.is_empty() {
            evidence.push(Evidence::file(
                "docs/decisions/",
                &format!("最近 {} 条决策记录", decs.len()),
            ));
        }
        let coverage = match counts {
            Some((f, _, _, _)) => Coverage::full(
                usize::try_from(f).unwrap_or(0),
                "图库里的文件数就是这次读到的规模",
            ),
            None => Coverage::partial(0, None, "图谱没建：规模与热点都没读到（不是 0）"),
        };
        let out = envelope(
            state,
            summary,
            evidence,
            coverage,
            serde_json::json!({
                "what": gj["what"], "ready": gj["ready"], "headline": gj["headline"],
                "blockers": gj["blockers"], "nextStep": gj["nextStep"],
                // **首屏三数字**：文件数 / 总体积 / 生命周期判断（口径见 `metrics()`）
                "metrics": metrics(root),
                "counts": counts.map(|(f, s, c, r)| serde_json::json!({"files": f, "symbols": s, "calls": c, "references": r})),
                "entrances": tops.iter().map(|(f, n)| serde_json::json!({"file": f, "incomingCalls": n})).collect::<Vec<_>>(),
                "fileLife": lives,
                // **刚才**（2026-10-22）：接手人最想知道的除了"现在什么状态"，还有"上一件事做完没有"。
                // 与 `next` 同一个来源（任务日志），同一个形状。
                "recent_task": recent_task(),
                "decisions": decs.iter().map(|d| serde_json::json!({"id": d.id, "title": d.title, "why": d.why})).collect::<Vec<_>>(),
            }),
        );
        crate::protocol::print(&out);
        return 0;
    }

    println!();
    // 先说"刚才"，再说"是什么"（2026-10-22）：前者是最近的，后者是长期的。
    {
        let log = jingwei_engine::understand::tasks::tasks_path();
        let all = jingwei_engine::understand::tasks::load(&log);
        match all.last() {
            Some(t) => {
                let secs = t.ended_at.map(|e| (e - t.started_at).max(0)).unwrap_or(0);
                println!(
                    "  刚才：{} · {} · {} 秒{}",
                    t.name,
                    t.state,
                    secs,
                    t.detail.as_deref().map(|d| format!(" · {d}")).unwrap_or_default()
                );
                println!();
            }
            None => {
                println!("  刚才：还没有任务记录（跑一次 jingwei index / envs 就有）");
                println!();
            }
        }
    }
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
    // **三数字先印**（2026-10-22）：JSON 里一直有 metrics，文本版却只有"规模"（图库里的），
    // 少了"这个项目有多大、还算活着吗"。两处都走同一个 metrics()，不各算一份。
    {
        let m = metrics(root);
        let files = m["files"].as_u64().unwrap_or(0);
        let bytes = m["volume_bytes"].as_u64().unwrap_or(0);
        let capped = m["capped"].as_bool().unwrap_or(false);
        let life = m["lifecycle"]["state"].as_str().unwrap_or("unknown");
        println!(
            "  ③ 三数字：{} 个文件 / {}{} / {}  {}",
            files, human_bytes(bytes),
            if capped { "（只量了一部分）" } else { "" },
            life_text(life),
            m["lifecycle"]["basis"].as_str().unwrap_or("")
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
    // 主题一览：让接手人一眼看到"这个项目里能学什么"，而不是只知道有条数。
    // 主题来自条目（编辑态：`topic:` 允许逗号多写）；条数仍以图为准。
    let mut topics: Vec<(String, usize)> = Vec::new();
    for e in crate::cmd_knowledge::load_entries(root) {
        let Some(raw) = e.fields.get("topic") else {
            continue;
        };
        for t in raw.split([',', '，', '、', ';', '；']) {
            let t = t.trim();
            if t.is_empty() {
                continue;
            }
            match topics.iter_mut().find(|(name, _)| name == t) {
                Some((_, c)) => *c += 1,
                None => topics.push((t.to_string(), 1)),
            }
        }
    }
    if topics.is_empty() {
        return Some(format!(
            "  ③b 知识层：{n} 条主张 / {linked} 条已挂符号（明细 jingwei knowledge check）"
        ));
    }
    topics.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let shown: Vec<String> = topics
        .iter()
        .take(4)
        .map(|(t, c)| format!("{t} {c}"))
        .collect();
    let more = if topics.len() > 4 {
        format!(" 等 {} 个主题", topics.len())
    } else {
        String::new()
    };
    Some(format!(
        "  ③b 知识层：{n} 条主张 / {linked} 条已挂符号 · 主题：{}{more}（jingwei learn <主题> 照着走）",
        shown.join(" / ")
    ))
}

/// 首屏三数字（2026-10-21）：文件数 / 总体积 / 生命周期判断。
///
/// 口径（都不猜）：
/// - **文件数**、**总体积**：走 `understand::projects::measure`（有 4000 文件 / 120ms 的硬上限，
///   撞上限就 `capped=true`——**"只量了一部分"必须说出来**，不许把部分当成全部）；
/// - **生命周期**：只用 git 能证明的东西（最近提交时间 + 工作区有没有改动）；
///   没有 `.git` 就报 `no_git`，不说"新建/停更"这种没有依据的话。
pub(crate) fn metrics(root: &Path) -> serde_json::Value {
    let (files, bytes, capped) = jingwei_engine::understand::projects::measure(root);
    let (state, basis) = lifecycle(root);
    serde_json::json!({
        "files": files,
        "volume_bytes": bytes,
        "capped": capped,
        "lifecycle": {"state": state, "basis": basis},
    })
}

/// 生命周期：`new_git | active | recent | maintenance | stale | no_git | unknown`。
fn lifecycle(root: &Path) -> (&'static str, String) {
    if !root.join(".git").exists() {
        return ("no_git", "没有 .git：判断不了（需要版本库才行）".to_string());
    }
    let log = std::process::Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["log", "-1", "--format=%ct"])
        .output();
    let secs = match log {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse::<i64>()
            .unwrap_or(0),
        _ => return ("unknown", "git log 跑不出来：判断不了".to_string()),
    };
    if secs <= 0 {
        return ("unknown", "git log 没给出提交时间：判断不了".to_string());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(secs);
    let days = ((now - secs) / 86_400).max(0);
    let dirty = std::process::Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    let state = if days < 30 && dirty {
        "active"
    } else if days < 30 {
        "recent"
    } else if days < 180 {
        "maintenance"
    } else {
        "stale"
    };
    (
        state,
        format!(
            "最近提交 {days} 天前；工作区{}（git log -1 --format=%ct + git status --porcelain 可复跑）",
            if dirty { "有未提交改动" } else { "干净" }
        ),
    )
}

/// **"刚才"**（2026-10-22）：任务日志里最近一条，形状与 `jingwei next` 的 `recent_task` 一致。
///
/// 为什么放成函数而不是塞进 `json!` 宏里：宏里不能写语句块。也顺手让"两处同形"看得见。
fn recent_task() -> serde_json::Value {
    let log = jingwei_engine::understand::tasks::tasks_path();
    let all = jingwei_engine::understand::tasks::load(&log);
    match all.last() {
        Some(t) => serde_json::json!({
            "kind": format!("{:?}", t.kind).to_lowercase(),
            "name": t.name,
            "state": t.state,
            "elapsed_s": match t.ended_at { Some(e) => (e - t.started_at).max(0), None => 0 },
            "detail": t.detail,
        }),
        None => serde_json::Value::Null,
    }
}

/// 字节说人话（与界面 fmtBytes 同口径）。
fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", U[i]) }
}

/// 生命周期说人话：既给状态也给判据。
fn life_text(state: &str) -> &'static str {
    match state {
        "active" => "最近改过",
        "recent" => "这几个月动过",
        "maintenance" => "维护期",
        "stale" => "很久没动",
        "no_git" => "没有版本库，判断不了",
        _ => "查不了",
    }
}
