//! `jingwei watch <目录> [更多目录...]`：**把变更写进持久 journal**（2026-10-20，D-03 第 1 步）。
//!
//! 只做"记录"，不做"重扫"：重扫由 `discover`/`tools/scan-changed.mjs` 按 journal 决定。
//! **进程活着才收得到事件**；Ctrl-C 结束（journal 逐条已落盘，不需要优雅退出）。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub fn cmd_watch(default_root: &Path, roots: &[String], journal: Option<&str>) -> i32 {
    let mut dirs: Vec<PathBuf> = if roots.is_empty() {
        vec![default_root.to_path_buf()]
    } else {
        roots.iter().map(PathBuf::from).collect()
    };
    // 相对路径按当前目录展开成绝对——监听句柄要的是真路径。
    for d in &mut dirs {
        if d.is_relative() {
            if let Ok(cwd) = std::env::current_dir() {
                *d = cwd.join(&*d);
            }
        }
    }

    let j = journal
        .map(PathBuf::from)
        .unwrap_or_else(jingwei_engine::understand::dirwatch::default_journal);

    println!("监听 {} 个目录", dirs.len());
    for d in &dirs {
        println!("  {}", d.display());
    }
    println!("{}", jingwei_engine::understand::dirwatch::journal_hint(&j));
    println!("（Ctrl-C 结束；没在跑时「没有记录」不等于「没有变化」）");

    match jingwei_engine::understand::dirwatch::watch_roots(
        &dirs,
        &j,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("监听失败：{e}");
            2
        }
    }
}
