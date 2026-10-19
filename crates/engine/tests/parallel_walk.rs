//! **并行扫 == 串行扫**（2026-10-19）：并行只许更快，不许改变结果。
//!
//! 这条测试是并行的"安全网"：判据共用 `skip_reason` / `row_for`，
//! 但队列、在路上的计数、提前退出都可能写出偏差——只比值集合不够，
//! 要比"每个目录的行 + 跳过原因"逐项相等。

#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::understand::dirs_index;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-par-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("a/b/c")).unwrap();
    std::fs::create_dir_all(d.join("a/node_modules")).unwrap();
    std::fs::create_dir_all(d.join("d/.git")).unwrap();
    std::fs::create_dir_all(d.join("d/e")).unwrap();
    std::fs::write(d.join("a/Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    std::fs::write(d.join("a/b/c/package.json"), "{}").unwrap();
    std::fs::write(d.join("d/e/pyproject.toml"), "").unwrap();
    d
}

fn digest(idx: &dirs_index::DirsIndex) -> Vec<String> {
    let mut v: Vec<String> = idx
        .rows
        .iter()
        .map(|(p, r)| format!("{}|{:?}", p.display(), r.skipped))
        .collect();
    v.sort();
    v
}

#[test]
fn 并行扫与串行扫结果一致() {
    let root = fixture("same");
    let stop = AtomicBool::new(false);

    let seq = dirs_index::build_with(&root, 4, &stop, |_, _, _| {});
    let par = dirs_index::build_parallel(&root, 4, 4, &stop, |_, _, _| {});

    assert_eq!(digest(&par), digest(&seq), "目录行与跳过原因必须逐项相等");
    assert_eq!(
        par.all_projects().len(),
        seq.all_projects().len(),
        "项目数必须一致"
    );
    assert_eq!(par.visited, seq.visited, "进过的目录数一致");
    assert_eq!(par.skipped, seq.skipped, "跳过数一致");

    let _ = std::fs::remove_dir_all(&root);
}

/// `stop` 一置位就**不能继续扩**（否则用户点"停"没用）。
#[test]
fn 停一下就停() {
    let root = fixture("stop");
    let stop = AtomicBool::new(true);
    let idx = dirs_index::build_parallel(&root, 4, 4, &stop, |_, _, _| {});
    assert!(idx.visited <= 1, "已经置位 stop 就不该再扫，实际 {}", idx.visited);
    let _ = std::fs::remove_dir_all(&root);
}
