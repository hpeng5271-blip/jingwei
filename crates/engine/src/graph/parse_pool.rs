//! **并行准备阶段**（2026-10-20，债 0.31）：把"读文件 + 解码 + 内容哈希 + 解析 + 结果指纹"
//! 从 `index.rs` 的主循环里抽出来，按文件并行做；**落库仍在主线程按原顺序串行**。
//!
//! 为什么这样安全（pro 的学习 #6）：
//! - 这些步骤对每个文件都是**纯函数**（不碰 store），并行不改变每个文件的解析结果；
//! - "同名/遮蔽"的最终胜负只由**落库顺序**决定，与谁先解析完无关——所以结果按下标回收、
//!   主线程按原顺序写，语义与串行完全一致。
//!
//! 为什么拆成单独文件：`index.rs` 贴着 1000 行上限（门禁守）。
//!
//! **没做的那半**：冷盘上并行读可能更慢（随机读）。这里先不猜"盘是 HDD 还是 SSD"，
//! 用 `JINGWEI_PARSE_THREADS` 让人能手动把线程数压到 1；默认按物理核数封顶 8。

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::graph::parse::ParseOutcome;
use crate::graph::store::FileState;

/// 为什么这个文件没进"待写"队列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// 大小与 mtime 都没动（一级截断）
    Unchanged,
    /// 超过单文件上限
    Big,
    /// 读不出来 / 不是可读文本
    Unreadable,
    /// 内容哈希没变（`touch` 骗不过二级截断）
    SameContent,
}

pub struct Prepared {
    pub rel: String,
    pub abs: PathBuf,
    pub skip: Option<Skip>,
    pub parsed: Option<ParseOutcome>,
    pub result_sha: String,
    /// 落库要写进 `FileState` 的三样（与解析结果一起带回来，省得再 stat/再读一遍）。
    pub size: i64,
    pub mtime_ns: i64,
    pub content_sha: String,
}

/// 并行准备：返回与输入**同序**的结果。
pub fn prepare(
    files: Vec<(String, PathBuf)>,
    known: &BTreeMap<String, FileState>,
    version_changed: bool,
    now: i64,
) -> Vec<Prepared> {
    let n = files.len();
    if n == 0 {
        return Vec::new();
    }
    let threads = threads_wanted(n);
    let chunk = n.div_ceil(threads);
    let mut out: Vec<Option<Prepared>> = (0..n).map(|_| None).collect();

    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for start in (0..n).step_by(chunk) {
            let end = (start + chunk).min(n);
            let part = &files[start..end];
            handles.push((start, s.spawn(move || {
                let mut local = Vec::with_capacity(end - start);
                for (rel, abs) in part {
                    local.push(one(rel, abs, known, version_changed, now));
                }
                local
            })));
        }
        for (start, h) in handles {
            for (k, p) in h.join().unwrap().into_iter().enumerate() {
                out[start + k] = Some(p);
            }
        }
    });

    out.into_iter().flatten().collect()
}

fn threads_wanted(n: usize) -> usize {
    let env = std::env::var("JINGWEI_PARSE_THREADS").ok().and_then(|v| v.parse::<usize>().ok());
    let hw = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(4);
    env.unwrap_or(hw).clamp(1, 8).min(n.max(1))
}

fn one(
    rel: &str,
    abs: &PathBuf,
    known: &BTreeMap<String, FileState>,
    version_changed: bool,
    now: i64,
) -> Prepared {
    let skip = |s: Skip| Prepared {
        rel: rel.to_string(),
        abs: abs.clone(),
        skip: Some(s),
        parsed: None,
        result_sha: String::new(),
        size: 0,
        mtime_ns: 0,
        content_sha: String::new(),
    };

    let Ok(meta) = std::fs::metadata(abs) else {
        return skip(Skip::Unreadable);
    };
    let size = meta.len() as i64;
    let mt = mtime_ns(&meta);
    let k = known.get(rel);
    if let Some(k) = k {
        if k.size == size && k.mtime_ns == mt && !version_changed {
            return skip(Skip::Unchanged);
        }
    }
    if size as u64 > crate::graph::index::MAX_FILE_BYTES {
        return skip(Skip::Big);
    }
    let Ok(bytes) = std::fs::read(abs) else {
        return skip(Skip::Unreadable);
    };
    let file = crate::util::textfile::decode(&bytes);
    if !file.readable {
        return skip(Skip::Unreadable);
    }
    let sha = crate::facts::stable_hash([&file.text]);
    if let Some(k) = k {
        if k.content_sha.as_deref() == Some(sha.as_str()) && !version_changed {
            return skip(Skip::SameContent);
        }
    }

    // **按扩展名分派**（与原来一致：两门语言两个解析器）。
    let parsed = if rel.ends_with(".py") {
        crate::graph::parse_python::parse_python(rel, &file.text, now)
    } else {
        crate::parse::parse_rust(rel, &file.text, now)
    };
    let result_sha = crate::graph::result_sha::parsed_result_sha(&parsed);
    Prepared {
        rel: rel.to_string(),
        abs: abs.clone(),
        skip: None,
        parsed: Some(parsed),
        result_sha,
        size,
        mtime_ns: mt,
        content_sha: sha,
    }
}

fn mtime_ns(m: &std::fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}
