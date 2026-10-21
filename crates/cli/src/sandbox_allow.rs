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

// ===== 网络白名单（2026-10-21 第二轮）=====
//
// 口径（人定的）：网络默认拒绝，**只放行包仓库**；允许只给本次会话或 24 小时，可随时收回。
// 这一层只管"这个域名现在许不许"，不管怎么执行——真正的拒绝点由调用方给。

/// 默认放行的包仓库（装依赖要用的正经仓库）。
pub const REGISTRY_HOSTS: &[&str] = &[
    "pypi.org",
    "files.pythonhosted.org",
    "registry.npmjs.org",
    "registry.yarnpkg.com",
    "crates.io",
    "static.crates.io",
    "index.crates.io",
    "repo.maven.apache.org",
    "repo1.maven.org",
];

/// 从 URL 里取主机名：`https://pypi.org/simple` → `pypi.org`；认不出返回 `None`。
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next()?.trim();
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    let host = host.split(':').next().unwrap_or(host); // 去掉端口
    let host = host.trim_start_matches("www.");
    // 最后一段必须是字母 TLD（≥2 位）——否则 `3.12` 这种版本号会被当成域名。
    let tld = host.rsplit('.').next().unwrap_or("");
    if host.is_empty() || tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// 一条命令里出现的网络目标主机（`--index-url X` / `--registry X` / 裸 URL）。
///
/// 只认"看得出是 URL"的片段：**认不出就不算**（不猜），免得把版本号当域名。
pub fn hosts_in_command(cmd: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in cmd.split_whitespace() {
        let candidate = tok.trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == ';');
        if !(candidate.contains("://") || candidate.starts_with("--")) {
            continue;
        }
        let value = match candidate.split_once('=') {
            Some((_, v)) => v,
            None => candidate,
        };
        if let Some(h) = host_of(value) {
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out
}

/// 这个主机现在允许吗：**白名单** 或 允许表里的 `net:<host>`（本次会话 / 24 小时）。
pub fn net_allowed_at(path: &Path, host: &str, now: i64) -> bool {
    let h = host.to_ascii_lowercase();
    if REGISTRY_HOSTS.iter().any(|w| h == *w || h.ends_with(&format!(".{w}"))) {
        return true;
    }
    allowed_at(path, &format!("net:{h}"), now)
}

/// 命令过网络闸门：返回第一个被拒的主机（`Err(host)`），全放行返回 `Ok(Vec<host>)`。
pub fn net_gate(path: &Path, cmd: &str, now: i64) -> Result<Vec<String>, String> {
    let hosts = hosts_in_command(cmd);
    for h in &hosts {
        if !net_allowed_at(path, h, now) {
            return Err(h.clone());
        }
    }
    Ok(hosts)
}

#[cfg(test)]
mod net_tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-net-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 取主机：带协议/端口/路径都要能剥掉；认不出（没有点）就别猜。
    #[test]
    fn 取主机名() {
        assert_eq!(host_of("https://pypi.org/simple").as_deref(), Some("pypi.org"));
        assert_eq!(host_of("https://registry.npmjs.org:443/x").as_deref(), Some("registry.npmjs.org"));
        assert_eq!(host_of("https://www.crates.io"), host_of("https://crates.io"));
        assert_eq!(host_of("3.12"), None, "版本号不是域名");
        assert_eq!(host_of("localhost"), None, "没有点就不当域名");
    }

    /// 命令里认得出 `--index-url` / `--registry` / 裸 URL，重复只留一个。
    #[test]
    fn 从命令里取主机() {
        let hs = hosts_in_command("pip install -i https://pypi.org/simple --extra-index-url=https://evil.example/x pypi.org");
        assert!(hs.contains(&"pypi.org".to_string()), "{hs:?}");
        assert!(hs.contains(&"evil.example".to_string()), "{hs:?}");
        assert_eq!(hs.iter().filter(|h| *h == "pypi.org").count(), 1, "重复只留一个");
    }

    /// 白名单直接放行；名单外的域名默认拒绝。
    #[test]
    fn 白名单放行_名单外拒绝() {
        let d = tmp("wl");
        let f = d.join("allow.json");
        assert!(net_allowed_at(&f, "pypi.org", 100), "白名单该放行");
        assert!(net_allowed_at(&f, "static.crates.io", 100), "白名单该放行");
        assert!(!net_allowed_at(&f, "evil.example", 100), "名单外该拒绝");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 24 小时允许能放行名单外的域名，**到点失效**。
    #[test]
    fn 名单外可临时允许_到点失效() {
        let d = tmp("allow");
        let f = d.join("allow.json");
        allow_24h(&f, "net:evil.example", 1_000).expect("写允许");
        assert!(net_allowed_at(&f, "evil.example", 1_000 + 60), "允许期内该放行");
        assert!(!net_allowed_at(&f, "evil.example", 1_000 + 86_401), "过了 24 小时必须再拒");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 闸门：全放行返回主机清单；遇到名单外的就返回那一个（**先拒后跑**）。
    #[test]
    fn 闸门先拒后跑() {
        let d = tmp("gate");
        let f = d.join("allow.json");
        let ok = net_gate(&f, "pip install -i https://pypi.org/simple .", 10).expect("白名单该放行");
        assert_eq!(ok, vec!["pypi.org".to_string()]);
        let bad = net_gate(&f, "npm i --registry https://evil.example/x", 10).unwrap_err();
        assert_eq!(bad, "evil.example");
        std::fs::remove_dir_all(&d).ok();
    }
}
