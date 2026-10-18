//! **`launch()` 失败时会带出「缺哪个件」**（2026-10-18 接线验收）。
//!
//! ## 为什么这条必须走 `launch()` 而不是直接调映射表
//!
//! 映射表本身有 5 条单测，但它们证明不了**它被接上了**。
//! 今天已经吃过两次同一个亏（Rust 分发、中文路径分支）：
//! **对着单元写测试，接线坏了照样绿。**
//!
//! 所以这里造一次**真的失败**：让命令打印一句真报错然后退出 1，
//! 断言 `Outcome.missing_tool` 里带出来的正是那一条。
//!
//! ## 三条判据
//!
//! 1. 失败（退出码 1）且输出里有 `link.exe not found` → `missing_tool.what` 说的是 **MSVC 链接器**
//! 2. `verify` 字段**不是空的**（没有自查的结论本项目不许报）
//! 3. **成功时不许有** `missing_tool`（不能给一次成功的运行挂一个"你缺东西"）

#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::launch::{launch, Event, LaunchOptions, Outcome, Purpose};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 找一个**真能用**的 Python，并给出绝对路径（同 `launch_e2e.rs` 的理由：
/// PATH 上那个 `python.exe` 可能是应用商店占位符）。
fn python_exe() -> Option<String> {
    for cand in ["python", "python3"] {
        let ok = std::process::Command::new(cand)
            .args(["-c", "print(1)"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            continue;
        }
        let out = std::process::Command::new(cand)
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

fn opts(root: &Path, run: &str) -> LaunchOptions {
    LaunchOptions {
        root: root.to_path_buf(),
        run_override: Some(run.to_string()),
        extra_args: Vec::new(),
        timeout: Duration::from_secs(60),
        purpose: Purpose::Run,
    }
}

fn run(o: &LaunchOptions) -> Outcome {
    launch(o, &mut |_: Event| {}).expect("launch 不该整次失败")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dd-missing-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn 失败时带出缺哪个件_成功时不带() {
    let Some(py) = python_exe() else {
        eprintln!("跳过：这台机器上没有能用的 python");
        return;
    };
    let d = scratch("wire");

    // ① 一次**真的失败**：打印真报错原文，退出 1
    //    注意反引号——真报错里就有它，这正是映射表归一化要处理的东西。
    let bad = format!(
        "{py} -c \"import sys; print('error: linker `link.exe` not found'); sys.exit(1)\""
    );
    let out = run(&opts(&d, &bad));
    assert_eq!(out.exit_code, 1, "这条命令该失败");
    let m = out
        .missing_tool
        .clone()
        .expect("失败输出里有 link.exe not found，必须带出「缺哪个件」");
    assert!(
        m.what.contains("MSVC 链接器"),
        "认错了件：{}",
        m.what
    );
    assert!(
        !m.verify.trim().is_empty(),
        "没有自查命令的结论不许报：{:?}",
        m
    );
    assert!(
        !m.why.trim().is_empty(),
        "必须说清「为什么光有本体不够」：{:?}",
        m
    );

    // ② 一次**成功**：不许挂"你缺东西"
    let good = format!("{py} -c \"print('OK')\"");
    let ok = run(&opts(&d, &good));
    assert_eq!(ok.exit_code, 0);
    assert!(
        ok.missing_tool.is_none(),
        "跑成功的运行不许带 missing_tool：{:?}",
        ok.missing_tool
    );

    let _ = std::fs::remove_dir_all(&d);
}
