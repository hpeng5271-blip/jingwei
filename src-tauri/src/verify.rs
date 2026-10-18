//! `verify_project`：**跑一次真实构建**，回答"这台机器够不够用"（界面上的那个按钮）。
//!
//! ## 为什么单独一个文件
//!
//! `main.rs` 有 1000 行上限（门禁守着），而这个命令本身是一段完整的转换逻辑。
//! 拆出来不是为了好看，是**让上限继续有效**——上限一旦被"登记放行"变松，
//! 下一次就会有人往里塞第二段。
//!
//! ## 四态
//!
//! `passed` / `failed` / `not_applicable` / `unclear`。
//! 最后一态是**我们没验成**，界面不能显示成"失败"（颜色与文案都分开）。

use jingwei_engine::understand::smoke::{smoke, SmokeVerdict};
use serde_json::Value;
use std::path::Path;

/// **跑一次真实构建**，回答"这台机器够不够用"（界面上的那个按钮）。
///
/// ## 为什么界面要有它
///
/// `verify` 是"够不够用"的唯一主动验证入口：检测到 rustc 不等于能编出来。
/// CLI 已经有 `jingwei verify`；界面没有的话，桌面用户就只能看着"能跑"猜。
///
/// ## 四态与退出码一一对应（传回给前端的是 `verdict` 字段）
///
/// `passed` / `failed` / `not_applicable` / `unclear` —— **不许压成"成功/失败"两态**。
/// `unclear` 是"我们没验成"，界面上不能显示成"失败"。
#[tauri::command]
pub fn verify_project(root: String) -> Value {
    let v = smoke(Path::new(&root), std::time::Duration::from_secs(300));
    let done = v.done();
    let mut out = serde_json::json!({
        "verdict": match &v {
            SmokeVerdict::Passed(_) => "passed",
            SmokeVerdict::Failed(_) => "failed",
            SmokeVerdict::NotApplicable { .. } => "not_applicable",
            SmokeVerdict::Unclear { .. } => "unclear",
        },
        "headline": v.headline(),
    });
    if let Some(d) = done {
        out["cmd"] = d.cmd.clone().into();
        out["exit_code"] = d.exit_code.into();
        out["ms"] = (d.ms as u64).into();
        if let Some(e) = &d.first_error {
            out["first_error"] = e.clone().into();
        }
        if let Some(m) = &d.missing {
            out["missing_tool"] = serde_json::json!({
                "what": m.what, "why": m.why, "shape": m.shape, "verify": m.verify, "url": m.url, "expect": m.expect
            });
        }
    }
    if let SmokeVerdict::NotApplicable { why } | SmokeVerdict::Unclear { why } = &v {
        out["why"] = why.clone().into();
    }
    out
}
