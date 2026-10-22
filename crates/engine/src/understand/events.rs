//! **使用事件的保留骨架**（2026-10-22，D-20261022-03 Q4 / pro 裁定）。
//!
//! 为什么先做"保留"再做"采集"：事件一旦开始写，**保留策略就得先定**，
//! 否则要么无限长，要么事后改规矩、把已采集数据的正当性推翻。
//!
//! 规矩（pro Q4 原话）：
//! - **事件流是源，其余是投影**（图谱/状态/笔记都能从流重算）；
//! - **90 天详情 + 长期只留计数**；
//! - 投影（缓存）坏了**直接丢弃重算**，不留修复路径。
//!
//! 所以本模块只做两件事，不碰采集：
//! 1. `split`：把一批事件按 90 天切成「留详情」与「只留计数」；
//! 2. `counts`：跨全部事件算每个 (kind,key) 的计数——**计数不受 90 天影响**。
//!
//! 形状：JSONL，一行一条 `{at, kind, key, detail}`（append-only）。
//! 位置：`%LOCALAPPDATA%\jingwei\events.jsonl`（`JINGWEI_EVENTS_FILE` 可覆盖，测试用）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 详情保留多少天（pro Q4：90 天）。
pub const DETAIL_DAYS: i64 = 90;
const DAY: i64 = 86_400;

/// 一条使用事件。
///
/// `key` 是"这一条关于谁"的稳定标识（文件路径 / 符号 id / 命令名）；
/// `detail` 是**能被说出口的意图**（改了哪几行、卡在哪条诊断），**不含文件内容**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// unix 秒。
    pub at: i64,
    /// `open` / `edit` / `check` / `revisit` / `abandon`（pro Q3 的 5 条）。
    pub kind: String,
    /// 关于谁（路径或 id）。
    pub key: String,
    /// 说得出的一句话；旧事件的 detail 会被丢掉，只剩计数。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

/// 计数：`(kind, key) → 次数`。**不受 90 天影响**，是长期保留的那部分。
pub type Counts = BTreeMap<(String, String), u64>;

/// 90 天的分界点（`now - 90 天`）。早于它的事件只留计数。
pub fn cutoff(now: i64) -> i64 {
    now - DETAIL_DAYS * DAY
}

/// 把事件切成「留详情」与「只留计数」两堆。
///
/// 返回 `(kept, counts)`：`kept` 是按时间升序保留详情的事件（≥ cutoff），
/// `counts` 是**全部事件**的计数（包括被丢掉详情的那些）。
pub fn split(events: &[Event], now: i64) -> (Vec<Event>, Counts) {
    let line = cutoff(now);
    let mut kept = Vec::new();
    let mut counts: Counts = BTreeMap::new();
    for e in events {
        *counts.entry((e.kind.clone(), e.key.clone())).or_insert(0) += 1;
        if e.at >= line {
            kept.push(e.clone());
        }
    }
    kept.sort_by_key(|e| e.at);
    (kept, counts)
}

/// 事件文件位置：`%LOCALAPPDATA%\jingwei\events.jsonl`；`JINGWEI_EVENTS_FILE` 可覆盖。
pub fn events_path() -> PathBuf {
    if let Some(p) = std::env::var_os("JINGWEI_EVENTS_FILE") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("jingwei").join("events.jsonl")
}

/// 读出来的东西：事件 + 已经压好的长期计数。
///
/// 为什么要有 counts：压缩后旧事件只剩一行 `__counts__`；如果读的时候不认它，
/// **第二次压缩就会把老计数丢掉**（少算），那是数据损坏，不是"精简"。
#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub events: Vec<Event>,
    pub counts: Counts,
}

/// 读全部事件与计数；**坏行当没有**（与缓存同一条规矩：不因为一行坏掉就整份作废）。
pub fn load_all(path: &Path) -> Snapshot {
    let Ok(text) = std::fs::read_to_string(path) else { return Snapshot::default() };
    let mut snap = Snapshot::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        match serde_json::from_str::<Event>(line) {
            Ok(e) => {
                *snap.counts.entry((e.kind.clone(), e.key.clone())).or_insert(0) += 1;
                snap.events.push(e);
            }
            Err(_) => {
                // 可能是计数行：{"kind":"__counts__","counts":[{"kind":..,"key":..,"n":..}]}
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(arr) = v.get("counts").and_then(|c| c.as_array()) {
                        for row in arr {
                            let k = row.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                            let key = row.get("key").and_then(|x| x.as_str()).unwrap_or("");
                            let n = row.get("n").and_then(|x| x.as_u64()).unwrap_or(0);
                            if !k.is_empty() {
                                snap.counts.insert((k.to_string(), key.to_string()), n);
                            }
                        }
                    }
                }
            }
        }
    }
    snap
}

/// 只要事件（不计数的简化入口）。
pub fn load(path: &Path) -> Vec<Event> {
    load_all(path).events
}

/// 追加一条事件（append-only；调用方负责"这一刻该不该记"）。
pub fn append(path: &Path, ev: &Event) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(ev).unwrap_or_default())?;
    Ok(())
}

/// 把 `events` 压缩成"90 天详情 + 全部计数"，写回 `path`（先写临时文件再改名）。
///
/// 为什么压缩也要原子：中途崩了不能把事件流写坏——那是**唯一的源**。
pub fn compact(path: &Path, now: i64) -> std::io::Result<(usize, usize)> {
    let snap = load_all(path);
    let total = snap.events.len();
    let (kept, mut counts) = split(&snap.events, now);
    // 把之前压好的计数并进来（同键相加，**不能覆盖**）
    for ((k, key), n) in snap.counts {
        *counts.entry((k, key)).or_insert(0) += n.saturating_sub(0);
    }
    let mut out = String::new();
    for e in &kept {
        out.push_str(&serde_json::to_string(e).unwrap_or_default());
        out.push('\n');
    }
    // 计数单独一行 `{"counts":{...}}`：让人一眼能看出"这是长期计数，不是事件"
    let counts_json = serde_json::json!({
        "kind": "__counts__",
        "counts": counts.iter().map(|((k, key), n)| serde_json::json!({"kind": k, "key": key, "n": n})).collect::<Vec<_>>(),
    });
    out.push_str(&counts_json.to_string());
    out.push('\n');

    let tmp = path.with_extension("jsonl.tmp");
    if let Some(dir) = tmp.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, path)?;
    Ok((total, kept.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(at: i64, kind: &str, key: &str, detail: Option<&str>) -> Event {
        Event { at, kind: kind.into(), key: key.into(), detail: detail.map(|s| s.to_string()) }
    }

    #[test]
    fn 九十天内的详情留着_更早的只留计数() {
        let now = 1_800_000_000;
        let old = now - (DETAIL_DAYS + 1) * DAY;
        let fresh = now - 10 * DAY;
        let events = vec![
            ev(old, "edit", "src/a.rs", Some("早期详情，该丢")),
            ev(fresh, "edit", "src/a.rs", Some("近期详情，该留")),
        ];
        let (kept, counts) = split(&events, now);
        assert_eq!(kept.len(), 1, "只该留 90 天内的：{kept:?}");
        assert_eq!(kept[0].detail.as_deref(), Some("近期详情，该留"));
        assert_eq!(counts.get(&("edit".into(), "src/a.rs".into())), Some(&2), "计数要算全部");
    }

    /// **把 90 天钉死**：不钉的话，改常量时上面那条测试会跟着变、照样绿（变异验证抓出来的）。
    #[test]
    fn 八十九天留_九十一天丢() {
        assert_eq!(DETAIL_DAYS, 90, "保留窗口是 90 天（D-20261022-03 Q4），改了要一起改契约");
        let now = 2_000_000_000;
        let in89 = now - 89 * DAY;
        let out91 = now - 91 * DAY;
        let events = vec![
            ev(out91, "open", "src/a.rs", Some("91 天前")),
            ev(in89, "open", "src/a.rs", Some("89 天前")),
        ];
        let (kept, counts) = split(&events, now);
        assert_eq!(kept.len(), 1, "只该留 89 天那条：{kept:?}");
        assert_eq!(kept[0].detail.as_deref(), Some("89 天前"));
        assert_eq!(counts.get(&("open".into(), "src/a.rs".into())), Some(&2), "丢详情不等于丢计数");
    }
    #[test]
    fn 计数不受九十天影响_且按kind与key分开() {
        let now = 1_900_000_000;
        let old = now - 400 * DAY;
        let events = vec![
            ev(old, "open", "src/a.rs", None),
            ev(now, "open", "src/a.rs", None),
            ev(now, "check", "src/a.rs", Some("1 error")),
        ];
        let (_kept, counts) = split(&events, now);
        assert_eq!(counts.get(&("open".into(), "src/a.rs".into())), Some(&2));
        assert_eq!(counts.get(&("check".into(), "src/a.rs".into())), Some(&1));
        assert_eq!(counts.len(), 2, "不同 kind 不该合并");
    }

    #[test]
    fn 压缩之后还能再压_且计数不丢() {
        let dir = std::env::temp_dir().join(format!("jw-events-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        std::fs::remove_file(&f).ok();
        let now = 1_950_000_000;
        for i in 0..5 {
            append(&f, &ev(now - i * DAY, "edit", "src/a.rs", Some("改了一行"))).unwrap();
        }
        append(&f, &ev(now - 300 * DAY, "edit", "src/a.rs", Some("很旧的"))).unwrap();

        let (total, kept) = compact(&f, now).unwrap();
        assert_eq!(total, 6);
        assert_eq!(kept, 5, "6 条里 1 条超 90 天");
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("__counts__"), "计数要单独留一行：{text}");
        assert!(!text.contains("很旧的"), "超 90 天的 detail 必须丢掉");
        // 再压一次：结果稳定（幂等）
        let (t2, k2) = compact(&f, now).unwrap();
        assert_eq!((t2, k2), (kept, kept), "再压不该变多");
        std::fs::remove_dir_all(&dir).ok();
    }
}
