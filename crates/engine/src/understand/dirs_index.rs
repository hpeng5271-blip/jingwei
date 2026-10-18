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
    if crate::util::scan::is_not_source_dir(&name) {
        return Some("不是源码目录（node_modules / target / dist 之类）");
    }
    if is_system_dir(&name) {
        return Some("系统目录（Windows / AppData / Program Files 之类）");
    }
    scan::should_skip(dir, root)
}

fn is_system_dir(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "appdata"
            | "windows"
            | "program files"
            | "program files (x86)"
            | "programdata"
            | "system volume information"
            | "$recycle.bin"
            | "perflogs"
            | "recovery"
    )
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

    /// **回归**：同一棵树，索引的"最外层"口径必须与 `discover` 一致。
    ///
    /// 这一条是"先有裁判，再换选手"——**接线之前先证明等价**。
    #[test]
    fn 索引与_discover_对同一棵树给出同样的项目集() {
        let d = scratch("same");
        mkproject(&d.join("a"), "Cargo.toml");
        mkproject(&d.join("b"), "package.json");
        // monorepo：外层是项目，里面还有两个
        mkproject(&d.join("mono"), "package.json");
        mkproject(&d.join("mono").join("pkg1"), "package.json");
        mkproject(&d.join("mono").join("pkg2"), "package.json");
        // 不是项目的目录
        fs::create_dir_all(d.join("notaproject")).unwrap();

        let old: Vec<String> = crate::understand::projects::discover(&d, 3)
            .into_iter()
            .map(|f| f.path.replace('\\', "/"))
            .collect();
        let idx = build(&d, 3);
        let new: Vec<String> = idx
            .top_level_projects()
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();

        assert_eq!(old, new, "索引的「最外层」口径必须和 discover 一致");
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
}
