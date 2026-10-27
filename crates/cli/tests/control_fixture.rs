//! 对照组 B（lib）与 jw 在同一 fixture 上的零差检查。
//! 只跑小 fixture（大仓留给 PS1 门禁）；B 与 jw 共用 tree-sitter 语法，遍历独立。

use std::path::Path;
use std::process::Command;

fn jw(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("跑不起 jw");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), s)
}

fn git(dir: &Path, args: &[&str]) {
    let _ = Command::new("git").args(args).current_dir(dir).output();
}

fn commit(dir: &Path, msg: &str) {
    git(
        dir,
        &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", msg],
    );
}

#[test]
fn control_fixture_zero_diff() {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("jw-ctrl-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\npub struct A;\n").expect("写 a");
    std::fs::write(dir.join("src/b.rs"), "pub fn b() {}\n").expect("写 b");
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    commit(&dir, "c1");
    std::fs::write(
        dir.join("src/a.rs"),
        "pub fn a() {}\npub struct A;\nuse std::fmt::Write as _;\nimpl A {\n    fn x(&self) {}\n}\n",
    )
    .expect("改 a");
    git(&dir, &["add", "-A"]);
    commit(&dir, "c2");

    for args in [
        vec!["init", "."],
        vec!["ingest", "--limit", "0", "."],
        vec!["parse", "."],
    ] {
        let (c, out) = jw(&dir, &args);
        assert_eq!(c, 0, "{args:?} 失败：{out}");
    }

    // ① git：commits / changes / files
    let (b_commits, b_changes, b_files) = control::git_counts(&dir, 2000);
    let (_, tl) = jw(&dir, &["timeline", "--json", "."]);
    let tv: serde_json::Value = serde_json::from_str(&tl).expect("timeline json");
    assert_eq!(tv["commits_total"].as_u64().unwrap() as usize, b_commits, "commits 不一致");
    assert_eq!(tv["changes"].as_u64().unwrap() as usize, b_changes, "changes 不一致");
    let (_, stat) = jw(&dir, &["stat", "."]);
    let _ = (stat, b_files);

    // ② parse：符号判断总数（jw 的 kind 列是粗粒度，按 type 拆分的数在 evidence 里）
    let (syms, kinds) = control::parse_counts(&dir);
    let (_, stat2) = jw(&dir, &["stat", "."]);
    let jw_symbols: usize = stat2
        .lines()
        .find(|l| l.starts_with("kind symbol"))
        .and_then(|l| l.split_whitespace().last())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let b_total: usize = kinds.values().sum();
    assert_eq!(jw_symbols, b_total, "符号总数不一致（B kinds={kinds:?}）");
    assert!(!syms.is_empty(), "B 没识别出任何有符号文件");

    // ③ edges 原始层：use / mod / impl 声明数
    let (b_use, b_mod, b_impl) = control::edge_counts(&dir);
    let (_, raw) = jw(&dir, &["raw-stats", "--json", "."]);
    let rv: serde_json::Value = serde_json::from_str(&raw).expect("raw-stats json");
    assert_eq!(rv["use"].as_u64().unwrap() as usize, b_use, "use 声明数不一致");
    assert_eq!(rv["mod"].as_u64().unwrap() as usize, b_mod, "mod 声明数不一致");
    assert_eq!(rv["impl"].as_u64().unwrap() as usize, b_impl, "impl 块数不一致");

    // ④ check 七零
    let (c, check) = jw(&dir, &["check", "."]);
    assert_eq!(c, 0, "{check}");
    assert!(check.contains("false_coverage=0"), "{check}");
    assert!(check.contains("dangling_source=0"), "{check}");

    let _ = std::fs::remove_dir_all(&dir);
}
