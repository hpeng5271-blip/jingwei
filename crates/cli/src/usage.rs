//! **命令调用记录**：`~/.jingwei/usage.jsonl`，一行一条。
//!
//! ## 为什么要有它（2026-10-21 删命令普查的结论）
//!
//! 普查推翻的是"看着没人用"这个判据：`env` **130** 处依赖、`clean` **33**、
//! `snapshot` **26**——按感觉挑三个删，删的是核心。可反过来"哪个真没人用"当时也答不了：
//! `tools/llm/limits.mjs` 记的是 pro 咨询，**不是命令调用**，所以这一层是空的。
//!
//! 于是顺序反过来：**先量，再删**。跑一周，`usage.jsonl` 里调用次数为 0 的才是候选
//! （仍要 grep 确认没有测试/文档依赖）。
//!
//! ## 三条约束（写在这里，免得以后被"顺手"改掉）
//!
//! 1. **只在入口记一次**（`main` 里那一个调用点），各子命令**不碰**它——
//!    否则每加一条命令就多一处"记得记"，迟早漏；
//! 2. **记不上不许影响命令本身**：路径拿不到、目录建不出、写失败，一律跳过。
//!    记使用量是**量**，不是功能，不能让它把 `jingwei next` 搞成非 0；
//! 3. **只记名字、时间和退出码**：不记参数、不记项目路径——参数里有别人的路径，
//!    那不属于我们要量的东西。
//!
//! ## 一行长这样
//!
//! ```text
//! {"ts":"2026-10-21T19:50:13Z","cmd":"next","exit":0}
//! ```
//!
//! 时间用 UTC 的 `Z` 结尾（RFC3339 的一种），因为仓库里已有 `facts::now_iso()`
//! 这套不引日期库的写法；本地时区换算要用到平台 API，为一个使用量日志不值得。
//!
//! `cmd` 是**主名**（别名归一：`check` / `inspect` → `doctor`）。这是"按调用数删命令"的
//! 前提：别名各记一条，同一个命令会被拆成几份、每份都显得没人用——**正好重演这个功能要防的错**。

use std::io::Write;
use std::path::{Path, PathBuf};

/// 记录文件的位置：`~/.jingwei/usage.jsonl`。
///
/// `JINGWEI_USAGE_FILE` 可以直接指定文件——**给测试和多机隔离用**：
/// 集成测试跑的是真二进制，不隔离的话每次 `cargo test` 都往用户那份真日志里
/// 掺几十行测试数据，一周后按调用数删命令就成了按"测试跑了几次"删。
pub fn usage_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("JINGWEI_USAGE_FILE") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    home_dir().map(|h| h.join(".jingwei").join("usage.jsonl"))
}

/// 家目录：Windows 用 `USERPROFILE`，其余用 `HOME`。
///
/// 自己拼而不引 `dirs`——仓库里取机器级路径都这么取（见 `cmd_learn::progress_path`），
/// 多一个依赖换不来什么。
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// 记一行。**任何失败都吞掉**：这条日志不是命令的输出，也不是它的退出条件。
pub fn record(cmd: &str, code: i32) {
    if cmd.is_empty() {
        return; // 名字都没有就没什么可量的（正常路径不会走到这儿）
    }
    if let Some(path) = usage_path() {
        append(&path, cmd, code);
    }
}

fn append(path: &Path, cmd: &str, code: i32) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // **追加而不是重写**：多个 jingwei 进程可能同时记，`append(true)` 是 O_APPEND，
    // 一行一次 write 不会互相覆盖。
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let json = serde_json::json!({
        "ts": jingwei_engine::facts::now_iso(),
        "cmd": cmd,
        "exit": code,
    });
    // **一行一次 write**：`writeln!` 是"正文 + 换行"两次写，多个 jingwei 同时记时
    // 两个进程的片段会交错，整份 JSONL 就坏了（坏行还看不出来，只会解析失败）。
    let line = format!("{json}\n");
    if let Err(e) = f.write_all(line.as_bytes()) {
        // 写失败也不许上屏：用户的屏是给"项目状态"的，不是给我们的日志的。
        jingwei_engine::dlog::w("9101", &format!("usage.jsonl 写入失败：{e}"));
    }
}
