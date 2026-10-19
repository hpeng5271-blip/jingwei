//! **指纹缓存**（2026-10-20，债 0.30）：`--json`/界面每次都要指纹，而算指纹要把项目里
//! 每个文件**读一遍**（422 个项目温缓存 3.3s，冷缓存更贵）。
//!
//! 做法：用**只看元数据**的"树签名"（相对路径 + 大小 + mtime）当键；
//! 签名没变就复用上次的指纹，变了才真读文件重算。
//!
//! **为什么这样安全**：写文件一定会改 mtime（或大小），签名就会变 → 重算。
//! 反过来"签名没变但内容变了"需要人为把 mtime 改回去，属于刻意伪造，不在我们的判据里。
//! **截断也要进签名**：截断与否会改变指纹含义，不能混用。
//!
//! **三态**：缓存文件坏了/读不了 → 当没有（重算），不报错、也不假装命中。

#![allow(clippy::let_underscore_must_use)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::fingerprint::Fingerprint;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct Entry {
    sig: String,
    fp: Fingerprint,
}

#[derive(Debug, Default)]
pub struct FpCache {
    path: PathBuf,
    map: BTreeMap<String, Entry>,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
}

impl FpCache {
    /// 打开（或新建）缓存。路径与 scan_cache 同一套约定：`%LOCALAPPDATA%\jingwei\fp-cache.json`。
    pub fn open() -> Self {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = base.join("jingwei").join("fp-cache.json");
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<BTreeMap<String, Entry>>(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            map,
            hits: Default::default(),
            misses: Default::default(),
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(std::sync::atomic::Ordering::Relaxed),
            self.misses.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// 只查（不计算）：签名一致就返回缓存的指纹。**短锁**，持锁期间不做 IO 重活。
    pub fn get(&self, root: &Path, sig: &str) -> Option<Fingerprint> {
        let key = root.display().to_string();
        let e = self.map.get(&key)?;
        if e.sig != sig {
            return None;
        }
        self.hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(e.fp.clone())
    }

    /// 记下"这个签名对应这个指纹"。**短锁**。
    pub fn put(&mut self, root: &Path, sig: String, fp: Fingerprint) {
        self.misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map.insert(root.display().to_string(), Entry { sig, fp });
    }

    /// 命中就返回指纹；没命中就真算，并记下来。
    pub fn get_or_compute(&mut self, root: &Path) -> Fingerprint {
        let sig = tree_sig(root);
        if let Some(fp) = self.get(root, &sig) {
            return fp;
        }
        let fp = super::fingerprint::of(root);
        self.put(root, sig, fp.clone());
        fp
    }

    /// 落盘。失败**不报错也不影响结果**（缓存是加速手段，不是事实）。
    pub fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string(&self.map) {
            let _ = std::fs::write(&self.path, s);
        }
    }
}

/// 只看元数据的"树签名"。与 `fingerprint::of` 用同一套过滤（跳过点目录与 vendored）。
pub fn tree_sig(root: &Path) -> String {
    const MAX: usize = 2000;
    let mut items: Vec<String> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut truncated = false;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if ft.is_dir() {
                if name.starts_with('.') || is_vendor(&name) {
                    continue;
                }
                stack.push(e.path());
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            if items.len() >= MAX {
                truncated = true;
                break;
            }
            let md = e.metadata().ok();
            let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = md
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let rel = e
                .path()
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or(name);
            items.push(format!("{rel}\u{1}{size}\u{1}{mtime}"));
        }
        if items.len() >= MAX {
            truncated = true;
            break;
        }
    }
    items.sort();
    let joined: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
    crate::facts::stable_hash(joined.iter().copied().chain(std::iter::once(if truncated {
        "truncated"
    } else {
        "complete"
    })))
}

fn is_vendor(name: &str) -> bool {
    let n = name.to_lowercase();
    matches!(
        n.as_str(),
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | "vendor"
            | "vendors"
            | ".git"
            | "__pycache__"
            | ".venv"
            | "venv"
            | "bower_components"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-fpc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 内容变了（大小/时间也变）→ 签名必须变；没变 → 签名不变。
    #[test]
    fn 树签名随内容变化() {
        let d = tmp("sig");
        std::fs::write(d.join("a.txt"), "one").unwrap();
        let s1 = tree_sig(&d);
        assert_eq!(s1, tree_sig(&d), "同一个目录两次签名必须一样");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(d.join("a.txt"), "two-longer").unwrap();
        let s2 = tree_sig(&d);
        assert_ne!(s1, s2, "内容变了签名必须变");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 命中路径：同签名复用；换签名重算。
    #[test]
    fn 缓存命中与失效() {
        let d = tmp("hit");
        std::fs::write(d.join("a.txt"), "one").unwrap();
        let mut c = FpCache {
            path: d.join("cache.json"),
            ..Default::default()
        };
        let fp1 = c.get_or_compute(&d);
        let sig = tree_sig(&d);
        assert_eq!(c.get(&d, &sig).map(|f| f.hash), Some(fp1.hash.clone()));
        assert_eq!(c.stats(), (1, 1), "一次算 + 一次命中");
        let _ = std::fs::remove_dir_all(&d);
    }
}
