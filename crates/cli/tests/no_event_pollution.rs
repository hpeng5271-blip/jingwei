//! **"绝不记"验收**（2026-10-22，pro 的闭环第 ② 项）：
//! 程序的自动取数（`file` / `doctor` / `handover` / `events`）**不许**在事件流里留下痕迹。
//!
//! 为什么放在 CLI 层测：界面里那些"为了显示而 spawn"的取数，最终都落到这些命令上。
//! 只要它们一个都不写事件，GUI 取数污染这条就证明了一半；另一半是 `event_note` 只认界面调用
//! （见 `src-tauri` 的单测）。

use std::path::PathBuf;
use std::process::Command;

fn bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_jingwei") {
        return Some(PathBuf::from(p));
    }
    let here = std::env::current_exe().ok()?;
    let dir = here.parent()?.parent()?.parent()?;
    let exe = dir.join(if cfg!(windows) { "jingwei.exe" } else { "jingwei" });
    exe.exists().then_some(exe)
}

#[test]
fn 自动取数命令不写事件() {
    let Some(bin) = bin() else { return };
    let d = std::env::temp_dir().join(format!("jw-noevent-{}", std::process::id()));
    std::fs::create_dir_all(d.join("src")).ok();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").ok();
    std::fs::write(d.join("src/a.rs"), "pub fn f() {}\n").ok();
    let root = d.to_str().unwrap().to_string();
    let events = d.join("events.jsonl");

    // 先索引（这一步本身不该写事件）
    Command::new(&bin)
        .args(["index", "--root", &root])
        .env("JINGWEI_EVENTS_FILE", &events)
        .output()
        .ok();

    for args in [
        vec!["file", "src/a.rs", "--root", &root],
        vec!["doctor", "--root", &root, "--json"],
        vec!["handover", "--root", &root, "--json"],
        vec!["events", "--json"],
        // 2026-10-23 收口：读侧新增的两条也必须零污染
        vec!["events", "progress", "--json"],
        vec!["data", "--json"],
    ] {
        let out = Command::new(&bin)
            .args(&args)
            .env("JINGWEI_EVENTS_FILE", &events)
            .output()
            .expect("命令该能跑");
        assert!(
            out.status.code().is_some(),
            "命令不该崩：{:?}",
            args
        );
    }

    let text = std::fs::read_to_string(&events).unwrap_or_default();
    let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(
        lines, 0,
        "自动取数写了事件（污染）：{text}"
    );
    std::fs::remove_dir_all(&d).ok();
}
