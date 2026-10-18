//! Rust 项目的**前四个阶段**：工具链、命令从哪来、依赖锁定、跑哪条子命令。
//!
//! ## 为什么不能走 Python 那一条（2026-10-17 演示时抓到的）
//!
//! Python 那条路的第 1 阶段是「找一个能用的解释器」。拿它去开一个 Rust 项目，
//! 屏幕上会出现：
//!
//! ```text
//! [1/6] 检查运行环境   ✓ 通过 · 280ms
//!       Python 3.15.0 · C:\...\python.exe
//! ```
//!
//! 而这个项目 `Cargo.toml` 里写着 `edition = "2021"`、契约里写着 `cargo run`。
//! **工具其实知道它是 Rust**（`doctor` 的 `facts.lang` 就是 `rust`）——
//! 只是"运行环境"这一段还钉在 Python 上。
//!
//! 这和 `node.rs` 头部记的那个真事故是同一类：**答非所问比不回答更糟**。
//! 它不报错、不拦人，但**在屏幕上说了一个错的事实**，而演示的人得替它解释。
//!
//! ## 这一条路里"确定"与"不确定"的边界
//!
//! | 事 | 凭什么 | 有多确定 |
//! |---|---|---|
//! | 用什么工具链 | PATH 里的 `cargo` / `rustc` | 确定（找不到就直说找不到） |
//! | 跑哪条命令 | 契约 / `--run` 覆盖 / 按用途推 `cargo run` | 确定（推的那条写明"推的"） |
//! | 依赖锁没锁 | 根目录有没有 `Cargo.lock`，里面几个 `[[package]]` | 确定（纯数文件） |
//! | 库项目能不能跑 | 有没有 `src/main.rs` 或 `[[bin]]` | **只说"没有可执行入口"**，不替它编一条 |

use super::{Event, LaunchOptions, Prepared, Purpose, StageStatus};
use crate::understand::contract::Contract;
use crate::util::winutil::which;
use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Command;

/// `cargo --version` 的第一行，读不出就 None。**不外抛**：读出版本号失败
/// 不该让整个准备阶段崩掉，它最多让提示少一段。
fn version_of(exe: &Path) -> Option<String> {
    let out = Command::new(exe).arg("--version").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// `Cargo.lock` 里锁住了多少个包（数 `[[package]]` 行）。没有锁文件返回 None。
fn locked_packages(root: &Path) -> Option<usize> {
    let txt = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    Some(txt.lines().filter(|l| l.trim() == "[[package]]").count())
}

/// 有没有可执行入口：`src/main.rs`、`src/bin/`、或 `Cargo.toml` 里声明了 `[[bin]]`。
fn has_bin(root: &Path) -> bool {
    if root.join("src").join("main.rs").is_file() {
        return true;
    }
    if root.join("src").join("bin").is_dir() {
        return true;
    }
    std::fs::read_to_string(root.join("Cargo.toml"))
        .map(|t| t.lines().any(|l| l.trim_start().starts_with("[[bin]]")))
        .unwrap_or(false)
}

/// 这个用途下推哪条子命令。**推的就要写清是推的**（见第 4 阶段的 detail）。
fn guessed(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Run => "cargo run",
        Purpose::Task => "cargo test",
        Purpose::Build => "cargo build",
        Purpose::Install => "cargo fetch",
    }
}

pub(crate) fn prepare(
    root: &Path,
    opts: &LaunchOptions,
    contract: &Option<Contract>,
    contract_err: &Option<String>,
    cb: &mut dyn FnMut(Event),
) -> Result<Prepared> {
    if !root.join("Cargo.toml").is_file() {
        return Err(anyhow!("不是一个 Rust 项目（根目录下没有 Cargo.toml）"));
    }

    // ---- 1 检查运行环境 ----
    let t = std::time::Instant::now();
    let cargo = which("cargo.exe").or_else(|| which("cargo"));
    let rustc = which("rustc.exe").or_else(|| which("rustc"));
    let (s1, d1) = match (&cargo, &rustc) {
        (Some(c), Some(r)) => (
            StageStatus::Ok,
            format!(
                "{} · {}",
                version_of(c).unwrap_or_else(|| "cargo（版本读不出）".into()),
                version_of(r).unwrap_or_else(|| "rustc（版本读不出）".into())
            ),
        ),
        (Some(c), None) => (
            StageStatus::Note,
            format!(
                "{} · **没找到 rustc**（只有 cargo 在 PATH 里）——`cargo run` 真要编译时会失败",
                version_of(c).unwrap_or_else(|| "cargo".into())
            ),
        ),
        _ => (
            StageStatus::Fail,
            "没找到 cargo（PATH 里没有）。装一个 Rust 工具链再回来——\
             https://rustup.rs 装完重开终端"
                .to_string(),
        ),
    };
    cb(Event::Stage {
        index: 1,
        total: super::TOTAL_STAGES,
        title: "检查运行环境".into(),
        status: s1,
        detail: d1,
        ms: t.elapsed().as_millis(),
    });
    if s1 == StageStatus::Fail {
        return Err(anyhow!("这台机器上没有 cargo，跑不了 Rust 项目"));
    }
    let cargo_exe = cargo.ok_or_else(|| anyhow!("PATH 里没有 cargo"))?;

    // ---- 2 校验项目契约 ----
    let t = std::time::Instant::now();
    let from_contract = contract
        .as_ref()
        .and_then(|c| c.run_command().map(String::from));
    let (s2, d2) = match (contract, contract_err) {
        (Some(c), _) if from_contract.is_some() => (
            StageStatus::Ok,
            format!(
                "{} · run: {}",
                c.name.clone().unwrap_or_else(|| "（未命名）".into()),
                from_contract.clone().unwrap_or_default()
            ),
        ),
        (Some(_), _) => (
            StageStatus::Note,
            "契约里没有 run 命令，按用途推一条（见第 4 阶段）".to_string(),
        ),
        (None, Some(e)) => (StageStatus::Fail, format!("契约解析失败：{e}")),
        (None, _) => (
            StageStatus::Note,
            "没有 env.contract.json；按 Cargo.toml 推一条".to_string(),
        ),
    };
    cb(Event::Stage {
        index: 2,
        total: super::TOTAL_STAGES,
        title: "校验项目契约".into(),
        status: s2,
        detail: d2,
        ms: t.elapsed().as_millis(),
    });

    // ---- 3 解析依赖声明 ----
    //
    // **不数 Cargo.toml 里写了几条**：那要解析 TOML 的三种写法，
    // 而真正有意义的是"锁没锁"——没锁的话，今天能装出什么、明天不一定。
    let t = std::time::Instant::now();
    let (s3, d3, n3) = match locked_packages(root) {
        Some(n) => (
            StageStatus::Ok,
            format!("Cargo.lock 锁住了 {n} 个包——版本可复现"),
            n,
        ),
        None => (
            StageStatus::Note,
            "没有 Cargo.lock：第一次 `cargo build` 会自己生成。\
             应用该提交它（否则同一份源码在不同时间装出不同结果）；\
             库通常不提交"
                .to_string(),
            0,
        ),
    };
    cb(Event::Stage {
        index: 3,
        total: super::TOTAL_STAGES,
        title: format!("解析依赖声明（{n3} 个锁定的包）"),
        status: s3,
        detail: d3,
        ms: t.elapsed().as_millis(),
    });

    // ---- 4 准备工具链 ----
    let t = std::time::Instant::now();
    // **按用途认契约**（2026-10-18 两处真跑抓到的 bug 修的）：
    //
    // ① 只有"跑"才认契约的 `run`——冒烟问的是"能不能构建"，照 run 跑只证明程序能起；
    // ② 但"构建"要认契约的 **`build`**：`hello_rust` 的契约里明明写着 `cargo build`，
    //    原来这里直接按用途推（"cargo build"），于是输出说"这条是按用途推的，不是契约里写的"
    //    ——**和文件内容直接矛盾**（codex 当第一视角试用时抓到的）。
    let from_contract_build = contract
        .as_ref()
        .and_then(|c| c.commands.build.clone());
    let (run_cmd, guessed_this) = match opts.purpose {
        Purpose::Run => match opts.run_override.clone().or(from_contract.clone()) {
            Some(c) => (c, false),
            None => (guessed(Purpose::Run).to_string(), true),
        },
        Purpose::Build => match from_contract_build {
            Some(c) => (c, false),
            None => (guessed(Purpose::Build).to_string(), true),
        },
        other => (guessed(other).to_string(), true),
    };
    let parts = super::split_quoted(&run_cmd);
    if parts.is_empty() {
        return Err(anyhow!("要跑的命令是空的（契约里写的是空白？）"));
    }
    // 第一个词是 `cargo` 就用我们找到的那个绝对路径；别的命令原样交给系统。
    let (program, args) = if parts[0].eq_ignore_ascii_case("cargo") {
        (cargo_exe.clone(), parts[1..].to_vec())
    } else {
        let p = which(&parts[0]).unwrap_or_else(|| std::path::PathBuf::from(&parts[0]));
        (p, parts[1..].to_vec())
    };
    let mut detail = format!("{}（{}）", run_cmd, program.display());
    if guessed_this {
        detail.push_str(" · 这条是按用途推的，不是契约里写的");
    }
    if opts.purpose == Purpose::Run && !has_bin(root) {
        // **不替它编一条能跑的命令**：库项目就是没有可执行入口。
        detail.push_str(
            " · **这个项目没有可执行入口**（没有 src/main.rs、src/bin/、[[bin]]）——\
             库项目本来就没有「跑起来」这回事",
        );
    }
    cb(Event::Stage {
        index: 4,
        total: super::TOTAL_STAGES,
        title: "准备工具链".into(),
        status: StageStatus::Ok,
        detail,
        ms: t.elapsed().as_millis(),
    });

    Ok(Prepared {
        program,
        args,
        // Rust 侧不需要替用户设环境变量（`RUST_BACKTRACE` 之类该由项目自己定）。
        env: Vec::new(),
        run_cmd,
        // Rust 程序跑完就退，所以到点还没就绪通常意味着**卡住了**。
        alive_unready: StageStatus::Fail,
        url_watch: false,
        ready_rule: "退出码 0".to_string(),
    })
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-rustlaunch-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        d
    }

    fn opts(purpose: Purpose) -> LaunchOptions {
        LaunchOptions {
            root: std::env::temp_dir(),
            run_override: None,
            extra_args: Vec::new(),
            timeout: std::time::Duration::from_secs(5),
            purpose,
        }
    }

    /// 没有可执行入口时**要说出来**，而不是让 `cargo run` 自己报错。
    #[test]
    fn 库项目要说没有可执行入口() {
        let d = scratch("lib");
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(d.join("src").join("lib.rs"), "pub fn f() {}").unwrap();

        let mut events = Vec::new();
        let mut cb = |e: Event| events.push(e);
        let contract = None;
        let err = None;
        let _ = prepare(&d, &opts(Purpose::Run), &contract, &err, &mut cb);
        let s4 = events
            .iter()
            .find_map(|e| match e {
                Event::Stage {
                    index: 4, detail, ..
                } => Some(detail.clone()),
                _ => None,
            })
            .expect("第 4 阶段必须有输出");
        assert!(
            s4.contains("没有可执行入口"),
            "库项目要说清楚它没有可执行入口：{s4}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
