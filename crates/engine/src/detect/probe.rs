use crate::contract::Contract;
use crate::facts::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 跑一条命令拿输出；超时返回 None（探针永不阻塞）
// 这两个是纯工具，已经下沉到 `winutil`（Q6 拆循环）。
// 这里转发一下：**所有调用点都不用改**，而依赖方向从此是单向的。
pub use crate::winutil::{run_cmd, which};

// ---------------------------------------------------------------- system

const WHITELIST_ENV: &[&str] = &[
    "PYTHONUTF8",
    "PYTHONIOENCODING",
    "PYTHONPATH",
    "PYTHONHOME",
    "VIRTUAL_ENV",
    "CONDA_PREFIX",
    "HTTP_PROXY",
    "HTTPS_PROXY",
];

pub fn probe_system() -> SystemFacts {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let entries: Vec<String> = std::env::split_paths(&path_var)
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.trim().is_empty())
        .collect();
    let mut env = BTreeMap::new();
    for k in WHITELIST_ENV {
        if let Ok(v) = std::env::var(k) {
            env.insert(k.to_string(), v);
        }
    }
    SystemFacts {
        os: format!("{} {}", std::env::consts::OS, os_release()),
        machine: std::env::consts::ARCH.to_string(),
        cwd: std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        user: std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_default(),
        stdout_encoding: std::env::var("PYTHONIOENCODING").unwrap_or_else(|_| "unknown".into()),
        path: PathFacts {
            count: entries.len(),
            hash: stable_hash(&entries),
            // 刻意不落条目：快照会被复制、会进 git，PATH 里的用户名和目录名
            // 属于私人信息。只留条数和 hash，够判断「变没变」了。
            sample: Vec::new(),
        },
        env,
    }
}

fn os_release() -> String {
    std::env::var("OS").unwrap_or_else(|_| {
        if cfg!(target_os = "windows") {
            "windows".into()
        } else {
            "unknown".into()
        }
    })
}

// ---------------------------------------------------------------- python

/// 一次子进程拿到 Python 的全部关键事实（省时间，也避免多次干扰）
const PY_PROBE: &str = r#"
import sys, os, json, platform, struct, importlib.metadata as md, importlib.util as iu
def pths():
    out = []
    for base in {os.path.dirname(sys.executable), getattr(sys, 'base_prefix', '')}:
        try:
            out += [os.path.join(base, f) for f in os.listdir(base) if f.endswith('._pth')]
        except Exception:
            pass
    return sorted(set(out))
inst = []
try:
    for d in md.distributions():
        n = (d.metadata['Name'] or '').strip()
        if n:
            inst.append(n + '==' + str(d.version))
except Exception:
    pass
inst.sort()
print(json.dumps({
  'exe': sys.executable,
  'version': '%d.%d.%d' % sys.version_info[:3],
  'isolated': bool(sys.flags.isolated),
  'in_venv': sys.prefix != getattr(sys, 'base_prefix', sys.prefix),
  'prefix': sys.prefix,
  'base_prefix': getattr(sys, 'base_prefix', sys.prefix),
  'pth_files': pths(),
  'site_packages': [p for p in sys.path if p.endswith('site-packages')],
  'sys_path0': (sys.path[0] if sys.path else ''),
  'installed': inst,
  'pip': bool(iu.find_spec('pip')),
  'bits': struct.calcsize('P') * 8,
  'arch': platform.machine(),
  'abi': getattr(sys.implementation, 'cache_tag', ''),
  'site_dirs': [p for p in sys.path if p.endswith('site-packages')],
}))
"#;

pub fn probe_python(exe: &Path) -> PythonFacts {
    let Some((ok, out)) = run_cmd(exe, &["-c", PY_PROBE], Duration::from_secs(30)) else {
        return PythonFacts::default();
    };
    if !ok {
        return PythonFacts::default();
    }
    let line = out.lines().find(|l| l.trim_start().starts_with('{')).unwrap_or("");
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return PythonFacts::default(),
    };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let b = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let arr = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    PythonFacts {
        found: true,
        exe: s("exe"),
        version: s("version"),
        isolated: b("isolated"),
        pth_files: arr("pth_files"),
        in_venv: b("in_venv"),
        prefix: s("prefix"),
        base_prefix: s("base_prefix"),
        site_packages: arr("site_packages"),
        sys_path0: s("sys_path0"),
        installed: arr("installed"),
        pip: b("pip"),
        path_on_path: which("python.exe").map(|p| p.to_string_lossy().to_string()),
        bits: v.get("bits").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        arch: s("arch"),
        abi: s("abi"),
        site_dirs: arr("site_dirs"),
    }
}

/// 挨个试，返回第一个真的跑得起来的解释器。
///
/// 为什么不直接用第一个候选：PATH 里经常躺着 Windows 应用商店的
/// `python.exe` 占位符——它「存在」，一跑却只是弹商店。不真跑一次，
/// 就会把「装了 Python 但 PATH 里有幽灵」误判成「没装 Python」。
pub fn first_working(
    root: &Path,
    contract: Option<&Contract>,
    limit: usize,
) -> Option<(PathBuf, PythonFacts)> {
    for c in find_pythons(root, contract, false).into_iter().take(limit) {
        let f = probe_python(&c.path);
        if f.found {
            return Some((c.path, f));
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct PyCandidate {
    pub path: PathBuf,
    pub source: String,
    pub version: Option<String>,
    /// 环境变量 / 契约 / 项目 venv 明确指定的解释器。
    /// 排序时永远置顶——启发式规则不许把用户自己选的那个挤下去。
    pub explicit: bool,
}

/// 找这台机器上所有能用的 Python：契约指定的 → 项目 venv → PATH → 常见安装位置
fn push_with(p: PathBuf, src: &str, explicit: bool, out: &mut Vec<PyCandidate>) {
    if p.is_file()
        && !out
            .iter()
            .any(|c| c.path.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy()))
    {
        out.push(PyCandidate {
            path: p,
            source: src.into(),
            version: None,
            explicit,
        });
    }
}

fn push_cand(p: PathBuf, src: &str, out: &mut Vec<PyCandidate>) {
    push_with(p, src, false, out);
}

fn push_explicit(p: PathBuf, src: &str, out: &mut Vec<PyCandidate>) {
    push_with(p, src, true, out);
}

/// 扫一个目录的直接子目录，看里面有没有 python.exe（解压版/便携版都这么躺）
fn scan_children(dir: &Path, src: &str, out: &mut Vec<PyCandidate>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        push_cand(p.join("python.exe"), src, out);
        push_cand(p.join("bin").join("python.exe"), src, out);
    }
}

/// `include_versions` 会为每个候选真跑一次 `python -V`。
/// 只有 `jingwei envs` 需要版本号——`doctor` / `run` 反正要对选中的那个做完整探测，
/// 这里再花一次进程启动纯属浪费（每次约 80ms）。
pub fn find_pythons(root: &Path, contract: Option<&Contract>, include_versions: bool) -> Vec<PyCandidate> {
    let mut out: Vec<PyCandidate> = Vec::new();
    let push = push_cand;

    // 环境变量改过名，两个都认：老用户机器上多半还挂着 DRYDOCK_PYTHON，
    // 只认新名字会让他"明明设过了却读不到"，而这种错最难自己想明白。
    for var in ["JINGWEI_PYTHON", "DRYDOCK_PYTHON"] {
        if let Ok(p) = std::env::var(var) {
            if !p.trim().is_empty() {
                push_explicit(PathBuf::from(&p), &format!("环境变量 {var}"), &mut out);
            }
        }
    }
    if let Some(c) = contract {
        if let Some(e) = &c.runtime.python_exe {
            push_explicit(PathBuf::from(e), "项目契约绑定", &mut out);
        }
    }
    for name in [".venv", "venv", "env"] {
        push_explicit(
            root.join(name).join("Scripts").join("python.exe"),
            "项目内虚拟环境",
            &mut out,
        );
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for exe in ["python.exe", "python3.exe"] {
                let p = dir.join(exe);
                if p.is_file() {
                    // 这里刻意不排除 WindowsApps：那里既有 0 字节的商店占位符，
                    // 也有商店版/管理器版的真实解释器。一律按「是不是真安装」排序，
                    // 排除掉反而会让装了商店版的人彻底找不着 Python。
                    push(p, "PATH", &mut out);
                }
            }
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(local);
        scan_children(&base.join("Programs").join("Python"), "常见安装位置", &mut out);
        scan_children(&base.join("uv").join("python"), "uv 安装的 Python", &mut out);
        // Python 安装管理器（py 的 pythoncore-*）装在这里，
        // 这是 3.14 之后官方推荐的位置，不会再出现在 PATH 里。
        scan_children(&base.join("Python"), "Python 安装管理器", &mut out);
        scan_children(&base.join("Microsoft").join("WindowsApps"), "应用商店别名", &mut out);
    }
    for c in ["C:\\Python312", "C:\\Python311", "C:\\Python310"] {
        push(PathBuf::from(c).join("python.exe"), "常见安装位置", &mut out);
    }

    // Windows 自带的 py 启动器知道所有登记过的解释器，是这里最可靠的一条线
    if let Some(py) = which("py.exe") {
        if let Some((true, txt)) = run_cmd(&py, &["-0p"], Duration::from_secs(10)) {
            for tok in txt.split_whitespace() {
                if tok.to_lowercase().ends_with("python.exe") {
                    push(PathBuf::from(tok), "py 启动器", &mut out);
                }
            }
        }
    }

    // 解压版/便携版：开发者习惯把它们塞在自己的工具目录下，
    // 这类解释器不在 PATH 里，也不在注册表里，只能靠目录特征找。
    for drv in ["C:\\", "D:\\", "E:\\", "F:\\"] {
        let d = Path::new(drv);
        if !d.is_dir() {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_dir() {
                    continue;
                }
                let n = e.file_name().to_string_lossy().to_lowercase();
                if n.contains("python") {
                    push(p.join("python.exe"), "常见安装位置", &mut out);
                }
                if n.contains("python") || n == "tools" || n == "sdk" || n == "dev" {
                    scan_children(&p, "工具目录里的便携解释器", &mut out);
                }
            }
        }
    }
    for rel in [
        "scoop/apps",
        "miniconda3",
        "anaconda3",
        // pyenv-win 把每个版本放在这里，且**故意不进 PATH**——
        // 不认这条线，用 pyenv 的人会被判成"没装 Python"。
        ".pyenv/pyenv-win/versions",
        ".local/bin",
        "AppData/Local/miniconda3",
        "AppData/Local/anaconda3",
    ] {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            scan_children(&PathBuf::from(home).join(rel), "常见安装位置", &mut out);
        }
    }
    // 稳定排序：真安装 / 明确指定的排前面，转发壳排后面。
    // sort_by_key 是稳定排序，同一组里仍保持上面的发现顺序（越可信越靠前）。
    // 规则只有一条，不再到处写「看到 windowsapps 就跳过」这种特例。
    out.sort_by_key(|c| {
        !(c.explicit
            || c.path
                .parent()
                .map(looks_like_install)
                .unwrap_or(false))
    });

    // 版本探测：多个候选并行跑，最多 12 个
    if include_versions {
        let n = out.len().min(12);
        let idx: Vec<usize> = (0..n).collect();
        let versions = crate::parallel::map(idx, |i| probe_version(&out[i].path));
        for (i, v) in versions.into_iter().enumerate() {
            out[i].version = v;
        }
    }
    out
}

fn probe_version(exe: &Path) -> Option<String> {
    match run_cmd(exe, &["-V"], Duration::from_secs(10)) {
        Some((true, v)) => v.lines().next().map(|s| s.replace("Python ", "")),
        _ => None,
    }
}

// ---------------------------------------------------------------- toolchain

pub fn probe_toolchain() -> BTreeMap<String, ToolFact> {
    const TOOLS: &[(&str, &str, &str)] = &[
        ("node", "node.exe", "-v"),
        ("npm", "npm.cmd", "-v"),
        ("java", "java.exe", "-version"),
        ("go", "go.exe", "version"),
        ("rustc", "rustc.exe", "--version"),
        ("gcc", "gcc.exe", "--version"),
        ("git", "git.exe", "--version"),
        ("uv", "uv.exe", "--version"),
    ];
    let mut out = BTreeMap::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = TOOLS
            .iter()
            .map(|(name, exe, flag)| {
                scope.spawn(move || {
                    let found = which(exe);
                    let fact = match &found {
                        Some(p) => {
                            let ver = run_cmd(p, &[flag], Duration::from_secs(15))
                                .and_then(|(_, s)| s.lines().next().map(String::from));
                            ToolFact {
                                ok: true,
                                path: Some(p.to_string_lossy().to_string()),
                                version: ver.map(|v| extract_version(&v)),
                            }
                        }
                        None => ToolFact::default(),
                    };
                    (name.to_string(), fact)
                })
            })
            .collect();
        for h in handles {
            if let Ok((k, v)) = h.join() {
                out.insert(k, v);
            }
        }
    });
    out
}

fn extract_version(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let v: String = chars[start..i].iter().collect();
            if v.contains('.') {
                return v.trim_end_matches('.').to_string();
            }
        }
        i += 1;
    }
    s.to_string()
}

// ---------------------------------------------------------------- project

pub fn probe_project(root: &Path) -> ProjectFacts {
    if !root.is_dir() {
        return ProjectFacts {
            root: root.to_string_lossy().to_string(),
            ..Default::default()
        };
    }
    let (contract, contract_error) = Contract::load(root);
    let declared = Contract::declared_deps(root);
    // 依赖清单按语言分派，和 GUI 那条路径共用同一段逻辑。
    // 这里没有 pip 清单（快速路径），Python 那一档会标成「无法确认」——
    // 不猜，是因为猜错一次，用户就再也不信这一栏了。
    let langs = crate::lang::detect_all(root);
    let (deps_detail, deps_status) =
        crate::deps::collect(root, &langs, contract.as_ref(), &[]);
    ProjectFacts {
        root: root.to_string_lossy().to_string(),
        exists: true,
        contract_hash: contract.as_ref().map(|c| c.hash()),
        contract,
        contract_error,
        declared_deps: declared,
        deps_detail,
        deps_status,
        venvs: scan_venvs(root),
        recent_files: recent_files(root, 10),
        hygiene: Some(crate::project::scan(root)),
    }
}

pub fn scan_venvs(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for name in [".venv", "venv", "env"] {
        let cfg = root.join(name).join("pyvenv.cfg");
        if cfg.is_file() {
            out.push(root.join(name).to_string_lossy().to_string());
        }
    }
    out
}

fn recent_files(root: &Path, limit: usize) -> Vec<String> {
    let now = SystemTime::now();
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            !(e.file_type().is_dir()
                && [
                    "__pycache__",
                    ".git",
                    ".jingwei",
                    ".drydock",
                    ".pylaunch",
                    "node_modules",
                    "target",
                    ".venv",
                ]
                    .contains(&n.as_str()))
        })
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Ok(md) = entry.metadata() {
            if let Ok(modi) = md.modified() {
                if now.duration_since(modi).unwrap_or_default() < Duration::from_secs(600) {
                    out.push(entry.path().to_string_lossy().to_string());
                    if out.len() >= limit {
                        break;
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- 汇总

pub fn make_facts(root: &Path, python_exe: Option<&Path>) -> Facts {
    let (contract, _) = Contract::load(root);
    let mut project = probe_project(root);

    // 这两串探针互相独立，各自都要起一批子进程。
    // 串着跑等于把它们的时间加起来；并起来，最慢的那条决定总耗时。
    let (py, toolchain) = std::thread::scope(|s| {
        let py_job = s.spawn(|| match python_exe {
            Some(p) => probe_python(p),
            None => first_working(root, contract.as_ref(), 5)
                .map(|(_, f)| f)
                .unwrap_or_default(),
        });
        let tools = probe_toolchain();
        (py_job.join().unwrap_or_default(), tools)
    });

    // 最近一次跑这个项目时的内存。规则要靠它把「为什么慢」接进排查链。
    let last_mem = crate::runs::list(root, 1)
        .into_iter()
        .next()
        .and_then(|r| r.memory.map(|m| crate::mem::MemFacts::from_report(&r.at, &r.command, &m)));

    // 按项目类型分派：只探该探的那门语言。
    // 打开一个 Python 项目去 spawn 一串 java，纯属浪费——这也是「小体量」的一部分。
    let langs = crate::lang::detect_all(root);
    let lang = langs.first().copied().unwrap_or(crate::lang::Lang::Unknown);
    let (jvm, cc) = match lang {
        crate::lang::Lang::Java => (Some(crate::jvm::probe_jvm(root)), None),
        crate::lang::Lang::Cpp => (None, Some(crate::ccpp::probe_cc(root))),
        crate::lang::Lang::Web => (None, None),
        _ if langs.contains(&crate::lang::Lang::Java) => {
            (Some(crate::jvm::probe_jvm(root)), None)
        }
        _ => (None, None),
    };

    // 依赖状态：用"已装包的 distribution 名"比对（比逐个 import 快几千倍）。
    // 顺带把「装上的版本」也留下——依赖管理的价值就是这两列并排：
    // 清单里要什么、机器上是什么、对得上对不上。
    // 依赖清单与两列状态：和快速路径共用同一段逻辑（见 deps::collect）
    let (detail, status) = crate::deps::collect(root, &langs, contract.as_ref(), &py.installed);
    project.deps_detail = detail;
    project.deps_status = status;

    Facts {
        schema_version: SCHEMA_VERSION,
        tool_version: TOOL_VERSION.into(),
        taken_at: now_iso(),
        system: probe_system(),
        lang,
        langs,
        jvm,
        cc,
        last_mem,
        python: py,
        toolchain,
        project,
    }
}

/// 这个目录是「一套真安装」，还是只放了个转发壳？
///
/// 真安装的目录里一定有 Lib\ 或 DLLs\ 或 python3.dll。
/// 虚拟环境是另一回事：python.exe 在 `.venv\Scripts\`（或 `bin\`）里，
/// 旁边当然没有 Lib —— 它的标志是上一级的 `pyvenv.cfg`。
/// 漏掉这一条会把用户自己选的 venv 判成「壳」，然后被排序挤到最后，
/// 结果装包装进全局解释器（这个坑真踩过）。
///
/// 剩下的就是转发壳：应用商店别名、Python 管理器的 bin\、py 的 shim，
/// 那些目录里只有几个 exe 和 `.__target__` 标记文件。
fn looks_like_install(dir: &Path) -> bool {
    if dir.join("Lib").is_dir() || dir.join("DLLs").is_dir() || dir.join("python3.dll").is_file() {
        return true;
    }
    dir.parent()
        .map(|up| up.join("pyvenv.cfg").is_file())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jingwei-probe-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_real_install_is_recognised_by_its_lib_directory() {
        let d = scratch("real");
        std::fs::create_dir_all(d.join("Lib")).unwrap();
        assert!(looks_like_install(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 虚拟环境里 python.exe 在 Scripts\ 下，旁边当然没有 Lib——
    /// 漏掉这一条会把它当成转发壳排到队尾，于是包装进全局解释器。
    /// 这个坑真踩过，所以留一条测试钉住。
    #[test]
    fn a_venv_scripts_directory_is_not_a_shim() {
        let venv = scratch("venv");
        std::fs::write(venv.join("pyvenv.cfg"), "home = somewhere").unwrap();
        let scripts = venv.join("Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        assert!(looks_like_install(&scripts), "venv 的 Scripts 被误判成壳了");
        let _ = std::fs::remove_dir_all(&venv);
    }

    #[test]
    fn a_forwarding_shim_is_not_an_install() {
        let d = scratch("shim");
        std::fs::write(d.join("python.exe"), b"stub").unwrap();
        assert!(!looks_like_install(&d));
        let _ = std::fs::remove_dir_all(&d);
    }
}

