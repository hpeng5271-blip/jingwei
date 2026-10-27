//! record 导入与 ask 的命令面测试：跑真实的 `jw` 二进制，不碰网络。
//!
//! 夹具在 `tests/fixtures/records/`；断言直接看 CLI 输出（三态、覆盖、scope、计数），
//! 因为“能跑的命令”才是交付口径。

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
    let d = std::env::temp_dir().join(format!("jw-cli-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// 从 `stat` 输出里取一张表的行数。
fn stat_count(out: &str, table: &str) -> i64 {
    out.lines()
        .find(|l| l.starts_with(table))
        .and_then(|l| l.split_whitespace().last())
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1)
}

#[test]
fn ask_no_data_has_own_label() {
    let dir = temp_dir("nodata");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);

    // 没有任何判断 → 首行 no_data:，退出码 2（不能跟 uncertain 混成一个标签）
    let (c, out) = run(&dir, &["ask", "file:nope.md", "."]);
    assert_eq!(c, 2, "{out}");
    assert_eq!(
        out.lines().next().unwrap_or(""),
        "no_data: file:nope.md",
        "{out}"
    );

    // 有判断但结论不明 → 仍是 uncertain（用 record：没有状态行的决策）
    let repo = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (c, _) = run(&dir, &["record", &fixture("records"), "."]);
    assert_eq!(c, 0);
    let subject = format!("decision:{repo}#D-20261022-01-测试失败定位");
    let (c, out) = run(&dir, &["ask", &subject, "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.starts_with(&format!("uncertain: {subject}")), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

fn repo_name(dir: &Path) -> String {
    dir.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[test]
fn record_import_end_to_end() {
    let dir = temp_dir("record");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["record", &fixture("records"), "."]);
    assert_eq!(c, 0, "{out}");
    let repo = repo_name(&dir);

    // 已落地 → held
    let held_subj = format!("decision:{repo}#D-20261021-05-主路径八动词");
    let (c, out) = run(&dir, &["ask", &held_subj, "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("held"), "{out}");
    assert!(out.contains("kind=decision"), "{out}");
    assert!(out.contains("scope=decision"), "{out}");
    // 依据是指针，instrument=record
    assert!(out.contains("(record)"), "{out}");

    // 没有状态行 → uncertain
    let unk_subj = format!("decision:{repo}#D-20261022-01-测试失败定位");
    let (c, out) = run(&dir, &["ask", &unk_subj, "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("uncertain"), "{out}");

    // 2 条决策 + 3 条假设 = 5 条判断，各带 1 条依据
    let (_, out) = run(&dir, &["stat", "."]);
    assert_eq!(stat_count(&out, "judgment"), 5, "{out}");
    assert_eq!(stat_count(&out, "evidence"), 5, "{out}");
    // 两条记录分别是 10-21、10-22 → 2 个 source（按记录日期，不是按导入次数）
    assert_eq!(stat_count(&out, "source"), 2, "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn record_alternatives_link_to_decision() {
    let dir = temp_dir("record-alt");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["record", &fixture("records"), "."]);
    assert_eq!(c, 0, "{out}");
    let repo = repo_name(&dir);

    let held_subj = format!("decision:{repo}#D-20261021-05-主路径八动词");
    let (c, out) = run(&dir, &["alt-list", &held_subj, "."]);
    assert_eq!(c, 0, "{out}");
    let candidates: Vec<&str> = out.lines().filter(|l| l.starts_with("candidate ")).collect();
    assert_eq!(candidates.len(), 3, "{out}");
    // 按 subject 自然序：/1、/2、/3（不是 id 哈希序）
    assert!(candidates[0].ends_with("/1"), "{out}");
    assert!(candidates[2].ends_with("/3"), "{out}");
    // 候选是 hypothesis 判断，能被 ask 查到
    let h1 = format!("hypothesis:{repo}#D-20261021-05-主路径八动词/1");
    let (c, out) = run(&dir, &["ask", &h1, "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("kind=hypothesis"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn record_import_is_stable() {
    let dir = temp_dir("record-stable");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["record", &fixture("records"), "."]);
    assert_eq!(c, 0);
    let (_, first) = run(&dir, &["stat", "."]);

    // 重跑同一批记录：at 来自文件名日期，不该翻倍
    let (c, out) = run(&dir, &["record", &fixture("records"), "."]);
    assert_eq!(c, 0, "{out}");
    let (_, second) = run(&dir, &["stat", "."]);

    for table in ["judgment", "evidence", "source"] {
        assert_eq!(
            stat_count(&first, table),
            stat_count(&second, table),
            "{table} 重跑后变了：{first}\n---\n{second}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

