//! **提示的闸门**（2026-10-21）：界面不自己算"该不该弹"，问引擎。
//!
//! 为什么不在 JS 里再写一遍公式：静音窗口（`clamp(30×(1+重犯)×(1−0.5×频率), 30, 1800)`）
//! 是判据的一部分，**判据只能有一处实现**。JS 再抄一份，两边迟早走偏。
//!
//! 界面负责"记"（上次弹是什么时候、重犯几次），这里负责"判"。

use serde_json::{json, Value};

/// `last_at` = 上次弹的时刻（秒，`None` 表示没弹过）；`freq` = 项目变更频率（0..1）。
///
/// 返回 `{ allow, silence, why }`：`allow=false` 时界面**不弹**，但内容仍然在（用户自己点开能看）。
#[tauri::command]
pub(crate) fn notify_gate(last_at: Option<i64>, now: i64, freq: f64, repeats: u32) -> Value {
    let silence = jingwei_engine::understand::notify::silence_secs(freq, repeats);
    let allow = jingwei_engine::understand::notify::should_notify(last_at, now, silence);
    let why = if allow {
        if last_at.is_none() {
            "第一次提示".to_string()
        } else {
            format!("距上次提示已过 {silence} 秒")
        }
    } else {
        format!("静音中：还要 {} 秒", silence - (now - last_at.unwrap_or(now)))
    };
    json!({ "allow": allow, "silence": silence, "why": why })
}
