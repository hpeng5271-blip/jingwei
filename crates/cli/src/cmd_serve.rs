//! `jingwei serve` —— **常驻通道的骨架**（2026-10-22，D-20261022-03 §三第 4 项的准备）。
//!
//! 为什么先做骨架而不是直接换掉界面的取数：现在没有"通道值不值得"的数字。
//! 先把**一问一答**做成能跑、能测的最小形态，再拿 `perf` 的往返耗时去决定要不要接界面。
//!
//! 协议：**一行一个请求、一行一个响应**（JSON），字段 `id` / `method` / `params`。
//! 方法（先只有三个，都是只读）：
//! - `ping` → `{"pong":true}`
//! - `metrics` → 首屏三数字（与 `handover` 同一处实现）
//! - `events` → 事件计数与最近条数（与 `jingwei events` 同一处实现）
//!
//! **不做**：并发、缓存、鉴权、流式输出——这些等数字说话再说。

use jingwei_engine::understand::events::{cutoff, events_path, load_all};
use std::io::{BufRead, Write};
use std::path::Path;

pub(crate) fn cmd_serve(root: &Path) -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(
                    stdout,
                    "{}",
                    serde_json::json!({"id": null, "ok": false, "error": format!("请求不是 JSON：{e}")})
                ).ok();
                stdout.flush().ok();
                continue;
            }
        };
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let resp = match method {
            "ping" => serde_json::json!({"id": id, "ok": true, "result": {"pong": true}}),
            "metrics" => {
                let m = crate::cmd_handover::metrics(root);
                serde_json::json!({"id": id, "ok": true, "result": m})
            }
            "events" => {
                let path = events_path();
                let snap = load_all(&path);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let line_at = cutoff(now);
                let total: u64 = snap.counts.values().sum();
                let recent = snap.events.iter().filter(|e| e.at >= line_at).count();
                serde_json::json!({
                    "id": id, "ok": true,
                    "result": {"total": total, "kinds": snap.counts.len(), "recent": recent},
                })
            }
            // 2026-10-23：这两条也要走常驻通道（原来只能 spawn）。
            // 判据复用 engine 的 `progress()` / `by_key()`，不在这里再实现一遍。
            "progress" => {
                let snap = load_all(&events_path());
                let (todo, learned) = snap.progress();
                serde_json::json!({"id": id, "ok": true, "result": {"todo": todo, "learned": learned}})
            }
            "events_summary" => {
                let key = req
                    .get("params")
                    .and_then(|p| p.get("key"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("");
                let snap = load_all(&events_path());
                let st = snap.by_key(key);
                serde_json::json!({"id": id, "ok": true, "result": {
                    "key": key, "open": st.open, "edit": st.edit, "check": st.check,
                    "revisit": st.revisit, "last_check": st.last_check,
                }})
            }
            other => serde_json::json!({
                "id": id, "ok": false,
                "error": format!("不认识的方法：{other}（支持 ping / metrics / events）"),
            }),
        };
        if writeln!(stdout, "{resp}").is_err() {
            break;
        }
        if stdout.flush().is_err() {
            break;
        }
    }
    0
}
