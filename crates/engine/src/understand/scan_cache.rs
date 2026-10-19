//! **扫盘结果的持久化**（2026-10-19）：下次打开界面不必从头扫。
//!
//! 用户的原话是"结果持久化（下次不必从头）"。做法刻意朴素：
//! - 一份 JSON，存在 `%LOCALAPPDATA%\jingwei\scan-cache.json`（没有就退到临时目录）；
//! - **原子写**（先写 `.tmp` 再 rename）——写到一半崩掉，宁可没有缓存也不能有半份；
//! - 读坏了就当没有（返回 `None`），**绝不拿坏数据冒充上次结果**；
//! - 缓存里带 `saved_at`（秒级时间戳）与 `roots`/`depth`，
//!   界面要能说清"这是什么时候、按什么参数扫的"。

use std::path::{Path, PathBuf};

/// 一次扫描的落盘结果。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ScanCache {
    /// 秒级时间戳（UTC）
    pub saved_at: u64,
    /// 扫过哪些根
    pub roots: Vec<String>,
    /// 深度
    pub depth: usize,
    /// 看过的目录数
    pub visited: u64,
    /// 跳过的目录数
    pub skipped: u64,
    /// 发现的项目（每项是前端直接能用的 JSON）
    pub found: Vec<serde_json::Value>,
}

impl ScanCache {
    /// 缓存是否"看起来可用"：有时间戳、有结果。
    pub fn usable(&self) -> bool {
        self.saved_at > 0 && !self.found.is_empty()
    }

    /// 有几秒了（拿"现在"当参考，调用方给）。
    pub fn age_secs(&self, now: u64) -> u64 {
        now.saturating_sub(self.saved_at)
    }
}

/// 缓存文件路径（不保证目录存在）。
pub fn cache_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("jingwei").join("scan-cache.json")
}

/// 读缓存。**任何异常都当"没有"**（文件不存在 / JSON 坏 / 读不出来）。
pub fn load(path: &Path) -> Option<ScanCache> {
    let text = std::fs::read_to_string(path).ok()?;
    let c: ScanCache = serde_json::from_str(&text).ok()?;
    if c.usable() {
        Some(c)
    } else {
        None
    }
}

/// 原子写缓存：先写 `.tmp` 再 rename。
///
/// 为什么必须原子：写到一半崩掉留下半份 JSON，下次读出来是坏的——
/// 那时候"有缓存"比"没缓存"更糟（会显示半份项目列表）。
pub fn save(path: &Path, cache: &ScanCache) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(cache).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // 临时目录清理失败不影响断言，也不能改变结论
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-cache-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建目录失败");
        d.join("scan-cache.json")
    }

    /// 存了能读回来，字段不丢。
    #[test]
    fn 存进去能读回来() {
        let p = scratch("roundtrip");
        let c = ScanCache {
            saved_at: 100,
            roots: vec!["D:\\projects".into()],
            depth: 4,
            visited: 10,
            skipped: 2,
            found: vec![serde_json::json!({"path": "D:\\projects\\a"})],
        };
        save(&p, &c).expect("写失败");
        let got = load(&p).expect("读失败");
        assert_eq!(got.visited, 10);
        assert_eq!(got.found.len(), 1);
        assert_eq!(got.age_secs(160), 60, "age 要按秒算");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// **坏文件当没有**：不许拿半份数据冒充上次结果。
    #[test]
    fn 坏缓存当没有() {
        let p = scratch("broken");
        std::fs::write(&p, "{ 这不是 json").expect("写失败");
        assert!(load(&p).is_none(), "坏 JSON 必须当没有");
        std::fs::write(&p, "").expect("写失败");
        assert!(load(&p).is_none(), "空文件必须当没有");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 空结果不算可用（否则界面会显示"上次扫到 0 个项目"）。
    #[test]
    fn 空结果不算可用() {
        let c = ScanCache {
            saved_at: 1,
            found: vec![],
            ..Default::default()
        };
        assert!(!c.usable());
    }

    /// 原子写：写完之后临时文件不该留下。
    #[test]
    fn 写完不留临时文件() {
        let p = scratch("atomic");
        let c = ScanCache {
            saved_at: 1,
            found: vec![serde_json::json!({"path": "x"})],
            ..Default::default()
        };
        save(&p, &c).expect("写失败");
        assert!(p.exists(), "目标文件要在");
        assert!(!p.with_extension("json.tmp").exists(), "临时文件要清掉");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
