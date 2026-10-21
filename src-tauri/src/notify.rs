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

// ── 消息阵列（2026-10-22）────────────────────────────────────────────────
//
// **判据仍在引擎**（去重键、计数、New→Shown→Read→Archived）；这里只持有那份状态，
// 让界面能"记一条 / 标记已读 / 归档"而不用把生命周期逻辑抄到 JS 里（失效模式 #17）。

use std::sync::{Mutex, OnceLock};

fn msgs() -> &'static Mutex<Vec<jingwei_engine::understand::notify::Message>> {
    static M: OnceLock<Mutex<Vec<jingwei_engine::understand::notify::Message>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Vec::new()))
}

fn level_of(s: &str) -> jingwei_engine::understand::notify::Level {
    use jingwei_engine::understand::notify::Level;
    match s {
        "error" => Level::Error,
        "status" => Level::Status,
        "progress" => Level::Progress,
        _ => Level::Info,
    }
}

/// 记一条消息（同 key 只占一槽并计数），返回当前整张表。
#[tauri::command]
pub(crate) fn msg_ingest(key: String, level: String, text: String, at: i64) -> Value {
    let mut g = msgs().lock().unwrap_or_else(|e| e.into_inner());
    jingwei_engine::understand::notify::ingest(&mut g, &key, level_of(&level), &text, at);
    json!(g.iter().map(|m| json!({
        "key": m.key, "level": format!("{:?}", m.level).to_lowercase(), "text": m.text,
        "count": m.count, "state": format!("{:?}", m.state).to_lowercase(),
        "first_at": m.first_at, "last_at": m.last_at,
    })).collect::<Vec<_>>())
}

/// 标记状态；非法转移返回 Err（界面据此提示，而不是悄悄当成成功）。
#[tauri::command]
pub(crate) fn msg_mark(key: String, to: String) -> Result<Value, String> {
    use jingwei_engine::understand::notify::MsgState;
    let target = match to.as_str() {
        "shown" => MsgState::Shown,
        "read" => MsgState::Read,
        "archived" => MsgState::Archived,
        _ => MsgState::New,
    };
    let mut g = msgs().lock().unwrap_or_else(|e| e.into_inner());
    jingwei_engine::understand::notify::mark(&mut g, &key, target)?;
    Ok(json!(g.iter().map(|m| json!({
        "key": m.key, "count": m.count, "state": format!("{:?}", m.state).to_lowercase(),
    })).collect::<Vec<_>>()))
}
