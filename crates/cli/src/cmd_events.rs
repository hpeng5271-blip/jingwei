//! `jingwei events` —— 使用事件的**读侧**（2026-10-22，债 0.45 闭环）。
//!
//! 写侧在 `engine::understand::events::Recorder`；这里只读：给计数 + 90 天内的详情。
//!
//! **三态**：没有事件文件 → `unknown`（"还没采集"不等于"没有行为"），带 what/why/how；
//! 有事件但全在 90 天外 → `yes`，但 recent 为空、counts 有数（这正是"只留计数"的样子）。

use crate::protocol::{envelope, print, Coverage, Evidence, State};
use jingwei_engine::understand::events::{cutoff, events_path, load_all, DETAIL_DAYS};
use std::path::Path;

/// 详情最多显示多少条（够回答"最近在干什么"，又不至于刷屏）。
const RECENT_MAX: usize = 20;

pub(crate) fn cmd_events(rest: &[String], json: bool) -> i32 {
    let sub = rest.first().map(|s| s.as_str()).unwrap_or("stats");
    if sub != "stats" && sub != "stat" {
        eprintln!("用法：jingwei events [stats] [--json]");
        return 2;
    }
    let path = events_path();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if !path.exists() {
        return 查不了(
            json,
            &path,
            "这件事",
            "还没采集过任何使用事件（事件文件不存在）",
            "正常使用一次界面（打开一个文件、跑一次检查）就会开始记；或先跑 jingwei index",
            0,
        );
    }

    let snap = load_all(&path);
    let line = cutoff(now);
    let mut recent: Vec<_> = snap
        .events
        .iter()
        .filter(|e| e.at >= line)
        .collect();
    recent.sort_by_key(|e| e.at);
    let shown: Vec<_> = recent.iter().rev().take(RECENT_MAX).collect();

    let mut counts: Vec<(String, String, u64)> = snap
        .counts
        .iter()
        .map(|((k, key), n)| (k.clone(), key.clone(), *n))
        .collect();
    counts.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));

    let total: u64 = counts.iter().map(|c| c.2).sum();
    let state = if total == 0 { State::Unknown } else { State::Yes };
    let summary = if total == 0 {
        "事件文件在，但还没有任何记录".to_string()
    } else {
        format!(
            "共 {total} 次动作（{} 种），90 天内 {} 条有详情",
            counts.len(),
            snap.events.iter().filter(|e| e.at >= line).count()
        )
    };

    if json {
        let v = envelope(
            state,
            summary,
            vec![Evidence::file(
                &path.display().to_string(),
                &format!("{} 行事件；计数与详情都从这里算", snap.events.len()),
            )],
            Coverage::full(
                snap.events.len(),
                &format!("读到全部事件；详情窗口 {DETAIL_DAYS} 天"),
            ),
            serde_json::json!({
                "path": path.display().to_string(),
                "window_days": DETAIL_DAYS,
                "total": total,
                "kinds": counts.len(),
                "counts": counts.iter().map(|(k, key, n)| serde_json::json!({"kind": k, "key": key, "n": n})).collect::<Vec<_>>(),
                "recent": shown.iter().map(|e| serde_json::json!({
                    "at": e.at, "kind": e.kind, "key": e.key, "detail": e.detail,
                })).collect::<Vec<_>>(),
            }),
        );
        print(&v);
        return 0;
    }

    println!("{summary}");
    if counts.is_empty() {
        println!("  还没记到任何动作。");
        return 0;
    }
    println!();
    println!("  按次数（长期保留，不受 {DETAIL_DAYS} 天窗口影响）：");
    for (kind, key, n) in counts.iter().take(10) {
        println!("    {n:>4}  {kind:<8} {key}");
    }
    if counts.len() > 10 {
        println!("    …还有 {} 组", counts.len() - 10);
    }
    if shown.is_empty() {
        println!();
        println!("  最近：没有（{DETAIL_DAYS} 天内没有动作，只剩计数）");
    } else {
        println!();
        println!("  最近 {} 条（{DETAIL_DAYS} 天内）：", shown.len());
        for e in &shown {
            let when = fmt_utc(e.at);
            match &e.detail {
                Some(d) => println!("    {when}  {:<8} {}  ·  {d}", e.kind, e.key),
                None => println!("    {when}  {:<8} {}", e.kind, e.key),
            }
        }
    }
    0
}

/// unix 秒 → `MM-DD HH:MM`（UTC；不假装本地时区）。
fn fmt_utc(secs: i64) -> String {
    let s = secs.max(0) as u64;
    let days = (s / 86_400) as i64;
    let rem = s % 86_400;
    let (_, m, d) = civil_from_days(days);
    format!("{m:02}-{d:02} {:02}:{:02}", rem / 3600, (rem % 3600) / 60)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn 查不了(json: bool, path: &Path, what: &str, why: &str, how: &str, code: i32) -> i32 {
    if json {
        let v = envelope(
            State::Unknown,
            format!("{what}：{why}"),
            vec![Evidence::file(&path.display().to_string(), why)],
            Coverage::partial(0, None, why),
            serde_json::json!({ "what": what, "why": why, "how": how }),
        );
        print(&v);
    } else {
        println!("{what}：{why}");
        println!("  怎么补：{how}");
    }
    code
}
