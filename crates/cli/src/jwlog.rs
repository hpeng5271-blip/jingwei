//! **CLI 调试日志**（D5）。
//!
//! 三条硬规矩：
//! 1. **stdout 只放数据**：日志一律走 stderr，`--json` 的 stdout 永远是纯 JSON。
//! 2. **关闭时零分配**：先看级别（一个原子读），不够就直接 return，不拼串、不格式化。
//! 3. **脱敏**：路径保留；token / 密码 / 连接串一律 `***`。
//!
//! 开关：`--debug`，或环境变量 `JW_LOG=error|warn|info|debug|trace`；
//! `JW_LOG=json` 输出 JSONL；`JW_LOG_FILE=1` 另写
//! `%LOCALAPPDATA%\jingwei\logs\jw-YYYYMMDD.log`（按天滚，保留 7 天）。

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }
    fn from_u8(v: u8) -> Level {
        match v {
            0 => Level::Error,
            1 => Level::Warn,
            2 => Level::Info,
            3 => Level::Debug,
            _ => Level::Trace,
        }
    }
}

/// 当前级别（默认 Info = 2）。
static LEVEL: AtomicU8 = AtomicU8::new(2);
static JSON: AtomicBool = AtomicBool::new(false);
static FILE: AtomicBool = AtomicBool::new(false);

pub fn parse_level(s: &str) -> Option<Level> {
    match s.trim().to_ascii_lowercase().as_str() {
        "error" | "err" => Some(Level::Error),
        "warn" | "warning" => Some(Level::Warn),
        "info" => Some(Level::Info),
        "debug" => Some(Level::Debug),
        "trace" => Some(Level::Trace),
        _ => None,
    }
}

/// 这条日志要不要发。**调用方必须先问它，再去拼字符串**（零分配的前提）。
pub fn on(l: Level) -> bool {
    (l as u8) <= LEVEL.load(Ordering::Relaxed)
}

pub fn current() -> Level {
    Level::from_u8(LEVEL.load(Ordering::Relaxed))
}

/// 初始化：`--debug` 优先；否则读 `JW_LOG`；`JW_LOG=json` 切 JSONL；`JW_LOG_FILE=1` 落盘。
pub fn init(debug_flag: bool) {
    let mut lvl = Level::Info;
    let mut json = false;
    let mut file = false;
    if let Ok(v) = std::env::var("JW_LOG") {
        let v = v.trim().to_ascii_lowercase();
        if v == "json" {
            json = true;
        } else if let Some(l) = parse_level(&v) {
            lvl = l;
        }
    }
    if let Ok(v) = std::env::var("JW_LOG_FILE") {
        file = v.trim() == "1" || v.trim().eq_ignore_ascii_case("true");
    }
    if debug_flag {
        lvl = Level::Debug;
    }
    LEVEL.store(lvl as u8, Ordering::Relaxed);
    JSON.store(json, Ordering::Relaxed);
    FILE.store(file, Ordering::Relaxed);
    if file {
        if let Some(dir) = log_dir() {
            let _ = std::fs::create_dir_all(&dir);
            prune(&dir, 7);
        }
    }
}

/// 发一条日志。`kvs` 是 `(键, 值)` 对；值会按 [`redact`] 处理。
pub fn emit(l: Level, event: &str, kvs: &[(&str, &str)]) {
    if !on(l) {
        return;
    }
    let (day, ts) = now_parts();
    let line = if JSON.load(Ordering::Relaxed) {
        let mut m = serde_json::Map::new();
        m.insert("ts".into(), serde_json::Value::String(ts.clone()));
        m.insert("level".into(), serde_json::Value::String(l.as_str().into()));
        m.insert("event".into(), serde_json::Value::String(event.into()));
        for (k, v) in kvs {
            m.insert((*k).into(), serde_json::Value::String(redact(k, v)));
        }
        serde_json::to_string(&serde_json::Value::Object(m)).unwrap_or_default()
    } else {
        let mut s = format!("{ts} {} {event}", l.as_str());
        for (k, v) in kvs {
            s.push(' ');
            s.push_str(k);
            s.push('=');
            s.push_str(&redact(k, v));
        }
        s
    };
    let _ = writeln!(std::io::stderr(), "{line}");
    if FILE.load(Ordering::Relaxed) {
        if let Some(dir) = log_dir() {
            let p = dir.join(format!("jw-{day}.log"));
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

/// 敏感键：整值替换为 `***`。
const SENSITIVE: &[&str] = &[
    "token", "password", "passwd", "secret", "api_key", "apikey", "authorization",
    "auth", "credential", "cookie", "conn", "conn_str", "connection", "dsn",
];

/// 脱敏：路径保留；敏感键或带 `user:pass@` 的连接串 → `***`。
pub fn redact(key: &str, val: &str) -> String {
    let k = key.to_ascii_lowercase();
    if SENSITIVE.iter().any(|s| k == *s || k.contains(s)) {
        return "***".to_string();
    }
    // 连接串：`scheme://user:pass@host` → `scheme://***@host`
    if let Some(scheme) = val.find("://") {
        let rest = &val[scheme + 3..];
        if let Some(at) = rest.find('@') {
            if rest[..at].contains(':') {
                return format!("{}://***@{}", &val[..scheme], &rest[at + 1..]);
            }
        }
    }
    val.to_string()
}

fn log_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join("AppData").join("Local")))?;
    Some(base.join("jingwei").join("logs"))
}

/// 只留最近 `keep_days` 天（按文件名里的日期判断，不依赖文件 mtime）。
pub fn prune(dir: &std::path::Path, keep_days: u32) {
    let (today, _) = now_parts();
    let Ok(td) = today.parse::<u32>() else { return };
    let cutoff = td.saturating_sub(keep_days);
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("jw-") else { continue };
        let day = rest.trim_end_matches(".log");
        let Ok(d) = day.parse::<u32>() else { continue };
        if d < cutoff {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// `(YYYYMMDD, YYYY-MM-DD HH:MM:SS)`，本地时间用 UTC 秒换算（不引第三方）。
fn now_parts() -> (String, String) {
    let secs = datacore::store::now_secs().max(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    (format!("{y:04}{m:02}{d:02}"), format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}"))
}

/// Howard Hinnant 的 civil_from_days（epoch 天数 → 年月日）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_keys_are_masked() {
        assert_eq!(redact("token", "abc"), "***");
        assert_eq!(redact("DB_PASSWORD", "hunter2"), "***");
        assert_eq!(redact("path", r"D:\projects\jingwei"), r"D:\projects\jingwei");
    }

    #[test]
    fn url_userinfo_is_masked_but_host_kept() {
        assert_eq!(
            redact("url", "postgres://alice:s3cret@db.local:5432/app"),
            "postgres://***@db.local:5432/app"
        );
        assert_eq!(redact("url", "https://example.com/x"), "https://example.com/x");
        // 敏感键优先：键名本身敏感就整值打码（不再做 URL 拆解）
        assert_eq!(redact("dsn", "postgres://alice:s3cret@db.local/app"), "***");
    }

    #[test]
    fn level_parsing_and_gate() {
        assert_eq!(parse_level("DEBUG"), Some(Level::Debug));
        assert_eq!(parse_level("nope"), None);
        assert!(Level::Error < Level::Info && Level::Info < Level::Trace);
    }

    #[test]
    fn prune_keeps_recent_days_only() {
        let dir = std::env::temp_dir().join(format!("jw-log-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (today, _) = now_parts();
        let old = format!("jw-{:08}.log", today.parse::<u32>().unwrap().saturating_sub(9));
        std::fs::write(dir.join(&old), "x").unwrap();
        std::fs::write(dir.join(format!("jw-{today}.log")), "x").unwrap();
        prune(&dir, 7);
        assert!(!dir.join(&old).exists(), "9 天前的该删");
        assert!(dir.join(format!("jw-{today}.log")).exists(), "今天的该留");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
