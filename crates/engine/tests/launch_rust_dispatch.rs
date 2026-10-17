//! **Rust 项目走的是自己那条路，不是 Python 那条**（2026-10-17 加）。
//!
//! ## 这条测试挡的是什么
//!
//! `launch.rs` 的分发原来是「JVM → Node → **其余都走 Python**」，Rust 掉进"其余"。
//! 于是对一个 `Cargo.toml` 里写着 `edition = "2021"`、契约里写着 `cargo run` 的项目，
//! 屏幕上出现的是：
//!
//! ```text
//! [1/6] 检查运行环境   ✓ 通过 · 280ms
//!       Python 3.15.0 · C:\...\python.exe
//! ```
//!
//! 不报错、不拦人，**但说了一个错的事实**——演示的人得替它解释。
//!
//! ## 为什么必须走 `launch()`，而不是直接调 `rust::prepare`
//!
//! 缺陷本身在**分发那一行**。对着 `rust.rs` 写单测，**分发坏了它照样绿**——
//! 同一天在「中文路径」那条测试上刚吃过同一个亏（分支与兜底产生同一个值）。
//! 所以这里只认 `launch()` 的**可观察输出**。
//!
//! ## 判据（三条都要）
//!
//! 1. 第 1 阶段的 detail **有 `cargo`**；
//! 2. 第 1 阶段的 detail **没有 `python`**；
//! 3. 那条 Rust 程序**真的被跑起来**（输出里出现它打印的字符串）——
//!    证明"分发对了之后，执行那条路也是通的"，而不是只换了一句文案。

#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::launch::{launch, Event, LaunchOptions, Purpose};
use std::path::PathBuf;
use std::time::Duration;

fn has_cargo() -> bool {
    std::process::Command::new("cargo")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 造一个最小的 Rust 项目：`Cargo.toml` + `src/main.rs`。
///
/// **照真东西写**：`edition` 与 `main.rs` 都在，跟演示里的 `hello_rust` 一个形状。
fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dd-rust-dispatch-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();

    let cargo_toml = "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
    std::fs::write(d.join("Cargo.toml"), cargo_toml).unwrap();

    let main_rs = "fn main() { println!(\"RUST_FIXTURE_RAN\"); }\n";
    std::fs::write(d.join("src").join("main.rs"), main_rs).unwrap();
    d
}

fn opts(root: &std::path::Path, run: &str) -> LaunchOptions {
    LaunchOptions {
        root: root.to_path_buf(),
        run_override: Some(run.to_string()),
        extra_args: Vec::new(),
        timeout: Duration::from_secs(120),
        purpose: Purpose::Run,
    }
}

#[test]
fn rust_项目的第1阶段说_cargo_不说_python() {
    if !has_cargo() {
        // **缺工具链就跳过，并且说出来**——无声的绿会被读成"验过了"。
        eprintln!("跳过：这台机器上没有 cargo");
        return;
    }
    let d = fixture("stage1");

    let mut stage1 = String::new();
    let mut text = String::new();
    let out = launch(&opts(&d, "cargo run -q"), &mut |e: Event| match e {
        Event::Stage {
            index: 1,
            detail,
            ..
        } => stage1 = detail.clone(),
        Event::Line { text: line, .. } => {
            text.push_str(&line);
            text.push('\n');
        }
        _ => {}
    })
    .expect("launch 不该整次失败");

    assert!(
        stage1.contains("cargo"),
        "Rust 项目的第 1 阶段该说 cargo，实际是：{stage1}"
    );
    assert!(
        !stage1.to_lowercase().contains("python"),
        "**Rust 项目的第 1 阶段不许提 Python**——这正是 2026-10-17 抓到的那个缺陷：{stage1}"
    );
    assert_eq!(out.exit_code, 0, "这条 Rust 程序该跑通");
    assert!(
        text.contains("RUST_FIXTURE_RAN"),
        "分发对了之后，那条 cargo run 也该真的被执行：{text}"
    );

    let _ = std::fs::remove_dir_all(&d);
}
