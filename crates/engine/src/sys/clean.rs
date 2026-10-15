//! 清理：A/B/C 三区扫描 + 删除（默认走回收站）。
//!
//! A 区 · 工具代删：纯粹是缓存/产物，删了会重新生成。
//! B 区 · 只给命令：需要管理员，工具有意不代跑（代跑风险不可控）。
//! C 区 · 只解释：永不碰。用户在这里得到的是「为什么不能删」。

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    A,
    B,
    C,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// 工具直接删（走回收站或永久）
    Delete,
    /// 给一条命令，用户自己以管理员身份跑
    Command,
    /// 只解释，不可操作
    Explain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub zone: Zone,
    pub action: Action,
    /// 分组名，例如「Python 缓存」
    pub category: String,
    /// 一行显示名
    pub label: String,
    /// 主要路径（命令类项目里是给人看的说明）
    pub path: String,
    pub size_bytes: u64,
    pub files: usize,
    /// 为什么可以清
    pub reason: String,
    /// 清了会怎样（必须诚实：有没有代价）
    pub consequence: String,
    pub needs_admin: bool,
    pub recommended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// 命令的用法说明。刻意和 command 分开：
    /// 混在一起的话，用户复制「命令」会把中文说明一起粘进终端，必然报错。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 真正要删的路径（TEMP 这类是一组）；不落盘
    #[serde(skip)]
    pub targets: Vec<PathBuf>,
}

pub fn human_size(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", b, U[0])
    } else {
        format!("{:.1} {}", v, U[i])
    }
}

// ------------------------------------------------------------------ 扫描

pub fn scan(root: &Path) -> Vec<Item> {
    let mut out = Vec::new();
    scan_project(root, &mut out);
    scan_project_junk(root, &mut out);
    // 先把所有候选目录量出来再填清单：量体积要遍历几十万个文件，
    // 这是 scan 里唯一的重活，串着做等于白等磁盘。
    let sizes = measure_candidates();
    scan_caches(&mut out, &sizes);
    scan_temp(&mut out);
    scan_admin(&mut out);
    scan_offlimits(&mut out);
    out.sort_by(|a, b| {
        (a.zone as u8, std::cmp::Reverse(a.size_bytes))
            .cmp(&(b.zone as u8, std::cmp::Reverse(b.size_bytes)))
    });
    out
}

/// 先纯路径推导（不碰磁盘），再并行量体积。
fn measure_candidates() -> BTreeMap<String, (u64, usize)> {
    let dirs: Vec<PathBuf> = CACHE_DIRS
        .iter()
        .filter_map(|(var, rel)| env_path(var, rel))
        .filter(|p| p.is_dir())
        .collect();
    // 关键：量的是 dirs，配对的也必须是 dirs。
    // 先按未过滤的候选列表建 key 会错位，把 A 目录的体积安到 B 头上。
    let measured = crate::util::parallel::map(dirs.clone(), |p| dir_stat(&p));
    let mut map = BTreeMap::new();
    for (p, m) in dirs.into_iter().zip(measured) {
        map.insert(p.to_string_lossy().to_lowercase(), m);
    }
    map
}

/// 全局缓存目录总表。量体积和填清单都以它为准，避免两边写歪。
const CACHE_DIRS: &[(&str, &str)] = &[
    ("LOCALAPPDATA", "pip/cache"),
    ("LOCALAPPDATA", "uv/cache"),
    ("LOCALAPPDATA", "Python/_cache"),
    ("USERPROFILE", ".conda/pkgs"),
    ("LOCALAPPDATA", "npm-cache"),
    ("LOCALAPPDATA", "Yarn/Cache"),
    ("LOCALAPPDATA", "pnpm-store"),
    ("LOCALAPPDATA", "go-build"),
    ("USERPROFILE", "go/pkg/mod"),
    ("USERPROFILE", ".cargo/registry"),
    ("USERPROFILE", ".gradle/caches"),
    ("APPDATA", "Code/Cache"),
    ("APPDATA", "Code/CachedData"),
    ("APPDATA", "Code/GPUCache"),
    ("APPDATA", "Code/Code Cache"),
    ("APPDATA", "Code/logs"),
    ("LOCALAPPDATA", "Microsoft/Edge/User Data/Default/Cache"),
    ("LOCALAPPDATA", "Google/Chrome/User Data/Default/Cache"),
    ("LOCALAPPDATA", "CrashDumps"),
];

pub fn total(items: &[Item], zone: Zone) -> (u64, usize) {
    let sel: Vec<&Item> = items
        .iter()
        .filter(|i| i.zone == zone && i.action == Action::Delete)
        .collect();
    (
        sel.iter().map(|i| i.size_bytes).sum(),
        sel.len(),
    )
}

const JUNK_DIRS: &[&str] = &[
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".ipynb_checkpoints",
    "htmlcov",
];
const BUILD_DIRS: &[&str] = &["build", "dist"];
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "env",
    ".idea",
    ".vs",
    ".jingwei",
    ".drydock",
    ".pylaunch",
];

fn scan_project(root: &Path, out: &mut Vec<Item>) {
    if !root.is_dir() {
        return;
    }
    let mut hits: Vec<PathBuf> = Vec::new();
    let walker = walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            let n = e.file_name().to_string_lossy().to_lowercase();
            if JUNK_DIRS.contains(&n.as_str())
                || BUILD_DIRS.contains(&n.as_str())
                || n.ends_with(".egg-info")
            {
                hits.push(e.path().to_path_buf());
                return false;
            }
            !SKIP_DIRS.contains(&n.as_str())
        });
    for _ in walker {}

    // 每个命中目录可能藏着几万个文件（尤其是 build/ 和 dist/），并行量
    let sized = crate::util::parallel::map(hits, |p| {
        let m = dir_stat(&p);
        (p, m)
    });
    for (p, (bytes, files)) in sized {
        let n = p
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if bytes == 0 {
            continue;
        }
        let (category, reason, consequence, recommended) =
            if n == "__pycache__" {
                (
                    "项目产物",
                    "Python 导入时自动生成的字节码；下次导入会重建",
                    "只是首次 import 慢一点点；不影响任何功能",
                    true,
                )
            } else if BUILD_DIRS.contains(&n.as_str()) {
                (
                    "项目产物",
                    "打包/构建目录；重新构建就会再生成",
                    "如果里面有你要交付的安装包，先拷走再删",
                    false,
                )
            } else if n.ends_with(".egg-info") {
                (
                    "项目产物",
                    "pip install -e 留下的元数据；重装时重建",
                    "之后 pip uninstall 会认不出它，必要时 pip install -e . 重来一次",
                    false,
                )
            } else {
                (
                    "项目产物",
                    "测试/类型检查/格式化工具的运行缓存",
                    "下次运行时重新计算，不影响结果",
                    true,
                )
            };
        out.push(Item {
            zone: Zone::A,
            action: Action::Delete,
            category: category.into(),
            label: n.clone(),
            path: p.to_string_lossy().to_string(),
            size_bytes: bytes,
            files,
            reason: reason.into(),
            consequence: consequence.into(),
            needs_admin: false,
            recommended,
            command: None,
            command_note: None,
            detail: None,
            targets: vec![p],
        });
    }
}

/// 一眼是垃圾的文件名。返回它属于哪一类，同时也是给人看的标签。
///
/// 这几个后缀不是我挑的：`*.bak / *.tmp / *.swp / env.bak / venv.bak`
/// 在 GitHub 官方 .gitignore 模板里是标配——普遍到需要被集体屏蔽，
/// 说明它们确实是所有人都在忍着的东西。
fn junk_kind(name: &str) -> Option<&'static str> {
    const SUFFIX: &[(&str, &str)] = &[
        (".bak", "*.bak · 备份文件"),
        (".old", "*.old · 旧版本副本"),
        (".orig", "*.orig · 打补丁留下的原件"),
        (".rej", "*.rej · 没打上的补丁片段"),
        (".tmp", "*.tmp · 临时文件"),
        (".temp", "*.temp · 临时文件"),
        (".swp", "*.swp · 编辑器崩溃残留"),
        (".swo", "*.swo · 编辑器崩溃残留"),
        (".save", "*.save · 编辑器崩溃残留"),
        ("~", "*~ · 编辑器自动备份"),
    ];
    for (s, label) in SUFFIX {
        if name.ends_with(s) {
            return Some(label);
        }
    }
    if name.contains(" - 副本") || name.contains(" - copy") {
        return Some("某某 - 副本 · 手抄的复制品");
    }
    None
}

fn rel_show(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 项目里一眼是垃圾的文件，和「手抄一份环境当备份」这种真实的坑。
///
/// 日志和空目录只列不建议：留不留只有项目自己知道，工具不该替它决定。
fn scan_project_junk(root: &Path, out: &mut Vec<Item>) {
    if !root.is_dir() {
        return;
    }
    let mut groups: BTreeMap<&'static str, Vec<PathBuf>> = BTreeMap::new();
    let mut sizes: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut logs: Vec<(PathBuf, u64)> = Vec::new();
    let mut empties: Vec<PathBuf> = Vec::new();

    for entry in walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            let n = e.file_name().to_string_lossy().to_lowercase();
            !(SKIP_DIRS.contains(&n.as_str())
                || JUNK_DIRS.contains(&n.as_str())
                || BUILD_DIRS.contains(&n.as_str())
                || n.ends_with(".egg-info"))
        })
        .flatten()
    {
        let p = entry.path().to_path_buf();
        if entry.file_type().is_dir() {
            if p != root
                && std::fs::read_dir(&p)
                    .map(|mut r| r.next().is_none())
                    .unwrap_or(false)
            {
                empties.push(p);
            }
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if let Some(kind) = junk_kind(&name) {
            groups.entry(kind).or_default().push(p.clone());
            *sizes.entry(kind).or_insert(0) += bytes;
            continue;
        }
        if name.ends_with(".log") && bytes >= 1024 * 1024 {
            logs.push((p, bytes));
        }
    }

    for (label, files) in groups {
        let bytes = sizes.get(label).copied().unwrap_or(0);
        out.push(Item {
            zone: Zone::A,
            action: Action::Delete,
            category: "项目垃圾".into(),
            label: format!("{}（{} 个）", label, files.len()),
            path: root.to_string_lossy().to_string(),
            size_bytes: bytes,
            files: files.len(),
            reason: "编辑器和补丁工具留下的中间产物，程序运行时不会去读它".into(),
            consequence: "不影响运行。万一是你要的备份，回收站里还能捞回来".into(),
            needs_admin: false,
            recommended: true,
            command: None,
            command_note: None,
            detail: Some(
                files
                    .iter()
                    .take(5)
                    .map(|p| rel_show(root, p))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            targets: files,
        });
    }

    if !logs.is_empty() {
        let bytes: u64 = logs.iter().map(|(_, b)| *b).sum();
        out.push(Item {
            zone: Zone::A,
            action: Action::Delete,
            category: "项目垃圾".into(),
            label: format!("日志文件（{} 个，单个 ≥ 1 MB）", logs.len()),
            path: root.to_string_lossy().to_string(),
            size_bytes: bytes,
            files: logs.len(),
            reason: "运行日志会一直追加，放久了很占地方；它只对「排查正在发生的问题」有用".into(),
            consequence: "删了不影响程序。但如果此刻正在查一个 bug，先别删——那是线索".into(),
            needs_admin: false,
            // 大到这个份上，留着多半只是没管过
            recommended: bytes >= 5 * 1024 * 1024,
            command: None,
            command_note: None,
            detail: Some(
                logs.iter()
                    .take(5)
                    .map(|(p, b)| format!("{}（{} KB）", rel_show(root, p), b / 1024))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            targets: logs.into_iter().map(|(p, _)| p).collect(),
        });
    }

    if !empties.is_empty() {
        out.push(Item {
            zone: Zone::A,
            action: Action::Delete,
            category: "项目垃圾".into(),
            label: format!("空目录（{} 个）", empties.len()),
            path: root.to_string_lossy().to_string(),
            size_bytes: 0,
            files: empties.len(),
            reason: "里面一个文件都没有。多半是删文件时留下的壳，或者某次失败的复制".into(),
            consequence:
                "有些程序要求目录先存在（日志目录、上传目录），删了它可能自己重建、也可能报错"
                    .into(),
            needs_admin: false,
            recommended: false,
            command: None,
            command_note: None,
            detail: Some(
                empties
                    .iter()
                    .take(6)
                    .map(|p| rel_show(root, p))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            targets: empties,
        });
    }

    scan_unused_venvs(root, out);
}

/// 虚拟环境：同一个项目里往往躺着两三个，只有一个真的在用。
/// 能确定「没在用」的两种情况：名字里写着 bak/副本，或者契约绑的是另一个。
fn scan_unused_venvs(root: &Path, out: &mut Vec<Item>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let (contract, _) = crate::Contract::load(root);
    let bound = contract
        .as_ref()
        .and_then(|c| c.runtime.python_exe.clone())
        .map(|s| s.to_lowercase());

    for e in rd.flatten() {
        let p = e.path();
        if !p.join("pyvenv.cfg").is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        let is_copy = lower.contains(".bak")
            || lower.contains("-bak")
            || lower.contains("backup")
            || lower.contains("副本")
            || lower.contains("-copy");
        let (reason, consequence, how) = if is_copy {
            (
                "名字里就写着备份/副本：虚拟环境里的路径是写死的，复制出来的那份多数时候根本跑不起来",
                "删了它，你原来那个环境一点不受影响。真要留备份，留 requirements.txt 就够了",
                "目录名里带着 bak / 副本",
            )
        } else {
            let Some(b) = &bound else {
                continue;
            };
            if b.contains(&lower) {
                continue;
            }
            (
                "项目契约绑定的是另一个虚拟环境，这一个没被任何配置指向",
                "删掉不影响现在能跑的项目；以后要用它得重新建、重新装依赖",
                "契约里的解释器路径不在这里",
            )
        };
        let (bytes, files) = dir_stat(&p);
        out.push(Item {
            zone: Zone::A,
            action: Action::Delete,
            category: "项目垃圾".into(),
            label: format!("没在用的虚拟环境：{}", name),
            path: p.to_string_lossy().to_string(),
            size_bytes: bytes,
            files,
            reason: reason.into(),
            consequence: consequence.into(),
            needs_admin: false,
            recommended: true,
            command: None,
            command_note: None,
            detail: Some(format!(
                "判定依据：{}\n要重建的话：python -m venv {}",
                how, name
            )),
            targets: vec![p],
        });
    }
}

struct Addr<'a> {
    category: &'a str,
    label: &'a str,
    reason: &'a str,
    consequence: &'a str,
    recommended: bool,
    command: Option<&'a str>,
    command_note: Option<&'a str>,
    detail: Option<String>,
}

fn scan_caches(out: &mut Vec<Item>, sizes: &BTreeMap<String, (u64, usize)>) {
    // 体积已经在 measure_candidates 里并行量好了，这里只做查表和排版。
    let add_dir = |out: &mut Vec<Item>, a: Addr, path: PathBuf| {
        let key = path.to_string_lossy().to_lowercase();
        let (bytes, files) = match sizes.get(&key) {
            Some(m) => *m,
            None => (0, 0),
        };
        if bytes == 0 {
            return;
        }
        out.push(Item {
            zone: Zone::A,
            action: Action::Delete,
            category: a.category.into(),
            label: a.label.into(),
            path: path.to_string_lossy().to_string(),
            size_bytes: bytes,
            files,
            reason: a.reason.into(),
            consequence: a.consequence.into(),
            needs_admin: false,
            recommended: a.recommended,
            command: a.command.map(String::from),
            command_note: a.command_note.map(String::from),
            detail: a.detail,
            targets: vec![path],
        });
    };
    let e = |var: &str, rel: &str| env_path(var, rel);

    if let Some(p) = e("LOCALAPPDATA", "pip/cache") {
        add_dir(
            out,
            Addr {
                category: "Python 缓存",
                label: "pip 下载缓存",
                reason: "pip 下载过的 wheel/源码缓存",
                consequence: "下次装同名包要重新下载（有网就行）",
                recommended: true,
                command: Some("python -m pip cache purge"),
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "uv/cache") {
        add_dir(
            out,
            Addr {
                category: "Python 缓存",
                label: "uv 包缓存",
                reason: "uv 的解析结果与包归档缓存",
                consequence: "已装的虚拟环境照常能用；新建环境要重新下载",
                recommended: true,
                command: Some("uv cache clean"),
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("USERPROFILE", ".conda/pkgs") {
        add_dir(
            out,
            Addr {
                category: "Python 缓存",
                label: "conda 包缓存",
                reason: "conda 解压过的包副本；已建环境不依赖它",
                consequence: "以后 conda install 会重新下载",
                recommended: true,
                command: Some("conda clean --all --dry-run"),
                command_note: Some("先看它列出什么。确认没问题，再去掉 --dry-run 真删"),
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "Python/_cache") {
        add_dir(
            out,
            Addr {
                category: "Python 缓存",
                label: "运行时下载缓存（安装管理器）",
                reason: "Python 安装管理器下载过的运行时归档；装好的版本不依赖它",
                consequence: "装好的运行时不依赖它；下次 python install 新版本要重新下载",
                recommended: false,
                command: Some("py --list"),
                command_note: Some("先看这台机器装了哪些版本，再决定删不删缓存"),
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "npm-cache") {
        add_dir(
            out,
            Addr {
                category: "Node 缓存",
                label: "npm 缓存",
                reason: "npm 下载过的 tarball",
                consequence: "npm install 重新下载；node_modules 不受影响",
                recommended: true,
                command: Some("npm cache clean --force"),
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "Yarn/Cache") {
        add_dir(
            out,
            Addr {
                category: "Node 缓存",
                label: "Yarn 缓存",
                reason: "Yarn 的包缓存",
                consequence: "重新 yarn install 时会再下载",
                recommended: true,
                command: Some("yarn cache clean"),
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "pnpm-store") {
        add_dir(
            out,
            Addr {
                category: "Node 缓存",
                label: "pnpm store",
                reason: "pnpm 的内容寻址仓库",
                consequence: "现有项目的 node_modules 是硬链接，删了可能让它们失效；要清用 pnpm store prune",
                recommended: false,
                command: Some("pnpm store prune"),
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "go-build") {
        add_dir(
            out,
            Addr {
                category: "Go 缓存",
                label: "go build 缓存",
                reason: "编译中间产物缓存",
                consequence: "下次 go build/go test 慢一些",
                recommended: true,
                command: Some("go clean -cache"),
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("USERPROFILE", "go/pkg/mod") {
        add_dir(
            out,
            Addr {
                category: "Go 缓存",
                label: "go 模块缓存",
                reason: "go mod 下载的依赖源码",
                consequence: "断网时没法再 build；有网就自动重下",
                recommended: false,
                command: Some("go clean -modcache"),
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("USERPROFILE", ".cargo/registry") {
        add_dir(
            out,
            Addr {
                category: "Rust 缓存",
                label: "cargo registry",
                reason: "crates.io 下载的包与索引",
                consequence: "下次 cargo build 要把依赖全下一遍（这块经常几个 GB）",
                recommended: false,
                command: None,
                command_note: None,
                detail: Some("建议只清 cache 子目录".into()),
            },
            p,
        );
    }
    if let Some(p) = e("USERPROFILE", ".gradle/caches") {
        add_dir(
            out,
            Addr {
                category: "JVM 缓存",
                label: "Gradle 依赖缓存",
                reason: "Gradle 下载的依赖与 daemon 缓存",
                consequence: "下次构建需重新下载；已 checkout 的项目要重跑配置",
                recommended: false,
                command: None,
                command_note: None,
                detail: None,
            },
            p,
        );
    }

    for (cat, label, rel) in [
        ("VS Code 缓存", "Code Cache", "Code/Cache"),
        ("VS Code 缓存", "CachedData", "Code/CachedData"),
        ("VS Code 缓存", "GPUCache", "Code/GPUCache"),
        ("VS Code 缓存", "Code Cache(old)", "Code/Code Cache"),
        ("VS Code 缓存", "日志", "Code/logs"),
    ] {
        if let Some(p) = e("APPDATA", rel) {
            add_dir(
                out,
                Addr {
                    category: cat,
                    label,
                    reason: "编辑器自己的缓存与日志，跟你的代码无关",
                    consequence: "VS Code 首次打开稍慢；设置和插件都不受影响",
                    recommended: true,
                    command: None,
                    command_note: None,
                    detail: None,
                },
                p,
            );
        }
    }
    if let Some(p) = e("LOCALAPPDATA", "Microsoft/Edge/User Data/Default/Cache") {
        add_dir(
            out,
            Addr {
                category: "浏览器缓存",
                label: "Edge 缓存",
                reason: "网页缓存",
                consequence: "首次打开常用网站慢一点；登录状态、密码不受影响",
                recommended: false,
                command: None,
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "Google/Chrome/User Data/Default/Cache") {
        add_dir(
            out,
            Addr {
                category: "浏览器缓存",
                label: "Chrome 缓存",
                reason: "网页缓存",
                consequence: "首次打开常用网站慢一点；登录状态、密码不受影响",
                recommended: false,
                command: None,
                command_note: None,
                detail: None,
            },
            p,
        );
    }
    if let Some(p) = e("LOCALAPPDATA", "CrashDumps") {
        add_dir(
            out,
            Addr {
                category: "系统",
                label: "崩溃转储",
                reason: "程序崩溃时留下的内存快照",
                consequence: "除非你正在查某个崩溃，否则没有任何用",
                recommended: true,
                command: None,
                command_note: None,
                detail: None,
            },
            p,
        );
    }

    // 缩略图缓存：一堆文件，不是目录
    if let Some(explorer) = env_path("LOCALAPPDATA", "Microsoft/Windows/Explorer") {
        let (mut bytes, mut files) = (0u64, 0usize);
        let mut targets = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&explorer) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if n.starts_with("thumbcache_") || n.starts_with("iconcache_") {
                    if let Ok(m) = e.metadata() {
                        bytes += m.len();
                        files += 1;
                        targets.push(e.path());
                    }
                }
            }
        }
        if bytes > 0 {
            out.push(Item {
                zone: Zone::A,
                action: Action::Delete,
                category: "系统".into(),
                label: "缩略图 / 图标缓存".into(),
                path: explorer.to_string_lossy().to_string(),
                size_bytes: bytes,
                files,
                reason: "资源管理器为图片/视频/图标生成的预览缓存".into(),
                consequence: "删除后资源管理器会自动重建；只是某些文件夹首次打开时图标要重算，不影响文件本身".into(),
                needs_admin: false,
                recommended: true,
                command: None,
                command_note: None,
                detail: Some(format!("{} 个 db 文件", files)),
                targets,
            });
        }
    }

    // 回收站：得让系统自己清
    out.push(Item {
        zone: Zone::A,
        action: Action::Command,
        category: "系统".into(),
        label: "回收站".into(),
        path: "所有驱动器".into(),
        size_bytes: 0,
        files: 0,
        reason: "已经删过一次、但还占着盘的东西".into(),
        consequence: "清空后无法还原。确认里面没有你要找回的文件再清".into(),
        needs_admin: false,
        recommended: false,
        command: Some("Clear-RecycleBin -Force".into()),
        command_note: Some("在 PowerShell 里跑".into()),
        detail: Some("回收站由系统托管，工具不直接删；这条命令自己跑一下即可".into()),
        targets: Vec::new(),
    });
}

fn scan_temp(out: &mut Vec<Item>) {
    let Some(temp) = std::env::var_os("TEMP")
        .map(PathBuf::from)
        .or_else(|| env_path("LOCALAPPDATA", "Temp"))
    else {
        return;
    };
    if !temp.is_dir() {
        return;
    }
    let now = SystemTime::now();
    let cutoff = Duration::from_secs(7 * 24 * 3600);
    let (mut bytes, mut files) = (0u64, 0usize);
    let mut targets = Vec::new();
    // 先筛出「够老」的条目（这一步只有 stat，很便宜），
    // 再把它们并行量体积——TEMP 里动辄几千个目录。
    let mut stale: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&temp) {
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            let t = m
                .modified()
                .or_else(|_| m.created())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if now.duration_since(t).unwrap_or_default() < cutoff {
                continue;
            }
            stale.push(p);
        }
    }
    for (p, (b, f)) in crate::util::parallel::map(stale, |p| {
        let m = if p.is_dir() {
            dir_stat(&p)
        } else {
            (std::fs::metadata(&p).map(|x| x.len()).unwrap_or(0), 1)
        };
        (p, m)
    }) {
        if b == 0 && f == 0 {
            continue;
        }
        bytes += b;
        files += f;
        targets.push(p);
    }
    if bytes == 0 {
        return;
    }
    out.push(Item {
        zone: Zone::A,
        action: Action::Delete,
        category: "系统".into(),
        label: "临时文件（7 天以上没动过）".into(),
        path: temp.to_string_lossy().to_string(),
        size_bytes: bytes,
        files,
        reason: "程序用完就丢的中间文件；7 天没碰过的基本没人要了".into(),
        consequence: "正在运行的程序占用的会被自动跳过；关掉再清更干净".into(),
        needs_admin: false,
        recommended: true,
        command: None,
        command_note: None,
        detail: Some(format!("{} 项", targets.len())),
        targets,
    });
}

fn scan_admin(out: &mut Vec<Item>) {
    let mut cmd = |category: &str,
                   label: &str,
                   path: PathBuf,
                   reason: &str,
                   consequence: &str,
                   command: &str,
                   recommended: bool,
                   detail: Option<String>| {
        let (bytes, files) = if path.is_dir() {
            dir_stat(&path)
        } else {
            match std::fs::metadata(&path) {
                Ok(m) => (m.len(), 1),
                Err(_) => (0, 0),
            }
        };
        out.push(Item {
            zone: Zone::B,
            action: Action::Command,
            category: category.into(),
            label: label.into(),
            path: path.to_string_lossy().to_string(),
            size_bytes: bytes,
            files,
            reason: reason.into(),
            consequence: consequence.into(),
            needs_admin: true,
            recommended,
            command: Some(command.into()),
            command_note: Some("在「管理员 PowerShell」里跑".into()),
            detail,
            targets: Vec::new(),
        });
    };

    if let Some(windir) = std::env::var_os("SystemRoot").map(PathBuf::from) {
        let drive_root = windir.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));
        let d = windir.join("SoftwareDistribution").join("Download");
        if d.is_dir() {
            cmd(
                "系统",
                "Windows 更新下载缓存",
                d,
                "系统更新下载完就留着不删的安装包",
                "下次更新会重新下载；不会影响已装的更新",
                "Stop-Service wuauserv,bits -Force -ErrorAction SilentlyContinue; Remove-Item \"$env:windir\\SoftwareDistribution\\Download\\*\" -Recurse -Force -ErrorAction SilentlyContinue; Start-Service bits,wuauserv -ErrorAction SilentlyContinue",
                true,
                Some("停服务是必须的：更新服务正占着那些文件，不先停掉删不干净".into()),
            );
        }
        let old = drive_root.join("Windows.old");
        if old.is_dir() {
            cmd(
                "系统",
                "以前的 Windows 安装",
                old,
                "大版本升级后留下的旧系统备份",
                "删掉就再也退不回上一个 Windows 版本了；升级完用了半个月没事就可以删",
                "Remove-Item \"$env:SystemDrive\\Windows.old\" -Recurse -Force",
                true,
                Some("更稳的路子：跑 cleanmgr，在列表里勾「以前的 Windows 安装文件」".into()),
            );
        }
        let hib = drive_root.join("hiberfil.sys");
        if hib.exists() {
            cmd(
                "系统",
                "休眠文件 hiberfil.sys",
                hib,
                "休眠/快速启动用的内存镜像，大小通常等于内存的一半以上",
                "会失去休眠和快速启动（开机能慢几秒）；想恢复跑 powercfg /h on",
                "powercfg /h off",
                true,
                Some("笔记本用户的取舍：关掉省几个 GB，开机慢一点".into()),
            );
        }
    }

    cmd(
        "系统",
        "系统还原点 / 卷影副本",
        PathBuf::from("%SystemDrive%\\System Volume Information"),
        "系统为还原点保存的历史快照，随时间越积越多",
        "删掉后无法用「系统还原」回滚到更早的时间点；保留最近一两个才划算",
        "vssadmin list shadowstorage",
        false,
        Some("这条只是看占用。要调上限用「系统保护」界面——别去手删那个目录".into()),
    );
    cmd(
        "系统",
        "组件存储精简（WinSxS 的合法清理方式）",
        PathBuf::from("%windir%\\WinSxS"),
        "Windows 官方提供的组件清理通道；它会顺手清掉被取代的旧组件",
        "加了 /ResetBase 之后就装不回已经装过的旧更新了，别的没有影响",
        "Dism /Online /Cleanup-Image /AnalyzeComponentStore",
        true,
        Some("这条只是出报告。确认要清再跑 Dism /Online /Cleanup-Image /StartComponentCleanup（加 /ResetBase 就装不回旧更新了）".into()),
    );
    cmd(
        "系统",
        "第三方驱动包（旧版本）",
        PathBuf::from("%windir%\\System32\\DriverStore\\FileRepository"),
        "装驱动时留下的旧版本，设备管理里看不见但占地方",
        "删错会让某个硬件失效；只删标记为「旧版本」的，并且删之前记下名字",
        "pnputil /enum-drivers",
        false,
        Some("上面会列出 oemNN.inf 和版本号。确认哪个是旧版本之后，再跑 pnputil /delete-driver oemNN.inf /uninstall /force —— oemNN 要换成真实编号，这条没法替你填".into()),
    );
    cmd(
        "系统",
        "磁盘清理向导",
        PathBuf::from("cleanmgr"),
        "Windows 自带的清理入口，比手工删安全得多",
        "按你勾选的项清理；不确定的项别勾",
        "cleanmgr /d C:",
        true,
        Some("想让它记住你的勾选：cleanmgr /sageset:1 一次，之后 cleanmgr /sagerun:1".into()),
    );
}

fn scan_offlimits(out: &mut Vec<Item>) {
    let mut c = |label: &str, path: &str, consequence: &str| {
        out.push(Item {
            zone: Zone::C,
            action: Action::Explain,
            category: "别碰".into(),
            label: label.into(),
            path: path.into(),
            size_bytes: 0,
            files: 0,
            reason: "".into(),
            consequence: consequence.into(),
            needs_admin: false,
            recommended: false,
            command: None,
            command_note: None,
            detail: None,
            targets: Vec::new(),
        });
    };

    c(
        "WinSxS（组件存储）",
        "%windir%\\WinSxS",
        "里面绝大多数是硬链接，删了不省空间；微软官方原文：手工删除可能严重损坏系统以至无法启动。要清只能用 DISM。",
    );
    c(
        "pagefile.sys（虚拟内存）",
        "%SystemDrive%\\pagefile.sys",
        "删掉会让程序在内存吃紧时直接崩。确实要小一点，去 系统属性 → 高级 → 性能 → 虚拟内存 调。",
    );
    c(
        "Windows\\Installer",
        "%windir%\\Installer",
        "MSI 安装包缓存。删了以后很多软件无法修复、无法卸载、无法更新——这是网上最常见的「清理教程」失败案例。",
    );
    c(
        "注册表",
        "HKEY_LOCAL_MACHINE",
        "没有任何可靠的「清理注册表」工具。所谓优化注册表，收益接近零，风险是系统起不来。",
    );
    c(
        "共享依赖仓库 .m2 / .nuget",
        "%USERPROFILE%\\.m2、%USERPROFILE%\\.nuget\\packages",
        "Java/.NET 的全局依赖仓库。删了项目还能重新下载，但公司内网或离线环境会直接构建失败。当作只读区对待。",
    );
    c(
        "%APPDATA%\\Roaming",
        "%APPDATA%",
        "程序的数据和配置（浏览器书签、聊天记录、编辑器设置）。占地方但只属于程序自己，清它等于重置那些软件。",
    );
    c(
        "Program Files / ProgramData",
        "%ProgramFiles%",
        "软件本体与它的公共数据。卸载必须走「应用和功能」，手删会留下无法修复的残留。",
    );
    c(
        "System Volume Information / Recovery",
        "%SystemDrive%\\System Volume Information",
        "系统还原与恢复环境。系统目录，需要权限才能碰——那正是它保护自己的方式。",
    );
}

// ------------------------------------------------------------------ 删除

#[derive(Debug, Default)]
pub struct DeleteReport {
    pub freed: u64,
    pub removed: usize,
    pub failed: Vec<String>,
    pub skipped: Vec<String>,
}

/// 只删 `Action::Delete` 的项目。默认走回收站，`permanent=true` 才永久删。
/// 这个路径是不是正被别的进程占着（打开着）。
///
/// 为什么要先问一句：删"正在被使用的文件"在 Windows 上会失败，
/// 更糟的是**部分成功**——目录删了一半，剩下的成了夹生饭。
/// 所以不试删，先探一次：能独占打开就是没人在用。
///
/// 检测方式：以写权限打开。被占用时 Windows 会返回共享冲突，
/// 这里只需要知道"失败且原因是共享冲突"，别的失败（权限、不存在）另论。
pub fn is_locked(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    if path.is_dir() {
        // 目录：能改名就说明没被占用（改名是最轻的探测，不留痕迹）
        let probe = path.with_extension("jingwei-probe");
        if std::fs::rename(path, &probe).is_ok() {
            // 必须改回来：这一步失败的话，用户目录会顶着 `.jingwei-probe` 这个名字
            // 留在原地，而界面上什么都不会说。这不是"清理时的边角"，是改了用户的东西。
            if let Err(e) = std::fs::rename(&probe, path) {
                crate::util::dlog::e(
                    "9003",
                    &format!(
                        "占用探测后目录名没还原：{} 仍叫 {}（{e}）——请手工改回",
                        path.display(),
                        probe.display()
                    ),
                );
            }
            return false;
        }
        return true;
    }
    !open_exclusive_ok(path)
}

/// 能不能**独占**打开这个文件。
///
/// 为什么不能用 `OpenOptions::write(true)` 去试：Rust 的 std 打开文件时
/// 默认带 `FILE_SHARE_READ | FILE_SHARE_WRITE`，于是"再打开一次"永远成功，
/// 探测等于没探（第一版就是这么写错的，被测试当场抓住）。
/// 这里要求独占（dwShareMode = 0）：只要有人拿着任何句柄，就会冲突。
#[cfg(windows)]
fn open_exclusive_ok(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *mut core::ffi::c_void,
            disposition: u32,
            flags: u32,
            template: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn CloseHandle(h: *mut core::ffi::c_void) -> i32;
    }
    const GENERIC_READ: u32 = 0x8000_0000;
    const OPEN_EXISTING: u32 = 3;
    const INVALID: isize = -1;
    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` 是以 NUL 结尾的 UTF-16 路径，在本行归还前一直存活；
    // 其余参数都是值（只读权限、不给共享、无安全属性、无模板句柄）。
    // 「不给共享」正是本函数的意图：拿得到句柄就说明没人占着它。
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            0, // 不给任何共享 —— 有人拿着句柄就一定失败
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if h as isize == INVALID || h.is_null() {
        return false;
    }
    // SAFETY: `h` 是本函数刚打开并判过有效性的句柄，此处只关一次。
    unsafe { CloseHandle(h) };
    true
}

#[cfg(not(windows))]
fn open_exclusive_ok(path: &Path) -> bool {
    std::fs::OpenOptions::new().read(true).write(true).open(path).is_ok()
}

/// 把要删的东西过一遍：被占用的挑出来，不删。
/// 返回（可以删的，被占用的路径）。
pub fn partition_locked(items: &[Item]) -> (Vec<Item>, Vec<String>) {
    let mut ok = Vec::new();
    let mut busy = Vec::new();
    for it in items {
        if is_locked(Path::new(&it.path)) {
            busy.push(it.path.clone());
        } else {
            ok.push(it.clone());
        }
    }
    (ok, busy)
}

pub fn delete(items: &[Item], permanent: bool) -> DeleteReport {
    let mut rep = DeleteReport::default();
    // 每个待删目标单独记住自己那份体积，这样失败时不会把没释放的空间算进去
    let mut batch: Vec<(PathBuf, u64)> = Vec::new();

    for it in items {
        match it.action {
            Action::Delete => {
                let live: Vec<PathBuf> = it.targets.iter().filter(|t| t.exists()).cloned().collect();
                let each = if live.is_empty() {
                    0
                } else {
                    it.size_bytes / live.len() as u64
                };
                for t in live {
                    if !guard(&t) {
                        rep.skipped.push(format!("{}（安全策略拒绝）", t.display()));
                        continue;
                    }
                    // 先问一句"是不是正被占用"。不问的后果很具体：
                    // 目录删到一半失败，留下一个夹生饭，比不删更糟。
                    if is_locked(&t) {
                        rep.skipped.push(format!(
                            "{}（正被别的程序占用，先关掉再清）",
                            t.display()
                        ));
                        continue;
                    }
                    batch.push((t, each));
                }
            }
            Action::Command => rep
                .skipped
                .push(format!("{}（需要自己跑命令）", it.label)),
            Action::Explain => rep.skipped.push(format!("{}（禁止操作）", it.label)),
        }
    }

    if batch.is_empty() {
        return rep;
    }
    crate::util::dlog::i(
        "3001",
        &format!("targets={} permanent={}", batch.len(), permanent),
    );

    if permanent {
        for (p, sz) in &batch {
            let r = if p.is_dir() {
                std::fs::remove_dir_all(p)
            } else {
                std::fs::remove_file(p)
            };
            match r {
                Ok(_) => {
                    rep.removed += 1;
                    rep.freed += sz;
                }
                Err(e) => rep.failed.push(format!("{}（{}）", p.display(), e)),
            }
        }
        return rep;
    }

    // 回收站：先整批一次，失败再逐个，排除被占用/权限问题的那几个
    let paths: Vec<PathBuf> = batch.iter().map(|(p, _)| p.clone()).collect();
    match recycle(&paths) {
        Ok(()) => {
            rep.removed = batch.len();
            rep.freed = batch.iter().map(|(_, s)| *s).sum();
        }
        Err(_) => {
            for (p, sz) in &batch {
                match recycle(std::slice::from_ref(p)) {
                    Ok(()) => {
                        rep.removed += 1;
                        rep.freed += sz;
                    }
                    Err(e) => rep.failed.push(format!("{}（{}）", p.display(), e)),
                }
            }
        }
    }
    rep
}

/// 安全闸：盘根、系统关键目录、.git、.jingwei、用户目录本体，一律不放行。
fn guard(p: &Path) -> bool {
    let s = p.to_string_lossy().to_lowercase();
    let trimmed = s.trim_end_matches(['\\', '/']);
    if trimmed.len() <= 2 {
        return false;
    }
    for comp in p.components() {
        if let Component::Normal(c) = comp {
            let c = c.to_string_lossy().to_lowercase();
            if c == ".git"
                || c == ".jingwei"
                || c == ".drydock"
                || c == ".pylaunch"
                || c == "system32"
            {
                return false;
            }
        }
    }
    for var in [
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "TEMP",
        "TMP",
        "SystemRoot",
        "ProgramFiles",
        "ProgramData",
        "PUBLIC",
        "OneDrive",
    ] {
        if let Some(v) = std::env::var_os(var) {
            let v = v.to_string_lossy().to_lowercase();
            if v.trim_end_matches(['\\', '/']) == trimmed {
                return false;
            }
        }
    }
    true
}

#[cfg(windows)]
fn recycle(paths: &[PathBuf]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct FileOp {
        hwnd: *mut core::ffi::c_void,
        w_func: u32,
        p_from: *const u16,
        p_to: *const u16,
        flags: u16,
        aborted: i32,
        mappings: *mut core::ffi::c_void,
        title: *const u16,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(op: *mut FileOp) -> i32;
    }
    const FO_DELETE: u32 = 3;
    const FOF_ALLOWUNDO: u16 = 0x0040;
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_SILENT: u16 = 0x0004;
    const FOF_NOERRORUI: u16 = 0x0400;

    // pFrom 是「一串以 \0 分隔、最后再来一个 \0」的双零结尾列表
    let mut buf: Vec<u16> = Vec::new();
    for p in paths {
        for w in p.as_os_str().encode_wide() {
            buf.push(w);
        }
        buf.push(0);
    }
    buf.push(0);

    let mut op = FileOp {
        hwnd: std::ptr::null_mut(),
        w_func: FO_DELETE,
        p_from: buf.as_ptr(),
        p_to: std::ptr::null(),
        flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        aborted: 0,
        mappings: std::ptr::null_mut(),
        title: std::ptr::null(),
    };
    // SAFETY: `op` 是栈上的入参结构；它的 `p_from` 指向 `buf`——一个「\0 分隔、
    // 双 \0 结尾」的列表，`buf` 在本次调用期间一直存活，正是这个 API 要求的格式。
    let rc = unsafe { SHFileOperationW(&mut op) };
    if rc != 0 {
        bail!("系统返回 {}", rc);
    }
    if op.aborted != 0 {
        bail!("操作被取消");
    }
    Ok(())
}

#[cfg(not(windows))]
fn recycle(paths: &[PathBuf]) -> Result<()> {
    // 非 Windows 平台没有统一的回收站 API，退化为直接删
    for p in paths {
        if p.is_dir() {
            std::fs::remove_dir_all(p)?;
        } else {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ 工具

fn env_path(var: &str, rel: &str) -> Option<PathBuf> {
    let base = std::env::var_os(var)?;
    let mut p = PathBuf::from(base);
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    Some(p)
}

pub fn dir_stat(path: &Path) -> (u64, usize) {
    let mut bytes = 0u64;
    let mut files = 0usize;
    for e in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if e.file_type().is_file() {
            if let Ok(m) = e.metadata() {
                bytes += m.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    /// 占用检测：一个正开着写句柄的文件必须被认出来。
    /// 否则删除会在中途失败，留下"删了一半"的烂摊子。
    #[test]
    fn a_file_held_open_is_detected_as_locked() {
        let d = std::env::temp_dir().join(format!("jingwei-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("held.txt");
        std::fs::write(&f, "x").unwrap();

        assert!(!is_locked(&f), "没人用的时候不该报占用");

        // 拿住一个独占句柄（Windows 上等价于"正被程序使用"）
        let held = std::fs::OpenOptions::new().write(true).open(&f).unwrap();
        let locked = is_locked(&f);
        drop(held);
        assert!(locked, "有独占句柄时必须认出来");

        assert!(!is_locked(&f), "句柄放开后要回到「没占用」");
        let _ = std::fs::remove_dir_all(&d);
    }


    use super::*;

    #[test]
    fn guard_refuses_drive_roots_and_system_dirs() {
        assert!(!guard(Path::new("C:\\")));
        assert!(!guard(Path::new("D:\\")));
        assert!(!guard(Path::new("C:\\Windows")));
        assert!(!guard(Path::new("C:\\Windows\\System32")));
    }

    #[test]
    fn guard_refuses_user_owned_roots() {
        for var in ["USERPROFILE", "LOCALAPPDATA", "APPDATA", "TEMP"] {
            if let Some(v) = std::env::var_os(var) {
                let p = PathBuf::from(v);
                assert!(!guard(&p), "{:?} 不该被放行", p);
            }
        }
    }

    #[test]
    fn guard_refuses_vcs_and_tool_state() {
        assert!(!guard(Path::new("D:\\proj\\.git\\objects")));
        assert!(!guard(Path::new("D:\\proj\\.jingwei\\snapshots")));
    }

    #[test]
    fn guard_allows_ordinary_build_output() {
        assert!(guard(Path::new("D:\\proj\\build")));
        assert!(guard(Path::new("D:\\proj\\pkg\\__pycache__")));
        assert!(guard(Path::new("D:\\proj\\dist")));
    }

    #[test]
    fn human_size_reads_like_a_person_wrote_it() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024 * 3), "3.0 MB");
    }

    /// TEMP 里的东西是逐个列出来的，安全闸要作用在实际条目上，而不是临时目录本身
    #[test]
    fn guard_allows_items_inside_temp() {
        if let Some(t) = std::env::var_os("TEMP") {
            let p = PathBuf::from(t).join("jingwei-selftest-dir");
            assert!(guard(&p));
        }
    }
}
