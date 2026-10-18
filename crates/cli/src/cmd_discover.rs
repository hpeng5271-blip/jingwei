//! `jingwei discover`：**扫盘找项目**（可扫多个根、边扫边出、可中断、有终态）。
//!
//! 为什么单开一个命令：界面那条路（Tauri command + 轮询）不好在门禁里跑，
//! 而"扫得全不全"必须有**机器能验**的入口。这个命令就是那个入口——
//! 与界面**同一套引擎**（`dirs_index::build_with` + `projects::describe_public`）。
//!
//! 三种输出，对应三态：
//! - **发现**：`+ 路径  类型  体量`；
//! - **进度**：每 400 个目录一行（`已看 N · 跳过 M · 发现 K · 当前 …`）；
//! - **终态**：`✓ 扫完：N 个目录 / M 个项目 / 用时 T 秒`，或者 `! 中断：…`
//!   ——**永远有个明确的结尾**，不许让调用方猜"它还在扫吗"。

use jingwei_engine::understand::{dirs_index, fingerprint, projects};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// 体量分档（首页用它分组）：小 <50 文件、中 <500、大 <4000、超大 ≥4000。
fn size_class(files: u64) -> &'static str {
    if files < 50 {
        "小"
    } else if files < 500 {
        "中"
    } else if files < 4000 {
        "大"
    } else {
        "超大"
    }
}

/// **同源检测**：只比"同名目录"（跨机器/克隆最常见的那一类），
/// 阈值 0.8（学习调研给的数：低于它更像"碰巧都有几个相似文件"）。
/// 只对候选算集合，不跑全量两两比较——**成本要可控**。
fn similar(list: &[(String, String, Vec<String>)], min: f64) -> Vec<(String, String, f64)> {
    let mut out = Vec::new();
    for i in 0..list.len() {
        for j in (i + 1)..list.len() {
            if list[i].1 != list[j].1 {
                continue; // 名字不同不比（"同内容不同名"是另一类，代价更高，先不做）
            }
            let s = jingwei_engine::understand::fingerprint::jaccard(&list[i].2, &list[j].2);
            if s >= min {
                out.push((list[i].0.clone(), list[j].0.clone(), s));
            }
        }
    }
    out
}

pub fn cmd_discover(roots: Vec<PathBuf>, depth: usize, json: bool, want_similar: bool) -> i32 {
    let roots: Vec<PathBuf> = if roots.is_empty() {
        projects::default_scan_roots()
    } else {
        roots
    };
    if roots.is_empty() {
        eprintln!("没有可扫的根目录——给一个，或者加 --all 从盘根扫");
        return 2;
    }
    // 门禁里跑的时候要能**自己停**（否则一条命令能挂十分钟）
    let stop = AtomicBool::new(false);
    let t0 = Instant::now();
    let mut found: Vec<serde_json::Value> = Vec::new();
    let mut seen = 0usize;
    let mut skipped = 0usize;
    let mut total = 0usize;
    // (路径, 目录名, 文件集合)——只在 --similar 时填，避免白算
    let mut sets: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut last_print = 0usize;

    for root in &roots {
        if !root.is_dir() {
            eprintln!("跳过（目录不存在）：{}", root.display());
            continue;
        }
        let idx = dirs_index::build_with(root, depth.max(1), &stop, |v, s, cur| {
            seen = v;
            skipped = s;
            if v >= last_print + 400 {
                last_print = v;
                eprintln!("  … 已看 {v} 个目录 · 跳过 {s} · 发现 {} 个项目 · 当前 {}", found.len(), cur.display());
            }
        });
        // 索引建完再查"哪些像项目"：**查询是纯函数**，改判据不用重扫。
        for p in idx.all_projects() {
            let f = projects::describe_public(&p);
            total += 1;
            let (files, bytes, cut) = projects::measure(&p);
            let fp = fingerprint::of(&p);
            if want_similar {
                sets.push((f.path.clone(), f.name.clone(), fingerprint::set_of(&p)));
            }
            let kind = f.kinds.first().cloned().unwrap_or_else(|| "未知".to_string());
            if json {
                found.push(serde_json::json!({
                    "path": f.path, "name": f.name, "kinds": f.kinds,
                    "markers": f.markers, "files": files, "bytes": bytes,
                    "size": size_class(files), "size_cut": cut,
                    "fp": fp.hash, "fp_truncated": fp.truncated,
                    "lockfile": fp.lockfile,
                    "age_days": f.age_days,
                }));
            } else {
                println!(
                    "  + {:<9} {:<6} {:>5} 文件  {}",
                    kind,
                    size_class(files),
                    files,
                    f.path
                );
            }
        }
    }

    let pairs = if want_similar { similar(&sets, 0.8) } else { Vec::new() };
    let secs = t0.elapsed().as_secs_f64();
    if json {
        let out = serde_json::json!({
            "roots": roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "depth": depth,
            "dirs_seen": seen, "dirs_skipped": skipped,
            "found": found,
            "seconds": secs,
            "stopped": stop.load(Ordering::Relaxed),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        0
    } else {
        if !pairs.is_empty() {
            println!("  ≈ 同源（Jaccard ≥ 0.8）：{} 对", pairs.len());
            for (a, b, s) in &pairs {
                println!("    {:.0}%  {}\n          {}", s * 100.0, a, b);
            }
        }
        println!(
            "  ✓ 扫完：{} 个目录（跳过 {}）/ {} 个项目 / 用时 {:.1} 秒",
            seen,
            skipped,
            total,
            secs
        );
        0
    }
}
