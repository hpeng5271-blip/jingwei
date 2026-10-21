//! **命令调用记录真的落盘**（2026-10-21，删命令普查的下一步）。
//!
//! ## 为什么要有它
//!
//! 普查推翻的正是"看着没人用"：`env` 有 **130** 处依赖。所以"某个命令没人用"
//! 不能靠感觉，得先量——`limits.mjs` 记的是 pro 咨询，不是命令调用，
//! 于是这一层之前是**空的**：删命令等于盲删。
//!
//! ## 怎么测（为什么从外部跑真二进制）
//!
//! 要断言的是"文件里真有几行、命令名对不对"，那是**进程退出前留下的东西**，
//! 不是 `main` 里调过某个函数。所以跟 `cli_contract.rs` 一样用 `CARGO_BIN_EXE_`：
//! 跑两次 `--version` + 一次 `next` + 一次别名 `inspect`，再读文件。
//!
//! 记录文件用 `JINGWEI_USAGE_FILE` 指到临时路径——**测的是真行为，但不往
//! 用户那份 `~/.jingwei/usage.jsonl` 里掺测试数据**（掺了就没法按一周真实调用数删命令了）。
//!
//! ## 判据
//!
//! ① 四次调用 → **恰好四行**（多出来就说明"统一记一次"没做到，记了两遍）；
//! ② 每行是 JSON，`ts` 是 RFC3339（UTC，`Z` 结尾）；
//! ③ `cmd` 是**主名**且顺序对得上——`inspect` 必须记成 `doctor`，别名不许各记一条；
//! ④ `exit` 与这次真跑出来的退出码一致。

// 临时文件清理失败不影响断言，也不能改变结论——显式豁免（与 cli_contract 同一句）。
#![allow(clippy::let_underscore_must_use)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_jingwei")
}

/// 这一轮测试专用的记录文件：先删掉，保证断言的是"这次跑出来的"。
fn usage_file(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("jingwei-usage-{tag}-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

/// 跑一次 CLI，把记录文件指到临时路径（不碰用户那份）。
fn run(args: &[&str], log: &Path) -> Output {
    Command::new(exe())
        .args(args)
        .env("JINGWEI_USAGE_FILE", log)
        .output()
        .expect("跑不起来")
}

/// RFC3339（UTC）：`YYYY-MM-DDTHH:MM:SSZ`。**不为这个引日期库**，形状对就够。
fn is_rfc3339_utc(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
}

#[test]
fn 调用记录落盘且命令名归一到主名() {
    let log = usage_file("main");
    let root = std::env::temp_dir().join(format!("jingwei-usage-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("建临时目录");
    let root_s = root.to_string_lossy().to_string();

    // 四种形状，缺一不算覆盖了"入口统一记一次"：
    // ① `--version`：解析阶段就返回，没走到分派表；② 两次 → 每次都记；
    // ③ `next`：正常分派；④ `inspect`：**`doctor` 的别名**——必须记成主名，
    //    否则按调用数删命令时，同一个命令会被拆成几份、每份都显得"没人用"。
    let cases: [(&[&str], &str); 4] = [
        (&["--version"], "version"),
        (&["--version"], "version"),
        (&["next", "--root", &root_s], "next"),
        (&["inspect", "--root", &root_s], "doctor"),
    ];

    // 真退出码留在手里：下面要和日志里的 `exit` 对账（自报的数和真跑的数必须一致）。
    let mut actual: Vec<i64> = Vec::new();
    for (args, _) in &cases {
        let out = run(args, &log);
        actual.push(out.status.code().map(i64::from).unwrap_or(-1));
    }
    assert_eq!(actual[0], 0, "`--version` 该成功退出");

    let text = std::fs::read_to_string(&log)
        .unwrap_or_else(|e| panic!("记录文件没被创建（{e}）：入口没记，或者记到了别的地方"));
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        cases.len(),
        "跑几次就该有几行（多出来说明记了两遍，少一行说明有入口漏记）：\n{text}"
    );

    for (i, line) in lines.iter().enumerate() {
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("一行一条 JSON：{line}\n{e}"));
        let ts = v["ts"].as_str().unwrap_or_default();
        assert!(is_rfc3339_utc(ts), "ts 该是 RFC3339（UTC）：{line}");
        assert_eq!(
            v["cmd"].as_str(),
            Some(cases[i].1),
            "第 {} 条该记 {}：{line}",
            i + 1,
            cases[i].1
        );
        assert_eq!(
            v["exit"].as_i64(),
            Some(actual[i]),
            "退出码要和真跑的一致（真跑 {}）：{line}",
            actual[i]
        );
    }

    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_dir_all(&root);
}
