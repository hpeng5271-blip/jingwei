//! 项目体检：这个项目自己长什么样——结构、依赖清单、忽略规则、体积。
//!
//! 和 doctor 的分工：doctor 查「机器和运行环境」，这里查「项目文件本身」。
//! 两边都不猜：这里只报能从文件里读出来的事实，读不出来的一律不说。
//! 每条结论都带一条自己能跑的自查命令——这是「不误报」的底线。
use jingwei_model::{Finding, Severity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// 走多深。产物目录不往里走，所以 4 层够看清前端/后端分仓那种结构。
const MAX_DEPTH: usize = 4;
/// 文件数上限：体检不该把一次打开变成一次全盘遍历。
const FILE_BUDGET: usize = 6000;
const BIG_FILE_MB: f64 = 20.0;
/// Windows 传统接口的路径上限是 260；留一点余量再提醒。
const LONG_PATH: usize = 220;

pub const G_IGNORE: &str = "忽略与提交";
pub const G_DEPS: &str = "依赖清单";
pub const G_HABIT: &str = "工程习惯";
pub const G_SIZE: &str = "目录与体积";

/// 体检条目的 id 清单一处集中。rules 用它给条目归档到排查链的最后一层；
/// 多一个 id 忘了登记，症状是它会掉进「其他」——所以这里有测试钉住。
pub const HYGIENE_IDS: &[&str] = &[
    "gitignore-missing",
    "gitignore-missing-entry",
    "env-file-exposed",
    "ide-dir-present",
    "nested-git",
    "detritus",
    "py-manifest-missing",
    "py-lock-missing",
    "py-unpinned",
    "py-multiple-manifests",
    "node-lock-missing",
    "node-lock-stale",
    "node-no-scripts",
    "node-mixed-lockfiles",
    "node-below-engines",
    "package-json-broken",
    "maven-snapshot-dep",
    "maven-version-conflict",
    "maven-encoding-unset",
    "maven-no-wrapper",
    "gradle-no-wrapper",
    "cargo-lock-missing",
    "go-sum-missing",
    "php-lock-missing",
    "ruby-lock-missing",
    "docker-no-ignore",
    "cc-no-compile-commands",
    "project-path-unsafe",
    "long-path",
    "no-src-test-layout",
    "no-tests",
    "project-artifacts",
    "large-file",
    "scan-truncated",
    "py-requires-unmet",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirStat {
    /// 相对项目根的路径，用 / 分隔
    pub path: String,
    pub size_mb: f64,
    pub files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BigFile {
    pub path: String,
    pub size_mb: f64,
}

/// 一份依赖清单。kind 决定它归哪门语言。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub path: String,
    pub kind: String,
    /// 从文件里读出来的事实，一句话。读不出来就是空。
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectScan {
    pub root: String,
    pub langs: Vec<String>,
    pub deep: bool,
    /// 真实扫过的文件数。界面拿它说明「这次体检看了多少东西」。
    pub files_scanned: usize,
    pub has_git: bool,
    pub has_gitignore: bool,
    pub has_readme: bool,
    pub manifests: Vec<Manifest>,
    pub lockfiles: Vec<String>,
    /// 可重新生成的目录（深扫才有体积）
    pub artifacts: Vec<DirStat>,
    /// .gitignore 已经覆盖的产物
    pub ignored: Vec<String>,
    /// 有产物但 .gitignore 没写——要命的地方
    pub unignored: Vec<String>,
    pub detritus: Vec<String>,
    /// 项目里的 .env 之类
    pub env_files: Vec<String>,
    /// 其中没被 .gitignore 挡住的
    pub exposed_env: Vec<String>,
    /// 子目录里还有独立仓库的位置
    pub nested_git: Vec<String>,
    pub longest_path: usize,
    pub longest_file: String,
    pub big_files: Vec<BigFile>,
    /// 各扩展名的文件数，给「这个项目到底有多少代码」当证据
    pub ext_counts: BTreeMap<String, usize>,
    /// 扫到的文件相对路径（有上限）。按文件名判断的检查靠它。
    #[serde(default)]
    pub all_files: Vec<String>,
    /// 从清单里抠出来的结构化事实，交给 rules 去和环境比对
    #[serde(default)]
    pub node_engines: Option<String>,
    #[serde(default)]
    pub py_requires: Option<String>,
    #[serde(default)]
    pub node_package_manager: Option<String>,
    /// 体检条目。形状就是 Finding，外壳不用为它写第二套渲染。
    pub items: Vec<Finding>,
}

// ── 产物目录：名字就是判据，跨语言通用 ────────────────────────────────

/// 一眼就该忽略的产物名。小写比较。
const ARTIFACT_ALWAYS: &[&str] = &[
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".venv",
    "node_modules",
    "target",
    ".gradle",
    ".next",
    ".nuxt",
    ".parcel-cache",
    ".turbo",
    "cmake-build-debug",
    "cmake-build-release",
    "cmakefiles",
];

/// 名字本身有歧义（可能真是你手写的源码目录）：只有当旁边有对应的构建
/// 描述文件时才认。宁可漏报，不可误报。
fn ambiguous_artifact(parent: &Path, name: &str) -> Option<&'static str> {
    let has = |f: &str| parent.join(f).exists();
    match name {
        "build" | "out" | "dist" => {
            if has("CMakeLists.txt")
                || has("build.gradle")
                || has("build.gradle.kts")
                || has("package.json")
                || has("pom.xml")
                || has("pyproject.toml")
                || has("setup.py")
            {
                Some("构建产物目录")
            } else {
                None
            }
        }
        "bin" | "obj" => {
            let dotnet = std::fs::read_dir(parent).ok()?.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.ends_with(".csproj") || n.ends_with(".sln") || n.ends_with(".fsproj")
            });
            dotnet.then_some("dotnet 产物目录")
        }
        _ => None,
    }
}

/// 判断一个目录是不是「删了会重新长出来」的产物。
pub(crate) fn artifact_kind(path: &Path, parent: &Path, name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    if let Some(r) = ambiguous_artifact(parent, &n) {
        return Some(r);
    }
    if n.ends_with(".egg-info") {
        return Some("Python 打包产物");
    }
    if n.starts_with("cmake-build-") {
        return Some("CMake 构建目录");
    }
    if ARTIFACT_ALWAYS.contains(&n.as_str()) {
        return Some("缓存/构建产物");
    }
    // venv / env 只有真的带了 pyvenv.cfg 才算虚拟环境——很多项目里
    // env 就是个放配置的普通目录，认错会把用户的手写文件说成产物。
    if (n == "venv" || n == "env" || n == ".venv") && path.join("pyvenv.cfg").exists() {
        return Some("虚拟环境");
    }
    None
}

const DETRITUS: &[&str] = &["thumbs.db", "desktop.ini", ".ds_store"];

const LOCKFILES: &[&str] = &[
    "uv.lock",
    "poetry.lock",
    "pipfile.lock",
    "requirements.lock",
    "requirements-lock.txt",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "composer.lock",
    "gemfile.lock",
    "mix.lock",
    "cargo.lock",
    "go.sum",
    "gradle.lockfile",
];

/// 清单文件名 -> 归类。用文件名判语言，不猜内容。
fn manifest_kind(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let r = match n.as_str() {
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
    Some(r)
}

fn read_text(p: &Path) -> Option<String> {
    let bytes = std::fs::read(p).ok()?;
    // 清单文件也可能是 GBK（中文 Windows 上很常见），按本机代码页认
    let s = crate::textfile::decode(&bytes).text;
    Some(s.trim_start_matches('\u{feff}').to_string())
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

struct Walk {
    artifacts: Vec<(String, &'static str)>,
    nested_git: Vec<String>,
    files: Vec<(String, u64)>,
    exts: BTreeMap<String, usize>,
    detritus: Vec<String>,
    env_files: Vec<String>,
    longest: usize,
    longest_file: String,
    src_dir: bool,
    test_dir: bool,
    truncated: bool,
}

fn walk(dir: &Path, root: &Path, depth: usize, w: &mut Walk) {
    if depth > MAX_DEPTH || w.files.len() >= FILE_BUDGET {
        if w.files.len() >= FILE_BUDGET {
            w.truncated = true;
        }
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            let n = name.to_lowercase();
            if n == ".git" {
                if depth > 0 {
                    w.nested_git.push(rel_of(root, &p));
                }
                continue;
            }
            if let Some(kind) = artifact_kind(&p, dir, &name) {
                w.artifacts.push((rel_of(root, &p), kind));
                continue; // 产物目录不往里走
            }
            // 项目**内部**的目录也要过一遍这道闸：网络盘断线会把扫描挂住、
            // 联接点可能指回上一层、超长路径老式接口读不了。
            // 这道闸原来只拦"扫项目列表"那一步，进来之后就没人管了——
            // 而 junction 在 node_modules 里遍地都是。
            if crate::scan::should_skip(&p, root).is_some() {
                continue;
            }
            if n == "src" || n == "app" {
                w.src_dir = true;
            }
            if n == "test" || n == "tests" {
                w.test_dir = true;
            }
            walk(&p, root, depth + 1, w);
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        let rel = rel_of(root, &p);
        let len = root.to_string_lossy().chars().count() + rel.chars().count() + 1;
        if len > w.longest {
            w.longest = len;
            w.longest_file = rel.clone();
        }
        let ln = name.to_lowercase();
        if DETRITUS.contains(&ln.as_str()) {
            w.detritus.push(rel.clone());
        }
        let bare = ln.trim_start_matches('.');
        if ln.starts_with(".env")
            && !["env.example", "env.sample", "env.template", "env.dist"].contains(&bare)
        {
            w.env_files.push(rel.clone());
        }
        let ext = Path::new(&name)
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !ext.is_empty() {
            *w.exts.entry(ext).or_insert(0) += 1;
        }
        w.files.push((rel, size));
    }
}

#[allow(clippy::too_many_arguments)]
fn it(
    id: &str,
    sev: Severity,
    group: &str,
    title: impl Into<String>,
    evidence: Vec<String>,
    verify: Vec<String>,
    boundary: impl Into<String>,
    next: impl Into<String>,
) -> Finding {
    Finding {
        id: id.into(),
        severity: sev,
        title: title.into(),
        group: group.into(),
        layer: String::new(),
        layer_note: String::new(),
        evidence,
        verify,
        boundary: boundary.into(),
        next: next.into(),
        excluded: vec![],
    }
}

fn sev_rank(s: &Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warn => 1,
        Severity::Info => 2,
    }
}

/// 快速体检：只读目录名、忽略规则、几份清单文件。毫秒级，可以随打开项目跑。
pub fn scan(root: &Path) -> ProjectScan {
    scan_inner(root, false)
}

/// 深体检：额外算产物目录体积、找大文件。给「时不时扫一次」用。
pub fn scan_deep(root: &Path) -> ProjectScan {
    scan_inner(root, true)
}

fn scan_inner(root: &Path, deep: bool) -> ProjectScan {
    let mut s = ProjectScan {
        root: root.to_string_lossy().to_string(),
        deep,
        has_git: root.join(".git").is_dir(),
        langs: crate::lang::detect_all(root)
            .iter()
            .map(|l| l.name().to_string())
            .collect(),
        ..Default::default()
    };
    if !root.is_dir() {
        return s;
    }

    let mut w = Walk {
        artifacts: vec![],
        nested_git: vec![],
        files: vec![],
        exts: BTreeMap::new(),
        detritus: vec![],
        env_files: vec![],
        longest: 0,
        longest_file: String::new(),
        src_dir: false,
        test_dir: false,
        truncated: false,
    };
    walk(root, root, 0, &mut w);
    s.files_scanned = w.files.len();
    s.ext_counts = w.exts.clone();
    s.all_files = w.files.iter().map(|(r, _)| r.clone()).collect();
    s.detritus = w.detritus.clone();
    s.env_files = w.env_files.clone();
    s.nested_git = w.nested_git.clone();
    s.longest_path = w.longest;
    s.longest_file = w.longest_file.clone();

    // ── 清单 / 锁文件 / README ──
    let mut lockfiles: Vec<String> = vec![];
    let mut have_requirements = false;
    for (rel, size) in &w.files {
        let base = rel.rsplit('/').next().unwrap_or(rel);
        let lb = base.to_lowercase();
        if LOCKFILES.contains(&lb.as_str()) {
            lockfiles.push(rel.clone());
        }
        if lb.starts_with("readme") {
            s.has_readme = true;
        }
        let Some(kind) = manifest_kind(base) else {
            continue;
        };
        // 清单只看项目根和一层子目录：再深的就是别人的东西了
        if rel.matches('/').count() > 1 {
            continue;
        }
        // 多份 requirements 只留第一份，避免同类重复刷屏
        if lb.starts_with("requirements") {
            if have_requirements {
                continue;
            }
            have_requirements = true;
        }
        let abs = root.join(rel);
        let text = if *size < 4 * 1024 * 1024 {
            read_text(&abs)
        } else {
            None
        };
        let note = text
            .as_deref()
            .map(|t| describe_manifest(kind, t, base, &mut s))
            .unwrap_or_default();
        s.manifests.push(Manifest {
            path: rel.clone(),
            kind: kind.to_string(),
            note,
        });
    }
    s.lockfiles = lockfiles;

    // ── 大文件 ──
    if deep {
        for (rel, size) in &w.files {
            let mb = *size as f64 / 1048576.0;
            if mb >= BIG_FILE_MB {
                s.big_files.push(BigFile {
                    path: rel.clone(),
                    size_mb: mb,
                });
            }
        }
        s.big_files.sort_by(|a, b| {
            b.size_mb
                .partial_cmp(&a.size_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // ── .gitignore ──
    let entries = read_text(&root.join(".gitignore")).map(|t| parse_gitignore(&t));
    if entries.is_some() {
        s.has_gitignore = true;
    }
    let entries = entries.unwrap_or_default();

    // 产物体积（深扫才算）
    let mut artifacts: Vec<DirStat> = Vec::new();
    for (rel, _kind) in &w.artifacts {
        let (bytes, files) = if deep {
            crate::clean::dir_stat(&root.join(rel))
        } else {
            (0, 0)
        };
        artifacts.push(DirStat {
            path: rel.clone(),
            size_mb: bytes as f64 / 1048576.0,
            files,
        });
    }
    artifacts.sort_by(|a, b| {
        b.size_mb
            .partial_cmp(&a.size_mb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.path.cmp(&b.path))
    });
    s.artifacts = artifacts;

    for (rel, _) in &w.artifacts {
        let base = rel.rsplit('/').next().unwrap_or(rel);
        if covers(&entries, base) {
            s.ignored.push(rel.clone());
        } else {
            s.unignored.push(rel.clone());
        }
    }
    for e in &w.env_files {
        let base = e.rsplit('/').next().unwrap_or(e);
        if !covers(&entries, base) {
            s.exposed_env.push(e.clone());
        }
    }

    // ── 生成条目 ──
    let mut items = Vec::new();
    ignore_items(&mut items, &s, &entries);
    deps_items(&mut items, &s, root);
    habit_items(&mut items, &s, &w);
    size_items(&mut items, &s, deep, w.truncated);
    items.sort_by(|a, b| {
        sev_rank(&a.severity)
            .cmp(&sev_rank(&b.severity))
            .then(a.id.cmp(&b.id))
    });
    s.items = items;
    s
}

/// .gitignore 里一行行读出来，只留「能被匹配的名字」。
/// 不做完整的 gitignore 语法——认不出来时说「没覆盖」，不假装覆盖了。
fn parse_gitignore(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with('!') {
            continue;
        }
        let l = l.trim_start_matches('/').trim_end_matches('/');
        let l = l.rsplit('/').next().unwrap_or(l);
        out.push(l.to_lowercase());
    }
    out
}

fn covers(entries: &[String], name: &str) -> bool {
    let n = name.to_lowercase();
    entries.iter().any(|e| {
        if let Some(suffix) = e.strip_prefix('*') {
            n.ends_with(suffix)
        } else {
            n == *e
        }
    })
}

/// 读一份清单，抠出「这个项目自己说要什么」。顺带把结构化事实存进 s。
fn describe_manifest(kind: &str, text: &str, base: &str, s: &mut ProjectScan) -> String {
    match kind {
        "node" => match serde_json::from_str::<serde_json::Value>(text) {
            Err(e) => format!(
                "这份 package.json 不是合法 JSON：{}",
                first_line(&e.to_string())
            ),
            Ok(v) => {
                let deps = v
                    .get("dependencies")
                    .and_then(|x| x.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                let dev = v
                    .get("devDependencies")
                    .and_then(|x| x.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                let scripts = v
                    .get("scripts")
                    .and_then(|x| x.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                if let Some(e) = v
                    .get("engines")
                    .and_then(|x| x.get("node"))
                    .and_then(|x| x.as_str())
                {
                    s.node_engines = Some(e.to_string());
                }
                if let Some(pm) = v.get("packageManager").and_then(|x| x.as_str()) {
                    s.node_package_manager = Some(pm.split('@').next().unwrap_or(pm).to_string());
                }
                format!("依赖 {} 个、开发依赖 {} 个、脚本 {} 条", deps, dev, scripts)
            }
        },
        "python" => {
            if base.to_lowercase().starts_with("requirements") {
                let (total, pinned) = requirements_stats(text);
                format!("{} 条依赖，其中 {} 条钉死了版本（==）", total, pinned)
            } else {
                let mut tags = vec![];
                if text.contains("[project]") {
                    tags.push("PEP 621 声明");
                }
                if text.contains("[tool.poetry]") {
                    tags.push("Poetry");
                }
                if text.contains("[build-system]") {
                    tags.push("有构建后端");
                }
                if let Some(rp) = toml_str(text, "requires-python") {
                    s.py_requires = Some(rp.clone());
                    tags.push("要求 Python");
                    return format!("{}：{}", tags.join("、"), rp);
                }
                if tags.is_empty() {
                    "从里面没读出可辨认的段落".into()
                } else {
                    tags.join("、")
                }
            }
        }
        "java" => {
            let deps = text.matches("<dependency>").count();
            let snaps = text.matches("-SNAPSHOT").count();
            format!("{} 个 <dependency>，其中 {} 处 SNAPSHOT", deps, snaps)
        }
        "rust" => {
            let bins = text.matches("[[bin]]").count();
            let deps = text.matches("[dependencies]").count();
            format!("{} 段 [dependencies]、{} 个 [[bin]]", deps, bins)
        }
        "go" => {
            let mods = text
                .lines()
                .filter(|l| l.starts_with('\t') && l.contains('/'))
                .count();
            format!("require 段里约 {} 条", mods)
        }
        "cc" => {
            let cm = text
                .lines()
                .find(|l| l.trim_start().starts_with("cmake_minimum_required"))
                .map(|l| l.trim().to_string())
                .unwrap_or_default();
            if cm.is_empty() {
                "没读到 cmake_minimum_required".into()
            } else {
                cm
            }
        }
        "dotnet" => format!("{} 行", text.lines().count()),
        "php" => match serde_json::from_str::<serde_json::Value>(text) {
            Err(e) => format!("这份 composer.json 不是合法 JSON：{}", first_line(&e.to_string())),
            Ok(v) => {
                let req = v
                    .get("require")
                    .and_then(|x| x.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                format!("require {} 条", req)
            }
        },
        "ruby" => {
            let gems = text
                .lines()
                .filter(|l| l.trim_start().starts_with("gem "))
                .count();
            format!("{} 个 gem", gems)
        }
        _ => String::new(),
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or(s).trim().to_string()
}

/// 极简 TOML 取值：只在「key = "value"」这种最普通的写法上工作。
fn toml_str(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        if !l.starts_with(key) {
            continue;
        }
        let rest = l[key.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let v = rest.trim().trim_matches('"').trim_matches('\'').trim();
        if v.is_empty() {
            return None;
        }
        return Some(v.to_string());
    }
    None
}

fn requirements_stats(text: &str) -> (usize, usize) {
    let mut total = 0;
    let mut pinned = 0;
    for line in text.lines() {
        let l = line.split('#').next().unwrap_or("").trim();
        if l.is_empty() || l.starts_with('-') {
            continue;
        }
        total += 1;
        if l.contains("==") {
            pinned += 1;
        }
    }
    (total, pinned)
}

/// .env 里有没有看起来像密钥的键。有才把「暴露」升级成要处理的事。
fn env_secret_keys(text: &str) -> Vec<String> {
    let mut out = vec![];
    for line in text.lines().take(200) {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let Some((k, _)) = l.split_once('=') else {
            continue;
        };
        let k = k.trim().to_uppercase();
        if [
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "PASSWD",
            "APIKEY",
            "API_KEY",
            "CREDENTIAL",
            "PRIVATE",
        ]
        .iter()
        .any(|x| k.contains(x))
        {
            out.push(k);
        }
    }
    out
}

// ── 各类条目 ─────────────────────────────────────────────────────────

fn ignore_items(out: &mut Vec<Finding>, s: &ProjectScan, entries: &[String]) {
    if !s.has_gitignore && !s.artifacts.is_empty() {
        let fix: String = s
            .artifacts
            .iter()
            .take(6)
            .map(|a| format!("{}/\n", a.path))
            .collect();
        out.push(it(
            "gitignore-missing",
            if s.has_git {
                Severity::Warn
            } else {
                Severity::Info
            },
            G_IGNORE,
            format!(
                "项目里没有 .gitignore，而已经攒下 {} 个产物目录",
                s.artifacts.len()
            ),
            s.artifacts
                .iter()
                .map(|a| a.path.clone())
                .take(6)
                .collect(),
            vec!["Test-Path .gitignore".into()],
            "如果不是 Git 仓库（或者你根本不打算提交），就没有「污染历史」这回事。",
            format!(
                "在项目根新建 .gitignore，把这几行写进去：\n{}",
                fix.trim_end()
            ),
        ));
    }

    if s.has_gitignore && !s.unignored.is_empty() {
        let fix: String = s
            .unignored
            .iter()
            .take(8)
            .map(|n| format!("{}/\n", n))
            .collect();
        out.push(it(
            "gitignore-missing-entry",
            Severity::Warn,
            G_IGNORE,
            format!(
                "有 {} 个产物目录没写进 .gitignore（{}）",
                s.unignored.len(),
                s.unignored
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            s.unignored.iter().take(8).cloned().collect(),
            vec!["Get-Content .gitignore".into()],
            "如果这些目录早就在仓库里了，光加 .gitignore 不会让它们消失。",
            format!("往 .gitignore 里补这几行：\n{}", fix.trim_end()),
        ));
    }

    if !s.exposed_env.is_empty() {
        let mut secrets: Vec<String> = vec![];
        for e in &s.exposed_env {
            if let Some(t) = read_text(&Path::new(&s.root).join(e)) {
                secrets.extend(env_secret_keys(&t));
            }
        }
        let mut ev: Vec<String> = s.exposed_env.clone();
        if !secrets.is_empty() {
            ev.push(format!("里面像是密钥的键：{}", secrets.join("、")));
        }
        let sev = if !secrets.is_empty() && s.has_git {
            Severity::Warn
        } else {
            Severity::Info
        };
        out.push(it(
            "env-file-exposed",
            sev,
            G_IGNORE,
            if secrets.is_empty() {
                format!("{} 没有被 .gitignore 挡住", s.exposed_env.join("、"))
            } else {
                format!(
                    "{} 里有密钥，但它没被 .gitignore 挡住",
                    s.exposed_env.join("、")
                )
            },
            ev,
            vec![
                "git check-ignore -v .env".into(),
                "Get-Content .gitignore".into(),
            ],
            "如果这个 .env 里只有本机路径、端口这类非机密配置，那它只是个配置文件。",
            "在 .gitignore 里加一行 .env；如果它已经被提交过，再跑一次 git rm --cached .env 把它从索引里摘出来（文件本身不会删）。",
        ));
    }

    if s.has_git {
        let ide: Vec<String> = [".idea", ".vscode"]
            .iter()
            .filter(|d| Path::new(&s.root).join(d).is_dir() && !covers(entries, d))
            .map(|d| d.to_string())
            .collect();
        if !ide.is_empty() {
            out.push(it(
                "ide-dir-present",
                Severity::Info,
                G_IGNORE,
                format!("{} 会跟着提交进去", ide.join("、")),
                vec![
                    "里面通常存着本机绝对路径、运行配置和插件设置".into(),
                    "两个人同时改它，容易互相覆盖".into(),
                ],
                vec!["git check-ignore -v .idea".into()],
                "单人项目、或者确实想把 .vscode/settings.json 分享给同事时，不加也行。",
                "在 .gitignore 里加 .idea/ 和 .vscode/；要分享编辑器配置的话，只保留 .vscode/settings.json。",
            ));
        }
    }

    for g in s.nested_git.iter().take(3) {
        out.push(it(
            "nested-git",
            Severity::Info,
            G_IGNORE,
            format!("{} 里面还有一整套 .git", g),
            vec!["常见于把别人的仓库直接 clone 进了自己的项目里".into()],
            vec![format!("Test-Path {}/.git", g), "git status".into()],
            "如果你只是本地放着一份别人的代码、从不提交外层仓库，那没关系。",
            "要么删掉里面那个 .git（它就变成一个普通文件夹），要么改用 git submodule 正式引进来。",
        ));
    }

    if !s.detritus.is_empty() {
        out.push(it(
            "detritus",
            Severity::Info,
            G_IGNORE,
            format!("{} 个系统/编辑器垃圾文件混在项目里", s.detritus.len()),
            s.detritus.iter().take(6).cloned().collect(),
            vec!["Get-ChildItem -Recurse -Force -Include Thumbs.db,desktop.ini".into()],
            "它们不影响运行，只是看着脏。",
            "删掉即可；Thumbs.db 是 Windows 缩略图缓存，删了还会再长，加进 .gitignore 更省事。",
        ));
    }
}

fn deps_items(out: &mut Vec<Finding>, s: &ProjectScan, root: &Path) {
    let has = |k: &str| s.manifests.iter().any(|m| m.kind == k);
    let py_files = *s.ext_counts.get("py").unwrap_or(&0);
    let py_locks: Vec<&String> = s
        .lockfiles
        .iter()
        .filter(|l| {
            let n = l.to_lowercase();
            n.ends_with("uv.lock")
                || n.ends_with("poetry.lock")
                || n.ends_with("pipfile.lock")
                || n.ends_with("requirements.lock")
                || n.ends_with("requirements-lock.txt")
        })
        .collect();

    if py_files > 0 && !has("python") {
        out.push(it(
            "py-manifest-missing",
            Severity::Info,
            G_DEPS,
            format!("有 {} 个 .py 文件，但没有任何依赖清单", py_files),
            vec!["找不到 pyproject.toml / requirements.txt / Pipfile / setup.py".into()],
            vec!["Get-ChildItem -Recurse -Filter *.py | Measure-Object".into()],
            "如果只用了标准库，或者在写单文件脚本，那确实不需要清单。",
            "在能跑通的环境里跑 python -m pip freeze > requirements.txt；用 uv/poetry 的话就让它们生成。",
        ));
    }
    if has("python") && py_locks.is_empty() && py_files >= 3 {
        out.push(it(
            "py-lock-missing",
            Severity::Info,
            G_DEPS,
            "Python 依赖没有锁文件",
            vec![
                "没有 uv.lock / poetry.lock / Pipfile.lock / requirements.lock".into(),
                "清单里用的是范围版本（>=、~= 或不写版本）时，装到哪一版由当时的上游决定".into(),
            ],
            vec!["Get-ChildItem *.lock,requirements.lock -ErrorAction SilentlyContinue".into()],
            "自己一个人写的小工具、不打算给别人跑，不锁也能过。",
            "uv 项目：uv lock；pip 项目：把 pip freeze 的输出另存成 requirements.lock.txt 一起提交。",
        ));
    }
    let reqs: Vec<&Manifest> = s
        .manifests
        .iter()
        .filter(|m| m.path.to_lowercase().contains("requirements"))
        .collect();
    for r in &reqs {
        if let Some(t) = read_text(&root.join(&r.path)) {
            let (total, pinned) = requirements_stats(&t);
            if total >= 3 && pinned * 2 < total {
                out.push(it(
                    "py-unpinned",
                    Severity::Info,
                    G_DEPS,
                    format!("{} 里 {} 条依赖只有 {} 条钉死了版本", r.path, total, pinned),
                    vec!["剩下的用 >=、~= 或者根本不写版本，今天装和下周装可能不是同一个版本".into()],
                    vec![format!("Select-String -Path {} -Pattern '=='", r.path)],
                    "只要是你一直在同一台机器上跑、也不给别人复现，影响有限。",
                    "把定版输出另存成锁文件，而不是直接改 requirements.txt。",
                ));
            }
        }
    }
    if !reqs.is_empty()
        && s.manifests
            .iter()
            .any(|m| m.path.to_lowercase().ends_with("pyproject.toml"))
    {
        out.push(it(
            "py-multiple-manifests",
            Severity::Info,
            G_DEPS,
            "pyproject.toml 和 requirements.txt 同时在声明依赖",
            vec!["两处都能装依赖，改了一处忘另一处时，症状是「我明明装了但还是 No module named」".into()],
            vec!["Get-ChildItem pyproject.toml,requirements*.txt".into()],
            "如果 requirements.txt 只是给部署用的导出产物，那两份并存是正常的。",
            "把依赖只写在一处：pyproject.toml 是源头，requirements.txt 由它导出。",
        ));
    }

    // ── Node ──
    if has("node") {
        let pkg = s.manifests.iter().find(|m| m.kind == "node");
        let node_locks: Vec<&String> = s
            .lockfiles
            .iter()
            .filter(|l| {
                let n = l.to_lowercase();
                n.ends_with("package-lock.json")
                    || n.ends_with("pnpm-lock.yaml")
                    || n.ends_with("yarn.lock")
                    || n.ends_with("bun.lockb")
            })
            .collect();
        if let Some(p) = pkg {
            if p.note.starts_with("这份 package.json 不是合法 JSON") {
                out.push(it(
                    "package-json-broken",
                    Severity::Error,
                    G_DEPS,
                    "package.json 不是合法 JSON，npm / vite 会直接失败",
                    vec![
                        p.note.clone(),
                        "JSON 不允许注释和多余的逗号，编辑器却往往不报错".into(),
                    ],
                    vec![format!(
                        "node -e \"JSON.parse(require('fs').readFileSync('{}','utf8'))\"",
                        p.path
                    )],
                    "只在文件真的写坏时才出现；解析通过说明没问题。",
                    "按报错里的位置删掉多余的逗号；想写注释就另建一个说明文件。",
                ));
            } else if node_locks.is_empty() {
                out.push(it(
                    "node-lock-missing",
                    Severity::Warn,
                    G_DEPS,
                    "没有 package-lock.json / pnpm-lock.yaml，依赖版本会漂",
                    vec![
                        "npm install 每次算出来的依赖树可能不同（尤其传递依赖）".into(),
                        "别人 clone 下来跑不通时，你没法复现他装的是什么".into(),
                    ],
                    vec!["Get-ChildItem package-lock.json,pnpm-lock.yaml,yarn.lock -ErrorAction SilentlyContinue".into()],
                    "只有你自己的学习项目、而且 node_modules 还在本地时，暂时不影响。",
                    "跑一次 npm install（或 pnpm install）把它生成出来，然后跟 package.json 一起提交。",
                ));
            } else if let Some(lock) = node_locks.first() {
                if let (Some(lt), Some(pt)) = (mtime(&root.join(lock)), mtime(&root.join(&p.path))) {
                    if let Ok(d) = pt.duration_since(lt) {
                        if d > Duration::from_secs(86400) {
                            out.push(it(
                                "node-lock-stale",
                                Severity::Warn,
                                G_DEPS,
                                format!("package.json 比 {} 新了 {} 天", lock, d.as_secs() / 86400),
                                vec![
                                    "说明改过依赖之后没重新装过".into(),
                                    "装出来的东西和锁文件记录的不一致，报的错会很奇怪".into(),
                                ],
                                vec![format!(
                                    "Get-Item {},{} | Select-Object Name,LastWriteTime",
                                    p.path, lock
                                )],
                                "刚 clone 下来、还没装过依赖的项目也会长这样。",
                                "重新跑一次安装，让锁文件跟上，然后一起提交。",
                            ));
                        }
                    }
                }
            }
        }
        let scripts_missing = s
            .manifests
            .iter()
            .find(|m| m.kind == "node")
            .map(|m| m.note.contains("脚本 0 条"))
            .unwrap_or(false);
        if scripts_missing {
            out.push(it(
                "node-no-scripts",
                Severity::Info,
                G_HABIT,
                "package.json 里没有 scripts",
                vec!["新拿到这个项目的人（包括三个月后的你）不知道该怎么启动它".into()],
                vec!["Select-String -Path package.json -Pattern '\"scripts\"'".into()],
                "纯函数库、只被别人 import 时，确实可以没有。",
                "加上 \"scripts\": { \"dev\": \"vite\", \"build\": \"vite build\", \"test\": \"vitest\" } 这样的入口。",
            ));
        }
    }

    // ── Java / Maven ──
    if has("java") {
        let wrapper = root.join("mvnw.cmd").exists() || root.join("mvnw").exists();
        if let Some(t) = read_text(&root.join("pom.xml")) {
            if t.contains("-SNAPSHOT") {
                let hits: Vec<String> = t
                    .lines()
                    .filter(|l| l.contains("-SNAPSHOT"))
                    .take(4)
                    .map(|l| l.trim().to_string())
                    .collect();
                out.push(it(
                    "maven-snapshot-dep",
                    Severity::Warn,
                    G_DEPS,
                    "pom.xml 里有 SNAPSHOT 依赖",
                    hits,
                    vec!["Select-String -Path pom.xml -Pattern SNAPSHOT".into()],
                    "只在跨团队联调、上游还没发正式版时才该用；自己练手时完全可以换成正式版。",
                    "换成一个已发布的版本号；「昨天还好好的」这类神鬼问题多半就是从这来的。",
                ));
            }
            let dupes = maven_version_dupes(&t);
            if !dupes.is_empty() {
                out.push(it(
                    "maven-version-conflict",
                    Severity::Warn,
                    G_DEPS,
                    format!("同名依赖在 pom 里出现了多个版本：{}", dupes.join("、")),
                    vec!["Maven 只会挑一个用，挑哪个取决于声明顺序和依赖调解，不是你能一眼看出的".into()],
                    vec![format!("Select-String -Path pom.xml -Pattern '{}'", dupes[0])],
                    "如果它们分别在 <dependencyManagement> 和 <dependencies> 里，那是正常的覆盖写法。",
                    "用 mvn dependency:tree 看最终生效的版本；同一份 pom 里同名依赖只留一处版本。",
                ));
            }
            if !t.contains("project.build.sourceEncoding") {
                out.push(it(
                    "maven-encoding-unset",
                    Severity::Info,
                    G_HABIT,
                    "pom.xml 没有声明源码编码",
                    vec![
                        "Windows 上 javac 默认按系统编码（中文系统是 GBK）读源码".into(),
                        "结果是中文注释/字符串变乱码，或者直接报 unmappable character".into(),
                    ],
                    vec!["Select-String -Path pom.xml -Pattern sourceEncoding".into()],
                    "如果项目全是英文、又只在 IDEA 里跑，影响小——IDEA 自己会设编码。",
                    "在 <properties> 里加 <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>。",
                ));
            }
            if !wrapper {
                out.push(it(
                    "maven-no-wrapper",
                    Severity::Info,
                    G_HABIT,
                    "项目里没有 mvnw（Maven Wrapper）",
                    vec![
                        "你本机装的是某个版本的 Maven，换台机器可能就换了版本，插件行为会不一样".into(),
                        "wrapper 把 Maven 版本钉在项目里，别人拿到就能用同一个版本构建".into(),
                    ],
                    vec!["Test-Path mvnw.cmd".into()],
                    "只有你一台机器在跑，就没必要。",
                    "在项目根跑一次 mvn wrapper:wrapper，会生成 mvnw / mvnw.cmd / .mvn 三样东西。",
                ));
            }
        }
    }
    if has("gradle") && !root.join("gradlew").exists() && !root.join("gradlew.bat").exists() {
        out.push(it(
            "gradle-no-wrapper",
            Severity::Info,
            G_HABIT,
            "Gradle 项目里没有 gradlew",
            vec!["和 Maven 同理：Gradle 各版本差异比 Maven 更大".into()],
            vec!["Test-Path gradlew.bat".into()],
            "只有你一台机器在跑时无所谓。",
            "在项目根跑 gradle wrapper，会生成 gradlew / gradlew.bat / gradle/wrapper。",
        ));
    }

    // ── Rust / Go / C++ ──
    if has("rust") && root.join("src").join("main.rs").exists() && !root.join("Cargo.lock").exists() {
        out.push(it(
            "cargo-lock-missing",
            Severity::Info,
            G_DEPS,
            "可执行程序没有 Cargo.lock",
            vec![
                "src/main.rs 说明这是个能跑的程序，而程序应该提交 Cargo.lock".into(),
                "不提交的话，别人 cargo build 出来的依赖版本和你不一样".into(),
            ],
            vec!["Test-Path Cargo.lock".into()],
            "纯库（只有 src/lib.rs）按惯例不提交 Cargo.lock，那时别管这条。",
            "跑一次 cargo build，然后提交生成的 Cargo.lock。",
        ));
    }
    if has("go") && !root.join("go.sum").exists() {
        out.push(it(
            "go-sum-missing",
            Severity::Info,
            G_DEPS,
            "go.mod 在，但没有 go.sum",
            vec!["go.sum 记录依赖内容的校验和，缺了它构建会报 checksum mismatch".into()],
            vec!["Test-Path go.sum".into()],
            "只有标准库、或者还没引入过外部包时，go.sum 本来就不存在。",
            "跑一次 go mod tidy 就会生成。",
        ));
    }
    if has("cc") {
        let build = ["build", "cmake-build-debug", "out"]
            .iter()
            .map(|d| root.join(d))
            .find(|d| d.is_dir());
        if let Some(b) = build {
            if !b.join("compile_commands.json").exists() {
                let rel = rel_of(root, &b);
                out.push(it(
                    "cc-no-compile-commands",
                    Severity::Info,
                    G_HABIT,
                    "CMake 已经构建过，但没有 compile_commands.json",
                    vec![
                        format!("在 {} 里没找到它", rel),
                        "编辑器（VS Code / CLion）靠这份文件才认得每个源文件该带哪些头文件路径".into(),
                        "没有它，补全和「跳转到定义」对第三方库基本失效".into(),
                    ],
                    vec![format!("Test-Path {}\\compile_commands.json", rel)],
                    "只用终端 gcc 编译、不在编辑器里读代码时，不需要。",
                    "重新配置一次：cmake -S . -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON",
                ));
            }
        }
    }

    // ── PHP / Ruby：国内中小项目里还很常见，锁文件的坑和 npm 一样 ──
    if has("php")
        && !s
            .lockfiles
            .iter()
            .any(|l| l.to_lowercase().ends_with("composer.lock"))
    {
        out.push(it(
            "php-lock-missing",
            Severity::Warn,
            G_DEPS,
            "没有 composer.lock",
            vec![
                "composer.json 写的是版本范围，实际装到哪一版由当时的上游决定".into(),
                "换台机器 composer install 出来的依赖可能不一样".into(),
            ],
            vec!["Test-Path composer.lock".into()],
            "应用项目必须提交它；只有你自己写的、给别人当库用的包才不提交。",
            "跑一次 composer install（或 update）生成 composer.lock，然后提交。",
        ));
    }
    if has("ruby")
        && !s
            .lockfiles
            .iter()
            .any(|l| l.to_lowercase().ends_with("gemfile.lock"))
    {
        out.push(it(
            "ruby-lock-missing",
            Severity::Warn,
            G_DEPS,
            "没有 Gemfile.lock",
            vec!["和 composer.lock 同理：Bundle 装出来的版本会漂".into()],
            vec!["Test-Path Gemfile.lock".into()],
            "只有你一台机器在跑、并且 bundle 目录还在本地时，暂时不影响。",
            "跑一次 bundle install 生成 Gemfile.lock，然后提交。",
        ));
    }

    // ── Docker：容器本身没错，缺 .dockerignore 才是那个让人卡住的坑 ──
    let has_dockerfile = s
        .all_files
        .iter()
        .any(|f| f.rsplit('/').next().unwrap_or(f).eq_ignore_ascii_case("dockerfile"));
    if has_dockerfile {
        let has_ignore = s
            .all_files
            .iter()
            .any(|f| f.rsplit('/').next().unwrap_or(f).eq_ignore_ascii_case(".dockerignore"));
        if !has_ignore {
            out.push(it(
                "docker-no-ignore",
                Severity::Info,
                G_HABIT,
                "有 Dockerfile，但没有 .dockerignore",
                vec![
                    "docker build 会把整个上下文目录发给构建引擎，然后再 COPY".into(),
                    "node_modules / .git / target 动辄几万个文件，构建会慢很多倍，镜像也白白变大".into(),
                ],
                vec!["Test-Path .dockerignore".into()],
                "如果这个项目没什么大目录、构建也很快，就不用加。",
                "在项目根加一份 .dockerignore，起码写上 node_modules/、.git/、target/、__pycache__/。",
            ));
        }
    }
}

/// 同一份 pom 里同一个 artifactId 挂了多个不同版本。
fn maven_version_dupes(text: &str) -> Vec<String> {
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cur: Option<String> = None;
    let mut in_mgmt = false;
    let mut in_deps = false;
    for raw in text.lines() {
        let l = raw.trim();
        if l.contains("<dependencyManagement>") {
            in_mgmt = true;
        }
        if l.contains("</dependencyManagement>") {
            in_mgmt = false;
        }
        if in_mgmt {
            // 管理段里写版本是正常写法，不算冲突
            cur = None;
            continue;
        }
        if l.contains("<dependencies>") {
            in_deps = true;
        }
        if l.contains("</dependencies>") {
            in_deps = false;
            cur = None;
            continue;
        }
        if !in_deps {
            // 只管 <dependencies> 里的：<plugins> 里同名是另一码事
            continue;
        }
        if let Some(v) = tag_value(l, "artifactId") {
            cur = Some(v);
        }
        if let Some(ver) = tag_value(l, "version") {
            if let Some(a) = &cur {
                seen.entry(a.clone()).or_default().push(ver);
            }
        }
    }
    seen.into_iter()
        .filter(|(_, vs)| {
            let uniq: std::collections::BTreeSet<&String> = vs.iter().collect();
            uniq.len() > 1
        })
        .map(|(k, _)| k)
        .take(4)
        .collect()
}

fn tag_value(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = line.find(&open)? + open.len();
    let j = line[i..].find(&close)? + i;
    let v = line[i..j].trim();
    (!v.is_empty() && !v.contains("${")).then(|| v.to_string())
}

fn habit_items(out: &mut Vec<Finding>, s: &ProjectScan, w: &Walk) {
    // 路径带中文/空格：一部分老工具链会在这里翻车
    let p = s.root.clone();
    let non_ascii = !p.is_ascii();
    let has_space = p.contains(' ');
    if non_ascii || has_space {
        let cc = s.langs.iter().any(|l| l == "C/C++" || l == "Rust");
        let title = if has_space && non_ascii {
            "项目路径里既有中文又有空格"
        } else if non_ascii {
            "项目路径里有中文"
        } else {
            "项目路径里有空格"
        };
        out.push(it(
            "project-path-unsafe",
            if cc { Severity::Warn } else { Severity::Info },
            G_HABIT,
            title,
            vec![
                format!("当前路径：{}", p),
                if cc {
                    "这个项目用到了 C/C++ 工具链：MinGW 的 make、部分 MSVC 组件最容易在这里报「找不到文件」".into()
                } else {
                    "纯 Python / Node / Java 一般能忍，但 shell 脚本和一部分命令行工具会需要额外加引号".into()
                },
            ],
            vec!["(Get-Item .).FullName".into()],
            "如果到现在都没遇到过奇怪报错，那就不用动它。",
            "把项目放到纯英文、无空格的路径下（例如 D:\\code\\myapp），比事后排查便宜得多。",
        ));
    }

    if s.longest_path >= LONG_PATH {
        out.push(it(
            "long-path",
            Severity::Warn,
            G_HABIT,
            format!("最深的一条路径已经 {} 个字符", s.longest_path),
            vec![
                format!("最深：{}", truncate_mid(&s.longest_file, 90)),
                "Windows 传统接口的上限是 260 字符；逼近之后工具报的是「找不到文件」，而你去看它明明在".into(),
                "Node、CMake、Gradle 最容易先撞上".into(),
            ],
            vec![
                "(Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\FileSystem').LongPathsEnabled".into(),
                "(Get-Item .).FullName.Length".into(),
            ],
            "只差得远（远小于 220）时不用管；已经开过长路径支持的话，多数工具能忍。",
            "把项目挪到更短的路径（少一层长目录名往往就够），或者开启长路径支持后重启。",
        ));
    }

    // 只看真正的源码文件：一堆 .md/.json 平铺在根目录不算「代码堆在一起」。
    let code_files: usize = [
        "py", "js", "ts", "jsx", "tsx", "java", "kt", "c", "h", "cpp", "hpp", "cc", "rs", "go",
        "cs", "php", "rb", "vue",
    ]
    .iter()
    .map(|e| *s.ext_counts.get(*e).unwrap_or(&0))
    .sum();
    if !w.src_dir && !w.test_dir && code_files >= 8 {
        out.push(it(
            "no-src-test-layout",
            Severity::Info,
            G_HABIT,
            "代码都在项目根目录下平铺，看不到 src / tests 这类划分",
            vec![format!("根附近能看到的源码文件有 {} 个", code_files)],
            vec!["Get-ChildItem".into()],
            "三五天就丢掉的练习脚本，平铺反而顺手。",
            "代码进 src/，测试进 tests/。不是为了好看：以后要「只对源码跑 lint」「打包排除测试」才有的挑。",
        ));
    }

    if w.src_dir && !w.test_dir && *s.ext_counts.get("py").unwrap_or(&0) >= 3 {
        out.push(it(
            "no-tests",
            Severity::Info,
            G_HABIT,
            "有源码目录，但还没有放测试的地方",
            vec!["看不到 tests / test 目录".into()],
            vec!["Get-ChildItem -Directory".into()],
            "刚起步阶段先跑通再说，这条永远可以放着。",
            "等有一个函数你改了三次都怕改坏时，就是建 tests/ 的时候——先把那一个函数测住。",
        ));
    }
}

fn size_items(out: &mut Vec<Finding>, s: &ProjectScan, deep: bool, truncated: bool) {
    if deep && !s.artifacts.is_empty() {
        let total: f64 = s.artifacts.iter().map(|a| a.size_mb).sum();
        out.push(it(
            "project-artifacts",
            Severity::Info,
            G_SIZE,
            format!(
                "{} 个可重新生成的目录，合计 {:.0} MB",
                s.artifacts.len(),
                total
            ),
            s.artifacts
                .iter()
                .take(5)
                .map(|a| format!("{}：{:.0} MB / {} 个文件", a.path, a.size_mb, a.files))
                .collect(),
            vec!["Get-ChildItem -Directory".into()],
            "它们留着也不影响运行，只是占地方。",
            "在「清理」页勾选后删除即可——下次构建会重新长出来。",
        ));
    }
    if deep && !s.big_files.is_empty() {
        out.push(it(
            "large-file",
            Severity::Warn,
            G_SIZE,
            format!(
                "有 {} 个超过 {:.0} MB 的文件在项目里",
                s.big_files.len(),
                BIG_FILE_MB
            ),
            s.big_files
                .iter()
                .take(5)
                .map(|f| format!("{}：{:.1} MB", f.path, f.size_mb))
                .collect(),
            vec!["Get-ChildItem -Recurse -File | Where-Object {$_.Length -gt 20MB} | Sort-Object Length -Descending".into()],
            "数据集、模型权重、安装包本来就该放在项目外面或者 .gitignore 里——前提是你确认过它没被提交。",
            "先确认它有没有进过仓库：git log --oneline -- 文件名。进过的话，删掉文件也救不回历史体积。",
        ));
    }
    if truncated {
        out.push(it(
            "scan-truncated",
            Severity::Info,
            G_SIZE,
            format!("这次只扫了前 {} 个文件", s.files_scanned),
            vec!["目录太大时工具会主动收手，避免一次体检卡住你的编辑器".into()],
            vec!["(Get-ChildItem -Recurse -File | Measure-Object).Count".into()],
            "只要没出现「没找到」的结论，就说明关键文件都在扫描范围里。",
            "需要完整的体积分布时，用 WizTree 这类专门工具看。",
        ));
    }
}

fn truncate_mid(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_string();
    }
    let head: String = chars[..n / 2].iter().collect();
    let tail: String = chars[chars.len() - n / 2..].iter().collect();
    format!("{}…{}", head, tail)
}

/// 「能清出多少」——给界面用的一句话。
pub fn artifact_summary(s: &ProjectScan) -> Option<String> {
    if s.artifacts.is_empty() {
        return None;
    }
    let total: f64 = s.artifacts.iter().map(|a| a.size_mb).sum();
    if total > 0.0 {
        Some(format!(
            "项目里有 {} 个可重新生成的目录，合计 {:.0} MB（最大的是 {}）",
            s.artifacts.len(),
            total,
            s.artifacts[0].path
        ))
    } else {
        Some(format!(
            "项目里有 {} 个可重新生成的目录（体积要看深扫）",
            s.artifacts.len()
        ))
    }
}

impl ProjectScan {
    /// 体检结果里只存了路径字符串；需要再落到磁盘上看东西时用它。
    pub(crate) fn root_path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.root)
    }
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-proj-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn has(s: &ProjectScan, id: &str) -> bool {
        s.items.iter().any(|i| i.id == id)
    }

    #[test]
    fn unignored_artifacts_are_named() {
        let d = scratch("basic");
        std::fs::create_dir_all(d.join("node_modules").join("x")).unwrap();
        std::fs::write(d.join("node_modules").join("x").join("a.js"), "1").unwrap();
        std::fs::create_dir_all(d.join("__pycache__")).unwrap();
        std::fs::write(d.join("__pycache__").join("a.pyc"), "1").unwrap();
        std::fs::write(d.join(".gitignore"), "node_modules/\n").unwrap();
        let s = scan(&d);
        assert!(s.ignored.iter().any(|x| x == "node_modules"));
        assert!(s.unignored.iter().any(|x| x == "__pycache__"));
        assert!(has(&s, "gitignore-missing-entry"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plain_directory_named_env_is_not_an_artifact() {
        let d = scratch("envdir");
        std::fs::create_dir_all(d.join("env")).unwrap();
        std::fs::write(d.join("env").join("config.json"), "{}").unwrap();
        let s = scan(&d);
        assert!(s.artifacts.is_empty(), "普通 env 目录不该被判成产物");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_real_venv_is_an_artifact() {
        let d = scratch("venv");
        std::fs::create_dir_all(d.join(".venv").join("Scripts")).unwrap();
        std::fs::write(d.join(".venv").join("pyvenv.cfg"), "home = x").unwrap();
        let s = scan(&d);
        assert!(s.artifacts.iter().any(|a| a.path == ".venv"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn without_gitignore_we_do_not_report_missing_entries() {
        let d = scratch("nogitignore");
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join("target").join("a.class"), "1").unwrap();
        let s = scan(&d);
        assert!(!s.has_gitignore);
        assert!(!has(&s, "gitignore-missing-entry"));
        assert!(has(&s, "gitignore-missing"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_package_json_is_an_error_with_a_verify_command() {
        let d = scratch("badjson");
        std::fs::write(d.join("package.json"), "{ \"name\": \"x\", }").unwrap();
        let s = scan(&d);
        let item = s
            .items
            .iter()
            .find(|i| i.id == "package-json-broken")
            .expect("应该报出来");
        assert_eq!(item.severity, Severity::Error);
        assert!(!item.verify.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_with_secrets_in_a_git_repo_is_flagged() {
        let d = scratch("env");
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".env"), "API_KEY=abc\nPORT=8080\n").unwrap();
        let s = scan(&d);
        let item = s
            .items
            .iter()
            .find(|i| i.id == "env-file-exposed")
            .expect("应该报出来");
        assert_eq!(item.severity, Severity::Warn);
        assert!(item.evidence.iter().any(|e| e.contains("API_KEY")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_without_secrets_stays_info() {
        let d = scratch("envplain");
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".env"), "PORT=8080\n").unwrap();
        let s = scan(&d);
        let item = s.items.iter().find(|i| i.id == "env-file-exposed").unwrap();
        assert_eq!(item.severity, Severity::Info);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pom_without_source_encoding_is_flagged() {
        let d = scratch("pom");
        std::fs::write(
            d.join("pom.xml"),
            "<project><dependencies><dependency><artifactId>okhttp</artifactId><version>4.9.3</version></dependency></dependencies></project>",
        )
        .unwrap();
        let s = scan(&d);
        assert!(has(&s, "maven-encoding-unset"));
        assert!(!has(&s, "maven-version-conflict"));
        assert!(!has(&s, "maven-snapshot-dep"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn same_artifact_with_two_versions_is_flagged() {
        let d = scratch("dupver");
        std::fs::write(
            d.join("pom.xml"),
            "<project>\n<dependencies>\n\
             <dependency><artifactId>guava</artifactId><version>30.0-jre</version></dependency>\n\
             <dependency><artifactId>guava</artifactId><version>32.0-jre</version></dependency>\n\
             </dependencies>\n</project>\n",
        )
        .unwrap();
        let s = scan(&d);
        assert!(has(&s, "maven-version-conflict"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 管理段里写版本是正常写法，不许被说成冲突。
    #[test]
    fn dependency_management_is_not_a_version_conflict() {
        let d = scratch("mgmt");
        std::fs::write(
            d.join("pom.xml"),
            "<project>\n<dependencyManagement>\n<dependencies>\n\
             <dependency><artifactId>guava</artifactId><version>32.0-jre</version></dependency>\n\
             </dependencies>\n</dependencyManagement>\n\
             <dependencies>\n\
             <dependency><artifactId>guava</artifactId><version>32.0-jre</version></dependency>\n\
             </dependencies>\n</project>\n",
        )
        .unwrap();
        let s = scan(&d);
        assert!(!has(&s, "maven-version-conflict"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn requirements_that_are_not_pinned_are_flagged() {
        let d = scratch("reqs");
        std::fs::create_dir_all(d.join("src")).unwrap();
        for n in ["a", "b", "c"] {
            std::fs::write(d.join("src").join(format!("{}.py", n)), "x = 1\n").unwrap();
        }
        std::fs::write(d.join("requirements.txt"), "requests>=2.0\nflask\nnumpy\n").unwrap();
        let s = scan(&d);
        assert!(has(&s, "py-unpinned"));
        assert!(has(&s, "py-lock-missing"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn node_without_lock_is_a_warning() {
        let d = scratch("nodelock");
        std::fs::write(d.join("package.json"), "{\"name\":\"x\",\"version\":\"1.0.0\"}").unwrap();
        let s = scan(&d);
        assert!(has(&s, "node-lock-missing"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn deep_scan_measures_artifact_size_and_quick_scan_does_not() {
        let d = scratch("size");
        let nm = d.join("node_modules").join("pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("big.js"), "x".repeat(200_000)).unwrap();
        let quick = scan(&d);
        assert!(quick.artifacts.iter().any(|a| a.path == "node_modules"));
        assert_eq!(quick.artifacts[0].size_mb, 0.0);
        let deeps = scan_deep(&d);
        assert!(deeps.artifacts[0].size_mb > 0.1);
        assert!(has(&deeps, "project-artifacts"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nested_git_is_noticed() {
        let d = scratch("nested");
        std::fs::create_dir_all(d.join("vendor").join("lib").join(".git")).unwrap();
        std::fs::write(d.join("vendor").join("lib").join("a.txt"), "1").unwrap();
        let s = scan(&d);
        assert!(has(&s, "nested-git"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn every_item_has_a_verify_command_and_a_title() {
        let d = scratch("verify");
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        std::fs::write(d.join("package.json"), "{}").unwrap();
        std::fs::write(d.join("requirements.txt"), "requests>=2\nflask\nnumpy\n").unwrap();
        std::fs::write(d.join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        let s = scan_deep(&d);
        for i in &s.items {
            assert!(!i.verify.is_empty(), "条目 {} 没有自查命令", i.id);
            assert!(!i.title.is_empty(), "条目 {} 没有标题", i.id);
            assert!(
                HYGIENE_IDS.contains(&i.id.as_str()),
                "条目 {} 没登记进 HYGIENE_IDS，它会被丢进「其他」层",
                i.id
            );
            assert!(!i.group.is_empty(), "条目 {} 没有分组", i.id);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn hygiene_ids_have_no_duplicates() {
        let uniq: std::collections::BTreeSet<&str> = HYGIENE_IDS.iter().copied().collect();
        assert_eq!(uniq.len(), HYGIENE_IDS.len());
    }

    /// PHP / Ruby / Docker 这三类也要有话说，而且说的是事实。
    #[test]
    fn php_ruby_and_docker_projects_get_the_right_notes() {
        let d = scratch("polyglot");
        std::fs::write(d.join("composer.json"), "{\"require\":{\"monolog/monolog\":\"^3\"}}").unwrap();
        std::fs::write(d.join("Gemfile"), "source 'https://rubygems.org'\ngem 'rails'\n").unwrap();
        std::fs::write(d.join("Dockerfile"), "FROM node:20\nCOPY . .\n").unwrap();
        let s = scan(&d);
        assert!(has(&s, "php-lock-missing"), "composer.json 没有锁文件该报");
        assert!(has(&s, "ruby-lock-missing"), "Gemfile 没有锁文件该报");
        assert!(has(&s, "docker-no-ignore"), "Dockerfile 没有 .dockerignore 该报");
        assert_eq!(
            s.manifests.iter().find(|m| m.kind == "php").map(|m| m.note.as_str()),
            Some("require 1 条")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 有 .dockerignore 时不许再念叨。
    #[test]
    fn a_dockerignore_shuts_the_docker_note_up() {
        let d = scratch("dockerignore");
        std::fs::write(d.join("Dockerfile"), "FROM scratch\n").unwrap();
        std::fs::write(d.join(".dockerignore"), "node_modules\n").unwrap();
        let s = scan(&d);
        assert!(!has(&s, "docker-no-ignore"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
