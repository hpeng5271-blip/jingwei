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
    // **BOM 不能当内容**：Windows 上写文件的工具经常带 BOM；不剥掉的话第一条事件
    // 会因为开头多一个 \u{feff} 而解析失败、被"坏行当没有"静默丢掉——那是数据丢失。
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
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
                                // **+ 不是覆盖**：计数行与事件行必须相加（2026-10-23）
                                *snap.counts.entry((k.to_string(), key.to_string())).or_insert(0) += n;
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
    let (kept, _all_counts) = split(&snap.events, now);
    // 保留事件（窗口内）的计数：这些行还在文件里，下次会被重新数，先从计数里扣掉
    let mut kept_counts: Counts = BTreeMap::new();
    for e in &kept {
        *kept_counts.entry((e.kind.clone(), e.key.clone())).or_insert(0) += 1;
    }
    // **计数行只存"文件里已经不在"的那部分**（2026-10-23 抓到的真 bug：压两次 2 → 5）。
    // 推导：snap.counts = 旧计数行 + 文件里每条事件；减去 all_counts（文件里事件的计数）
    // 剩下的才是"已归档"。**不要**再把 all_counts 加回去——那正是重复相加的来源。
    let mut counts = snap.counts;
    for (k, n) in &kept_counts {
        if let Some(slot) = counts.get_mut(k) {
            *slot = slot.saturating_sub(*n);
        }
    }
    counts.retain(|_, n| *n > 0);
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

// ───────────────────────── 采集策略（pro Q3） ─────────────────────────

/// 采集时**必须回答的上下文**。任何拿不准的情况都退回"不记"——宁可少记，不许记错。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// 人在界面/命令行里主动做的事。
    Human,
    /// 程序自己发起的取数（例如 GUI 为了显示而调 CLI）——**绝不记**，否则事件流被自己污染。
    ProgramSpawn,
    /// 后台对账/自动刷新（允许表、任务角标轮询）——**绝不记**。
    AutoReconcile,
}

/// "绝不记"清单（pro Q3 原话）：帮助调用、纯滚动/悬停、后台内部查询、自动对账、GUI spawn 的取数。
pub fn should_record(source: Source, kind: &str) -> bool {
    if source != Source::Human {
        return false;
    }
    matches!(kind, "open" | "edit" | "check" | "revisit" | "abandon")
}

/// 同一条 `open` 在这个窗口内不重复记（pro Q3：5 分钟）。
pub const OPEN_DEDUP_SECS: i64 = 300;

/// 复访判定窗口（pro Q3：30 分钟）。
pub const REVISIT_GAP_SECS: i64 = 1800;

/// 采集器：所有"该不该记"的判断都在这里，调用方只描述发生了什么。
///
/// 为什么集中在一处：如果界面和 CLI 各写各的判断，同一条规矩会被实现两遍，然后漂移。
pub struct Recorder {
    path: PathBuf,
}

impl Recorder {
    pub fn new(path: PathBuf) -> Self { Self { path } }

    /// 用默认位置（`%LOCALAPPDATA%\jingwei\events.jsonl`）。
    pub fn with_default_path() -> Self { Self::new(events_path()) }

    /// 记一条事件；返回 `true` 表示真的写进去了（便于测试与自查）。
    pub fn record(&self, ev: Event, source: Source) -> bool {
        if !should_record(source, &ev.kind) {
            return false;
        }
        if ev.key.trim().is_empty() {
            return false;
        }
        append(&self.path, &ev).is_ok()
    }

    /// 打开一个文件：**5 分钟内同一个 key 只记一次**。
    pub fn open_file(&self, path: &str, now: i64, source: Source) -> bool {
        let recent = load(&self.path)
            .iter()
            .rev()
            .take(50)
            .any(|e| e.kind == "open" && e.key == path && now - e.at < OPEN_DEDUP_SECS);
        if recent {
            return false;
        }
        self.record(Event { at: now, kind: "open".into(), key: path.into(), detail: None }, source)
    }

    /// 保存后内容变了：**只记行数变化，不记内容**（pro Q3 的硬边界）。
    pub fn edit_file(&self, path: &str, lines_before: usize, lines_after: usize, now: i64) -> bool {
        if lines_before == lines_after {
            return false; // 没变就不算"改动"
        }
        let delta = lines_after as i64 - lines_before as i64;
        self.record(
            Event {
                at: now,
                kind: "edit".into(),
                key: path.into(),
                detail: Some(format!("行数 {lines_before} → {lines_after}（{delta:+}）")),
            },
            Source::Human,
        )
    }

    /// 检查跑完：只记这个文件的 error/warn 数与是否变绿。
    pub fn check_file(&self, path: &str, errors: usize, warns: usize, now: i64) -> bool {
        self.record(
            Event {
                at: now,
                kind: "check".into(),
                key: path.into(),
                detail: Some(format!("error {errors} / warn {warns}")),
            },
            Source::Human,
        )
    }

    /// 复访：距上次同一 key 的 `open` 超过 30 分钟才记。
    pub fn revisit(&self, path: &str, now: i64) -> bool {
        let last = load(&self.path)
            .iter()
            .rev()
            .find(|e| e.key == path && (e.kind == "open" || e.kind == "revisit"))
            .map(|e| e.at);
        match last {
            Some(t) if now - t < REVISIT_GAP_SECS => false,
            _ => self.record(
                Event { at: now, kind: "revisit".into(), key: path.into(), detail: None },
                Source::Human,
            ),
        }
    }

    /// 放弃：打开后很短时间就离开、且期间没有 `edit`（时间式，先能跑；行为式以后替换）。
    pub fn abandon(&self, path: &str, now: i64) -> bool {
        let events = load(&self.path);
        let last_open = events.iter().rev().find(|e| e.kind == "open" && e.key == path);
        let Some(open) = last_open else { return false };
        if now - open.at > ABANDON_WINDOW_SECS {
            return false;
        }
        let edited = events
            .iter()
            .any(|e| e.kind == "edit" && e.key == path && e.at >= open.at);
        if edited {
            return false;
        }
        self.record(
            Event { at: now, kind: "abandon".into(), key: path.into(), detail: None },
            Source::Human,
        )
    }
}

/// "打开又马上离开"的窗口（abandon 的时间式判据）。
pub const ABANDON_WINDOW_SECS: i64 = 30;

    #[test]
    fn 绝不记清单_程序取数不记_帮助调用不记() {
        assert!(!should_record(Source::ProgramSpawn, "open"), "GUI spawn 的取数不许记");
        assert!(!should_record(Source::AutoReconcile, "check"), "自动对账不许记");
        assert!(!should_record(Source::Human, "-h"), "帮助调用不算行为");
        assert!(!should_record(Source::Human, "scroll"), "纯滚动不记");
        assert!(should_record(Source::Human, "edit"), "人改文件要记");
    }

    #[test]
    fn edit事件不含文件内容_只含行数变化() {
        let dir = std::env::temp_dir().join(format!("jw-ev-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        std::fs::remove_file(&f).ok();
        let rec = Recorder::new(f.clone());
        assert!(rec.edit_file("src/a.rs", 10, 12, 1_000));
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("行数 10 → 12"), "要有行数变化：{text}");
        // 断言"不含代码"要挑真正的代码特征（JSON 本身就有花括号，别拿它当判据）
        for code in ["fn ", "def ", "=>", "import "] {
            assert!(!text.contains(code), "**不许含代码内容**（发现 {code}）：{text}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 五分钟内同一个open只记一次() {
        let dir = std::env::temp_dir().join(format!("jw-ev-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        std::fs::remove_file(&f).ok();
        let rec = Recorder::new(f.clone());
        assert!(rec.open_file("src/a.rs", 10_000, Source::Human));
        assert!(!rec.open_file("src/a.rs", 10_100, Source::Human), "5 分钟内不重复");
        assert!(rec.open_file("src/a.rs", 10_000 + OPEN_DEDUP_SECS + 1, Source::Human), "过了窗口要记");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 复访要隔三十分钟_放弃只在没改动时记() {
        let dir = std::env::temp_dir().join(format!("jw-ev-rev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        std::fs::remove_file(&f).ok();
        let rec = Recorder::new(f.clone());
        assert!(rec.open_file("src/a.rs", 1_000, Source::Human));
        assert!(!rec.revisit("src/a.rs", 1_100), "30 分钟内不算复访");
        assert!(rec.revisit("src/a.rs", 1_000 + REVISIT_GAP_SECS + 1), "过了窗口算复访");
        // 同一秒内打开后马上离开：没 edit → 记 abandon
        assert!(rec.open_file("src/b.rs", 2_000, Source::Human));
        assert!(rec.abandon("src/b.rs", 2_010));
        // 有 edit 就不算放弃
        assert!(rec.open_file("src/c.rs", 3_000, Source::Human));
        assert!(rec.edit_file("src/c.rs", 5, 6, 3_005));
        assert!(!rec.abandon("src/c.rs", 3_010));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// **边界**（2026-10-23）：详情滚出 90 天后，计数**不许丢，也不许重复相加**。
    /// 这条曾抓到真 bug：压两次后 2 变成 5。
    #[test]
    fn 滚动九十天后计数既不多也不少() {
        let dir = std::env::temp_dir().join(format!("jw-ev-roll2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        std::fs::remove_file(&f).ok();
        let now = 2_100_000_000;
        append(&f, &Event { at: now - 120 * DAY, kind: "edit".into(), key: "src/a.rs".into(), detail: Some("很旧的".into()) }).unwrap();
        append(&f, &Event { at: now - DAY, kind: "edit".into(), key: "src/a.rs".into(), detail: Some("新的".into()) }).unwrap();

        let (t1, k1) = compact(&f, now).unwrap();
        assert_eq!((t1, k1), (2, 1), "压完该留 1 条详情");
        assert_eq!(
            load_all(&f).counts.get(&("edit".into(), "src/a.rs".into())),
            Some(&2),
            "压一次后计数该是 2"
        );

        let (_t2, k2) = compact(&f, now).unwrap();
        assert_eq!(k2, 1, "再压一次详情仍是 1 条");
        assert_eq!(
            load_all(&f).counts.get(&("edit".into(), "src/a.rs".into())),
            Some(&2),
            "**计数还是 2**（滚出窗口不等于没发生过，也不许重复相加）"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 手写json也能解析() {
        let raw = "{\"at\":1,\"kind\":\"open\",\"key\":\"x\"}";
        let e: Event = serde_json::from_str(raw).expect("手写 JSON 该能解析");
        assert_eq!(e.kind, "open");
        assert_eq!(e.key, "x");
        assert!(e.detail.is_none());
    }

    #[test]
    fn load_all_能读手写文件() {
        let dir = std::env::temp_dir().join(format!("jw-ev-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        std::fs::write(&f, "{\"at\":1000,\"kind\":\"open\",\"key\":\"src/a.rs\"}\n").unwrap();
        let snap = load_all(&f);
        assert_eq!(snap.events.len(), 1, "手写文件该读到 1 条：{:?}", snap.events);
        assert_eq!(snap.counts.len(), 1, "计数该有 1 组：{:?}", snap.counts);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 带_bom_的文件第一条也要读到() {
        let dir = std::env::temp_dir().join(format!("jw-ev-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        std::fs::write(&f, "\u{feff}{\"at\":1000,\"kind\":\"open\",\"key\":\"src/a.rs\"}\n").unwrap();
        let snap = load_all(&f);
        assert_eq!(snap.events.len(), 1, "BOM 不该让第一条消失：{:?}", snap.events);
        std::fs::remove_dir_all(&dir).ok();
    }

/// 一个 key（文件路径）的汇总（2026-10-23）：serve 与 CLI 共用，不许各算一份。
#[derive(Debug, Default, Clone)]
pub struct KeySummary {
    pub open: u64,
    pub edit: u64,
    pub check: u64,
    pub revisit: u64,
    pub last_check: Option<String>,
}

impl Snapshot {
    /// 按 key 汇总（只数 `events` 里还在的那些；已归档的只剩计数，不参与分类）。
    pub fn by_key(&self, key: &str) -> KeySummary {
        let mut out = KeySummary::default();
        for e in self.events.iter().filter(|e| e.key == key) {
            match e.kind.as_str() {
                "open" => out.open += 1,
                "edit" => out.edit += 1,
                "check" => { out.check += 1; out.last_check = e.detail.clone(); }
                "revisit" => out.revisit += 1,
                _ => {}
            }
        }
        out
    }

    /// 学习进度：`复访 ≥3 且最后一次检查仍红` = 待解决；检查过且不红 = 已变绿。
    pub fn progress(&self) -> (Vec<String>, Vec<String>) {
        let mut todo = Vec::new();
        let mut learned = Vec::new();
        let mut keys: Vec<&str> = self.events.iter().map(|e| e.key.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        for k in keys {
            let st = self.by_key(k);
            let red = st
                .last_check
                .as_deref()
                .map(|d| d.contains("error ") && !d.contains("error 0"))
                .unwrap_or(false);
            if st.revisit >= 3 && red {
                todo.push(k.to_string());
            } else if st.check > 0 && !red {
                learned.push(k.to_string());
            }
        }
        (todo, learned)
    }
}
