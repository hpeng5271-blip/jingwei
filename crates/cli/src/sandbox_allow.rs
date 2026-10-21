// 沙箱的「动作声明 + 允许表」（2026-10-21 第二轮）。
//
// 设计口径（人定的）：
// 1. 每个会碰外部世界的动作先**声明**它要碰什么（写哪些路径 / 起什么进程 / 访问哪些网络目标）；
// 2. runner 按声明核对——**没声明的东西不许碰**；越界的写直接拒；
// 3. 允许只给两种时长：**本次会话**（进程结束即失效）与 **24 小时**（到点自动失效）；
//    不给"永久关闭"——那等于把沙箱废掉。
//
// 这一轮只做骨架：声明结构 + 路径核对 + 允许表（含过期）。真正接进命令的目前是 `rename --write`。

// 允许表这一轮只落了数据结构与过期语义（测试已覆盖）；网络/执行前的核对下一轮接上。
// 在那之前它只被测试用到——这不是"忘了删"，是"等一下就用"。
#![allow(dead_code)]

use crate::sandbox::ensure_within;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 一次动作的声明：**它会碰什么**。没写进来的东西，runner 不允许碰。
#[derive(Debug, Clone)]
pub struct Action {
    /// 人读的名字（会进日志/界面）：例如 `rename --write`。
    pub name: String,
    /// 打算写的路径（全部要过 `ensure_within`）。
    pub writes: Vec<PathBuf>,
}

impl Action {
    pub fn declare(name: &str, writes: Vec<PathBuf>) -> Self {
        Self {
            name: name.to_string(),
            writes,
        }
    }

    /// 核对声明：每条写路径都必须落在项目内、且不碰 `.git`。
    ///
    /// 返回"将碰的东西"摘要——**给日志和界面看的**，不是给程序判断的：
    /// 用户有权知道这次动作打算动哪些文件（"便捷不等于隐身"）。
    pub fn check(&self, root: &Path) -> Result<Vec<String>, String> {
        let mut touched = Vec::new();
        for w in &self.writes {
            let p = ensure_within(root, w)?;
            touched.push(format!("写 {}", p.display()));
        }
        Ok(touched)
    }
}

/// 允许表：本次会话（内存）+ 24 小时（落盘）。
fn session_allows() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 允许文件：`%LOCALAPPDATA%\jingwei\allow.json`；测试用 `JINGWEI_ALLOW_FILE` 指定。
pub fn allow_path() -> PathBuf {
    if let Some(p) = std::env::var_os("JINGWEI_ALLOW_FILE") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("jingwei").join("allow.json")
}

/// 「本次会话允许」：只在当前进程内有效，进程结束自动失效（不落盘）。
pub fn allow_session(what: &str) {
    if let Ok(mut s) = session_allows().lock() {
        s.insert(what.to_string());
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 读允许表（坏文件当没有——和缓存同一条规矩）。
pub fn read_allows(path: &Path) -> Vec<(String, i64)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v.as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some((
                        r["what"].as_str()?.to_string(),
                        r["until"].as_i64().unwrap_or(0),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 落一条 24 小时允许；顺带清掉过期项（文件不会无限长）。
pub fn allow_24h(path: &Path, what: &str, now: i64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut rows: Vec<Value> = read_allows(path)
        .into_iter()
        .filter(|(_, until)| *until > now)
        .map(|(w, u)| json!({"what": w, "until": u}))
        .collect();
    rows.push(json!({"what": what, "until": now + 86_400}));
    std::fs::write(path, serde_json::to_string_pretty(&rows).unwrap_or_default())
}

/// 这个动作现在允许吗？本次会话 或 24 小时未过期，二者之一即允许。
pub fn allowed_at(path: &Path, what: &str, now: i64) -> bool {
    if let Ok(s) = session_allows().lock() {
        if s.contains(what) {
            return true;
        }
    }
    read_allows(path).into_iter().any(|(w, until)| w == what && until > now)
}

/// 用真实时钟判断（命令里用这个）。
pub fn allowed(what: &str) -> bool {
    allowed_at(&allow_path(), what, now_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-allow-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 本次会话允许：进程内有效；不同键互不影响。
    #[test]
    fn 本次会话允许只对同一个键生效() {
        allow_session("net:crates.io");
        let p = tmp("session");
        let file = p.join("allow.json");
        assert!(allowed_at(&file, "net:crates.io", 100));
        assert!(!allowed_at(&file, "net:pypi.org", 100), "别的键不该被顺带允许");
        std::fs::remove_dir_all(&p).ok();
    }

    /// 24 小时允许：到点自动失效（不做"永久"）。
    #[test]
    fn 二十四小时允许到点失效() {
        let p = tmp("ttl");
        let file = p.join("allow.json");
        allow_24h(&file, "net:pypi.org", 1_000).expect("写允许表");
        assert!(allowed_at(&file, "net:pypi.org", 1_000 + 3_600), "一小时后仍允许");
        assert!(
            !allowed_at(&file, "net:pypi.org", 1_000 + 86_401),
            "超过 24 小时必须失效"
        );
        std::fs::remove_dir_all(&p).ok();
    }

    /// 写允许表时顺手清过期项：文件不该无限长。
    #[test]
    fn 允许表会清掉过期项() {
        let p = tmp("gc");
        let file = p.join("allow.json");
        allow_24h(&file, "old", 0).expect("写旧项");
        allow_24h(&file, "new", 100_000).expect("写新项（此时旧项已过期）");
        let rows = read_allows(&file);
        assert!(
            rows.iter().all(|(w, _)| w != "old"),
            "过期项该被清掉：{rows:?}"
        );
        std::fs::remove_dir_all(&p).ok();
    }

    /// 动作声明：写到项目外 → 拒绝（连"打算"都不许）。
    #[test]
    fn 声明里项目外的写被拒() {
        let p = tmp("action");
        let outside = std::env::temp_dir().join("jw-outside-action.txt");
        let a = Action::declare("rename --write", vec![outside]);
        let e = a.check(&p).unwrap_err();
        assert!(e.contains("项目外面"), "要说明越界：{e}");
        std::fs::remove_dir_all(&p).ok();
    }

    /// 动作声明：项目内的写 → 通过，并给出"将碰的东西"摘要。
    #[test]
    fn 声明里项目内的写放行并给摘要() {
        let p = tmp("action-ok");
        let a = Action::declare("rename --write", vec![p.join("src/a.rs")]);
        let touched = a.check(&p).expect("项目内该放行");
        assert_eq!(touched.len(), 1);
        assert!(touched[0].contains("src"), "摘要要能看出写哪：{touched:?}");
        std::fs::remove_dir_all(&p).ok();
    }
}
