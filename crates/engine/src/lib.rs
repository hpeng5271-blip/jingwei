//! engine：Tauri 壳与 CLI 共用的只读门面（唯一分析源 = datacore + collect）。
// view: 卡片取数与渲染投影（cli/壳共用）

use std::fmt;
use std::path::{Path, PathBuf};

use datacore::model::PARSE_RULES_VERSION;
use datacore::store::Store;

pub mod view;

/// 门面错误：只暴露"没有库"与"库打不开"两类，调用方据此给用户可操作提示。
#[derive(Debug)]
pub enum EngineError {
    NoDb(String),
    Db(String),
    Io(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NoDb(p) => write!(f, "no_db: {p} 还没有 .jingwei/jingwei.db（先 jw init/parse）"),
            EngineError::Db(e) => write!(f, "db: {e}"),
            EngineError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<datacore::Error> for EngineError {
    fn from(e: datacore::Error) -> Self {
        EngineError::Db(e.to_string())
    }
}

/// 某文件里的函数 subject 列表（impact 专用；与 cli 旧实现同构）。
fn fns_of_file(store: &Store, file: &str) -> Result<Vec<String>, EngineError> {
    let mut out: Vec<String> = Vec::new();
    for (name, path, kind) in store.symbol_def_index()? {
        if kind == "fn" && path == file {
            out.push(format!("symbol:{path}#{name}"));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// §18.4 弱仓体检：返回 Some(skip) 表示当前仓库不足以给可靠 permalink。
fn weak_repo_skip(root: &Path) -> Option<&'static str> {
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    };
    let inside = git(&["rev-parse", "--is-inside-work-tree"]);
    let shallow = git(&["rev-parse", "--is-shallow-repository"]);
    let head = git(&["rev-parse", "--verify", "HEAD"]);
    let sym = git(&["symbolic-ref", "-q", "HEAD"]);
    if inside.is_none() {
        return Some("no_git");
    }
    if shallow.as_deref() == Some("true") {
        return Some("shallow_clone");
    }
    if head.is_none() {
        return Some("no_commit");
    }
    if sym.is_none() {
        return Some("detached");
    }
    None
}

/// 去掉 Windows canonicalize 的 `\\?\` 前缀（UNC 的 `\\?\UNC\` 还原成 `\\`）。
fn clean_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    if let Some(r) = s.strip_prefix("\\\\?\\UNC\\") {
        return format!("\\\\{r}");
    }
    s.strip_prefix("\\\\?\\").map(|r| r.to_string()).unwrap_or(s)
}


/// 根目录一层的 kinds（按扩展名，不递归）。
fn root_kinds(dir: &Path) -> Vec<String> {
    let map = [
        ("rs", "rust"),
        ("ts", "ts"),
        ("tsx", "tsx"),
        ("js", "js"),
        ("py", "py"),
        ("go", "go"),
    ];
    let mut out: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                if let Some((_, k)) = map.iter().find(|(e, _)| *e == ext) {
                    if !out.iter().any(|x| x == k) {
                        out.push((*k).to_string());
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// 递归数源文件（`.rs/.ts/.tsx/.js/.py/.go`），上限 2000。
fn count_sources(dir: &Path, cap: usize) -> (usize, bool) {
    let exts = ["rs", "ts", "tsx", "js", "py", "go"];
    let mut n = 0usize;
    let mut truncated = false;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let nm = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if ["node_modules", "target", ".git", "dist"].contains(&nm.as_str()) {
                    continue;
                }
                stack.push(p);
            } else if p.is_file() {
                if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                    if exts.contains(&ext) {
                        n += 1;
                        if n >= cap {
                            truncated = true;
                            return (n, truncated);
                        }
                    }
                }
            }
        }
    }
    (n, truncated)
}


/// 全量递归量目录体积（无 20k/300ms 上限，与旧 clean.rs dir_stat 一致）。
fn dir_stat(dir: &std::path::Path) -> u64 {
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() {
                bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    bytes
}

/// 人看的体积（与旧壳同口径：一位小数 + 单位）。
fn human_bytes(b: u64) -> String {
    const KB: f64 = 1024.0;
    let f = b as f64;
    if f >= KB * KB * KB {
        format!("{:.1} GB", f / (KB * KB * KB))
    } else if f >= KB * KB {
        format!("{:.1} MB", f / (KB * KB))
    } else if f >= KB {
        format!("{:.1} KB", f / KB)
    } else {
        format!("{b} B")
    }
}

/// 全局缓存目录规格：`var`/`rel` 供发现层拼路径，其余供枚举层填 Item。
#[derive(Clone, Copy)]
pub struct CacheSpec {
    pub var: &'static str, pub rel: &'static str,
    pub label: &'static str, pub reason: &'static str, pub consequence: &'static str,
    pub recommended: bool, pub command: Option<&'static str>, pub command_note: Option<&'static str>,
    pub action: &'static str,
}

/// 全局缓存主体（刀1：Python/Node/Go/Rust/JVM，11 条）；尾 11 条 + zone B/C 后续刀补。
pub const CACHES_MAIN: &[CacheSpec] = &[
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "pip/cache", label: "pip 下载缓存", reason: "pip 下载过的 wheel/源码缓存", consequence: "下次装同名包要重新下载（有网就行）", recommended: true, command: Some("python -m pip cache purge"), command_note: None },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "uv/cache", label: "uv 包缓存", reason: "uv 的解析结果与包归档缓存", consequence: "已装的虚拟环境照常能用；新建环境要重新下载", recommended: true, command: Some("uv cache clean"), command_note: None },
    CacheSpec { action: "command", var: "USERPROFILE", rel: ".conda/pkgs", label: "conda 包缓存", reason: "conda 解压过的包副本；已建环境不依赖它", consequence: "以后 conda install 会重新下载", recommended: false, command: Some("conda clean --all --dry-run"), command_note: Some("确认后去掉 --dry-run") },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "Python/_cache", label: "运行时下载缓存（安装管理器）", reason: "Python 安装管理器下载过的运行时归档；装好的版本不依赖它", consequence: "装好的运行时不依赖它；下次 python install 新版本要重新下载", recommended: false, command: Some("py --list"), command_note: Some("先看这台机器装了哪些版本，再决定删不删缓存") },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "npm-cache", label: "npm 缓存", reason: "npm 下载过的 tarball", consequence: "npm install 重新下载；node_modules 不受影响", recommended: true, command: Some("npm cache clean --force"), command_note: None },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "Yarn/Cache", label: "Yarn 缓存", reason: "Yarn 的包缓存", consequence: "重新 yarn install 时会再下载", recommended: true, command: Some("yarn cache clean"), command_note: None },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "pnpm-store", label: "pnpm store", reason: "pnpm 的内容寻址仓库", consequence: "现有项目的 node_modules 是硬链接，删了可能让它们失效；要清用 pnpm store prune", recommended: false, command: Some("pnpm store prune"), command_note: None },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "go-build", label: "go build 缓存", reason: "编译中间产物缓存", consequence: "下次 go build/go test 慢一些", recommended: true, command: Some("go clean -cache"), command_note: None },
    CacheSpec { action: "delete", var: "USERPROFILE", rel: "go/pkg/mod", label: "go 模块缓存", reason: "go mod 下载的依赖源码", consequence: "断网时没法再 build；有网就自动重下", recommended: false, command: Some("go clean -modcache"), command_note: None },
    CacheSpec { action: "delete", var: "USERPROFILE", rel: ".cargo/registry", label: "cargo registry", reason: "crates.io 下载的包与索引", consequence: "下次 cargo build 要把依赖全下一遍（这块经常几个 GB）", recommended: false, command: None, command_note: None },
    CacheSpec { action: "delete", var: "USERPROFILE", rel: ".gradle/caches", label: "Gradle 依赖缓存", reason: "Gradle 下载的依赖与 daemon 缓存", consequence: "下次构建需重新下载；已 checkout 的项目要重跑配置", recommended: false, command: None, command_note: None },
];

/// 全局缓存尾（刀2：VS Code 5 / 浏览器 2 / 崩溃转储 1，8 条目录型）。
pub const CACHES_TAIL: &[CacheSpec] = &[
    CacheSpec { action: "delete", var: "APPDATA", rel: "Code/Cache", label: "Code Cache", reason: "编辑器自己的缓存与日志，跟你的代码无关", consequence: "VS Code 首次打开稍慢；设置和插件都不受影响", recommended: true, command: None, command_note: None },
    CacheSpec { action: "delete", var: "APPDATA", rel: "Code/CachedData", label: "CachedData", reason: "编辑器自己的缓存与日志，跟你的代码无关", consequence: "VS Code 首次打开稍慢；设置和插件都不受影响", recommended: true, command: None, command_note: None },
    CacheSpec { action: "delete", var: "APPDATA", rel: "Code/GPUCache", label: "GPUCache", reason: "编辑器自己的缓存与日志，跟你的代码无关", consequence: "VS Code 首次打开稍慢；设置和插件都不受影响", recommended: true, command: None, command_note: None },
    CacheSpec { action: "delete", var: "APPDATA", rel: "Code/Code Cache", label: "Code Cache(old)", reason: "编辑器自己的缓存与日志，跟你的代码无关", consequence: "VS Code 首次打开稍慢；设置和插件都不受影响", recommended: true, command: None, command_note: None },
    CacheSpec { action: "delete", var: "APPDATA", rel: "Code/logs", label: "日志", reason: "编辑器自己的缓存与日志，跟你的代码无关", consequence: "VS Code 首次打开稍慢；设置和插件都不受影响", recommended: true, command: None, command_note: None },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "Microsoft/Edge/User Data/Default/Cache", label: "Edge 缓存", reason: "网页缓存", consequence: "首次打开常用网站慢一点；登录状态、密码不受影响", recommended: false, command: None, command_note: None },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "Google/Chrome/User Data/Default/Cache", label: "Chrome 缓存", reason: "网页缓存", consequence: "首次打开常用网站慢一点；登录状态、密码不受影响", recommended: false, command: None, command_note: None },
    CacheSpec { action: "delete", var: "LOCALAPPDATA", rel: "CrashDumps", label: "崩溃转储", reason: "程序崩溃时留下的内存快照", consequence: "除非你正在查某个崩溃，否则没有任何用", recommended: true, command: None, command_note: None },
];

/// env 变量 + 相对路径（`/` 分段 push → `\` 分隔绝对路径，与旧 env_path 一致）。
fn env_path(var: &str, rel: &str) -> Option<PathBuf> {
    let base = std::env::var_os(var)?;
    let mut p = PathBuf::from(base);
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    Some(p)
}

/// 发现层（薄，不单测）：env 查找 → 已解析路径，目录不存在就跳过。
fn discover_caches(specs: &[CacheSpec]) -> Vec<(&CacheSpec, PathBuf)> {
    specs
        .iter()
        .filter_map(|s| env_path(s.var, s.rel).map(|p| (s, p)))
        .filter(|(_, p)| p.is_dir())
        .collect()
}

/// 枚举层（hermetic，可单测）：显式路径逐项量体积；空目录跳过，超上限（20k 文件/300ms）显式标 `truncated`。
pub fn enumerate_global_caches(roots: &[(&CacheSpec, PathBuf)]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for (spec, path) in roots {
        let bytes = dir_stat(path);
        if bytes == 0 {
            continue;
        }
        let mut item = serde_json::json!({"path": path.to_string_lossy().to_string(), "label": spec.label, "action": spec.action, "zone": "a", "recommended": spec.recommended, "size_bytes": bytes, "reason": spec.reason, "consequence": spec.consequence});
        if let Some(c) = spec.command { item["command"] = serde_json::json!(c); }
        if let Some(c) = spec.command_note { item["command_note"] = serde_json::json!(c); }
        out.push(item);
    }
    out
}

/// 缩略图/图标缓存（hermetic）：Explorer 目录里 thumbcache_*/iconcache_* 文件，按文件量不是目录。
pub fn scan_thumbcache(dir: &Path) -> Option<serde_json::Value> {
    let mut bytes = 0u64;
    let Ok(rd) = std::fs::read_dir(dir) else { return None };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.starts_with("thumbcache_") || n.starts_with("iconcache_") {
            bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    if bytes == 0 {
        return None;
    }
    Some(serde_json::json!({"path": dir.to_string_lossy().to_string(), "label": "缩略图 / 图标缓存", "action": "delete", "zone": "a", "recommended": true, "size_bytes": bytes, "reason": "资源管理器为图片/视频/图标生成的预览缓存", "consequence": "删除后资源管理器会自动重建；只是某些文件夹首次打开时图标要重算，不影响文件本身"}))
}

/// 回收站：系统托管、工具不直接删，给命令（固定项，action=command，不进 zoneA 字节合计）。
pub fn recycle_bin_item() -> serde_json::Value {
    serde_json::json!({"path": "所有驱动器", "label": "回收站", "action": "command", "zone": "a", "recommended": false, "size_bytes": 0, "reason": "已经删过一次、但还占着盘的东西", "consequence": "清空后无法还原。确认里面没有你要找回的文件再清", "command": "Clear-RecycleBin -Force", "command_note": "在 PowerShell 里跑"})
}

/// 临时文件（hermetic）：目录里 7 天以上没动过的条目；够老才列，不是全部 TEMP。
pub fn scan_temp(dir: &Path) -> Option<serde_json::Value> {
    let Ok(rd) = std::fs::read_dir(dir) else { return None };
    let now = std::time::SystemTime::now();
    let cutoff = std::time::Duration::from_secs(7 * 24 * 3600);
    let mut bytes = 0u64;
    for e in rd.flatten() {
        let p = e.path();
        let Ok(m) = e.metadata() else { continue };
        let t = m.modified().or_else(|_| m.created()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if now.duration_since(t).unwrap_or_default() < cutoff {
            continue;
        }
        bytes += if p.is_dir() { dir_stat(&p) } else { m.len() };
    }
    if bytes == 0 {
        return None;
    }
    Some(serde_json::json!({"path": dir.to_string_lossy().to_string(), "label": "临时文件（7 天以上没动过）", "action": "delete", "zone": "a", "recommended": true, "size_bytes": bytes, "reason": "程序用完就丢的中间文件；7 天没碰过的基本没人要了", "consequence": "正在运行的程序占用的会被自动跳过；关掉再清更干净"}))
}

/// 项目垃圾（hermetic，对齐旧 clean.rs 项目垃圾）：junk 后缀文件 + 日志≥1MB + 空目录 + 未用 venv。
pub fn scan_project_junk(root: &Path) -> Vec<serde_json::Value> {
    const JUNK: &[(&str, &str)] = &[
        (".bak", "*.bak · 备份文件"), (".old", "*.old · 旧版本副本"), (".orig", "*.orig · 打补丁留下的原件"),
        (".rej", "*.rej · 没打上的补丁片段"), (".tmp", "*.tmp · 临时文件"), (".temp", "*.temp · 临时文件"),
        (".swp", "*.swp · 编辑器崩溃残留"), (".swo", "*.swo · 编辑器崩溃残留"), (".save", "*.save · 编辑器崩溃残留"),
        ("~", "*~ · 编辑器自动备份"),
    ];
    let mut out = Vec::new();
    let mut groups: std::collections::BTreeMap<&'static str, (u64, usize)> = Default::default();
    let mut logs: Vec<u64> = Vec::new();
    let mut empties: Vec<String> = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    let mut depth_of: std::collections::HashMap<std::path::PathBuf, u32> = Default::default();
    depth_of.insert(root.to_path_buf(), 0);
    while let Some(dir) = stack.pop() {
        let d = *depth_of.get(&dir).unwrap_or(&0);
        if d >= 6 { continue; }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            let n = e.file_name().to_string_lossy().to_lowercase();
            if ft.is_dir() {
                let sk = matches!(n.as_str(), "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".tox" | ".ipynb_checkpoints" | "htmlcov" | "build" | "dist" | ".git" | "node_modules" | "target" | ".venv" | "venv" | "env" | ".idea" | ".vs" | ".jingwei" | ".drydock" | ".pylaunch") || n.ends_with(".egg-info");
                if sk { continue; }
                if p != root && std::fs::read_dir(&p).map(|mut r| r.next().is_none()).unwrap_or(false) {
                    empties.push(p.to_string_lossy().to_string());
                }
                depth_of.insert(p.clone(), d + 1);
                stack.push(p);
            } else if ft.is_file() {
                let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
                if let Some((_, label)) = JUNK.iter().find(|(s, _)| n.ends_with(s)) {
                    let g = groups.entry(label).or_insert((0, 0)); g.0 += bytes; g.1 += 1;
                } else if n.contains(" - 副本") || n.contains(" - copy") {
                    let g = groups.entry("某某 - 副本 · 手抄的复制品").or_insert((0, 0)); g.0 += bytes; g.1 += 1;
                } else if n.ends_with(".log") && bytes >= 1024 * 1024 {
                    logs.push(bytes);
                }
            }
        }
    }
    for (label, (bytes, files)) in groups {
        out.push(serde_json::json!({"path": root.to_string_lossy().to_string(), "label": format!("{label}（{files} 个）"), "action": "delete", "zone": "a", "recommended": true, "size_bytes": bytes, "reason": "编辑器和补丁工具留下的中间产物，程序运行时不会去读它", "consequence": "不影响运行。万一是你要的备份，回收站里还能捞回来"}));
    }
    if !logs.is_empty() {
        let bytes: u64 = logs.iter().sum();
        out.push(serde_json::json!({"path": root.to_string_lossy().to_string(), "label": format!("日志文件（{} 个，单个 ≥ 1 MB）", logs.len()), "action": "delete", "zone": "a", "recommended": bytes >= 5 * 1024 * 1024, "size_bytes": bytes, "reason": "运行日志会一直追加，放久了很占地方；它只对「排查正在发生的问题」有用", "consequence": "删了不影响程序。但如果此刻正在查一个 bug，先别删——那是线索"}));
    }
    if !empties.is_empty() {
        out.push(serde_json::json!({"path": root.to_string_lossy().to_string(), "label": format!("空目录（{} 个）", empties.len()), "action": "delete", "zone": "a", "recommended": false, "size_bytes": 0, "reason": "里面一个文件都没有。多半是删文件时留下的壳，或者某次失败的复制", "consequence": "有些程序要求目录先存在（日志目录、上传目录），删了它可能自己重建、也可能报错"}));
    }
    out.extend(scan_unused_venvs(root));
    out
}

/// 未用 venv：名字里写着备份/副本，或契约绑的是另一个解释器（hermetic，显式 root）。
pub fn scan_unused_venvs(root: &Path) -> Vec<serde_json::Value> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let bound = std::fs::read_to_string(root.join("env.contract.json")).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["runtime"]["python_exe"].as_str().map(|s| s.to_lowercase()));
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if !p.join("pyvenv.cfg").is_file() { continue; }
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        let is_copy = lower.contains(".bak") || lower.contains("-bak") || lower.contains("backup") || lower.contains("副本") || lower.contains("-copy");
        let (reason, consequence) = if is_copy {
            ("名字里就写着备份/副本：虚拟环境里的路径是写死的，复制出来的那份多数时候根本跑不起来", "删了它，你原来那个环境一点不受影响。真要留备份，留 requirements.txt 就够了")
        } else {
            let Some(b) = &bound else { continue };
            if b.contains(&lower) { continue; }
            ("项目契约绑定的是另一个虚拟环境，这一个没被任何配置指向", "删掉不影响现在能跑的项目；以后要用它得重新建、重新装依赖")
        };
        let bytes = dir_stat(&p);
        out.push(serde_json::json!({"path": p.to_string_lossy().to_string(), "label": format!("没在用的虚拟环境：{name}"), "action": "delete", "zone": "a", "recommended": true, "size_bytes": bytes, "reason": reason, "consequence": consequence}));
    }
    out
}

/// zone B 管理员项（给命令不执行，不进 zoneA 合计）：7 条固定。真目录/文件才量体积，字面量路径 0。
pub fn admin_items() -> Vec<serde_json::Value> {
    let windir = std::env::var_os("SystemRoot").map(PathBuf::from);
    let drive = windir.as_ref().and_then(|w| w.parent().map(PathBuf::from));
    let mut items = Vec::new();
    let mut push = |label: &str, path: PathBuf, reason: &str, consequence: &str, recommended: bool, command: &str| {
        let size = if path.is_dir() {
            dir_stat(&path)
        } else if path.exists() {
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        items.push(serde_json::json!({"path": path.to_string_lossy().to_string(), "label": label, "action": "command", "zone": "b", "recommended": recommended, "size_bytes": size, "reason": reason, "consequence": consequence, "command": command, "command_note": "在「管理员 PowerShell」里跑"}));
    };
    if let Some(w) = windir.as_ref() {
        push("Windows 更新下载缓存", w.join("SoftwareDistribution").join("Download"), "系统更新下载完就留着不删的安装包", "下次更新会重新下载；不会影响已装的更新", true, r#"Stop-Service wuauserv,bits -Force -ErrorAction SilentlyContinue; Remove-Item "$env:windir\SoftwareDistribution\Download\*" -Recurse -Force -ErrorAction SilentlyContinue; Start-Service bits,wuauserv -ErrorAction SilentlyContinue"#);
        if let Some(d) = drive.as_ref() {
            let old = d.join("Windows.old");
            if old.is_dir() {
                push("以前的 Windows 安装", old, "大版本升级后留下的旧系统备份", "删掉就再也退不回上一个 Windows 版本了；升级完用了半个月没事就可以删", true, r#"Remove-Item "$env:SystemDrive\Windows.old" -Recurse -Force"#);
            }
            let hib = d.join("hiberfil.sys");
            if hib.exists() {
                push("休眠文件 hiberfil.sys", hib, "休眠/快速启动用的内存镜像，大小通常等于内存的一半以上", "会失去休眠和快速启动（开机能慢几秒）；想恢复跑 powercfg /h on", true, "powercfg /h off");
            }
        }
    }
    push("系统还原点 / 卷影副本", PathBuf::from("%SystemDrive%\\System Volume Information"), "系统为还原点保存的历史快照，随时间越积越多", "删掉后无法用「系统还原」回滚到更早的时间点；保留最近一两个才划算", false, "vssadmin list shadowstorage");
    push("组件存储精简（WinSxS 的合法清理方式）", PathBuf::from("%windir%\\WinSxS"), "Windows 官方提供的组件清理通道；它会顺手清掉被取代的旧组件", "加了 /ResetBase 之后就装不回已经装过的旧更新了，别的没有影响", true, "Dism /Online /Cleanup-Image /AnalyzeComponentStore");
    push("第三方驱动包（旧版本）", PathBuf::from("%windir%\\System32\\DriverStore\\FileRepository"), "装驱动时留下的旧版本，设备管理里看不见但占地方", "删错会让某个硬件失效；只删标记为「旧版本」的，并且删之前记下名字", false, "pnputil /enum-drivers");
    push("磁盘清理向导", PathBuf::from("cleanmgr"), "Windows 自带的清理入口，比手工删安全得多", "按你勾选的项清理；不确定的项别勾", true, "cleanmgr /d C:");
    items
}

/// zone C 别碰 8 条（只解释不操作）：action=none，不进任何合计。
pub fn offlimits_items() -> Vec<serde_json::Value> {
    const C: &[(&str, &str, &str)] = &[
        ("WinSxS（组件存储）", "%windir%\\WinSxS", "里面绝大多数是硬链接，删了不省空间；微软官方原文：手工删除可能严重损坏系统以至无法启动。要清只能用 DISM。"),
        ("pagefile.sys（虚拟内存）", "%SystemDrive%\\pagefile.sys", "删掉会让程序在内存吃紧时直接崩。确实要小一点，去 系统属性 → 高级 → 性能 → 虚拟内存 调。"),
        ("Windows\\Installer", "%windir%\\Installer", "MSI 安装包缓存。删了以后很多软件无法修复、无法卸载、无法更新——这是网上最常见的「清理教程」失败案例。"),
        ("注册表", "HKEY_LOCAL_MACHINE", "没有任何可靠的「清理注册表」工具。所谓优化注册表，收益接近零，风险是系统起不来。"),
        ("共享依赖仓库 .m2 / .nuget", "%USERPROFILE%\\.m2、%USERPROFILE%\\.nuget\\packages", "Java/.NET 的全局依赖仓库。删了项目还能重新下载，但公司内网或离线环境会直接构建失败。当作只读区对待。"),
        ("%APPDATA%\\Roaming", "%APPDATA%", "程序的数据和配置（浏览器书签、聊天记录、编辑器设置）。占地方但只属于程序自己，清它等于重置那些软件。"),
        ("Program Files / ProgramData", "%ProgramFiles%", "软件本体与它的公共数据。卸载必须走「应用和功能」，手删会留下无法修复的残留。"),
        ("System Volume Information / Recovery", "%SystemDrive%\\System Volume Information", "系统还原与恢复环境。系统目录，需要权限才能碰——那正是它保护自己的方式。"),
    ];
    C.iter().map(|(label, path, consequence)| serde_json::json!({"path": *path, "label": *label, "action": "explain", "zone": "c", "recommended": false, "size_bytes": 0, "reason": "", "consequence": *consequence})).collect()
}

/// 解析一篇笔记（与旧壳 `notes::parse` 同规则；只认 frontmatter 的 5 个键）。
fn parse_note(text: &str) -> (String, String, String, String, Vec<String>, String) {
    let mut kind = String::new();
    let mut title = String::new();
    let mut created = String::new();
    let mut updated = String::new();
    let mut tags: Vec<String> = Vec::new();
    let mut body = text.to_string();
    if let Some(after) = text.strip_prefix("---") {
        if let Some(end) = after.find("\n---") {
            let head = &after[..end];
            body = after[end + 4..].trim_start_matches('\n').to_string();
            for line in head.lines() {
                let Some((k, v)) = line.split_once(':') else { continue };
                let v = v.trim();
                match k.trim() {
                    "kind" => kind = v.to_string(),
                    "title" => title = v.to_string(),
                    "created" => created = v.to_string(),
                    "updated" => updated = v.to_string(),
                    "tags" => {
                        tags = v
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    }
                    _ => {}
                }
            }
        }
    }
    (kind, title, created, updated, tags, body.trim().to_string())
}

/// `Some(_)` → true（has_test / has_install 这类"有没有"的判据）。
fn v_has(v: &Option<String>) -> bool {
    v.is_some()
}

/// 旧 NOT_SOURCE_DIRS（util/scan.rs，15 项）+ `.` 开头（调用方另判）。
fn is_not_source_dir(name: &str) -> bool {
    let n = name.to_lowercase();
    matches!(
        n.as_str(),
        ".git" | ".jingwei" | ".drydock" | ".pylaunch" | ".idea" | ".vscode"
            | "node_modules" | "target" | "build" | "out" | "dist"
            | ".venv" | "venv" | "env" | "__pycache__"
    )
}

/// 工具链探测（I2）：8 项工具逐个跑一次 `--version` 定性。
///
/// 纪律：**只读**——不改 PATH、不装组件、不写任何配置文件。
/// 名字/参数沿用旧壳那张表（`detect/probe.rs:354-363`），判定交给 `collect::env`。
/// Docker daemon 探活（npipe 不是 TCP）：`docker info` 成功 = daemon 在。
fn docker_daemon_reachable() -> Option<bool> {
    match std::process::Command::new("docker").arg("info").output() {
        Ok(o) => Some(o.status.success()),
        Err(_) => None,
    }
}

pub fn env_probe_value() -> serde_json::Value {
    const TOOLS: &[(&str, &str, &[&str])] = &[
        ("node", "node.exe", &["-v"]),
        ("npm", "npm.cmd", &["-v"]),
        ("java", "java.exe", &["-version"]),
        ("go", "go.exe", &["version"]),
        ("rustc", "rustc.exe", &["--version"]),
        ("gcc", "gcc.exe", &["--version"]),
        ("git", "git.exe", &["--version"]),
        ("uv", "uv.exe", &["--version"]),
    ];
    let shims = shim_dirs();
    let commons = common_dirs();
    let mut tools: Vec<serde_json::Value> = Vec::new();
    for (name, exe, args) in TOOLS {
        let p = collect::env::probe_tool(name, exe, args, None, &shims, &commons);
        tools.push(serde_json::json!({
            "name": p.name,
            "status": p.status.as_str(),
            "version": p.version,
            "path": p.path.map(|x| x.to_string_lossy().to_string()),
            "source": p.source.map(|s| s.as_str()),
            "repro": p.repro,
        }));
    }
    let generated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 外置服务（D4：只探测不启停）
    let mut services: Vec<serde_json::Value> = Vec::new();
    for (kind, exe, host, port) in [
        ("docker", "docker.exe", "", None::<u16>),
        ("redis", "redis-cli.exe", "localhost", Some(6379)),
        ("postgresql", "psql.exe", "localhost", Some(5432)),
        ("mysql", "mysql.exe", "localhost", Some(3306)),
        ("mongodb", "mongosh.exe", "localhost", Some(27017)),
        ("sqlite", "sqlite3.exe", "", None),
    ] {
        let mut p = collect::env::probe_service(kind, exe, host, port, &shims, &commons);
        // Docker daemon 走 npipe 不是 TCP：客户端在就用 `docker info` 探
        if kind == "docker" && p.status == collect::env::Status::Ok {
            p.reachable = docker_daemon_reachable();
        }
        services.push(serde_json::json!({
            "kind": p.kind,
            "status": p.status.as_str(),
            "version": p.version,
            "host": host,
            "port": port,
            "reachable": p.reachable,
            "repro": p.repro,
        }));
    }
    serde_json::json!({
        "schema": 1,
        "generated_at": generated_at,
        "tools": tools,
        "services": services,
    })
}

/// 版本管理器 shim 目录（存在与否都列，候选层自己判 is_file）。
fn shim_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let h = PathBuf::from(home);
        out.push(h.join(".cargo").join("bin"));
        out.push(h.join(".pyenv").join("pyenv-win").join("bin"));
        out.push(h.join(".pyenv").join("pyenv-win").join("shims"));
        out.push(h.join("scoop").join("shims"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        out.push(PathBuf::from(appdata).join("nvm"));
    }
    out
}

/// 常见安装目录。
fn common_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        let p = PathBuf::from(pf);
        out.push(p.join("Git").join("cmd"));
        out.push(p.join("nodejs"));
    }
    if let Some(pf86) = std::env::var_os("ProgramFiles(x86)") {
        out.push(PathBuf::from(pf86).join("Git").join("cmd"));
    }
    out
}

/// 不依赖 store 的目录扫描（列表页用；`Engine::scan` 也走它）。
pub fn scan_dir(parent_path: &Path, depth: u32, limit: usize) -> Result<serde_json::Value, EngineError> {
    let parent_path = parent_path.to_path_buf();
    if !parent_path.is_dir() {
        return Err(EngineError::Io(format!("不是目录：{}", parent_path.display())));
    }
    let depth = depth.clamp(1, 6);
    let limit = limit.clamp(1, 500);
    let mut visited: std::collections::HashSet<PathBuf> = Default::default();
    let mut scanned: usize = 0;
    let mut found: Vec<(PathBuf, u32)> = Vec::new();
    let root_canon = std::fs::canonicalize(&parent_path).unwrap_or_else(|_| parent_path.clone());
    let mut stack: Vec<(PathBuf, u32)> = vec![(parent_path.clone(), 0)];
    while let Some((dir, d)) = stack.pop() {
        let canon = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !visited.insert(canon) {
            continue;
        }
        if std::fs::read_dir(&dir).is_err() {
            continue;
        }
        scanned += 1;
        let (markers, _kind, has_git, has_contract, _has_index) = probe_project(&dir);
        if (!markers.is_empty() || has_git || has_contract) && dir != root_canon && dir != parent_path {
            found.push((dir.clone(), d));
            continue; // 遇项目不下钻
        }
        if d < depth {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let Ok(ft) = e.file_type() else { continue };
                    if !ft.is_dir() || ft.is_symlink() {
                        continue;
                    }
                    let nm = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    if nm.starts_with('.') || is_not_source_dir(&nm) {
                        continue;
                    }
                    stack.push((p, d + 1));
                }
            }
        }
    }
    // top_level：祖先里有项目就不算
    let set: std::collections::HashSet<PathBuf> = found.iter().map(|(p, _)| p.clone()).collect();
    let mut items: Vec<serde_json::Value> = Vec::new();
    for (p, _d) in &found {
        let covered = p
            .ancestors()
            .skip(1)
            .any(|a| a != root_canon && set.contains(a));
        if covered {
            continue;
        }
        let (markers, kind, has_git, has_contract, has_index) = probe_project(p);
        let (files, files_truncated) = count_sources(p, 2000);
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        // 卡片上的「最近改动 N 天前」：scan 必须给 touched/age_days，
        // 否则前端只能显示 "undefined 天前"（换芯时踩过）。
        let touched = std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mt| mt.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(touched);
        let age_days = if touched == 0 { serde_json::Value::Null } else {
            serde_json::json!(now_s.saturating_sub(touched) / 86_400)
        };
        items.push(serde_json::json!({
            "name": name,
            "path": p.to_string_lossy(),
            "touched": touched,
            "age_days": age_days,
            "markers": markers,
            "kind": kind,
            "kinds": root_kinds(p),
            "has_git": has_git,
            "has_contract": has_contract,
            "has_index": has_index,
            "files": files,
            "files_truncated": files_truncated,
        }));
        if items.len() >= limit {
            break;
        }
    }
    items.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    // §4.4 承载力：**每个特色返回都必须带 scope 三件套**，截断要显式（为什么截、怎么继续）。
    let n_items = items.len();
    let truncated = n_items >= limit;
    let files_considered: u64 = items
        .iter()
        .map(|it| it["files"].as_u64().unwrap_or(0))
        .sum();
    // 父目录里 ≥2 个项目 → multi；1 个 → project；0 个 → 也算 multi（还没选到项目）
    let scope = if n_items <= 1 { "project" } else { "multi" };
    let next_cursor = if truncated {
        serde_json::json!(n_items as u64)
    } else {
        serde_json::Value::Null
    };
    Ok(serde_json::json!({
        "parent": parent_path.to_string_lossy(),
        "depth": depth,
        "scanned_dirs": scanned,
        "scope": scope,
        "files_considered": files_considered,
        "truncated": truncated,
        "next_cursor": next_cursor,
        "residue": residue_at(&parent_path),
        "items": items,
    }))
}

/// §4.5 残渣 dry-run：**只数不删**。三类最多、最容易长出来的（派生数据）。
/// 统计范围是父目录一层：扫整棵树去数 target 会把这一刀变成"再走一遍盘"。
pub fn residue_at(parent: &Path) -> serde_json::Value {
    let mut jingwei = 0u64;
    let mut targets = 0u64;
    let mut caches = 0u64;
    if let Ok(rd) = std::fs::read_dir(parent) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir {
                continue;
            }
            match name.as_str() {
                ".jingwei" => jingwei += 1,
                "target" => targets += 1,
                "node_modules" | "__pycache__" | "dist" => caches += 1,
                _ => {}
            }
        }
    }
    serde_json::json!({
        "jingwei_dirs": jingwei,
        "target_dirs": targets,
        "cache_dirs": caches,
        "apply": false,
    })
}

/// 旧 `project::manifest_kind`（18 种）。
fn manifest_kind(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let k = match n.as_str() {
        "pyproject.toml" | "setup.py" | "pipfile" => "python",
        "requirements.txt" | "requirements-dev.txt" | "requirements-test.txt" => "python",
        "package.json" => "node",
        "pom.xml" => "java",
        "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" => "gradle",
        "cargo.toml" => "rust",
        "go.mod" => "go",
        "cmakelists.txt" => "cc",
        "makefile" => "make",
        "composer.json" => "php",
        "gemfile" => "ruby",
        _ => {
            if n.ends_with(".csproj") || n.ends_with(".fsproj") || n.ends_with(".sln") {
                "dotnet"
            } else {
                return None;
            }
        }
    };
    Some(k)
}

/// 探测一个目录：markers/kind/has_git/has_contract/has_index。
fn probe_project(dir: &Path) -> (Vec<String>, String, bool, bool, bool) {
    let mut markers: Vec<String> = Vec::new();
    let mut kind = String::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let nm = e.file_name().to_string_lossy().to_string();
            if let Some(k) = manifest_kind(&nm) {
                if kind.is_empty() {
                    kind = k.to_string();
                }
                markers.push(nm);
            }
        }
    }
    markers.sort();
    let has_git = dir.join(".git").exists();
    let has_contract = dir.join("env.contract.json").exists();
    let has_index = dir.join(".jingwei").exists();
    (markers, kind, has_git, has_contract, has_index)
}

/// 只读门面：持有仓库根与已打开的 store。
pub struct Engine {
    root: PathBuf,
    store: Store,
}

impl Engine {
    /// 打开已有索引；**不建库**（无库返回 `NoDb`）。
    pub fn open(root: impl AsRef<Path>) -> Result<Self, EngineError> {
        let root = root.as_ref().to_path_buf();
        let db = root.join(".jingwei").join("jingwei.db");
        if !db.is_file() {
            return Err(EngineError::NoDb(root.to_string_lossy().to_string()));
        }
        let store = Store::open_existing(&root).map_err(|e| EngineError::Db(e.to_string()))?;
        Ok(Self { root, store })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    /// Rust 模块路径表：直接转调 collect（不复制逻辑）。
    pub fn module_map(&self) -> std::collections::HashMap<String, String> {
        collect::parse::module_map(&self.root)
    }

    /// 只读概览：与 serve 的 `stats` result 内层同构（不含 v/id/ok 信封）。
    /// rules 取编译期常量（与 cli 的 PARSE_RULES_VERSION 同步，T2.4 不改 CLI）。
    pub fn stats(&self) -> Result<serde_json::Value, EngineError> {
        let db = |e: datacore::Error| EngineError::Db(e.to_string());
        // files = 当前树可分析文件（与 parse 同一真相）；不用 distinct_change_paths（那是含已删文件的历史数）
        let files = collect::parse::source_files(&self.root).len() as i64;
        // symbols = DB 里有判定的符号数，不是源码符号总数（壳若要源码总数另立字段）
        let symbols = self
            .store
            .kind_counts()
            .map_err(db)?
            .into_iter()
            .find(|(k, _)| k == "symbol")
            .map(|(_, n)| n)
            .unwrap_or(0);
        let edges = self.store.count("edge").map_err(db)?;
        // write_seq 缺失 = 0：open 已拦 NoDb，能到这说明库存在，没写只意味"还没写过"
        let write_seq = self
            .store
            .get_meta("write_seq")
            .map_err(db)?
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        Ok(serde_json::json!({
            "source": "db",
            "files": files,
            "symbols": symbols,
            "edges": edges,
            "rules": PARSE_RULES_VERSION,
            "write_seq": write_seq,
        }))
    }

    /// 12 项只读自检（与 `jw check` 同源；名字固定，供 UI/SHA 用）。
    pub fn check(&self) -> Result<serde_json::Value, EngineError> {
        let db = |e: datacore::Error| EngineError::Db(e.to_string());
        let a = self.store.false_coverage().map_err(db)?;
        let b = self.store.refuted_without_full_coverage().map_err(db)?;
        let c = self.store.orphan_evidence().map_err(db)?;
        let d = self.store.orphan_alternative().map_err(db)?;
        let e = self.store.unregistered_kinds().map_err(db)?;
        let f = self.store.dangling_sources().map_err(db)?;
        let g = if self.store.write_log_ok().map_err(db)? { 0 } else { 1 };
        let h = self.store.edge_empty().map_err(db)?;
        let rules_now = self.store.get_meta("parse_rules_version").map_err(db)?.unwrap_or_default();
        let i = if rules_now == PARSE_RULES_VERSION { 0 } else { 1 };
        let caps = collect::capability::LangCaps;
        let mut cap_bad = 0i64;
        for (kind, _) in self.store.kind_counts().map_err(db)? {
            let Some(feat) = datacore::capability::feature_required(&kind) else {
                continue;
            };
            for subject in self.store.subjects_by_kind(&kind).map_err(db)? {
                if let Some(path) = datacore::capability::path_of_subject(&subject) {
                    if !datacore::capability::Caps::supports(&caps, path, feat) {
                        cap_bad += 1;
                    }
                }
            }
        }
        let mut edge_cap_bad = 0i64;
        for (kind, src, leaf) in self.store.edges_brief().map_err(db)? {
            let Some(feat) = collect::capability::feature_of_kind(&kind) else {
                continue;
            };
            let file: String = if kind == "impl" {
                leaf.clone().unwrap_or_else(|| src.clone())
            } else if kind == "test" || kind == "test_macro" {
                src.strip_prefix("test:")
                    .and_then(|r| r.split_once('#'))
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_else(|| src.clone())
            } else if kind == "call" {
                crate::view::subject_path(&src).to_string()
            } else {
                src.clone()
            };
            if !datacore::capability::Caps::supports(&caps, &file, feat) {
                edge_cap_bad += 1;
            }
        }
        let lsp = self.store.count_lsp_evidence().map_err(db)?;
        let items = [
            ("false_coverage", a),
            ("refuted_without_full", b),
            ("orphan_evidence", c),
            ("orphan_alternative", d),
            ("unregistered_kind", e),
            ("dangling_source", f),
            ("write_log_bad", g),
            ("edge_empty", h),
            ("rules_version_bad", i),
            ("capability_violation", cap_bad),
            ("edge_capability_violation", edge_cap_bad),
            ("lsp_in_history", lsp),
        ];
        let ok = items.iter().all(|(_, v)| *v == 0);
        Ok(serde_json::json!({
            "source": "db",
            "rules": PARSE_RULES_VERSION,
            "ok": ok,
            "items": items.iter().map(|(n, v)| serde_json::json!({
                "name": n, "value": v, "ok": *v == 0,
            })).collect::<Vec<_>>(),
        }))
    }

    /// 一层目录列表（serve `files` 同构）；越界与读不了都显式报错，不静默空数组。
    pub fn files(&self, rel: &str) -> Result<serde_json::Value, EngineError> {
        let io = |e: std::io::Error| EngineError::Io(e.to_string());
        let root_c = std::fs::canonicalize(&self.root).map_err(io)?;
        let full_c = std::fs::canonicalize(self.root.join(rel)).map_err(io)?;
        if !full_c.starts_with(&root_c) {
            return Err(EngineError::Io("outside root".to_string()));
        }
        let rd = std::fs::read_dir(&full_c).map_err(io)?;
        let mut all: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        all.sort();
        // 最多返回 500 项；超出会显式标 truncated（不静默截断）
        let truncated = all.len() > 500;
        all.truncate(500);
        Ok(serde_json::json!({"source": "db", "path": rel, "items": all, "truncated": truncated}))
    }

    /// 读一个仓库内文本文件（serve `read_file` 同构）；越界/目录/读不了都显式 Err。
    /// 只读前 200_004 字节，再回退到 UTF-8 字符边界截到 200_000，避免 `String::truncate` panic。
    pub fn read_file(&self, rel: &str) -> Result<serde_json::Value, EngineError> {
        use std::io::Read;
        let io = |e: std::io::Error| EngineError::Io(e.to_string());
        let root_c = std::fs::canonicalize(&self.root).map_err(io)?;
        let full_c = std::fs::canonicalize(self.root.join(rel)).map_err(io)?;
        if !full_c.starts_with(&root_c) {
            return Err(EngineError::Io("outside root".to_string()));
        }
        if full_c.is_dir() {
            return Err(EngineError::Io("is a directory".to_string()));
        }
        let f = std::fs::File::open(&full_c).map_err(io)?;
        let mut buf = Vec::new();
        f.take(200_004).read_to_end(&mut buf).map_err(io)?;
        let full_read = buf.len() == 200_004;
        let truncated = buf.len() > 200_000;
        let mut text = match std::str::from_utf8(&buf) {
            Ok(s) => s.to_string(),
            Err(e) => {
                let vu = e.valid_up_to();
                if full_read && vu + 3 >= buf.len() {
                    String::from_utf8(buf[..vu].to_vec()).unwrap()
                } else {
                    return Err(EngineError::Io("not utf8".to_string()));
                }
            }
        };
        if text.len() > 200_000 {
            let mut cut = 200_000;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        Ok(serde_json::json!({"source": "db", "path": rel, "text": text, "truncated": truncated}))
    }

    /// 机器信息（serve `machine` 同构）：纯 sys，取不到 cpus 就是 0（不是错误，uname 语义）。
    pub fn machine() -> serde_json::Value {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0);
        serde_json::json!({
            "source": "sys",
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "cpus": cpus,
        })
    }

    /// 清理清点（R2 · clean 换芯，**只清点不删**）：项目可重新生成产物 + 全局缓存主体（刀1）。
    ///
    /// 顶层口径与旧页信封一致：`zoneA`/`recommendedBytes`/`recommendedCount` 只算
    /// zone A + action=delete；被截断的项不进字节合计（不留假精确）。
    pub fn clean_scan(&self) -> Result<serde_json::Value, EngineError> {
        const JUNK_DIRS: &[&str] = &["__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".tox", ".ipynb_checkpoints", "htmlcov"];
        const BUILD_DIRS: &[&str] = &["build", "dist"];
        const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".venv", "venv", "env", ".idea", ".vs", ".jingwei", ".drydock", ".pylaunch"];
        let mut items: Vec<serde_json::Value> = Vec::new();
        let mut files_considered: u64 = 0;
        let mut project_truncated = false;
        let root = PathBuf::from(clean_path(&std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone())));
        let mut stack: Vec<std::path::PathBuf> = vec![root.clone()];
        let mut depth_of: std::collections::HashMap<std::path::PathBuf, u32> = Default::default();
        depth_of.insert(root.clone(), 0);
        while let Some(dir) = stack.pop() {
            let d = *depth_of.get(&dir).unwrap_or(&0);
            if d >= 6 {
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Ok(ft) = e.file_type() else { continue };
                if !ft.is_dir() {
                    continue;
                }
                let lower = name.to_lowercase();
                let is_junk = JUNK_DIRS.contains(&lower.as_str());
                let is_build = BUILD_DIRS.contains(&lower.as_str());
                let is_egg = lower.ends_with(".egg-info");
                if is_junk || is_build || is_egg {
                    if items.len() >= 50 {
                        project_truncated = true;
                        continue;
                    }
                    let bytes = dir_stat(&e.path());
                    files_considered += 1;
                    let (reason, consequence, recommended) = if lower == "__pycache__" {
                        ("Python 导入时自动生成的字节码；下次导入会重建", "只是首次 import 慢一点点；不影响任何功能", true)
                    } else if is_build {
                        ("打包/构建目录；重新构建就会再生成", "如果里面有你要交付的安装包，先拷走再删", false)
                    } else if is_egg {
                        ("pip install -e 留下的元数据；重装时重建", "之后 pip uninstall 会认不出它，必要时 pip install -e . 重来一次", false)
                    } else {
                        ("测试/类型检查/格式化工具的运行缓存", "下次运行时重新计算，不影响结果", true)
                    };
                    items.push(serde_json::json!({
                        "path": e.path().to_string_lossy().to_string(),
                        "label": lower,
                        "action": "delete",
                        "zone": "a",
                        "recommended": recommended,
                        "size_bytes": bytes,
                        "reason": reason,
                        "consequence": consequence,
                    }));
                    continue;
                }
                if SKIP_DIRS.contains(&lower.as_str()) {
                    continue;
                }
                depth_of.insert(e.path(), d + 1);
                stack.push(e.path());
            }
        }
        items.extend(scan_project_junk(&root));
        items.extend(enumerate_global_caches(&discover_caches(CACHES_MAIN)));
        items.extend(enumerate_global_caches(&discover_caches(CACHES_TAIL)));
        if let Some(p) = env_path("LOCALAPPDATA", "Microsoft/Windows/Explorer") {
            if let Some(it) = scan_thumbcache(&p) { items.push(it); }
        }
        items.push(recycle_bin_item());
        let temp = std::env::var_os("TEMP").map(PathBuf::from).or_else(|| std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join("Temp")));
        if let Some(t) = temp {
            if let Some(it) = scan_temp(&t) { items.push(it); }
        }
        items.extend(admin_items());
        items.extend(offlimits_items());

        // 顶层口径：只算 zone A + action=delete。
        let za = |i: &&serde_json::Value| i["zone"] == "a" && i["action"] == "delete";
        let rec = |i: &&serde_json::Value| i["recommended"] == true;
        let zone_bytes: u64 = items.iter().filter(|i| za(i)).map(|i| i["size_bytes"].as_u64().unwrap_or(0)).sum();
        let zone_count = items.iter().filter(|i| za(i)).count();
        let rec_bytes: u64 = items.iter().filter(|i| za(i) && rec(i)).map(|i| i["size_bytes"].as_u64().unwrap_or(0)).sum();
        let rec_count = items.iter().filter(|i| za(i) && rec(i)).count();

        Ok(serde_json::json!({
            "scope": "project",
            "files_considered": files_considered,
            "truncated": project_truncated,
            "next_cursor": serde_json::Value::Null,
            "hasProject": true,
            "items": items,
            "zoneA": { "bytes": zone_bytes, "count": zone_count },
            "recommendedBytes": rec_bytes,
            "recommendedCount": rec_count,
            "total_bytes": zone_bytes,
            "human": human_bytes(zone_bytes),
            "humanRecommended": human_bytes(rec_bytes),
        }))
    }

    /// 硬件（R2 · machine 换芯）：CPU / 内存 / 显卡 / 系统盘。
    ///
    /// 只走**外部命令**（PowerShell CIM），不在 engine 里写 WMI FFI——
    /// 与 git 同一原则：工具链从外面来，我们只解析结果。失败给 `fails` 明说，不编默认值。
    pub fn machine_hw() -> serde_json::Value {
        let script = r#"
$ErrorActionPreference='SilentlyContinue'
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$os  = Get-CimInstance Win32_OperatingSystem | Select-Object -First 1
$gpu = @(Get-CimInstance Win32_VideoController | Where-Object { $_.Name -notmatch 'Basic|Remote|Virtual' } | ForEach-Object { $_.Name })
$disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object {
  @{ drive = $_.DeviceID; free_gb = [math]::Round($_.FreeSpace/1GB,0); total_gb = [math]::Round($_.Size/1GB,0) }
})
[pscustomobject]@{
  cpu = $cpu.Name
  cpu_mhz = $cpu.MaxClockSpeed
  cores = $cpu.NumberOfLogicalProcessors
  mem_gb = if ($os) { [math]::Round($os.TotalVisibleMemorySize/1MB,1) } else { $null }
  gpus = $gpu
  disks = $disks
  os_product = $os.Caption
  os_version = $os.Version
  os_build = $os.BuildNumber
} | ConvertTo-Json -Depth 5 -Compress
"#;
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output();
        let mut fails: Vec<String> = Vec::new();
        let val = match out {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                match serde_json::from_str::<serde_json::Value>(&s) {
                    Ok(v) => v,
                    Err(e) => {
                        fails.push(format!("CIM 输出不是 JSON：{e}"));
                        serde_json::Value::Null
                    }
                }
            }
            Ok(o) => {
                fails.push(format!(
                    "powershell 退出码 {}：{}",
                    o.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&o.stderr).trim()
                ));
                serde_json::Value::Null
            }
            Err(e) => {
                fails.push(format!("起不来 powershell：{e}"));
                serde_json::Value::Null
            }
        };
        if val.is_null() {
            return serde_json::json!({
                "scope": "project", "files_considered": 0, "truncated": false, "next_cursor": serde_json::Value::Null,
                "source": "cim", "fails": fails, "hw": serde_json::Value::Null,
            });
        }
        serde_json::json!({
            "scope": "project", "files_considered": 0, "truncated": false, "next_cursor": serde_json::Value::Null,
            "source": "cim",
            "fails": fails,
            "hw": val,
        })
    }

    /// 同级项目列表（serve `projects` 同构）：父目录里带 .git 或 .jingwei 的目录，最多 200 项。
    /// 读目录失败显式 Err，不返回空列表冒充"没有项目"。

/// 扫描父目录下的项目（R2/S3 版）：markers 用 manifest_kind 18 种 + has_git/has_contract/has_index；
/// 跳过 NOT_SOURCE_DIRS 15 项 + 任何 `.` 开头；不跟 symlink；`scanned_dirs` = 实际 read_dir 打开数；
/// 遇项目不下钻（top_level），最后按路径排序。
pub fn scan(&self, parent: &str, depth: u32, limit: usize) -> Result<serde_json::Value, EngineError> {
    let parent_path = if parent.trim().is_empty() {
        self.root.clone()
    } else {
        PathBuf::from(parent)
    };
    scan_dir(&parent_path, depth, limit)
}


    /// 工具链探测（serve `env_probe` 同构）：只读，跑一次版本命令定性。
    pub fn env_probe(&self) -> Result<serde_json::Value, EngineError> {
        Ok(env_probe_value())
    }

    /// 工作笔记（R2 · note 换芯）：读 `.jingwei/notes/*.md`，格式与旧壳**同一份**。
    ///
    /// 只做读：写入仍走壳（下一刀再接）。解析规则照抄旧实现——
    /// frontmatter `kind/title/created/updated/tags`，缺省字段留空，别编造。
    pub fn notes(&self) -> Result<serde_json::Value, EngineError> {
        let dir = self.root.join(".jingwei").join("notes");
        let mut out: Vec<serde_json::Value> = Vec::new();
        let mut files = 0u64;
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) != Some("md") {
                    continue;
                }
                files += 1;
                let Some(slug) = p.file_stem().map(|s| s.to_string_lossy().to_string()) else {
                    continue;
                };
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                let (kind, title, created, updated, tags, body) = parse_note(&text);
                out.push(serde_json::json!({
                    "slug": slug,
                    "kind": kind,
                    "title": title,
                    "created": created,
                    "updated": updated,
                    "tags": tags,
                    "body": body,
                }));
            }
        }
        // 与旧实现同序：updated 倒序，其次 slug 倒序
        out.sort_by(|a, b| {
            let u = b["updated"].as_str().unwrap_or("").cmp(a["updated"].as_str().unwrap_or(""));
            u.then(b["slug"].as_str().unwrap_or("").cmp(a["slug"].as_str().unwrap_or("")))
        });
        Ok(serde_json::json!({
            "scope": "project",
            "files_considered": files,
            "truncated": false,
            "next_cursor": serde_json::Value::Null,
            "dir": ".jingwei/notes",
            "notes": out,
        }))
    }

    /// 可编辑文件清单（R2 · editor 换芯）：扁平相对路径（`/` 分隔），由 UI 自己树化。
    ///
    /// 与 `files()` 的区别：那个是"某一层目录"，这个是"整棵源码树里能编辑的文件"。
    /// 走 `NOT_SOURCE_DIRS` 跳过表，别把 target/node_modules 也扫进来。
    pub fn source_files(&self, limit: usize) -> Result<serde_json::Value, EngineError> {
        let limit = limit.clamp(1, 2000);
        let mut files: Vec<String> = Vec::new();
        let mut walked: u64 = 0;
        let mut stack: Vec<std::path::PathBuf> = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            if files.len() >= limit {
                break;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                if files.len() >= limit {
                    break;
                }
                let name = e.file_name().to_string_lossy().to_string();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    if name.starts_with('.') || is_not_source_dir(&name) {
                        continue;
                    }
                    stack.push(e.path());
                    continue;
                }
                if !ft.is_file() {
                    continue;
                }
                walked += 1;
                let rel = e
                    .path()
                    .strip_prefix(&self.root)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| name.clone());
                files.push(rel);
            }
        }
        files.sort();
        let truncated = files.len() >= limit;
        Ok(serde_json::json!({
            "source": "fs",
            "scope": "module",
            "files_considered": walked,
            "truncated": truncated,
            "next_cursor": if truncated { serde_json::json!(limit as u64) } else { serde_json::Value::Null },
            "files": files,
        }))
    }

    /// 四步计划（R2 · run 换芯）：装依赖 → 构建 → 测试 → 运行。
    ///
    /// **只给计划，不执行**：执行仍走壳里的 launch（下一刀再接）。缺命令的步给 `available:false`
    /// 与 `reason`，不许静默省略——用户看到"没有这一步"和"这一步没配"是两件事。
    pub fn run_plan(&self) -> Result<serde_json::Value, EngineError> {
        let cpath = self.root.join("env.contract.json");
        let (cname, cerror, cmds) = if cpath.is_file() {
            match std::fs::read_to_string(&cpath)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            {
                Some(v) => {
                    let g = |k: &str| v["commands"][k].as_str().map(|s| s.to_string());
                    (
                        v["name"].as_str().map(|s| s.to_string()),
                        None,
                        [g("install"), g("build"), g("test"), g("run")],
                    )
                }
                None => (
                    None,
                    Some("env.contract.json 解析失败".to_string()),
                    [None, None, None, None],
                ),
            }
        } else {
            (
                None,
                Some("没有 env.contract.json（不知道这一步该跑什么）".to_string()),
                [None, None, None, None],
            )
        };

        let kinds = ["install", "build", "test", "run"];
        let mut steps = Vec::new();
        for (i, kind) in kinds.iter().enumerate() {
            let cmd = cmds[i].clone();
            let (available, reason) = match (&cmd, &cerror) {
                (Some(_), _) => (true, serde_json::Value::Null),
                (None, Some(e)) => (false, serde_json::json!(e)),
                (None, None) => (false, serde_json::json!("契约里没有这一步")),
            };
            steps.push(serde_json::json!({
                "kind": kind,
                "cmd": cmd,
                "available": available,
                "reason": reason,
            }));
        }

        Ok(serde_json::json!({
            "scope": "project",
            "files_considered": 0,
            "truncated": false,
            "next_cursor": serde_json::Value::Null,
            "contract_name": cname,
            "contract_error": cerror,
            "steps": steps,
        }))
    }

    /// 项目详情（R2 · inspect 换芯第一刀）：**只迁这 5 组**，`unavailable` 明说还没迁的部分。
    ///
    /// 已迁：路径 / 类型 / 最近改动 / 怎么跑 / 契约（+ §4.4 scope 三件套）。
    /// 未迁：待处理(findings) / 卫生(hygiene) / 理由句(guide)——它们依赖 rules/guide 两套移植，
    /// 先留旧源，UI 侧按字段兜底，**不许静默变空**。
    pub fn inspect(&self) -> Result<serde_json::Value, EngineError> {
        let root = self.root.clone();
        let exists = root.is_dir();
        let kinds = if exists { root_kinds(&root) } else { Vec::new() };
        let (markers, kind, has_git, has_contract, has_index) = if exists {
            probe_project(&root)
        } else {
            (Vec::new(), String::new(), false, false, false)
        };

        // 最近改动：根目录一层文件的最大 mtime（"今天/昨天"这种量级够用；精确到天）
        let mut newest: Option<u64> = None;
        if exists {
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten() {
                    let Ok(ft) = e.file_type() else { continue };
                    if !ft.is_file() {
                        continue;
                    }
                    if let Ok(md) = e.metadata() {
                        if let Ok(mt) = md.modified() {
                            if let Ok(d) = mt.duration_since(std::time::UNIX_EPOCH) {
                                let s = d.as_secs();
                                newest = Some(newest.map_or(s, |n| n.max(s)));
                            }
                        }
                    }
                }
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let recent_days = newest.map(|s| now.saturating_sub(s) / 86_400);

        // 契约：直接读 env.contract.json（与旧壳同一份文件；读不了就如实给 error）
        let cpath = root.join("env.contract.json");
        let (cname, cerror, run_cmd, install, build, test) = if cpath.is_file() {
            match std::fs::read_to_string(&cpath)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            {
                Some(v) => {
                    let cmd = |k: &str| v["commands"][k].as_str().map(|s| s.to_string());
                    (
                        v["name"].as_str().map(|s| s.to_string()),
                        None,
                        cmd("run"),
                        cmd("install"),
                        cmd("build"),
                        cmd("test"),
                    )
                }
                None => (
                    None,
                    Some("env.contract.json 解析失败".to_string()),
                    None,
                    None,
                    None,
                    None,
                ),
            }
        } else {
            (None, None, None, None, None, None)
        };

        // files_considered：库里登记的源文件数（没有库也没关系，0 是事实不是猜）
        let files_considered = self.store.timeline_file_stats().map(|(_, cur, _)| cur).unwrap_or(0) as u64;

        // 展示用根：canonicalize 一下，避免同一个项目因为 "." / 相对路径在卡片上显示成两样
        let display_root = clean_path(&std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone()));
        Ok(serde_json::json!({
            "root": display_root,
            "exists": exists,
            "kind": kind,
            "kinds": kinds,
            "markers": markers,
            "has_git": has_git,
            "has_contract": has_contract,
            "has_index": has_index,
            // §4.4 scope 三件套（inspect 是单项目范围）
            "scope": "project",
            "files_considered": files_considered,
            "truncated": false,
            "next_cursor": serde_json::Value::Null,
            "recent": { "days": recent_days, "at": newest },
            "run_command": run_cmd,
            "install_command": install,
            "build_command": build,
            "test_command": test,
            "has_test": v_has(&test),
            "has_install": v_has(&install),
            "contract_name": cname,
            "contract_error": cerror,
            // 明确"这部分还没迁"，UI 拿不到时必须回落到旧源，不许静默变空
            "unavailable": ["findings", "hygiene", "guide"],
        }))
    }

    pub fn projects(&self) -> Result<serde_json::Value, EngineError> {
        let io = |e: std::io::Error| EngineError::Io(e.to_string());
        // 相对路径（"."）先规范化，否则 parent() 为空、项目列表恒空
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        let parent = root.parent().map(clean_path).unwrap_or_default();
        let mut items = Vec::new();
        if !parent.is_empty() {
            let rd = std::fs::read_dir(&parent).map_err(io)?;
            for e in rd.take(200) {
                let e = e.map_err(io)?;
                let p = e.path();
                if p.is_dir() && (p.join(".git").exists() || p.join(".jingwei").exists()) {
                    items.push(serde_json::json!({
                        "name": p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                        "path": clean_path(&p),
                    }));
                }
            }
        }
        items.sort_by_key(|x| x["name"].as_str().unwrap_or("").to_string());
        Ok(serde_json::json!({
            "source": "db",
            "parent": parent,
            "root": clean_path(&root),
            "items": items,
        }))
    }

    /// 单卡/多卡（serve `card` 同构；返回 (exit, card)）。
    /// state 支持 def/dups/original/deps；history=true 走 range_history。
    /// 可达测试列表：与 serve 的 `tests_for` result 同构（不含 v/id/ok 信封）。
    /// `bad_subject` / `no_symbol` 也以 Value 返回（reason 字段），由调用方决定退出码。

    pub fn cite(&self, subject: &str) -> Result<(serde_json::Value, u8), EngineError> {
        let store = &self.store;
        let root = &self.root;
        // §18.4 弱仓体检：浅克隆 / detached / 无 commit / 非 git 一律显式 skip，不造 permalink。
        if let Some(skip) = weak_repo_skip(root) {
            return Ok((serde_json::json!({"v": 1, "subject": subject, "skip": skip}), 2));
        }
    let j = store.find_judgment(subject)?;
    let verdict = j
        .as_ref()
        .map(|x| x.verdict.as_str().to_string())
        .unwrap_or_else(|| "no_data".to_string());
    let evidence_kind = j
        .as_ref()
        .and_then(|x| store.evidence_for(&x.id).ok())
        .and_then(|v| v.first().map(|e| e.instrument.clone()))
        .unwrap_or_default();

    let mut rows: Vec<(String, i64, String, String)> = Vec::new();
    if let Some(rest) = subject.strip_prefix("symbol:") {
        let (path, name) = match rest.split_once('#') {
            Some(x) => x,
            None => {
                return Ok((serde_json::json!({"v": 1, "subject": subject, "reason": "bad_subject"}), 2))
            }
        };
        let defs = store.symbol_definitions_for_path(path)?;
        let mut lines: Vec<i64> = defs
            .iter()
            .filter(|(n, _, _)| n == name)
            .filter_map(|(_, _, l)| l.parse().ok())
            .collect();
        lines.sort();
        lines.dedup();
        if lines.is_empty() {
            return Ok((serde_json::json!({"v": 1, "subject": subject, "reason": "no_line"}), 2));
        }
        let file_commit = match collect::git::last_commit(root, path) {
            Some(c) => c,
            None => {
                return Ok((serde_json::json!({"v": 1, "subject": subject, "reason": "git_failed"}), 2))
            }
        };
        for line in lines {
            match collect::git::blame_line(root, path, line) {
                Some(c) => rows.push((path.to_string(), line, c, file_commit.clone())),
                None => {
                    return Ok((serde_json::json!({"v": 1, "subject": subject, "reason": "git_failed"}), 2))
                }
            }
        }
    } else if let Some(path) = subject.strip_prefix("file:") {
        let abs = root.join(path);
        let n = std::fs::read_to_string(&abs)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        if n == 0 {
            return Ok((serde_json::json!({"v": 1, "subject": subject, "reason": "no_line"}), 2));
        }
        match collect::git::last_commit(root, path) {
            Some(c) => rows.push((path.to_string(), 1, c.clone(), c)),
            None => {
                return Ok((serde_json::json!({"v": 1, "subject": subject, "reason": "git_failed"}), 2))
            }
        }
    } else {
        return Ok((serde_json::json!({"v": 1, "subject": subject, "reason": "bad_subject"}), 2));
    }

    let arr: Vec<serde_json::Value> = rows
        .iter()
        .map(|(f, l, c, fc)| {
            serde_json::json!({
                "file": f, "line": l, "commit": c, "file_commit": fc,
                "verdict": verdict, "evidence_kind": evidence_kind,
            })
        })
        .collect();
    Ok((
        serde_json::json!({"v": 1, "subject": subject, "rows": arr}),
        0,
    ))
}

    pub fn impact(&self, target: &str) -> Result<(serde_json::Value, u8), EngineError> {
        let store = &self.store;
        let root = &self.root;
    // 目录/前缀目标：模块级影响（direct_total 全量 + 明细截断）
    let raw = target.trim_start_matches("file:").trim_end_matches('/').to_string();
    if target.trim().is_empty() {
        return Ok((serde_json::json!({"v": 1, "target": target, "reason": "no_path"}), 2));
    }
    if !target.starts_with("symbol:")
        && !root.join(&raw).exists()
        && !store.find_judgment(&format!("file:{raw}"))?.is_some()
    {
        return Ok((serde_json::json!({"v": 1, "target": target, "reason": "no_path"}), 2));
    }
    if !target.starts_with("symbol:") && root.join(&raw).is_file() && !raw.ends_with(".rs") {
        return Ok((serde_json::json!({"v": 1, "target": target, "reason": "no_path"}), 2));
    }
    if !target.starts_with("symbol:") && root.join(&raw).is_dir() {
        let prefix = raw.trim_end_matches('/').to_string();
        let mut fn_count = 0i64;
        for (_name, path, kind) in store.symbol_def_index()? {
            if kind == "fn" && path.strip_prefix(&format!("{prefix}/")).is_some() {
                fn_count += 1;
            }
        }
        if fn_count == 0 {
            return Ok((
                serde_json::json!({"v": 1, "target": target, "reason": "empty_range"}),
                2,
            ));
        }
        let direct_rows = store.callers_into_prefix(&prefix)?;
        let direct_total = direct_rows.len() as i64;
        const DETAIL_CAP: usize = 100;
        let direct_truncated = direct_rows.len() > DETAIL_CAP;
        let mut direct_subjects: std::collections::BTreeSet<String> = Default::default();
        for (src, _dst, _line) in direct_rows.iter().take(DETAIL_CAP) {
            direct_subjects.insert(src.clone());
        }
        // 传递：direct 调用者的调用者（depth=2），排除已在 direct 的
        let mut trans_rows: Vec<(String, i64, String)> = Vec::new();
        for s in &direct_subjects {
            for (src, line) in store.call_edges_to(s)? {
                if !direct_subjects.contains(&src) {
                    trans_rows.push((src, line, s.clone()));
                }
            }
        }
        trans_rows.sort();
        trans_rows.dedup();
        let trans_truncated = trans_rows.len() > DETAIL_CAP;
        // 受影响文件 = direct/trans 调用者所在文件；tests 只到文件级
        let mut files: std::collections::BTreeSet<String> = Default::default();
        for (src, _dst, _line) in direct_rows.iter().take(DETAIL_CAP) {
            if let Some(p) = datacore::core::typed_path(src) {
                files.insert(p);
            }
        }
        for (src, _line, _via) in trans_rows.iter().take(DETAIL_CAP) {
            if let Some(p) = datacore::core::typed_path(src) {
                files.insert(p);
            }
        }
        let mut tests: Vec<serde_json::Value> = Vec::new();
        for f in &files {
            for (name, line, kind) in store.tests_for_path(f)? {
                tests.push(serde_json::json!({"file": f, "name": name, "line": line, "kind": kind, "scope": "file"}));
            }
        }
        let importers = store.importers_of_prefix(&prefix)?;
        let (call_total, call_un) = store.edge_kind_stats("call")?;
        let v = serde_json::json!({
            "v": 1,
            "target": target,
            "target_kind": "range",
            "targets": fn_count,
            "direct_total": direct_total,
            "direct": direct_rows.iter().take(DETAIL_CAP).map(|(src, dst, line)| {
                serde_json::json!({"from": src, "line": line, "callee": dst})
            }).collect::<Vec<_>>(),
            "direct_truncated": direct_truncated,
            "transitive": trans_rows.iter().take(DETAIL_CAP).map(|(from, line, via)| {
                serde_json::json!({"from": from, "line": line, "via": via})
            }).collect::<Vec<_>>(),
            "transitive_truncated": trans_truncated,
            "tests": {"scope": "file", "files": files.iter().cloned().collect::<Vec<_>>(), "items": tests},
            "importers": importers.iter().map(|(s, leaf, dst)| serde_json::json!({"file": s, "leaf": leaf, "dst": dst})).collect::<Vec<_>>(),
            "completeness": {"call_resolved": call_total - call_un, "call_unresolved": call_un, "depth_cap": 2, "detail_cap": DETAIL_CAP},
            "scope": "module",
            "files_considered": files.len(),
            "truncated": direct_truncated || trans_truncated,
            "next_cursor": serde_json::Value::Null,
        });
        return Ok((v, 0));
    }
    let (targets, base_file, kind_of_target): (Vec<String>, String, &str) =
        if target.starts_with("symbol:") {
            match datacore::core::Subject::parse(target) {
                Ok(datacore::core::Subject::Symbol { path, name }) => {
                    (vec![format!("symbol:{path}#{name}")], path, "symbol")
                }
                _ => {
                    return Ok((serde_json::json!({"v": 1, "target": target, "reason": "bad_subject"}), 2))
                }
            }
        } else {
            let p = target.trim_start_matches("file:").to_string();
            (fns_of_file(store, &p)?, p, "file")
        };
    if targets.is_empty() {
        return Ok((serde_json::json!({"v": 1, "target": target, "reason": "no_symbol"}), 2));
    }
    if kind_of_target == "symbol" && store.find_judgment(&targets[0])?.is_none() {
        return Ok((serde_json::json!({"v": 1, "target": target, "reason": "no_symbol"}), 2));
    }

    let mut direct: Vec<(String, i64, String)> = Vec::new();
    for t in &targets {
        for (src, line) in store.call_edges_to(t)? {
            direct.push((src, line, t.clone()));
        }
    }
    direct.sort();
    direct.dedup();
    let direct_subjects: std::collections::BTreeSet<String> =
        direct.iter().map(|(s, _, _)| s.clone()).collect();

    let mut trans: Vec<(String, i64, String)> = Vec::new();
    for s in &direct_subjects {
        for (src, line) in store.call_edges_to(s)? {
            if !direct_subjects.contains(&src) && !targets.contains(&src) {
                trans.push((src, line, s.clone()));
            }
        }
    }
    trans.sort();
    trans.dedup();

    let mut files: std::collections::BTreeSet<String> = Default::default();
    for (s, _, _) in direct.iter().chain(trans.iter()) {
        if let Some(p) = datacore::core::typed_path(s) {
            files.insert(p);
        }
    }
    let mut tests: Vec<serde_json::Value> = Vec::new();
    for f in &files {
        for (name, line, kind) in store.tests_for_path(f)? {
            tests.push(serde_json::json!({"file": f, "name": name, "line": line, "kind": kind, "scope": "file"}));
        }
    }
    let importers = store.use_importers_of(&base_file)?;
    let (call_total, call_un) = store.edge_kind_stats("call")?;

    let v = serde_json::json!({
        "v": 1,
        "target": target,
        "target_kind": kind_of_target,
        "targets": targets,
        "direct": direct.iter().map(|(f, l, t)| serde_json::json!({"from": f, "line": l, "callee": t})).collect::<Vec<_>>(),
        "transitive": trans.iter().map(|(f, l, via)| serde_json::json!({"from": f, "line": l, "via": via})).collect::<Vec<_>>(),
        "tests": {"scope": "file", "files": files.iter().cloned().collect::<Vec<_>>(), "items": tests},
        "importers": importers.iter().map(|(s, leaf)| serde_json::json!({"file": s, "leaf": leaf})).collect::<Vec<_>>(),
        "completeness": {"call_resolved": call_total - call_un, "call_unresolved": call_un, "depth_cap": 2},
        "scope": "file",
        "files_considered": files.len(),
        "truncated": false,
        "next_cursor": serde_json::Value::Null,
    });
    Ok((v, 0))
}

    pub fn tests_for(&self, target: &str, depth: i64) -> Result<serde_json::Value, EngineError> {
        let db = |e: datacore::Error| EngineError::Db(e.to_string());
        let store = &self.store;
        let subject = match datacore::core::Subject::parse(target) {
            Ok(datacore::core::Subject::Symbol { path, name }) => format!("symbol:{path}#{name}"),
            _ => {
                return Ok(serde_json::json!({"v": 1, "target": target, "reason": "bad_subject"}))
            }
        };
        if store.find_judgment(&subject).map_err(db)?.is_none() {
            return Ok(serde_json::json!({"v": 1, "target": target, "reason": "no_symbol"}));
        }
        let mut seen: std::collections::BTreeMap<String, (i64, String)> = Default::default();
        seen.insert(subject.clone(), (0, String::new()));
        let mut frontier: Vec<String> = vec![subject.clone()];
        for d in 1..=depth.max(0) {
            let mut next: Vec<String> = Vec::new();
            for node in &frontier {
                for (src, _line) in store.call_edges_to(node).map_err(db)? {
                    if !seen.contains_key(&src) {
                        seen.insert(src.clone(), (d, node.clone()));
                        next.push(src);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        let tests_all = store.test_edges_all().map_err(db)?;
        let mut hits: Vec<serde_json::Value> = Vec::new();
        for (file, name, line, kind) in &tests_all {
            let subj = format!("symbol:{file}#{name}");
            if let Some((d, via)) = seen.get(&subj) {
                hits.push(serde_json::json!({
                    "name": name, "file": file, "line": line, "kind": kind,
                    "depth": d, "via": via,
                }));
            }
        }
        hits.sort_by(|a, b| {
            (a["file"].as_str(), a["line"].as_i64(), a["name"].as_str())
                .cmp(&(b["file"].as_str(), b["line"].as_i64(), b["name"].as_str()))
        });
        hits.dedup_by(|a, b| a == b);
        let (call_total, call_un) = store.edge_kind_stats("call").map_err(db)?;
        Ok(serde_json::json!({
            "v": 1,
            "target": subject,
            "tests": hits,
            "completeness": {
                "call_resolved": call_total - call_un,
                "call_unresolved": call_un,
                "depth_cap": depth,
                "tests_total": tests_all.len(),
            },
            "note": "静态可达，不等于运行覆盖",
        }))
    }

    pub fn card(
        &self,
        input: &str,
        history: bool,
        state: Option<&str>,
    ) -> Result<(u8, serde_json::Value), EngineError> {
        crate::view::card_value(&self.store, &self.root, input, history, state)
            .map_err(|e| EngineError::Db(e.to_string()))
    }

    /// 边层（serve `edges` 同构）：{path,total,edges[{kind,dst}],unresolved_reasons?}
    pub fn edges(&self, path: &str) -> Result<serde_json::Value, EngineError> {
        let db = |e: datacore::Error| EngineError::Db(e.to_string());
        let rows = self.store.edges_for(path).map_err(db)?;
        let edges: Vec<serde_json::Value> = rows
            .iter()
            .map(|(dst, k)| serde_json::json!({"kind": k, "dst": dst}))
            .collect();
        let mut result = serde_json::json!({"path": path, "total": edges.len(), "edges": edges, "scope": "file", "files_considered": 1, "truncated": false, "next_cursor": serde_json::Value::Null});
        // `?use` 原因码与 jw edges 同一份 collect::gap（不写第二份）
        let unresolved = self.store.unresolved_use_edges().map_err(db)?;
        let table = collect::gap::GapTable::new(&self.root);
        let mut m: std::collections::BTreeMap<&'static str, i64> = Default::default();
        for (s, dst, line) in unresolved.iter().filter(|(s, _, _)| s == path) {
            *m.entry(table.reason(s, dst, *line).as_str()).or_default() += 1;
        }
        if !m.is_empty() {
            result["unresolved_reasons"] = serde_json::json!(m);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jw-engine-{}-{tag}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("src")).unwrap();
        d
    }

    #[test]
    fn module_map_matches_collect() {
        let d = tmp("mm");
        std::fs::write(d.join("src/lib.rs"), "pub mod a;\n").unwrap();
        std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        assert_eq!(e.module_map(), collect::parse::module_map(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn open_without_db_is_no_db() {
        let d = tmp("nodb");
        match Engine::open(&d) {
            Err(EngineError::NoDb(_)) => {}
            other => panic!("expected NoDb, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    fn db_state(root: &Path) -> (u64, Vec<u8>) {
        let bytes = std::fs::read(root.join(".jingwei").join("jingwei.db")).unwrap();
        (bytes.len() as u64, bytes)
    }

    #[test]
    fn stats_matches_store_counts() {
        let d = tmp("stats");
        std::fs::write(d.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let v = e.stats().unwrap();
        assert_eq!(v["files"], collect::parse::source_files(&d).len() as i64);
        let sym = e
            .store()
            .kind_counts()
            .unwrap()
            .into_iter()
            .find(|(k, _)| k == "symbol")
            .map(|(_, n)| n)
            .unwrap_or(0);
        assert_eq!(v["symbols"], sym);
        assert_eq!(v["edges"], e.store().count("edge").unwrap());
        assert_eq!(v["rules"], PARSE_RULES_VERSION);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stats_without_db_is_no_db() {
        let d = tmp("stats-nodb");
        match Engine::open(&d) {
            Err(EngineError::NoDb(_)) => {}
            other => panic!("expected NoDb, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn check_has_twelve_stable_items() {
        let d = tmp("check");
        std::fs::write(d.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let v = e.check().unwrap();
        let names: Vec<&str> = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "false_coverage",
                "refuted_without_full",
                "orphan_evidence",
                "orphan_alternative",
                "unregistered_kind",
                "dangling_source",
                "write_log_bad",
                "edge_empty",
                "rules_version_bad",
                "capability_violation",
                "edge_capability_violation",
                "lsp_in_history"
            ]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn check_ok_when_all_zero() {
        let d = tmp("check-ok");
        std::fs::write(d.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let s = Store::open(&d).unwrap();
        s.set_meta("parse_rules_version", PARSE_RULES_VERSION).unwrap();
        let e = Engine::open(&d).unwrap();
        let v = e.check().unwrap();
        assert_eq!(v["ok"], true, "{v}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn files_lists_one_level() {
        let d = tmp("files");
        std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let v = e.files("src").unwrap();
        let items: Vec<&str> = v["items"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert_eq!(items, vec!["a.rs"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn projects_lists_siblings_with_git_or_jingwei() {
        let base = std::env::temp_dir().join(format!(
            "jw-proj-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ws = base.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        Store::open(&ws).unwrap();
        std::fs::create_dir_all(base.join("a/.git")).unwrap();
        std::fs::create_dir_all(base.join("b/.jingwei")).unwrap();
        std::fs::create_dir_all(base.join("c")).unwrap();
        let e = Engine::open(&ws).unwrap();
        let v = e.projects().unwrap();
        let names: Vec<&str> = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"a"), "{names:?}");
        assert!(names.contains(&"b"), "{names:?}");
        assert!(!names.contains(&"c"), "{names:?}");
        let txt = serde_json::to_string(&v).unwrap();
        assert!(!txt.contains("\\\\?\\"), "{txt}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn machine_reports_os_arch() {
        let v = Engine::machine();
        assert_eq!(v["source"], "sys");
        assert_eq!(v["os"], std::env::consts::OS);
        assert_eq!(v["arch"], std::env::consts::ARCH);
        assert!(v["cpus"].as_u64().is_some());
    }

    #[test]
    fn read_file_normal() {
        let d = tmp("read");
        std::fs::write(d.join("src/a.rs"), "hello\n").unwrap();
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let v = e.read_file("src/a.rs").unwrap();
        assert_eq!(v["text"], "hello\n");
        assert_eq!(v["truncated"], false);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn read_file_long_cjk_does_not_panic() {
        let d = tmp("read-cjk");
        let s = "中".repeat(70_000); // 210_000 bytes, 200_000 is mid-char
        std::fs::write(d.join("src/big.rs"), &s).unwrap();
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let v = e.read_file("src/big.rs").unwrap();
        let text = v["text"].as_str().unwrap();
        assert!(text.len() <= 200_000);
        assert_eq!(v["truncated"], true);
        assert!(text.chars().all(|c| c == '中'));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn read_file_rejects_escape_and_absolute() {
        let d = tmp("read-bad");
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        match e.read_file("..") {
            Err(EngineError::Io(m)) if m == "outside root" => {}
            other => panic!("expected outside root, got {:?}", other.map(|_| ())),
        }
        let abs = if cfg!(windows) { "C:\\Windows\\win.ini" } else { "/etc/hostname" };
        match e.read_file(abs) {
            Err(EngineError::Io(_)) => {}
            other => panic!("expected Io, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn read_file_binary_is_error() {
        let d = tmp("read-bin");
        std::fs::write(d.join("src/bin.dat"), [0xFFu8, 0xFE, 0x00]).unwrap();
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        match e.read_file("src/bin.dat") {
            Err(EngineError::Io(m)) if m == "not utf8" => {}
            other => panic!("expected not utf8, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn files_rejects_escape() {
        let d = tmp("files-escape");
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        match e.files("..") {
            Err(EngineError::Io(_)) => {}
            other => panic!("expected Io, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn files_rejects_absolute() {
        let d = tmp("files-abs");
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let abs = if cfg!(windows) { "C:\\Windows" } else { "/etc" };
        match e.files(abs) {
            Err(EngineError::Io(_)) => {}
            other => panic!("expected Io, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn decision_original_missing_dir_says_so() {
        use datacore::model::{Judgment, Scope, Verdict};
        let d = tmp("dec-orig");
        let s = Store::open(&d).unwrap();
        s.put_judgment(&Judgment {
            id: "j1".into(),
            subject: "decision:repo#d1".into(),
            verdict: Verdict::Held,
            kind: "decision".into(),
            scope: Scope::default(),
            role: None,
            expires_at: None,
            at: 0,
            coverage_read: 0,
            coverage_total: 0,
        })
        .unwrap();
        let e = Engine::open(&d).unwrap();
        let (_code, v) = crate::view::card_value(e.store(), &d, "decision:d1", false, Some("original")).unwrap();
        let msg = v["original"]["error"].as_str().unwrap_or("");
        assert!(msg.contains("目录不存在"), "{v}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn open_does_not_write_db() {
        let d = tmp("ro");
        std::fs::write(d.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
        Store::open(&d).unwrap();
        let before = db_state(&d);
        let e = Engine::open(&d).unwrap();
        let _ = e.store().count("file").unwrap_or(0);
        let after = db_state(&d);
        assert_eq!(before, after, "只读打开不得写库（size+bytes）");
        let mut extra: Vec<String> = std::fs::read_dir(d.join(".jingwei"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n != "jingwei.db" && !n.starts_with("jingwei.db-"))
            .collect();
        extra.sort();
        assert!(extra.is_empty(), "多余文件: {extra:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn tests_for_no_db_is_no_db() {
        let d = std::env::temp_dir().join(format!("eng-tf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        match Engine::open(&d) {
            Ok(_) => panic!("no-db dir must not open"),
            Err(EngineError::NoDb(_)) => {}
            Err(e) => panic!("unexpected: {e}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }



    #[test]
    fn tests_for_shape_bad_subject() {
        // keys-only 契约：bad_subject 分支的形状固定（v/target/reason），不随数据变。
        let d = tmp("tf-shape");
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let v = e.tests_for("not-a-subject", 3).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["reason", "target", "v"]);
        assert_eq!(v["reason"], "bad_subject");
        let _ = std::fs::remove_dir_all(&d);
    }


    #[test]
    fn impact_empty_db_is_no_symbol() {
        // 空库 = "为什么空"必须自证（reason），不是静默空结果。
        let d = tmp("impact-empty");
        Store::open(&d).unwrap();
        let e = Engine::open(&d).unwrap();
        let (v, code) = e.impact("symbol:src/lib.rs#a").unwrap();
        assert_eq!(code, 2);
        assert_eq!(v["reason"], "no_symbol");
        assert!(v.get("target").is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

}
