//! `jingwei data` —— **数据台账**（2026-10-22，债 0.46 的"先量后合"）。
//!
//! 只回答一件事：`%LOCALAPPDATA%\jingwei` 里**每一份数据**多大、多少行、最后写于何时；
//! 事件另给"近 24 小时多少条"——这是判断"要不要统一事件流"的唯一依据。
//!
//! **不改结构、不清理**：这一版只读。统一/迁移要等观测一周之后（pro：先量后合）。

use crate::protocol::{envelope, print, Coverage, Evidence, State};
use jingwei_engine::understand::events::{cutoff, events_path, load_all};
use std::path::{Path, PathBuf};

/// 数据目录：`%LOCALAPPDATA%\jingwei`；`JINGWEI_DATA_DIR` 可覆盖（测试/隔离）。
fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("JINGWEI_DATA_DIR") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("jingwei")
}

pub(crate) fn cmd_data(rest: &[String], json: bool) -> i32 {
    let sub = rest.first().map(|s| s.as_str()).unwrap_or("stats");
    if sub != "stats" && sub != "stat" {
        eprintln!("用法：jingwei data [stats] [--json]");
        return 2;
    }
    let dir = data_dir();
    if !dir.exists() {
        return 查不了(
            json,
            &dir,
            "数据目录",
            "目录还不存在（这台机器上还没产生过数据）",
            "正常用一次（跑 jingwei index 或用界面打开一个文件）就会建目录",
            0,
        );
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut files: Vec<(String, u64, Option<u64>, Option<i64>)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let lines = if name.ends_with(".jsonl") {
                std::fs::read_to_string(&p)
                    .ok()
                    .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count() as u64)
            } else {
                None
            };
            let mtime = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            files.push((name, size, lines, mtime));
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    let total_bytes: u64 = files.iter().map(|f| f.1).sum();

    // 事件：总数 + 近 24 小时增量（判断"要不要统一事件流"的核心数字）
    let ev_path = events_path();
    let snap = load_all(&ev_path);
    let total_events: u64 = snap.counts.values().sum();
    let last_24h = snap.events.iter().filter(|e| e.at >= now - 86_400).count() as u64;
    let in_window = snap.events.iter().filter(|e| e.at >= cutoff(now)).count();

    let state = if files.is_empty() { State::Unknown } else { State::Yes };
    let summary = if files.is_empty() {
        "目录在，但里面还没有文件".to_string()
    } else {
        format!(
            "{} 份数据，共 {}；事件 {total_events} 次（近 24 小时 {last_24h} 条）",
            files.len(),
            human_bytes(total_bytes)
        )
    };

    if json {
        let v = envelope(
            state,
            summary,
            vec![Evidence::file(&dir.display().to_string(), "数据目录里的每一份文件都列在 files 里")],
            Coverage::full(files.len(), "目录读完了；只统计，不修改"),
            serde_json::json!({
                "dir": dir.display().to_string(),
                "total_bytes": total_bytes,
                "files": files.iter().map(|(n, size, lines, mtime)| serde_json::json!({
                    "name": n, "bytes": size, "lines": lines, "mtime": mtime,
                })).collect::<Vec<_>>(),
                "events": {
                    "total": total_events,
                    "last_24h": last_24h,
                    "in_window": in_window,
                },
            }),
        );
        print(&v);
        return 0;
    }

    println!("{summary}");
    println!();
    println!("  文件                            大小      行     最后写入");
    for (n, size, lines, mtime) in &files {
        println!(
            "  {:<28} {:>8}  {:>6}  {}",
            n,
            human_bytes(*size),
            lines.map(|l| l.to_string()).unwrap_or_else(|| "—".into()),
            mtime.map(fmt_utc).unwrap_or_else(|| "—".into())
        );
    }
    if files.is_empty() {
        println!("  （空）");
    }
    println!();
    println!(
        "  事件：{total_events} 次累计（近 24 小时 {last_24h} 条，90 天窗口内 {in_window} 条有详情）"
    );
    0
}

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
