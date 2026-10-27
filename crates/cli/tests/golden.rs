//! v1 黄金值验收（与 scripts/exp-v1.ps1 共用同一份 expected.json）。
//! fixture 在临时目录里重建：3 提交 / 2 rs / 1 md / 1 record，跑同样的命令，对固定计数断言。

use std::path::{Path, PathBuf};
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
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@e",
            "commit",
            "-q",
            "-m",
            msg,
        ],
    );
}

fn json_of(out: &str) -> serde_json::Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("不是 JSON（{e}）：{out}"))
}

#[test]
fn v1_golden_acceptance() {
    // 与测试同进程的临时目录，互不干扰
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("jw-golden-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::create_dir_all(dir.join("docs/decisions")).expect("建 docs");

    std::fs::write(
        dir.join("src/a.rs"),
        "use std::fmt;\npub trait Speak {}\npub struct A;\nimpl Speak for A {}\n",
    )
    .expect("写 a");
    std::fs::write(dir.join("src/b.rs"), "pub fn b() {}\n").expect("写 b");
    std::fs::write(dir.join("docs/x.md"), "# x\n").expect("写 x.md");
    std::fs::write(
        dir.join("docs/decisions/D-20261021-05-x.md"),
        "# D-20261021-05 · x\n\n- 日期：2026-10-21\n- 状态：**已落地**\n\n## 决策\n\n固定 8 个。\n\n## 三个选项与取舍\n\n| 选项 | 做法 | 代价 | 结论 |\n|---|---|---|---|\n| A | 固定 | 小 | 选它 |\n| B | 不删 | 中 | 否决 |\n| C | 不动 | 大 | 否决 |\n",
    )
    .expect("写 record");

    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "src/a.rs"]);
    commit(&dir, "c1");
    git(&dir, &["add", "src/b.rs", "docs/x.md"]);
    commit(&dir, "c2");
    std::fs::write(
        dir.join("src/a.rs"),
        "use std::fmt;\npub trait Speak {}\npub struct A;\nimpl Speak for A {}\npub fn c() {}\n",
    )
    .expect("改 a");
    git(&dir, &["add", "-A"]);
    commit(&dir, "c3");
    // c4：改名（黄金层覆盖 moved_to）
    git(&dir, &["mv", "src/b.rs", "src/c.rs"]);
    commit(&dir, "c4");

    for args in [
        vec!["init", "."],
        vec!["ingest", "--limit", "0", "."],
        vec!["docs", "."],
        vec!["parse", "."],
        vec!["record", "docs/decisions"],
    ] {
        let (c, out) = jw(&dir, &args);
        assert_eq!(c, 0, "{args:?} 失败：{out}");
    }

    // 观察值（与 exp-v1 同一口径）
    let (_, s) = jw(&dir, &["card", "src/a.rs", "--json", "."]);
    let card = json_of(&s);
    let (_, s) = jw(&dir, &["card", "src", "--json", "."]);
    let dirc = json_of(&s);
    let (_, s) = jw(&dir, &["timeline", "--json", "."]);
    let tl = json_of(&s);
    let (_, s) = jw(&dir, &["card", "decision:D-20261021-05", "--json", "."]);
    let dec = json_of(&s);
    let (_, s) = jw(&dir, &["find", "a", "--kind", "symbol", "--json", "."]);
    let find = json_of(&s);
    let (_, s) = jw(&dir, &["card", "src/b.rs", "--json", "."]);
    let old_card = json_of(&s);
    let (_, s) = jw(&dir, &["card", "src/c.rs", "--json", "."]);
    let new_card = json_of(&s);
    let (_, check) = jw(&dir, &["check", "."]);

    let got = serde_json::json!({
        "commits": tl["commits"],
        "changes": tl["changes"],
        "files": dirc["files"],
        "dir_changes": dirc["changes"],
        "file_symbols": card["symbols"]["names"],
        "decision_candidates": dec["candidates"],
        "decision_picked": dec["picked"],
        "find_hits": find["hits"].as_array().map(|a| a.len()).unwrap_or(0),
        "check_ok": check.contains("ok") && check.contains("false_coverage=0"),
        "moved_to_old": old_card["moved_to"],
        "moved_to_new": new_card["moved_to"],
        "edges_use_internal": card["edges"]["use"]["internal"],
        "edges_use_ext": card["edges"]["use"]["ext"],
        "edges_impl": card["edges"]["impl"],
    });

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("v1-golden")
        .join("expected.json");
    let want: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture).expect("读 expected.json"))
            .expect("解析 expected.json");

    let mut bad = Vec::new();
    for (k, v) in want.as_object().expect("object") {
        if got[k] != *v {
            bad.push(format!("{k}: want {v}, got {}", got[k]));
        }
    }
    assert!(bad.is_empty(), "黄金值不符：\n{}", bad.join("\n"));

    let _ = std::fs::remove_dir_all(&dir);
}
