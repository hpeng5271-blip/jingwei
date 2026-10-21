//! **`discover --depth` 必须真的生效**（2026-10-19 的真 bug 的回归测试）。
//!
//! 背景：`--depth` 在全局参数解析里被消费进 `depth_arg`，而 `discover` 分支
//! 又从 `rest` 里找 `--depth` → 永远找不到 → **一直用默认深度**。
//! 症状：整盘扫时"扫不完整"（D:\ 只出 39 个项目，实际 422）。
//!
//! 这条测试直接跑编译出来的二进制，**从外部断言深度真的改变结果**——
//! 参数管线这种事，只有从外部调才测得到（内部函数测不到"参数被谁吃了"）。

// 临时目录清理失败不影响断言，也不能改变结论——显式豁免
#![allow(clippy::let_underscore_must_use)]

use std::path::PathBuf;
use std::process::Command;

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_jingwei")
}

/// 要跑的 CLI。**`JINGWEI_USAGE_FILE` 指到临时文件**：入口会记
/// `~/.jingwei/usage.jsonl`，而那份日志是用来按真实调用数删命令的——
/// 测试往里掺数据就废了（与 `cli_contract::run` 同一句话）。
fn cmd() -> Command {
    let mut c = Command::new(exe());
    let log = std::env::temp_dir().join(format!("jingwei-depth-usage-{}.jsonl", std::process::id()));
    c.env("JINGWEI_USAGE_FILE", log);
    c
}

/// 铺一棵 `root/a/b/c/d/proj`（proj 有 Cargo.toml）。
fn fixture(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("jw-depth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let proj = root.join("a").join("b").join("c").join("d").join("proj");
    std::fs::create_dir_all(&proj).expect("建目录失败");
    std::fs::write(proj.join("Cargo.toml"), "[package]\nname=\"p\"\n").expect("写文件失败");
    std::fs::write(proj.join("main.rs"), "fn main() {}").expect("写文件失败");
    root
}

/// 跑一次 `discover --depth N <root>`，返回找到的项目路径列表。
fn discover(root: &PathBuf, depth: &str) -> Vec<String> {
    let out = cmd()
        .args(["discover", "--depth", depth])
        .arg(root)
        .output()
        .expect("跑不起来");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    text.lines()
        .filter(|l| l.trim_start().starts_with('+'))
        .map(|l| l.split_whitespace().last().unwrap_or("").to_string())
        .collect()
}

/// 深度不够 → 找不到；深度够 → 找得到。**同一个夹具，只有 depth 不同。**
#[test]
fn 深度真的会改变扫描结果() {
    let root = fixture("range");

    let shallow = discover(&root, "2");
    assert!(
        shallow.is_empty(),
        "深度 2 不该看见 a/b/c/d/proj（实际 {shallow:?}）"
    );

    let deep = discover(&root, "6");
    assert!(
        deep.iter().any(|p| p.ends_with("proj")),
        "深度 6 必须看见 a/b/c/d/proj（实际 {deep:?}）"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// **无效参数不许静默成功**：要么报错退出、要么明确用默认值继续——
/// 但**绝不能"看起来跑了、其实没按你说的跑"**。
#[test]
fn 深度参数非法时要能看出来() {
    let root = fixture("bad");
    let out = cmd()
        .args(["discover", "--depth", "abc"])
        .arg(&root)
        .output()
        .expect("跑不起来");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let ok = if out.status.success() {
        // 退回默认深度也算"说得清"：必须有终态行
        stdout.contains("扫完")
    } else {
        // 直接拒绝：stderr 要提到 depth（不能只给一个退出码）
        stderr.contains("depth") || stderr.contains("深度")
    };
    assert!(ok, "非法深度既没报错也没终态：stdout={stdout} stderr={stderr}");
    let _ = std::fs::remove_dir_all(&root);
}
