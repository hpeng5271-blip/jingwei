//! **后台任务的真任务源**（2026-10-21）。
//!
//! 为什么需要它：角落那三个图标（索引 / 环境探测 / 深绘）**不许用假状态**——
//! 亮着就得有真任务在跑，灭了就得真跑完。所以先落一个极小的任务日志，
//! 由真正干活的命令写、由界面读；没有任务源的东西，界面只能显示"还没有任务源"。
//!
//! 形状：`%LOCALAPPDATA%\jingwei\tasks.json`（测试用 `JINGWEI_TASKS_FILE` 指定），
//! 一个数组，元素 `{id, kind, name, state, started_at, ended_at, detail}`。
//! 只留最近若干条；坏文件当没有（与缓存同一条规矩）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 任务种类：与界面那三个图标一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// 索引 / 扫描。
    Index,
    /// 环境探测。
    Env,
    /// 深度绘制。
    DeepGraph,
}

/// 一条任务记录。`state` 只有三种：跑着 / 完成 / 失败。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub kind: TaskKind,
    pub name: String,
    /// `running` / `done` / `failed`。
    pub state: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub detail: Option<String>,
}

/// 保留最近多少条（够界面看"刚才干了什么"，又不会无限长）。
const KEEP: usize = 20;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 任务文件位置：`%LOCALAPPDATA%\jingwei\tasks.json`；`JINGWEI_TASKS_FILE` 可覆盖（测试/隔离）。
pub fn tasks_path() -> PathBuf {
    if let Some(p) = std::env::var_os("JINGWEI_TASKS_FILE") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("jingwei").join("tasks.json")
}

/// 读任务表。坏文件 / 读不到 → 空表（**坏缓存当没有**）。
pub fn load(path: &Path) -> Vec<Task> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Task>>(&text).unwrap_or_default()
}

fn save(path: &Path, tasks: &[Task]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let keep: Vec<&Task> = tasks.iter().rev().take(KEEP).collect();
    let mut keep: Vec<Task> = keep.into_iter().cloned().collect();
    keep.reverse();
    std::fs::write(path, serde_json::to_string_pretty(&keep).unwrap_or_default())
}

/// 开一条任务：写 `running`，返回它的 id（收尾时用）。
pub fn start(path: &Path, kind: TaskKind, name: &str) -> String {
    let mut tasks = load(path);
    let id = format!("{}-{}", now_secs(), tasks.len());
    tasks.push(Task {
        id: id.clone(),
        kind,
        name: name.to_string(),
        state: "running".to_string(),
        started_at: now_secs(),
        ended_at: None,
        detail: None,
    });
    // 写不进去不算失败：任务照跑，界面最多显示"没有任务源"。
    save(path, &tasks).ok();
    id
}

/// 收尾：把对应那条改成 `done` / `failed`，并记一句 detail。
pub fn finish(path: &Path, id: &str, ok: bool, detail: Option<&str>) {
    let mut tasks = load(path);
    let Some(t) = tasks.iter_mut().find(|t| t.id == id) else {
        return;
    };
    t.state = if ok { "done" } else { "failed" }.to_string();
    t.ended_at = Some(now_secs());
    t.detail = detail.map(|s| s.to_string());
    save(path, &tasks).ok();
}

/// 现在有没有在跑的任务（界面据此决定图标亮不亮）。
pub fn running(path: &Path) -> Vec<Task> {
    load(path).into_iter().filter(|t| t.state == "running").collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-tasks-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 开一条 → 是 running；收尾 → done 且有结束时间。
    #[test]
    fn 开与收尾() {
        let d = tmp("life");
        let f = d.join("tasks.json");
        let id = start(&f, TaskKind::Index, "jingwei index");
        let r = running(&f);
        assert_eq!(r.len(), 1, "开着的任务该是 running：{r:?}");
        assert_eq!(r[0].id, id);
        finish(&f, &id, true, Some("1 文件 / 2 符号"));
        let all = load(&f);
        assert_eq!(all[0].state, "done");
        assert!(all[0].ended_at.is_some());
        assert!(running(&f).is_empty(), "收尾后不该还在跑");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 失败要如实记 `failed`，别写成 done。
    #[test]
    fn 失败记成_failed() {
        let d = tmp("fail");
        let f = d.join("tasks.json");
        let id = start(&f, TaskKind::DeepGraph, "深绘");
        finish(&f, &id, false, Some("超时"));
        assert_eq!(load(&f)[0].state, "failed");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 坏文件当没有（不许因为一份坏 JSON 让界面炸）。
    #[test]
    fn 坏文件当没有() {
        let d = tmp("bad");
        let f = d.join("tasks.json");
        std::fs::write(&f, "{not json").unwrap();
        assert!(load(&f).is_empty());
        assert!(running(&f).is_empty());
        std::fs::remove_dir_all(&d).ok();
    }

    /// 只留最近若干条：文件不会无限长。
    #[test]
    fn 只留最近二十条() {
        let d = tmp("keep");
        let f = d.join("tasks.json");
        for i in 0..25 {
            start(&f, TaskKind::Index, &format!("t{i}"));
        }
        let all = load(&f);
        assert!(all.len() <= KEEP, "超过上限了：{}", all.len());
        assert_eq!(all.last().map(|t| t.name.as_str()), Some("t24"), "最新的要留");
        std::fs::remove_dir_all(&d).ok();
    }
}
