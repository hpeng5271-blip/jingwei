//! `jingwei verify`：**跑一次真实构建**，回答"这台机器到底够不够用"。
//!
//! ## 为什么这个子命令值得存在
//!
//! `取舍记录.md` T-01 定死了"不装、不提权、不接管系统"。那么产品能给的最高价值就是
//! **把"够不够用"验准**——而"检测到 rustc"证明不了什么：没有链接器，`cargo build` 照样失败。
//!
//! ## 四态输出（与三态一一对应，`smoke.rs` 里写清了）
//!
//! ```text
//! ✓ 构建跑通了（退出码 0，1234ms）        ← 是
//! ✗ 构建没过（退出码 101）                ← 不是（这是结论）
//! · 这个项目没有构建这一步                 ← 不适用（退出码 0）
//! ? 我们没能验成：<原因>                   ← 查不了（退出码 2）
//! ```
//!
//! 失败时**先给"缺哪个件"**，再给第一条错误行——因为缺件时去看代码是白费力气。

use std::path::Path;
use std::time::Duration;

use jingwei_engine::understand::smoke::{smoke, SmokeVerdict};

use super::{c, BOLD, DIM, GREEN, RED, YELLOW};

/// 返回值直接当退出码用：0 = 通过/不适用，1 = 没过，2 = 我们没验成。
pub fn cmd_verify(root: &Path, json: bool) -> i32 {
    let v = smoke(root, Duration::from_secs(300));
    if json {
        // JSON 走同一份结论，不另算一遍（**一个事实一个出口**）。
        let (kind, done) = match &v {
            SmokeVerdict::Passed(d) => ("passed", Some(d)),
            SmokeVerdict::Failed(d) => ("failed", Some(d)),
            SmokeVerdict::NotApplicable { .. } => ("not_applicable", None),
            SmokeVerdict::Unclear { .. } => ("unclear", None),
        };
        let mut obj = serde_json::json!({ "verdict": kind, "headline": v.headline() });
        if let Some(d) = done {
            obj["cmd"] = d.cmd.clone().into();
            obj["exit_code"] = d.exit_code.into();
            obj["ms"] = (d.ms as u64).into();
            if let Some(e) = &d.first_error {
                obj["first_error"] = e.clone().into();
            }
            if let Some(m) = &d.missing {
                obj["missing_tool"] = serde_json::json!({
                    "what": m.what, "why": m.why, "shape": m.shape, "verify": m.verify
                });
            }
        }
        if let SmokeVerdict::NotApplicable { why } | SmokeVerdict::Unclear { why } = &v {
            obj["why"] = why.clone().into();
        }
        println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
        return match v {
            SmokeVerdict::Passed(_) | SmokeVerdict::NotApplicable { .. } => 0,
            SmokeVerdict::Failed(_) => 1,
            SmokeVerdict::Unclear { .. } => 2,
        };
    }

    println!();
    match &v {
        SmokeVerdict::Passed(d) => {
            println!("  {} {}", c(GREEN, "✓"), v.headline());
            println!("    {} {}", c(DIM, "跑了"), d.cmd);
        }
        SmokeVerdict::Failed(d) => {
            println!("  {} {}", c(RED, "✗"), v.headline());
            println!("    {} {}", c(DIM, "跑了"), d.cmd);
            // **先缺件，后错误行**：缺件时用户去改代码是白费力气。
            if let Some(m) = &d.missing {
                println!("  {} 这台机器缺 {}", c(YELLOW, "!"), c(BOLD, &m.what));
                println!("    {} {}", c(DIM, "为什么"), m.why);
                println!("    {} {}", c(DIM, "怎么装"), m.shape);
                println!("    {} {}", c(DIM, "怎么自己验"), m.verify);
            }
            if let Some(e) = &d.first_error {
                println!("    {} {}", c(DIM, "第一条错误"), e);
            }
            println!(
                "    {}",
                c(DIM, "完整输出用 jingwei run 看；这条命令只是替你验证「能不能编出来」。")
            );
        }
        SmokeVerdict::NotApplicable { why } => {
            println!("  {} {}", c(GREEN, "·"), v.headline());
            println!("    {}", c(DIM, why));
        }
        SmokeVerdict::Unclear { why } => {
            println!("  {} {}", c(YELLOW, "?"), v.headline());
            println!("    {}", c(DIM, why));
        }
    }
    println!();
    match v {
        SmokeVerdict::Passed(_) | SmokeVerdict::NotApplicable { .. } => 0,
        SmokeVerdict::Failed(_) => 1,
        SmokeVerdict::Unclear { .. } => 2,
    }
}
