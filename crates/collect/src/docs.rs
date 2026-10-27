use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Doc {
    pub path: String,
    pub title: String,
    pub excerpt: String,
    /// 文件 mtime（unix 秒）；0 = 取不到。判断的 at 用它，重跑才幂等。
    pub mtime: i64,
    /// 纯内容 hash（查同内容实体用）。
    pub content_hash: String,
    /// 内容身份：改名/移动后不变（FNV-1a）。
    pub entity: String,
}

const SKIP_DIRS: &[&str] = &["target", ".git", ".jingwei", "node_modules", "dist"];

/// 扫描仓库里的 Markdown 文档（最多 `limit` 个）。
pub fn scan(root: &Path, limit: usize) -> Vec<Doc> {
    let mut out = Vec::new();
    walk(root, root, limit, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, limit: usize, out: &mut Vec<Doc>) {
    if out.len() >= limit {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            walk(root, &p, limit, out);
            if out.len() >= limit {
                return;
            }
            continue;
        }
        let is_md = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md"))
            .unwrap_or(false);
        if !is_md {
            continue;
        }
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let rel = p
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.clone());
        out.push(Doc {
            path: rel.clone(),
            title: first_heading(&text).unwrap_or_else(|| name.clone()),
            excerpt: excerpt(&text),
            mtime: mtime_secs(&p),
            content_hash: crate::hash::content_id(text.as_bytes()),
            entity: crate::hash::entity_id(text.as_bytes(), &rel),
        });
        if out.len() >= limit {
            return;
        }
    }
}

fn mtime_secs(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn first_heading(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.trim_start().starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
}

fn excerpt(text: &str) -> String {
    let mut buf = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        buf.push_str(t);
        buf.push(' ');
        if buf.chars().count() > 120 {
            break;
        }
    }
    buf.chars().take(120).collect()
}

pub fn doc_path(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}
