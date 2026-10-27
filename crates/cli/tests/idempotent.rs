//! 幂等：docs / parse 用文件 mtime 当 at，重跑不翻倍。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

fn run(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("跑不起 jw");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), s)
}

fn temp_dir(tag: &str) -> PathBuf {
    // tag 在本文件内唯一：固定到 pid 命名，同名先删再建，避免每轮在 %TEMP% 留新目录
    let d = std::env::temp_dir().join(format!("jw-idem-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn stat_count(out: &str, table: &str) -> i64 {
    out.lines()
        .find(|l| l.starts_with(table))
        .and_then(|l| l.split_whitespace().last())
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1)
}

fn bump_mtime(path: &Path, by_secs: u64) {
    let f = std::fs::File::options().write(true).open(path).expect("开文件");
    f.set_modified(SystemTime::now() + Duration::from_secs(by_secs))
        .expect("set mtime");
}

#[test]
fn search_escapes_like_wildcards() {
    // `%` / `_` 必须按字面匹配：搜 `%` 只该命中名字里真有 % 的 subject
    let dir = temp_dir("search");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写文件");
    std::fs::write(dir.join("a%bc.md"), "# pct\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["search", "%", "."]);
    assert_eq!(c, 0, "{out}");
    let hits: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("doc:"))
        .collect();
    assert_eq!(hits.len(), 1, "搜 % 应只命中 1 条，实际：{out}");
    assert!(hits[0].contains("a%bc.md"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn docs_import_is_idempotent() {
    let dir = temp_dir("docs");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n\n正文\n").expect("写文件");

    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (_, first) = run(&dir, &["stat", "."]);
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (_, second) = run(&dir, &["stat", "."]);

    for table in ["judgment", "evidence", "source"] {
        assert_eq!(
            stat_count(&first, table),
            stat_count(&second, table),
            "{table} 重跑后变了（不幂等）：{first}\n---\n{second}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn docs_edit_makes_new_judgment_and_expires_old() {
    let dir = temp_dir("docs-edit");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (_, before) = run(&dir, &["ask", "doc:a.md", "."]);
    assert!(before.starts_with("held: doc:a.md"), "{before}");

    // mtime 往前推 → 新判断（旧判断保留但过期）
    bump_mtime(&dir.join("a.md"), 10);
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["ask", "doc:a.md", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.starts_with("held: doc:a.md"), "{out}");

    let (_, out) = run(&dir, &["stat", "."]);
    assert_eq!(stat_count(&out, "judgment"), 2, "改文件应多一条判断：{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parse_import_is_idempotent() {
    let dir = temp_dir("parse");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("x.rs"), "pub fn hello() {}\n").expect("写文件");

    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("parsed 1 symbols"), "{out}");
    let (_, first) = run(&dir, &["stat", "."]);

    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (_, second) = run(&dir, &["stat", "."]);
    for table in ["judgment", "evidence", "source"] {
        assert_eq!(
            stat_count(&first, table),
            stat_count(&second, table),
            "{table} 重跑后变了（不幂等）：{first}\n---\n{second}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reparse_revives_expired_symbol() {
    // 过期 → 重采：同一 (subject, at) 的行必须复活，否则 live 符号永远 0
    let dir = temp_dir("revive");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.rs"), "pub fn hello() {}\n").expect("写");
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");

    let (_, before) = run(&dir, &["find", "hello", "--kind", "symbol", "."]);
    assert!(before.contains("a.rs"), "先要有符号：{before}");

    let (c, out) = run(&dir, &["expire", "symbol:a.rs#hello", "."]);
    assert_eq!(c, 0, "{out}");
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let (_, gone) = run(&dir, &["find", "hello", "--kind", "symbol", "."]);
    assert!(!gone.contains("a.rs"), "过期后不该还能查到：{gone}");

    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    let (_, back) = run(&dir, &["find", "hello", "--kind", "symbol", "."]);
    assert!(back.contains("a.rs"), "重采没复活：{back}");
}
