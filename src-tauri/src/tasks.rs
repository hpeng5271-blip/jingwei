//! **任务角标的数据源**（2026-10-21）。
//!
//! 界面角落那三个图标（索引 / 环境 / 深绘）**只认真任务**：数据来自
//! `jingwei_engine::understand::tasks` 写下的任务日志，而不是界面自己编的动画。
//!
//! 返回形状：
//! ```json
//! { "running": [{...}], "latest": { "index": {...}, "env": {...}, "deep_graph": {...} } }
//! ```
//! `latest` 里没有某类，就说明**这一类还没有任务源**——界面该显示"还没接"，而不是假装空闲。

use serde_json::{json, Value};

#[tauri::command]
pub(crate) fn tasks_state() -> Value {
    let path = jingwei_engine::understand::tasks::tasks_path();
    let all = jingwei_engine::understand::tasks::load(&path);
    let running = jingwei_engine::understand::tasks::running(&path);
    // "已跑多久 / 上次用了多久"（2026-10-22）：角标 hover 要说人话。
    // 数据就是日志里的 started_at / ended_at——**不新增采集**。
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut latest = serde_json::Map::new();
    for kind in ["index", "env", "deep_graph"] {
        if let Some(t) = all.iter().rev().find(|t| format!("{:?}", t.kind).to_lowercase() == kind) {
            latest.insert(
                kind.to_string(),
                json!({
                    "id": t.id, "state": t.state, "name": t.name,
                    "started_at": t.started_at, "ended_at": t.ended_at, "detail": t.detail,
                    // 跑着的报"已跑多久"，跑完的报"上次用了多久"。
                    "elapsed_s": match t.ended_at { Some(e) => (e - t.started_at).max(0), None => (now - t.started_at).max(0) },
                    "running": t.state == "running",
                }),
            );
        }
    }
    json!({ "running": running.len(), "latest": latest })
}
