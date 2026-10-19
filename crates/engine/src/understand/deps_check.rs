//! **依赖对账**（2026-10-19）：`Cargo.toml` 声明的依赖，`Cargo.lock` 里到底锁到了什么。
//!
//! 为什么这条算"独立检验"：两边都是**外部真值**（cargo 自己的文件格式），
//! 与我们解析源码的逻辑零共享——不是拿我们自己抽出来的东西自证。
//!
//! 三态（不许压扁）：
//! - `ok`：声明了、锁里有、版本要求满足；
//! - `missing`：声明了、锁里**没有**（真问题：清单和锁不一致）；
//! - `unverifiable`：版本要求我们判不了（`*`、git、path、区间、带 feature 的复杂写法）——
//!   **判不了就说判不了**，不当成通过。
//!
//! 刻意**不做**的事：`Cargo.lock` 里多出来的包（传递依赖）**不算问题**——那是正常的。

use std::path::Path;

/// 一条依赖的核对结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum DepState {
    Ok,
    Missing,
    Unverifiable,
}

impl DepState {
    pub fn as_str(&self) -> &'static str {
        match self {
            DepState::Ok => "ok",
            DepState::Missing => "missing",
            DepState::Unverifiable => "unverifiable",
        }
    }
}

/// 一条依赖 + 它的核对结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DepRow {
    pub name: String,
    pub declared: String,
    pub locked: Option<String>,
    pub state: DepState,
}

/// 一次对账的汇总。
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct DepReport {
    pub rows: Vec<DepRow>,
    pub ok: usize,
    pub missing: usize,
    pub unverifiable: usize,
}

impl DepReport {
    pub fn pass(&self) -> bool {
        self.missing == 0
    }
    pub fn checked(&self) -> usize {
        self.rows.len()
    }
}

/// 找 `Cargo.lock`：本目录没有就往上找（workspace 的锁在根上）。
fn find_lock(root: &Path) -> Option<std::path::PathBuf> {
    let mut dir = Some(root);
    for _ in 0..4 {
        let d = dir?;
        let p = d.join("Cargo.lock");
        if p.is_file() {
            return Some(p);
        }
        dir = d.parent();
    }
    None
}

/// 对 `root` 这个 crate 做一次声明 vs 锁的对账。
///
/// 读不到 `Cargo.toml` 或 `Cargo.lock` → 返回 `None`（**不是**"全部通过"）。
pub fn check(root: &Path) -> Option<DepReport> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    // **workspace 的锁在仓库根**：子 crate 自己没有 Cargo.lock，往上找（最多 3 层）
    let lock = std::fs::read_to_string(find_lock(root)?).ok()?;
    let declared = parse_manifest(&manifest);
    let locked = parse_lock(&lock);

    let mut rep = DepReport::default();
    for (name, req) in declared {
        let hit = locked.iter().find(|(n, _)| *n == name);
        let (state, locked_v) = match hit {
            None => (DepState::Missing, None),
            Some((_, v)) => match satisfies(&req, v) {
                Some(true) => (DepState::Ok, Some(v.clone())),
                Some(false) => (DepState::Missing, Some(v.clone())),
                None => (DepState::Unverifiable, Some(v.clone())),
            },
        };
        match state {
            DepState::Ok => rep.ok += 1,
            DepState::Missing => rep.missing += 1,
            DepState::Unverifiable => rep.unverifiable += 1,
        }
        rep.rows.push(DepRow {
            name,
            declared: req,
            locked: locked_v,
            state,
        });
    }
    Some(rep)
}

/// 从 `Cargo.toml` 里抄出 `[dependencies]` / `[dev-dependencies]` / `[build-dependencies]`
/// （以及带 `target.'cfg(...)'.dependencies` 的写法）的 `名字 → 版本要求`。
///
/// 这是个**刻意保守**的行解析：认不出的写法把版本要求记成 `?`（后面会判成不可验证）。
fn parse_manifest(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let sec = line.trim_matches(|c| c == '[' || c == ']').to_lowercase();
            in_deps = sec == "dependencies"
                || sec == "dev-dependencies"
                || sec == "build-dependencies"
                || sec.ends_with(".dependencies");
            continue;
        }
        if !in_deps {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"').to_string();
        let rest = rest.trim();
        // 名字为空 / 不是合法包名：跳过（保守）
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            continue;
        }
        let req = if let Some(v) = rest.strip_prefix('{') {
            // inline table：挑 version = "…"
            let inner = v.trim_end_matches('}');
            inner
                .split(',')
                .find_map(|kv| {
                    let (k, val) = kv.split_once('=')?;
                    if k.trim() == "version" {
                        Some(val.trim().trim_matches('"').to_string())
                    } else {
                        None
                    }
                })
                // 没写 version（path/git/workspace 依赖）→ 版本要求未知
                .unwrap_or_else(|| "?".to_string())
        } else {
            rest.trim_matches('"').to_string()
        };
        out.push((name, req));
    }
    out
}

/// 从 `Cargo.lock` 抄出 `(包名, 版本)`。
fn parse_lock(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (mut name, mut ver): (Option<String>, Option<String>) = (None, None);
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            if let (Some(n), Some(v)) = (name.take(), ver.take()) {
                out.push((n, v));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("name = ") {
            name = Some(rest.trim().trim_matches('"').to_string());
        } else if let Some(rest) = line.strip_prefix("version = ") {
            ver = Some(rest.trim().trim_matches('"').to_string());
        }
    }
    if let (Some(n), Some(v)) = (name, ver) {
        out.push((n, v));
    }
    out
}

/// 版本要求是否被锁到的版本满足。
/// - `Some(true/false)`：判得了
/// - `None`：**判不了**（`?`、`*`、区间、带 `,` 的多条件、git/path 之类）
fn satisfies(req: &str, locked: &str) -> Option<bool> {
    let req = req.trim();
    if req.is_empty() || req == "?" || req == "*" || req.contains(',') || req.contains(' ') {
        return None;
    }
    let (op, body) = if let Some(r) = req.strip_prefix('^') {
        ("caret", r)
    } else if let Some(r) = req.strip_prefix('~') {
        ("tilde", r)
    } else if let Some(r) = req.strip_prefix('=') {
        ("exact", r)
    } else if req.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        ("caret", req)
    } else {
        return None; // git / path / 分支名之类
    };
    let a = nums(body)?;
    let b = nums(locked)?;
    let same_prefix = match op {
        "exact" => a == b,
        "tilde" => a.0 == b.0 && a.1 == b.1,
        _ => {
            if a.0 > 0 {
                a.0 == b.0 && (b.1, b.2) >= (a.1, a.2)
            } else {
                // 0.x 语义：次版本号是"兼容边界"
                a.0 == b.0 && a.1 == b.1 && b.2 >= a.2
            }
        }
    };
    Some(same_prefix)
}

fn nums(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next().unwrap_or(v);
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let c = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    Some((a, b, c))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-deps-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建目录失败");
        d
    }

    /// **workspace 的锁在根上**：子 crate 自己没有 Cargo.lock，必须往上找。
    #[test]
    fn 子crate能用根上的锁() {
        let root = scratch("ws");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("Cargo.lock"), "[[package]]\nname = \"anyhow\"\nversion = \"1.0.0\"\n").unwrap();
        std::fs::write(root.join("sub/Cargo.toml"), "[dependencies]\nanyhow = \"1\"\n").unwrap();
        let rep = check(&root.join("sub")).expect("应当能往上找到锁");
        assert_eq!(rep.ok, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 解析清单里的三种写法() {
        let t = r#"
[package]
name = "x"

[dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }
local = { path = "../local" }

[dev-dependencies]
tempfile = "3.10"
"#;
        let d = parse_manifest(t);
        assert!(d.iter().any(|(n, v)| n == "anyhow" && v == "1"));
        assert!(d.iter().any(|(n, v)| n == "serde" && v == "1"));
        // path 依赖没有版本要求 → 记 "?"（判成不可验证，不许当通过）
        assert!(d.iter().any(|(n, v)| n == "local" && v == "?"));
        assert!(d.iter().any(|(n, _)| n == "tempfile"));
    }

    #[test]
    fn 三种状态都能出现() {
        let d = scratch("three");
        std::fs::write(
            d.join("Cargo.toml"),
            "[dependencies]\nanyhow = \"1\"\nghost = \"2\"\nweird = { path = \"./w\" }\n",
        )
        .unwrap();
        std::fs::write(
            d.join("Cargo.lock"),
            "[[package]]\nname = \"anyhow\"\nversion = \"1.0.86\"\n\n[[package]]\nname = \"weird\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let rep = check(&d).expect("应当能读");
        assert_eq!(rep.ok, 1, "anyhow 该对上");
        assert_eq!(rep.missing, 1, "ghost 锁里没有");
        assert_eq!(rep.unverifiable, 1, "path 依赖判不了");
        assert!(!rep.pass(), "有 missing 就不算通过");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 版本要求判据：caret / tilde / 0.x 的兼容边界。
    #[test]
    fn 版本要求怎么判() {
        assert_eq!(satisfies("1", "1.2.3"), Some(true));
        assert_eq!(satisfies("1.2", "1.9.0"), Some(true));
        assert_eq!(satisfies("2", "1.9.0"), Some(false));
        assert_eq!(satisfies("~1.2", "1.3.0"), Some(false));
        assert_eq!(satisfies("0.2", "0.3.0"), Some(false));
        assert_eq!(satisfies("0.2", "0.2.9"), Some(true));
        assert_eq!(satisfies("?", "1.0.0"), None, "判不了就要说判不了");
        assert_eq!(satisfies(">=1, <2", "1.5.0"), None);
    }
}
