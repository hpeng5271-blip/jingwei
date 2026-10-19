//! **目录级索引：把"采集"和"判定"拆开**（2026-10-18）。
//!
//! ## 为什么要拆（`框架.md` §4.9 有实测数字）
//!
//! 现在的 `projects::discover()` 是**一边走一边判**：`walk_from` 遇到 `looks_like_project`
//! 就 `push` 然后 `return`。这带来三个后果，**没有一个是"慢"**：
//!
//! 1. **规则一改就得重扫**——判定是遍历的一部分，不是数据的一部分。
//! 2. **"这个目录为什么被跳过"没有记录**——误判无法复现，只能"再扫一遍看看"。
//! 3. **遇项目就不再下钻**——monorepo 里那 5 个子项目一个都看不见。
//!
//! 实测（SSD，代理测量）：`D:\projects` 深度 4 一共 3,680 个目录、**287 ms**；
//! 只有选到 `AppData\Local` 这种巨型目录才到 2.5 秒。**所以"慢"不是主因。**
//!
//! ## 这一层的形状
//!
//! ```text
//! 建索引：遍历一次，每个**目录**一行（mtime / 有哪些标记文件 / 被谁跳过 / 为什么）
//! 判定：  = 对这张表的查询（`projects_under` / `top_level`）
//! ```
//!
//! **为什么是目录级、不是文件级**：判定只需要"这个目录里有没有 `Cargo.toml` / `.git`"，
//! 不需要知道每个文件。文件级索引贵一个数量级，**换不到任何新判据**。
//!
//! ## 这一刀故意不接线
//!
//! `projects::discover()` 一行没动。新东西要先用**回归测试**证明"同样输入给同样结果"
//! （`索引与_discover_对同一棵树给出同样的项目集`），再接上去——
//! **先有裁判，再换选手**（同 `取舍记录.md` T-07）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::util::scan;

/// 一个目录在索引里的一行。**只有事实，没有结论。**
#[derive(Debug, Clone, Default)]
pub struct DirRow {
    /// 目录本身的 mtime（秒）。增量靠它：没变就不重扫。
    pub mtime: u64,
    /// 这个目录里有哪些"项目标记文件"（`Cargo.toml` / `package.json` / …）。
    ///
    /// **空不等于"不是项目"**：`.git` 与 `env.contract.json` 也算标记，
    /// 但它们不是"清单文件"，所以分开存（见 `has_git` / `has_contract`）。
    pub markers: Vec<String>,
    pub has_git: bool,
    pub has_contract: bool,
    /// 如果这一行是"我们决定不进去"的目录，这里是**原因**。
    ///
    /// 这是"误判可复现"的落点：**跳过也要留一行**，并且说清是按哪条规则跳的。
    pub skipped: Option<&'static str>,
}

impl DirRow {
    /// 只回答"这个目录像不像一个项目"，**不再读盘**（这是和 `looks_like_project` 的区别：
    /// 那个每次都 `read_dir`，这个用建索引时记下的事实）。
    pub fn looks_like_project(&self) -> bool {
        !self.markers.is_empty() || self.has_git || self.has_contract
    }

    /// 给人看的一句话：为什么这一格是"不是项目"。
    pub fn why_not_project(&self) -> &'static str {
        if let Some(r) = self.skipped {
            return r;
        }
        if self.markers.is_empty() && !self.has_git && !self.has_contract {
            "这个目录里没有项目标记文件（清单 / .git / env.contract.json）"
        } else {
            "看起来像项目"
        }
    }
}

/// 一次建好的索引：路径 → 行。
///
/// 用 `BTreeMap`（不是 HashMap）：**输出顺序稳定**，测试与界面都不用再排一次。
#[derive(Debug, Default)]
pub struct DirsIndex {
    pub root: PathBuf,
    pub rows: BTreeMap<PathBuf, DirRow>,
    /// 遍历时真的进过的目录数（不含被跳过的）——用来在界面上说"这次碰了多少个目录"。
    pub visited: usize,
    /// 被跳过的目录数。
    pub skipped: usize,
}

/// 跳过规则**只有一处**：名字规则（`projects::skip_dir` 那三条）与
/// `util::scan::should_skip`（网络盘 / 联接点 / 超长路径）。
///
/// 返回 `Some(原因)` 表示**不进去**。原因会原样记进索引，供"为什么没扫到"回答。
pub fn skip_reason(dir: &Path, root: &Path) -> Option<&'static str> {
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.starts_with('.') {
        return Some("名字以 . 开头（约定为工具/配置目录）");
    }
    // **Windows 上名字不区分大小写**（学习调研的坑 2）：Node_Modules / TARGET 也要挡住。
    if crate::util::scan::is_not_source_dir(&name) {
        return Some("不是源码目录（node_modules / target / dist 之类）");
    }
    if is_system_dir(&name) {
        return Some("系统目录（Windows / AppData / Program Files 之类）");
    }
    scan::should_skip(dir, root)
}

/// **机器级的系统目录**：扫盘时进去既慢又没意义。
///
/// **这份是唯一真源**（2026-10-18 合并）：原来 `projects.rs` 里还有一份，
/// 而两份几乎一样——"两份清单各写各的，必然漂"是这个仓库最熟的事故形状。
pub const SYSTEM_DIRS: &[&str] = &[
    "appdata",
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "system volume information",
    "perflogs",
    "recovery",
];

fn is_system_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    SYSTEM_DIRS.contains(&lower.as_str())
}

fn mtime_secs(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 读一个目录的"事实"：有哪些标记文件、有没有 `.git` / `env.contract.json`。
///
/// **只读一层**（不递归）。读不出来（权限/坏盘）就返回空行——**空行不等于"不是项目"**，
/// 它等于"我们没读到"，而这一格在界面上必须显示成「查不了」。
fn facts_of(dir: &Path) -> (Vec<String>, bool, bool, bool) {
    let mut markers = Vec::new();
    let mut readable = false;
    if let Ok(rd) = std::fs::read_dir(dir) {
        readable = true;
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if crate::understand::project::manifest_kind(&name).is_some() {
                markers.push(name);
            }
        }
    }
    markers.sort();
    let has_git = dir.join(".git").is_dir();
    let has_contract = dir.join("env.contract.json").is_file();
    (markers, has_git, has_contract, readable)
}

/// **可增量、可中断的建索引**（2026-10-19，用户要"先出结果、后台扫到完"）。
///
/// 与 `build` 的区别只有两点，但都是"扫全盘"必须的：
///   ① 每进一个目录回调一次（界面据此**边扫边显示**，不用等整盘扫完）；
///   ② `stop` 被置位就提前返回（用户点"停"要立刻停，不能扫完才理人）。
///
/// 回调里给的是**累计数**：`(已进目录数, 已跳过数, 当前目录)`。
pub fn build_with(
    root: &Path,
    depth: usize,
    stop: &std::sync::atomic::AtomicBool,
    mut on_dir: impl FnMut(usize, usize, &Path),
) -> DirsIndex {
    let mut idx = DirsIndex {
        root: root.to_path_buf(),
        ..Default::default()
    };
    if !root.is_dir() {
        return idx;
    }
    walk_with(root, root, 0, depth.max(1), &mut idx, stop, &mut on_dir);
    idx
}

/// **并行扫**（2026-10-19）：整盘冷缓存时 146 秒太长，而瓶颈是**元数据 I/O**，
/// 不是 CPU——所以按目录并行是对的，不是"为了快而乱并发"。
///
/// 纪律（三条，缺一条就会出错）：
/// 1. **判据和串行版完全一样**：`skip_reason` / `row_for` 同一个函数，别在并发里另写一份；
/// 2. **结果与串行逐项相等**（有测试钉着：`并行扫与串行扫结果一致`）；
/// 3. `stop` 一置位就**尽快**收手（每处理完一个目录看一次）。
///
/// 回调的计数是"当前快照"，可能略滞后——进度条不需要精确到个位。
pub fn build_parallel(
    root: &Path,
    depth: usize,
    threads: usize,
    stop: &std::sync::atomic::AtomicBool,
    on_dir: impl FnMut(usize, usize, &Path) + Send,
) -> DirsIndex {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    let mut idx = DirsIndex {
        root: root.to_path_buf(),
        ..Default::default()
    };
    if !root.is_dir() {
        return idx;
    }
    let max = depth.max(1);
    let threads = threads.max(1);
    let queue: Mutex<Vec<(std::path::PathBuf, usize)>> = Mutex::new(vec![(root.to_path_buf(), 0)]);
    let pending = AtomicUsize::new(1);
    let visited = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let on_dir = Mutex::new(on_dir);

    // 每个线程**本地攒**，最后一次性合并：
    // 第一版每进一个目录就抢一次全局锁，实测比串行还慢（11s → 52s）——锁争用把收益吃光了。
    let collected: Mutex<Vec<(std::path::PathBuf, DirRow)>> = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                let mut local: Vec<(std::path::PathBuf, DirRow)> = Vec::with_capacity(256);
                loop {
                    if stop.load(Ordering::Relaxed) || pending.load(Ordering::Acquire) == 0 {
                        break;
                    }
                    // **成批取**：一批 16 个，等于把锁的频率降两个数量级
                    let batch: Vec<(std::path::PathBuf, usize)> = {
                        let mut q = queue.lock().unwrap();
                        let len = q.len();
                        let n = len.min(16);
                        q.drain(len - n..).collect()
                    };
                    if batch.is_empty() {
                        // 队列空但别人手上还有活：让出 CPU（自旋会把机器烧热）
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    for (dir, level) in batch {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let (row, readable) = row_for(&dir);
                        local.push((dir.clone(), row));
                        let v = visited.fetch_add(1, Ordering::Relaxed) + 1;
                        (on_dir.lock().unwrap())(v, skipped.load(Ordering::Relaxed), &dir);
                        if readable && level < max {
                            if let Ok(rd) = std::fs::read_dir(&dir) {
                                let mut children: Vec<(std::path::PathBuf, usize)> = Vec::new();
                                for e in rd.flatten() {
                                    if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                                        continue;
                                    }
                                    let child = e.path();
                                    if let Some(reason) = skip_reason(&child, root) {
                                        local.push((
                                            child.clone(),
                                            DirRow {
                                                mtime: mtime_secs(&child),
                                                skipped: Some(reason),
                                                ..Default::default()
                                            },
                                        ));
                                        skipped.fetch_add(1, Ordering::Relaxed);
                                        continue;
                                    }
                                    children.push((child, level + 1));
                                }
                                if !children.is_empty() {
                                    pending.fetch_add(children.len(), Ordering::Release);
                                    queue.lock().unwrap().extend(children);
                                }
                            }
                        }
                        pending.fetch_sub(1, Ordering::Release);
                    }
                }
                collected.lock().unwrap().extend(local);
            });
        }
    });

    for (path, row) in collected.into_inner().unwrap() {
        idx.rows.insert(path, row);
    }

    idx.visited = visited.load(Ordering::Relaxed);
    idx.skipped = skipped.load(Ordering::Relaxed);
    idx
}

fn walk_with(
    root: &Path,
    dir: &Path,
    level: usize,
    max: usize,
    idx: &mut DirsIndex,
    stop: &std::sync::atomic::AtomicBool,
    on_dir: &mut impl FnMut(usize, usize, &Path),
) {
    // **中断检查放在最前面**：用户点停之后，连"这一层剩下的目录"都不该再碰。
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let (row, readable) = row_for(dir);
    idx.rows.insert(dir.to_path_buf(), row);
    idx.visited += 1;
    on_dir(idx.visited, idx.skipped, dir);
    if !readable || level >= max {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let child = e.path();
        if let Some(reason) = skip_reason(&child, root) {
            idx.rows.insert(
                child.clone(),
                DirRow {
                    mtime: mtime_secs(&child),
                    skipped: Some(reason),
                    ..Default::default()
                },
            );
            idx.skipped += 1;
            continue;
        }
        walk_with(root, &child, level + 1, max, idx, stop, on_dir);
    }
}

/// 建索引：从 `root` 往下走 `depth` 层，**每个目录一行**，并且**不因为"像项目"就停**。
pub fn build(root: &Path, depth: usize) -> DirsIndex {
    let mut idx = DirsIndex {
        root: root.to_path_buf(),
        ..Default::default()
    };
    if !root.is_dir() {
        return idx;
    }
    walk(root, root, 0, depth.max(1), &mut idx);
    idx
}

/// 给一个目录建一行：**事实 + 读不出来的原因**。
///
/// 返回 `(行, 可读)`。为什么把"可读"单独返回：`walk` 要靠它决定还要不要往下走，
/// 而"行里带着原因"是给查询用的——**两件事，别混成一个字段**。
pub fn row_for(dir: &Path) -> (DirRow, bool) {
    let (markers, has_git, has_contract, readable) = facts_of(dir);
    let row = DirRow {
        mtime: mtime_secs(dir),
        markers,
        has_git,
        has_contract,
        // **读不出来的目录也留一行**，`skipped` 记成原因——这是三态里的"查不了"。
        skipped: if readable {
            None
        } else {
            Some("目录读不出来（权限或坏盘）")
        },
    };
    (row, readable)
}

fn walk(root: &Path, dir: &Path, level: usize, max: usize, idx: &mut DirsIndex) {
    let (row, readable) = row_for(dir);
    idx.rows.insert(dir.to_path_buf(), row);
    idx.visited += 1;
    if !readable || level >= max {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let child = e.path();
        if let Some(reason) = skip_reason(&child, root) {
            // **跳过也要留一行**：否则"为什么没扫到"永远答不出来。
            idx.rows.insert(
                child.clone(),
                DirRow {
                    mtime: mtime_secs(&child),
                    skipped: Some(reason),
                    ..Default::default()
                },
            );
            idx.skipped += 1;
            continue;
        }
        walk(root, &child, level + 1, max, idx);
    }
}

impl DirsIndex {
    /// **按原因统计被跳过的目录**（最多带 3 个样例路径）。
    ///
    /// 为什么要有它：用户问"为什么这个项目没扫到"时，最没用的回答是"没扫到"。
    /// 这个函数让答案变成"这一类目录被跳过了，例如 …"。
    pub fn skip_stats(&self) -> Vec<(String, usize, Vec<String>)> {
        let mut m: std::collections::BTreeMap<String, (usize, Vec<String>)> = Default::default();
        for (path, row) in &self.rows {
            if let Some(reason) = row.skipped {
                let e = m.entry(reason.to_string()).or_insert((0, Vec::new()));
                e.0 += 1;
                if e.1.len() < 3 {
                    e.1.push(path.display().to_string());
                }
            }
        }
        let mut out: Vec<(String, usize, Vec<String>)> =
            m.into_iter().map(|(k, (n, s))| (k, n, s)).collect();
        out.sort_by_key(|e| std::cmp::Reverse(e.1));
        out
    }

    /// **查询**：这一层下面所有"像项目"的目录（含嵌套的）。
    ///
    /// 与 `projects::discover` 的差别：那个遇到项目就停；这个**全都给**——
    /// monorepo 里那 5 个子项目正是这样才看得见。
    pub fn all_projects(&self) -> Vec<PathBuf> {
        self.rows
            .iter()
            .filter(|(p, r)| **p != self.root && r.looks_like_project())
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// **查询**：最外层的那些项目（遇项目就不再往里算）。
    ///
    /// 这是**今天 `discover` 的口径**，写成查询是为了：口径要换（比如改成"全都给"）
    /// 时**不用重建索引**。
    pub fn top_level_projects(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for (p, r) in &self.rows {
            if *p == self.root || !r.looks_like_project() {
                continue;
            }
            // 祖先里有项目 → 它不算"最外层"
            let covered = p.ancestors().skip(1).any(|a| {
                a != self.root.as_path() && self.rows.get(a).map(|r| r.looks_like_project()).unwrap_or(false)
            });
            if !covered {
                out.push(p.clone());
            }
        }
        out.sort();
        out
    }

    /// 这一行为什么没被算成项目（给界面/CLI 用）。
    pub fn why_not(&self, dir: &Path) -> Option<&'static str> {
        self.rows.get(dir).map(|r| r.why_not_project())
    }

    /// 有多少个目录是"我们跳过的"——**这个数必须能说出来**，
    /// 否则"没扫到"会被读成"没有"。
    pub fn skipped_count(&self) -> usize {
        self.skipped
    }
}


// ---------------------------------------------------------------------------
// 持久化：**第二次启动能复用多少**（2026-10-18）
//
// ## 先说清一件做不到的事
//
// 判据里写的是"第二次启动**不再全量遍历**"。只用 mtime 做不到：
// 在 `a/b/c` 里加一个文件，只会改 `a/b/c` 的 mtime——**祖先目录的 mtime 不变**。
// 所以"祖先没变 → 整棵子树都不用看"是**错的**，会漏掉深层变化。
// 真要跳过遍历，只有两条路：① 读 NTFS 的 USN 日志（要管理员）；② 常驻监听
// （`ReadDirectoryChangesW`，要一直跑）。两条都碰"绿色免安装、不提权"的边界，
// 所以这一版**老实只做能做的**：遍历照走，但**每个目录的"事实"能复用**
// （省掉一次 `read_dir` 去数标记文件），并且**判定与查询零 IO**。
//
// ## 复用判据
//
// 目录 mtime 与缓存里一致 → 直接采用缓存里的 markers / has_git / has_contract；
// 不一致 → 重新读这一层。**文件内容变了但目录项没变**的场合，我们的"事实"本来也不变
// （我们只关心有没有标记文件），所以按 mtime 复用是安全的。
/// 缓存复用统计（**要能报出来**，否则"复用了多少"只能靠感觉）。
#[derive(Debug, Default, Clone, Copy)]
pub struct Reuse {
    /// 复用缓存事实的目录数
    pub facts_reused: usize,
    /// 重新读过的目录数
    pub facts_read: usize,
}

fn store_path(root: &Path, depth: usize, store: Option<&Path>) -> PathBuf {
    let base = match store {
        Some(s) => s.to_path_buf(),
        None => std::env::var_os("LOCALAPPDATA")
            .map(|v| PathBuf::from(v).join("jingwei").join("dirs-index"))
            .unwrap_or_else(|| std::env::temp_dir().join("jingwei-dirs-index")),
    };
    // 文件名 = 根路径 + 层数的哈希（同一台机器上不同根不互相覆盖）
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{}|{}", root.display(), depth).as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    base.join(format!("{h:016x}.json"))
}

fn load_rows(path: &Path) -> Option<(BTreeMap<PathBuf, DirRow>, String)> {
    let txt = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&txt).ok()?;
    let root = v.get("root")?.as_str()?.to_string();
    let mut rows = BTreeMap::new();
    for r in v.get("rows")?.as_array()? {
        let path = PathBuf::from(r.get("path")?.as_str()?);
        let markers: Vec<String> = r
            .get("markers")
            .and_then(|m| m.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        rows.insert(
            path,
            DirRow {
                mtime: r.get("mtime").and_then(|x| x.as_u64()).unwrap_or(0),
                markers,
                has_git: r.get("has_git").and_then(|x| x.as_bool()).unwrap_or(false),
                has_contract: r.get("has_contract").and_then(|x| x.as_bool()).unwrap_or(false),
                skipped: None, // 跳过原因不持久化：它由当前规则算，规则改了本就该重判
            },
        );
    }
    Some((rows, root))
}

fn save_rows(path: &Path, idx: &DirsIndex) {
    if let Some(dir) = path.parent() {
        // 写不进缓存不算失败：索引是加速用的，坏了下次重建即可（但**不许**吞掉真正的错误）
        let _created = std::fs::create_dir_all(dir).is_ok();
    }
    let rows: Vec<serde_json::Value> = idx
        .rows
        .iter()
        .map(|(p, r)| {
            serde_json::json!({
                "path": p.to_string_lossy(),
                "mtime": r.mtime,
                "markers": r.markers,
                "has_git": r.has_git,
                "has_contract": r.has_contract,
            })
        })
        .collect();
    let v = serde_json::json!({
        "root": idx.root.to_string_lossy(),
        "saved_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "rows": rows,
    });
    // 同上：缓存写失败只影响"下次能不能复用"，不影响这次的结果
    std::fs::write(path, serde_json::to_string(&v).unwrap_or_default()).ok();
}

/// 带缓存的建索引：**遍历照走，但事实能复用**（见上面那段诚实边界）。
pub fn build_cached(root: &Path, depth: usize, store: Option<&Path>) -> (DirsIndex, Reuse) {
    let path = store_path(root, depth, store);
    let cached = load_rows(&path);
    let mut idx = DirsIndex {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let mut reuse = Reuse::default();
    if !root.is_dir() {
        return (idx, reuse);
    }
    let cache = cached.as_ref().map(|(r, _)| r);
    walk_cached(root, root, 0, depth.max(1), &mut idx, cache, &mut reuse);
    save_rows(&path, &idx);
    (idx, reuse)
}

#[allow(clippy::too_many_arguments)]
fn walk_cached(
    root: &Path,
    dir: &Path,
    level: usize,
    max: usize,
    idx: &mut DirsIndex,
    cache: Option<&BTreeMap<PathBuf, DirRow>>,
    reuse: &mut Reuse,
) {
    let m = mtime_secs(dir);
    let hit = cache.and_then(|c| c.get(dir)).filter(|r| r.mtime == m);
    let row = match hit {
        Some(r) => {
            reuse.facts_reused += 1;
            DirRow {
                mtime: m,
                markers: r.markers.clone(),
                has_git: r.has_git,
                has_contract: r.has_contract,
                skipped: None,
            }
        }
        None => {
            reuse.facts_read += 1;
            row_for(dir).0
        }
    };
    let readable = dir.read_dir().is_ok();
    idx.rows.insert(dir.to_path_buf(), row);
    idx.visited += 1;
    if !readable || level >= max {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let child = e.path();
        if let Some(reason) = skip_reason(&child, root) {
            idx.rows.insert(
                child.clone(),
                DirRow {
                    mtime: mtime_secs(&child),
                    skipped: Some(reason),
                    ..Default::default()
                },
            );
            idx.skipped += 1;
            continue;
        }
        walk_cached(root, &child, level + 1, max, idx, cache, reuse);
    }
}
#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-dirsidx-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn mkproject(dir: &Path, marker: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(marker), "{}").unwrap();
    }

    /// **最外层口径 = 显式预期**（不是拿 `discover` 跟自己比）。
    ///
    /// 2026-10-18 改：`discover` 自己已经走索引了，所以原来那条"索引与 discover 一致"
    /// **变成了自己比自己**（永远绿）。换成写死预期的写法——这才是真判据。
    ///
    /// 口径（与界面一致）：**遇项目就不再往里算**；因此 `mono/pkg1`、`mono/pkg2` 不出现。
    #[test]
    fn 最外层口径与显式预期一致() {
        let d = scratch("expect");
        mkproject(&d.join("a"), "Cargo.toml");
        mkproject(&d.join("b"), "package.json");
        mkproject(&d.join("mono"), "package.json");
        mkproject(&d.join("mono").join("pkg1"), "package.json");
        mkproject(&d.join("mono").join("pkg2"), "package.json");
        fs::create_dir_all(d.join("notaproject")).unwrap();

        let idx = build(&d, 3);
        let got: Vec<String> = idx
            .top_level_projects()
            .iter()
            .map(|p| p.strip_prefix(&d).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(got, vec!["a", "b", "mono"], "最外层只该是这三个，实际：{got:?}");

        // 同一个索引，换个查询就看得见嵌套的——**规则改了不用重建**。
        let all: Vec<String> = idx
            .all_projects()
            .iter()
            .map(|p| p.strip_prefix(&d).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(all.len(), 5, "全都给应该是 5 个（含 mono/pkg1、mono/pkg2）：{all:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// **嵌套项目不许丢**：同一个索引，换成"全都给"就是 5 个，不是 3 个。
    ///
    /// 这是"改规则不用重扫"的第一条证据：**索引没动，换一个查询**。
    #[test]
    fn 换个查询就能看见嵌套项目_不用重建索引() {
        let d = scratch("nested");
        mkproject(&d.join("mono"), "package.json");
        mkproject(&d.join("mono").join("pkg1"), "package.json");
        mkproject(&d.join("mono").join("pkg2"), "package.json");

        let idx = build(&d, 3);
        let top = idx.top_level_projects();
        let all = idx.all_projects();
        assert_eq!(top.len(), 1, "最外层只有 mono：{top:?}");
        assert_eq!(all.len(), 3, "全都给是 mono + pkg1 + pkg2：{all:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// **跳过要留痕**：跳过的目录在表里有一行，并且能说清是按哪条规则跳的。
    #[test]
    fn 跳过的目录要留一行并且说清原因() {
        let d = scratch("skip");
        mkproject(&d.join("ok"), "Cargo.toml");
        fs::create_dir_all(d.join("node_modules").join("x")).unwrap();

        let idx = build(&d, 2);
        let nm = d.join("node_modules");
        assert!(
            idx.rows.contains_key(&nm),
            "被跳过的目录也必须在表里留一行——否则「为什么没扫到」答不出来"
        );
        let why = idx.why_not(&nm).unwrap_or("");
        assert!(
            why.contains("不是源码目录"),
            "原因要说清是哪条规则跳的，实际：{why}"
        );
        assert_eq!(idx.skipped_count(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    /// **读不出来 ≠ 空目录**（三态的那一态，钉在**建行**这一层）。
    ///
    /// 第一版我把这条判据钉在 `skip_reason` 上，**前提就错了**：
    /// 那个函数回答的是"要不要进去"，不是"读不读得到"。
    /// 跑出来当场红（不存在的路径也不跳），正好说明"判据放错层"本身会被测试抓到。
    #[test]
    fn 读不出来的目录要带原因_而不是空行() {
        let d = scratch("unreadable");
        let ghost = d.join("ghost"); // 不创建：一定读不到

        let (row, readable) = row_for(&ghost);
        assert!(!readable, "不存在的目录不可能可读");
        assert_eq!(
            row.skipped,
            Some("目录读不出来（权限或坏盘）"),
            "读不出来必须带原因——否则界面上会显示成「没有项目」，而那正是三态压扁"
        );
        assert!(!row.looks_like_project(), "读不到的目录不许被当成项目");
        let _ = fs::remove_dir_all(&d);
    }

    /// **第二次建索引时，事实全部复用**（判据①的落点）。
    ///
    /// 说明：**遍历照走**（mtime 做不到"祖先不变就跳过子树"），但每个目录的
    /// markers / has_git / has_contract 来自缓存，不再 read_dir 去数。
    #[test]
    fn 第二次建索引时事实全部复用() {
        let d = scratch("reuse");
        mkproject(&d.join("a"), "Cargo.toml");
        mkproject(&d.join("b"), "package.json");
        let store = scratch("reuse-store");

        let (i1, r1) = build_cached(&d, 2, Some(&store));
        assert!(r1.facts_read >= 1, "第一次必然全是新读：{r1:?}");

        let (i2, r2) = build_cached(&d, 2, Some(&store));
        assert_eq!(r2.facts_read, 0, "第二次不该再读目录：{r2:?}");
        assert_eq!(
            r2.facts_reused, i2.visited,
            "每个目录的事实都该来自缓存：{r2:?} visited={}",
            i2.visited
        );
        // 结论不变：同一棵树，项目集合一样
        assert_eq!(i1.top_level_projects(), i2.top_level_projects());
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&store);
    }

    /// **改一个深层目录，只有它（及其父链上被读到的目录）重读**，别人仍复用。
    ///
    /// 这条钉的是"缓存不许把变化吃掉"——如果实现偷懒成"根没变就全复用"，
    /// 这里会红：新建的项目看不见。
    #[test]
    fn 深层新增项目第二次就看得见() {
        let d = scratch("deep");
        mkproject(&d.join("a"), "Cargo.toml");
        let store = scratch("deep-store");
        let (_, _) = build_cached(&d, 3, Some(&store));

        // 在深层新建一个项目
        mkproject(&d.join("a").join("inner"), "package.json");
        let (i2, r2) = build_cached(&d, 3, Some(&store));
        let all = i2.all_projects();
        assert!(
            all.iter().any(|p| p.ends_with("inner")),
            "深层新建的项目必须看得见（缓存放行就是吃掉了变化）：{all:?}"
        );
        assert!(r2.facts_read >= 1, "至少那个新目录是读出来的：{r2:?}");
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&store);
    }
}
