//! 实时层：**让系统自己知道项目变了**。
//!
//! ## 为什么先做"轮询版"，而不是监听系统事件
//!
//! Windows 上当然可以用 `ReadDirectoryChangesW`，但它有几个**静默失效**的场景：
//! 网络盘、WSL 路径、某些同步网盘——**事件根本不触发，而且不报错**。
//! 那种失败最难查：界面上什么都不动，用户只会以为"没变化"。
//!
//! 所以这一版**先用最笨也最可靠的办法**：定时比对文件的 `mtime`。
//! 代价是"最多晚一个轮询间隔"；换来的是**它在哪儿都工作，而且失败看得见**
//! （`poll` 返回空 = 真的没变，不是"我们不知道"）。
//!
//! 原生事件将来可以作为**加速层**加进来（有事件就立刻扫一次），
//! 但**不能取代这一层**——它是兜底，而且必须是可靠的那一层。
//!
//! ## 三件事一起做，缺一个都会出问题
//!
//! | 做什么 | 不做的后果 |
//! |---|---|
//! | **轮询兜底** | 网络盘上永远不更新，且静默 |
//! | **debounce**（合并同一批改动）| `git checkout` 一个分支会产生几千个事件，逐个排队会把写队列压死 |
//! | **背压**（忙就明说）| 队列堆着不说，用户以为它在跟，其实已经落后几百个文件 |

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 上一次扫到的样子：路径 → 修改时间（纳秒）。
type Stamps = BTreeMap<String, i64>;

/// 一次 poll 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    /// 新增或改过的文件（相对路径）。
    pub changed: Vec<String>,
    /// 已经不在磁盘上的文件。
    pub removed: Vec<String>,
    /// 这次扫了多少个 `.rs`（用来判断"是不是真的在干活"）。
    pub scanned: usize,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }
}

/// 盯住一个目录。**只记状态，不做索引**——索引是 `index` 的事。
pub struct Watcher {
    root: PathBuf,
    /// 上次扫到的样子
    last: Stamps,
    /// 上一次真的去扫磁盘是什么时候（配合 `min_gap` 做 debounce）
    last_poll: Option<Instant>,
    /// 两次实际扫描之间的最小间隔：`git checkout` 那种场景下，
    /// 事件会连续来好几秒，每次都扫一遍纯属浪费。
    min_gap: Duration,
}

impl Watcher {
    /// 建一个监视器，并立刻记下当前状态作为基线。
    ///
    /// **基线必须现在就取**：不然后面第一次 `poll` 会把"本来就存在的所有文件"
    /// 当成"刚变的"，于是打开一个项目就触发一次全量。
    pub fn new(root: &Path, min_gap: Duration) -> Watcher {
        Watcher {
            root: root.to_path_buf(),
            last: scan_stamps(root).0,
            last_poll: None,
            min_gap,
        }
    }

    /// 距上次实际扫描够不够久。**不够就返回 `None`**——
    /// 调用方据此跳过这一拍，而不是傻等（等待会把 Tauri 的定时线程堵住）。
    pub fn poll(&mut self) -> Option<ChangeSet> {
        if let Some(t) = self.last_poll {
            if t.elapsed() < self.min_gap {
                return None;
            }
        }
        self.last_poll = Some(Instant::now());

        let (now, scanned) = scan_stamps(&self.root);
        let mut changed = Vec::new();
        let mut removed = Vec::new();

        for (path, mt) in &now {
            match self.last.get(path) {
                Some(old) if old == mt => {}
                _ => changed.push(path.clone()),
            }
        }
        for path in self.last.keys() {
            if !now.contains_key(path) {
                removed.push(path.clone());
            }
        }
        self.last = now;

        Some(ChangeSet {
            changed,
            removed,
            scanned,
        })
    }

    /// 强制重取基线（比如用户手动点了"重新扫描"）。
    pub fn resync(&mut self) {
        self.last = scan_stamps(&self.root).0;
        self.last_poll = Some(Instant::now());
    }
}

/// 扫一遍目录，记下每个 `.rs` 的修改时间。
///
/// 与 `index::collect_rust_files` 共用同一道跳过闸（`scan::should_skip`）——
/// **同一道检查只写一次**，不然"索引扫的"和"看着变化的"会慢慢不是一批文件。
fn scan_stamps(root: &Path) -> (Stamps, usize) {
    let mut out = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 12 {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                if crate::util::scan::should_skip(&p, root).is_some() {
                    continue;
                }
                // 判法只有一处（`util::scan::is_not_source_dir`，内部会转小写）——
                // 原来这里直接比 `name.as_str()`，于是 Windows 上 `Target/` 漏过去了。
                if crate::util::scan::is_not_source_dir(&name) {
                    continue;
                }
                stack.push((p, depth + 1));
            } else if name.ends_with(".rs") {
                let Ok(rel) = p.strip_prefix(root) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if !seen.insert(rel.clone()) {
                    continue;
                }
                let mt = e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as i64)
                    .unwrap_or(0);
                out.insert(rel, mt);
            }
        }
    }
    let n = out.len();
    (out, n)
}

#[cfg(test)]
mod tests {
    // 内层属性必须在最前面（放在 use 之后编译器不认）。
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-watch-{tag}-{}", std::process::id()));
        if d.exists() {
            let _ = std::fs::remove_dir_all(&d);
        }
        std::fs::create_dir_all(&d).expect("建临目录失败");
        d
    }

    /// **基线不能少**：建监视器的那一刻必须记下现状，
    /// 否则第一次 poll 会把"本来就有的文件"全当成刚变的，一开项目就全量。
    #[test]
    fn 建好之后第一次轮询什么都不该报() {
        let d = tmp("baseline");
        std::fs::write(d.join("a.rs"), "fn a() {}").unwrap();
        let mut w = Watcher::new(&d, Duration::ZERO);
        let cs = w.poll().expect("第一次该能扫");
        assert!(cs.is_empty(), "基线刚取过，不该报变化：{cs:?}");
        assert_eq!(cs.scanned, 1);
    }

    #[test]
    fn 改一个文件只报那一个() {
        let d = tmp("one");
        std::fs::write(d.join("a.rs"), "fn a() {}").unwrap();
        std::fs::write(d.join("b.rs"), "fn b() {}").unwrap();
        let mut w = Watcher::new(&d, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(d.join("b.rs"), "fn b() { let _ = 1; }").unwrap();
        let cs = w.poll().expect("该能扫");
        assert_eq!(cs.changed, vec!["b.rs".to_string()], "{cs:?}");
    }

    #[test]
    fn 删掉的文件进_removed() {
        let d = tmp("removed");
        std::fs::write(d.join("a.rs"), "fn a() {}").unwrap();
        std::fs::write(d.join("gone.rs"), "fn g() {}").unwrap();
        let mut w = Watcher::new(&d, Duration::ZERO);
        std::fs::remove_file(d.join("gone.rs")).unwrap();
        let cs = w.poll().expect("该能扫");
        assert_eq!(cs.removed, vec!["gone.rs".to_string()], "{cs:?}");
        assert_eq!(cs.scanned, 1, "剩下一个");
    }

    /// **`min_gap` 必须真的挡住第二次**——这就是 debounce：
    /// `git checkout` 期间事件连续来好几秒，每一拍都扫磁盘纯属浪费。
    #[test]
    fn 间隔不够时返回_none_而不是空集() {
        let d = tmp("gap");
        std::fs::write(d.join("a.rs"), "fn a() {}").unwrap();
        let mut w = Watcher::new(&d, Duration::from_secs(60));
        assert!(w.poll().is_some(), "第一次该扫");
        assert!(
            w.poll().is_none(),
            "间隔不够时返回 None——调用方据此跳过这一拍，而不是把'没扫'当成'没变'"
        );
    }

    /// 目标目录与 `node_modules` 里的 `.rs` 不该被盯上。
    #[test]
    fn 目标目录里的_rs_不算项目文件() {
        let d = tmp("skiptarget");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("target/debug")).unwrap();
        std::fs::write(d.join("src/a.rs"), "fn a() {}").unwrap();
        std::fs::write(d.join("target/debug/gen.rs"), "fn g() {}").unwrap();
        let mut w = Watcher::new(&d, Duration::ZERO);
        let cs = w.poll().expect("该能扫");
        assert_eq!(cs.scanned, 1, "只该看见 src/a.rs");
        assert!(cs.is_empty());
    }
}
