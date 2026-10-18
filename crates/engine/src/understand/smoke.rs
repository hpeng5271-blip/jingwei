//! **冒烟：主动跑一次构建，看这台机器到底够不够用**（2026-10-18）
//!
//! ## 为什么"检测到 rustc 存在"不算数
//!
//! `取舍记录.md` T-01 已经把环境这件事定死：不装、不提权、不接管系统。
//! 那么"够不够用"这个问题，**只能由一次真实的构建来回答**——
//! 装了 rustc 但没有链接器，`cargo build` 会失败；那一行报错才是答案。
//!
//! 所以这一层的判据是：**拿项目自己的构建命令跑一次，退出码说话。**
//!
//! ## 结果为什么不是 `Verdict<T>`
//!
//! `Verdict` 的 `No` / `Unknown` **不带载荷**（全仓 32 处 `matches!` 依赖它无载荷），
//! 而冒烟必须把"跑了什么、退出码几、第一条错误、缺哪个件"一起带出来。
//! 所以这里用**自己的四态**，但语义与三态一一对应：
//!
//! | 这里 | 对应 | 什么时候 |
//! |---|---|---|
//! | `Passed` | 是 | 退出码 0 |
//! | `Failed` | 不是 | 跑了但退出码非 0（真结论，不是流程错误） |
//! | `NotApplicable` | 不适用 | 这个项目没有构建这一步（纯脚本 / 库） |
//! | `Unclear` | 查不了 | 我们自己起不来（没有命令、超时、启动失败） |
//!
//! **`Unclear` 不许伪装成 `Failed`**：前者是"我们没验成"，后者是"验了、没过"。

use std::path::Path;
use std::time::{Duration, Instant};

use crate::launch::{launch, toolchain_need::MissingToolOut, Event, LaunchOptions, Purpose};

/// 冒烟跑出来的东西。**每一个字段都是事实**（没有推断位）。
#[derive(Debug, Clone)]
pub struct SmokeDone {
    /// 真跑的那条命令（人看得懂的那份）。
    pub cmd: String,
    pub exit_code: i32,
    /// 输出里第一条像错误的行（取不到就 None）。
    ///
    /// **只取一行**：构建工具的第一条错误通常就决定了问题性质，
    /// 而把整段贴回来会让"结论"变成"日志"。
    pub first_error: Option<String>,
    /// 由那条错误行翻出来的"缺哪个件"（认不出就是 None）。
    pub missing: Option<MissingToolOut>,
    pub ms: u128,
}

#[derive(Debug, Clone)]
pub enum SmokeVerdict {
    /// 跑了，退出码 0。
    Passed(SmokeDone),
    /// 跑了，没过。**这是结论，不是流程错误。**
    Failed(SmokeDone),
    /// 这个项目没有"构建"这一步（例如纯脚本项目）。
    NotApplicable { why: String },
    /// 我们没验成（没有可用命令 / 起不来）。**不许说成失败。**
    Unclear { why: String },
}

/// 一条输出行像不像"错误"。
///
/// 判据刻意保守（**宁可漏，不可把进度条当错误**）：
/// 只看行首的几种已知形状，而不是"包含 error"——`0 errors` 也包含 error。
fn looks_like_error(line: &str) -> bool {
    let l = line.trim_start();
    if l.is_empty() {
        return false;
    }
    let lower = l.to_lowercase();
    if lower.starts_with("error") || lower.starts_with("fatal") || lower.starts_with("failure") {
        return true;
    }
    if lower.starts_with("npm err!") || lower.starts_with("gyp err!") {
        return true;
    }
    // Rust 的 `error[E0425]: ...` 已经被 starts_with("error") 覆盖
    false
}

/// 跑一次冒烟。**默认 180 秒**——构建可能很慢，但也不能无限等。
///
/// 用 `Purpose::Build`：我们要的是"能不能编出来"，不是"能不能起服务"。
pub fn smoke(root: &Path, timeout: Duration) -> SmokeVerdict {
    // **先问一句"这是不是一个项目"**：对一个空目录跑构建，结论不是"我们没验成"，
    // 而是"问错了对象"（`NotApplicable`）。两者退出码不同，不能混。
    if !crate::understand::projects::looks_like_project(root) {
        return SmokeVerdict::NotApplicable {
            why: "这里不像一个项目（没有清单文件、没有 .git、也没有 env.contract.json）".into(),
        };
    }
    let opts = LaunchOptions {
        root: root.to_path_buf(),
        run_override: None,
        extra_args: Vec::new(),
        timeout,
        purpose: Purpose::Build,
    };

    let t = Instant::now();
    let mut first_error: Option<String> = None;
    let mut cmd = String::new();
    let mut cb = |e: Event| match e {
        // 第 4 阶段是"准备工具链/运行时"，那里的 detail 就是那条命令。
        Event::Stage {
            index: 4,
            detail,
            status,
            ..
        } if status != crate::launch::StageStatus::Fail => {
            if cmd.is_empty() {
                cmd = detail.clone();
            }
        }
        Event::Line { text, .. } if first_error.is_none() && looks_like_error(&text) => {
            first_error = Some(text.clone());
        }
        _ => {}
    };

    match launch(&opts, &mut cb) {
        Ok(out) => {
            let done = SmokeDone {
                cmd: if cmd.is_empty() {
                    out.diagnosis
                        .as_ref()
                        .map(|d| d.crux.clone())
                        .unwrap_or_else(|| "（没拿到命令）".into())
                } else {
                    cmd
                },
                exit_code: out.exit_code,
                first_error,
                missing: out.missing_tool,
                ms: t.elapsed().as_millis(),
            };
            if out.exit_code == 0 {
                SmokeVerdict::Passed(done)
            } else {
                SmokeVerdict::Failed(done)
            }
        }
        // **起不来 ≠ 没通过**。起不来是我们没验成。
        Err(e) => {
            let msg = e.to_string();
            // 常见的"这个项目没有构建这一步"：契约没声明、也推不出来。
            if msg.contains("契约里没有声明")
                || msg.contains("没有可跑的脚本")
                || msg.contains("没有单独的构建这一步")
            {
                SmokeVerdict::NotApplicable { why: msg }
            } else {
                SmokeVerdict::Unclear { why: msg }
            }
        }
    }
}

impl SmokeVerdict {
    /// 一句话结论（给界面与 CLI 用）。**四种结局各一句，不许合并。**
    pub fn headline(&self) -> String {
        match self {
            Self::Passed(d) => format!("构建跑通了（退出码 0，{}ms）", d.ms),
            Self::Failed(d) => format!("构建没过（退出码 {}）", d.exit_code),
            // **不说成"这个项目没有构建这一步"**：空目录根本不是项目，
            // 一句"没有构建"会让人以为"是个项目、只是不用构建"。理由由 `why` 说。
            Self::NotApplicable { .. } => "这一步不适用".to_string(),
            Self::Unclear { .. } => "我们没能验成".to_string(),
        }
    }

    pub fn done(&self) -> Option<&SmokeDone> {
        match self {
            Self::Passed(d) | Self::Failed(d) => Some(d),
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dd-smoke-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("src")).unwrap();
        d
    }

    fn has_cargo() -> bool {
        std::process::Command::new("cargo")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// **好的 Rust 项目 → Passed，并且带出来的命令是构建、不是 run。**
    #[test]
    fn 好项目冒烟通过并带出构建命令() {
        if !has_cargo() {
            eprintln!("跳过：这台机器上没有 cargo");
            return;
        }
        let d = scratch("ok");
        fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"smoke_ok\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(d.join("src").join("main.rs"), "fn main() {}\n").unwrap();

        match smoke(&d, Duration::from_secs(180)) {
            SmokeVerdict::Passed(done) => {
                assert_eq!(done.exit_code, 0);
                assert!(
                    done.cmd.contains("build"),
                    "冒烟必须跑构建，不许跑 run：{}",
                    done.cmd
                );
            }
            other => panic!("该通过，实际：{:?}", other.headline()),
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// **编不过的 Rust 项目 → Failed，而且第一条错误行要带出来。**
    ///
    /// 同时钉住一个边界：**编译错误不是"缺件"**——`missing` 必须是 None，
    /// 否则用户会被引去装一个他根本不缺的东西。
    #[test]
    fn 编不过时是失败_不是缺件() {
        if !has_cargo() {
            eprintln!("跳过：这台机器上没有 cargo");
            return;
        }
        let d = scratch("bad");
        fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"smoke_bad\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        // 故意的语法错误
        fs::write(d.join("src").join("main.rs"), "fn main( {\n").unwrap();

        match smoke(&d, Duration::from_secs(180)) {
            SmokeVerdict::Failed(done) => {
                assert_ne!(done.exit_code, 0);
                let e = done.first_error.clone().unwrap_or_default();
                assert!(
                    e.to_lowercase().contains("error"),
                    "该带出第一条错误行，实际：{:?}",
                    done.first_error
                );
                assert!(
                    done.missing.is_none(),
                    "语法错误不是缺件，不许挂「缺什么」：{:?}",
                    done.missing
                );
            }
            other => panic!("该失败，实际：{:?}", other.headline()),
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// **没有构建步骤的项目 → NotApplicable，不许说成失败。**
    #[test]
    fn 没有构建步骤时说不适用() {
        let d = scratch("none");
        // 只有一个脚本，没有任何清单 → launch 会报"没有声明的 run 命令"之类
        fs::write(d.join("hello.py"), "print('hi')\n").unwrap();

        match smoke(&d, Duration::from_secs(30)) {
            SmokeVerdict::NotApplicable { .. } | SmokeVerdict::Unclear { .. } => {}
            other => panic!(
                "没有清单的目录不该被判成「跑了没过」：{:?}",
                other.headline()
            ),
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// **契约里写了 `build`，就必须跑它**（2026-10-18，codex 当第一视角试用时抓到的）。
    ///
    /// 两个真 bug：
    /// - `hello_rust` 契约写 `cargo build`，输出却说"这条是按用途推的，不是契约里写的"；
    /// - `hello_python` 契约写 `python -m compileall -q .`，输出却说"Python 没有构建这一步"。
    ///
    /// 这条测试用一条**明显不是默认值**的命令（`cargo --version`）钉住：
    /// 如果实现又退回"按用途推"，`cmd` 里就不会出现它。
    #[test]
    fn 契约里的构建命令必须被用上() {
        if !has_cargo() {
            eprintln!("跳过：这台机器上没有 cargo");
            return;
        }
        let d = scratch("contract-build");
        fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"cb\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(d.join("src").join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            d.join("env.contract.json"),
            r#"{"schema_version":1,"name":"cb","language":"Rust",
               "runtime":{"kind":"Rust"},"deps":[],
               "commands":{"build":"cargo --version","run":"cargo run"},
               "ready_when":{"exit_code":0}}"#,
        )
        .unwrap();

        match smoke(&d, Duration::from_secs(120)) {
            SmokeVerdict::Passed(done) => {
                assert!(
                    done.cmd.contains("cargo --version"),
                    "契约里写了构建命令就必须用它，实际跑了：{}",
                    done.cmd
                );
            }
            other => panic!("该通过，实际：{:?}", other.headline()),
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// **契约没有 `build` 时，Python 才可以说"没有这一步"**。
    ///
    /// 反过来也一样重要：没有声明 ≠ 我们没跑——要说"契约里没声明"，
    /// 而不是"Python 没有构建"（后者是替用户下结论）。
    #[test]
    fn 没有声明构建时说不适用而不是没跑() {
        let d = scratch("no-build");
        fs::write(d.join("main.py"), "print(1)\n").unwrap();
        fs::write(
            d.join("env.contract.json"),
            r#"{"schema_version":1,"name":"nb","language":"Python",
               "runtime":{"kind":"Python"},"deps":[],
               "commands":{"run":"python main.py"},"ready_when":{"exit_code":0}}"#,
        )
        .unwrap();
        match smoke(&d, Duration::from_secs(60)) {
            SmokeVerdict::NotApplicable { why } => {
                assert!(why.contains("契约里没有声明"), "该说清是契约没写：{why}");
            }
            other => panic!("该是「不适用」，实际：{:?}", other.headline()),
        }
        let _ = fs::remove_dir_all(&d);
    }
}
