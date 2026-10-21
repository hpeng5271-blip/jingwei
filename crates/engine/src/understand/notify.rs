//! **消息与提示的判据**（2026-10-21）。
//!
//! 这一层不渲染、不开窗：只回答四件事——
//! 1. 同一条消息怎么**去重计数**（消息是资源，不是日志）；
//! 2. 状态怎么走（`New → Shown → Read → Archived`，**归档后不许再未读**）；
//! 3. 静音多久（按"重犯次数 × 项目变更频率"自适应，夹在 30 秒 ~ 30 分钟）；
//! 4. 现在该不该提示（静音期内不重复，从没提示过就提示）。
//!
//! GUI 只消费结果：**判据在这里只有一份**，界面不另算一套。
use serde::{Deserialize, Serialize};


/// 消息的优先级（P0 最高）。与界面的"槽位预算"对齐：P0/P1 不许被挤掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// 阻塞/报错：必须看见，可抢占任何槽。
    Error,
    /// 重要结论（状态/黄条）：常驻一条，只更新内容。
    Status,
    /// 一般信息：会被挤到"消息阵列"。
    Info,
    /// 后台进度：默认不占主屏，只计数。
    Progress,
}

/// 一条消息的生命周期。**只有一条合法链**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MsgState {
    New,
    Shown,
    Read,
    Archived,
}

/// 一条消息。`key` 是去重键（例如 `错误码:场景:项目`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub key: String,
    pub level: Level,
    pub text: String,
    /// 出现次数（去重后的计数）。
    pub count: u32,
    pub state: MsgState,
    pub first_at: i64,
    pub last_at: i64,
}

/// 收一条消息：同 key 只留一条，计数 +1，并记下最后一次时间。
///
/// 重复出现 = **新事件**，所以状态回到 `New`（哪怕之前已读/已归档）——
/// 归档不等于"这类事以后不会再有"。
pub fn ingest(list: &mut Vec<Message>, key: &str, level: Level, text: &str, at: i64) {
    if let Some(m) = list.iter_mut().find(|m| m.key == key) {
        m.count = m.count.saturating_add(1);
        m.text = text.to_string();
        m.last_at = at;
        m.state = MsgState::New;
        return;
    }
    list.push(Message {
        key: key.to_string(),
        level,
        text: text.to_string(),
        count: 1,
        state: MsgState::New,
        first_at: at,
        last_at: at,
    });
}

/// 状态转移：只认 `New → Shown|Read`、`Shown → Read`、`Read → Archived`、`Shown → Archived`。
///
/// 非法转移返回 `Err`（**不许静默**）：例如"已归档 → 未读"会让处理过的东西反复跳出来。
pub fn mark(list: &mut [Message], key: &str, to: MsgState) -> Result<(), String> {
    let Some(m) = list.iter_mut().find(|m| m.key == key) else {
        return Err(format!("没有这条消息：{key}"));
    };
    let from = m.state;
    let ok = matches!(
        (from, to),
        (MsgState::New, MsgState::Shown)
            | (MsgState::New, MsgState::Read)
            | (MsgState::New, MsgState::Archived)
            | (MsgState::Shown, MsgState::Read)
            | (MsgState::Shown, MsgState::Archived)
            | (MsgState::Read, MsgState::Archived)
            | (MsgState::Shown, MsgState::Shown)
    );
    if !ok {
        return Err(format!("非法转移：{from:?} → {to:?}（{key}）"));
    }
    m.state = to;
    Ok(())
}

/// 静音窗口（秒）：`30 × (1 + 重犯次数) × (1 − 0.5 × 变更频率)`，夹在 `[30, 1800]`。
///
/// 两条直觉：项目变得越快，旧提示越快过期 → 静音要**短**；同一场景反复炸 → 静音要**长**。
/// 参数可调，但"有上下限"这条不许动——否则要么刷屏，要么永远闭嘴。
pub fn silence_secs(freq: f64, repeats: u32) -> i64 {
    let f = freq.clamp(0.0, 1.0);
    let base = 30.0 * (1.0 + repeats as f64) * (1.0 - 0.5 * f);
    base.clamp(30.0, 1800.0) as i64
}

/// 现在该不该提示：没提示过 → 提示；距上次提示超过静音窗口 → 提示；否则闭嘴。
///
/// **场景变了要由调用方传 `last_at = None`**（换了错误码/目标文件就是另一件事），
/// 这样"同类不同处"不会被误当成"重复"。
pub fn should_notify(last_at: Option<i64>, now: i64, silence: i64) -> bool {
    match last_at {
        None => true,
        Some(t) => now.saturating_sub(t) >= silence,
    }
}
