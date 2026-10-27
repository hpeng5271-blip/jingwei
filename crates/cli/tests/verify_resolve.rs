//! verify / resolve / expire 的命令面测试。
//!
//! verify 现在写**新判断**并把旧判断过期；resolve 只能人来做（--by/--why 必填）。

use std::path::{Path, PathBuf};
use std::process::Command;

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
    let d = std::env::temp_dir().join(format!("jw-vr-{}-{tag}", std::process::id()));
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

fn judgment_id_line(out: &str) -> String {
    out.lines()
        .find(|l| l.trim_start().starts_with("judgment "))
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string()
}

#[test]
fn verify_after_delete_turns_refuted() {
    let dir = temp_dir("verify");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写文件");
    let (c, out) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0, "{out}");
    let (c, out) = run(&dir, &["ask", "doc:a.md", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.starts_with("held: doc:a.md"), "{out}");

    // docs 与 verify 的 at 落在不同秒，才是两条不同判断
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::remove_file(dir.join("a.md")).expect("删文件");
    let (c, out) = run(&dir, &["verify", "doc:a.md", "."]);
    assert_eq!(c, 1, "{out}");
    assert!(out.starts_with("refuted: doc:a.md"), "{out}");

    // 关键：不再是 conflict（旧判断已过期，contradicts 挂在新判断上）
    let (c, out) = run(&dir, &["ask", "doc:a.md", "."]);
    assert_eq!(c, 1, "{out}");
    assert!(out.starts_with("refuted: doc:a.md"), "{out}");
    assert!(!out.contains("conflict"), "{out}");

    // 旧判断没删：两条都在
    let (_, out) = run(&dir, &["stat", "."]);
    assert_eq!(stat_count(&out, "judgment"), 2, "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_writes_new_judgment_not_update() {
    let dir = temp_dir("verify-id");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("b.md"), "# b\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (_, before) = run(&dir, &["ask", "doc:b.md", "."]);
    let id_before = judgment_id_line(&before);
    assert!(!id_before.is_empty(), "{before}");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let (c, _) = run(&dir, &["verify", "doc:b.md", "."]);
    assert_eq!(c, 0);
    let (_, after) = run(&dir, &["ask", "doc:b.md", "."]);
    let id_after = judgment_id_line(&after);
    assert_ne!(id_before, id_after, "verify 原地改了同一条判断：{after}");
    let (_, out) = run(&dir, &["stat", "."]);
    assert_eq!(stat_count(&out, "judgment"), 2, "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_twice_makes_two_observations() {
    // 同秒连跑两次 verify：id 只到秒，必须把 at 往前推，否则第二次静默 no-op
    let dir = temp_dir("verify-twice");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("e.md"), "# e\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, _) = run(&dir, &["verify", "doc:e.md", "."]);
    assert_eq!(c, 0);
    let (_, first) = run(&dir, &["ask", "doc:e.md", "."]);
    let id1 = judgment_id_line(&first);
    let (c, _) = run(&dir, &["verify", "doc:e.md", "."]);
    assert_eq!(c, 0);
    let (_, second) = run(&dir, &["ask", "doc:e.md", "."]);
    let id2 = judgment_id_line(&second);
    assert_ne!(id1, id2, "同秒第二次 verify 没写新观测：{second}");
    let (_, out) = run(&dir, &["stat", "."]);
    assert_eq!(stat_count(&out, "judgment"), 3, "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_rejects_unknown_subject() {
    // 不认识的 subject 不能当"文件不存在"给 decision: 写一条 refuted
    let dir = temp_dir("verify-unknown");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let records = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/records")
        .to_string_lossy()
        .into_owned();
    let (c, out) = run(&dir, &["record", &records, "."]);
    assert_eq!(c, 0, "{out}");

    let repo = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let subject = format!("decision:{repo}#D-20261022-01-测试失败定位");
    let (c, out) = run(&dir, &["verify", &subject, "."]);
    assert_eq!(c, 3, "{out}");
    assert!(out.contains("not verifiable"), "{out}");

    // 判断没被改写：仍是 2 decision + 3 hypothesis
    let (_, out) = run(&dir, &["stat", "."]);
    assert_eq!(stat_count(&out, "judgment"), 5, "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_without_conflict_is_no_conflict() {
    let dir = temp_dir("resolve-none");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("c.md"), "# c\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(
        &dir,
        &[
            "resolve", "doc:c.md", "--verdict", "held", "--by", "alice", "--why", "复验", ".",
        ],
    );
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("no_conflict"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_requires_by_and_why() {
    let dir = temp_dir("resolve-args");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);

    // 缺 --by：clap 直接拒绝（非 0），不许默认一个人名
    let (c, out) = run(
        &dir,
        &["resolve", "doc:x.md", "--verdict", "held", "--why", "理由", "."],
    );
    assert_ne!(c, 0, "{out}");
    assert!(out.contains("by"), "{out}");

    // 缺 --why 同理
    let (c, out) = run(
        &dir,
        &["resolve", "doc:x.md", "--verdict", "held", "--by", "alice", "."],
    );
    assert_ne!(c, 0, "{out}");
    assert!(out.contains("why"), "{out}");

    // --by 给空串 → 我们自己的校验拒绝（exit 3）
    let (c, out) = run(
        &dir,
        &["resolve", "doc:x.md", "--verdict", "held", "--by", "", "--why", "理由", "."],
    );
    assert_eq!(c, 3, "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expire_by_id_rejects_foreign_id() {
    let dir = temp_dir("expire-id");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("d.md"), "# d\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["expire", "doc:d.md", "--id", "not-a-judgment", "."]);
    assert_eq!(c, 3, "{out}");
    assert!(out.contains("不属于"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}
