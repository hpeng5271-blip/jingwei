//! 工具链探测：**以实跑为准，不信路径**。
//!
//! 判定只有三态：`Ok`（跑出可解析的版本）/ `Timeout`（进程存在但 2 秒内没跑完）/
//! `Absent`（候选都不在、或跑了但没给出可解析的版本）。
//!
//! 候选顺序固定：项目 pin → 版本管理器 shim 目录 → 常见安装目录 → PATH。
//! 探测**只读**：不写 PATH、不装组件、不改任何环境变量。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 单次探测上限。超过就按 `Timeout` 记，不再等。
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Timeout,
    Absent,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Timeout => "timeout",
            Status::Absent => "absent",
        }
    }
}

/// 候选来自哪一层（顺序即优先级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// 项目自己钉的（env.contract.json / 项目配置）
    Pin,
    /// 版本管理器的 shim 目录（rustup / nvm / pyenv / sdkman …）
    Shim,
    /// 常见安装目录
    Common,
    /// PATH
    Path,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Pin => "pin",
            Source::Shim => "shim",
            Source::Common => "common",
            Source::Path => "path",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProbe {
    pub name: String,
    pub status: Status,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub source: Option<Source>,
    /// `Absent` 时给的复现命令（用户照着敲就能自己确认）
    pub repro: Option<String>,
}

impl ToolProbe {
    fn absent(name: &str, exe: &str, args: &[&str]) -> Self {
        ToolProbe {
            name: name.to_string(),
            status: Status::Absent,
            version: None,
            path: None,
            source: None,
            repro: Some(format!("{} {}", exe, args.join(" ")).trim().to_string()),
        }
    }
}

/// 候选列表：pin → shim → common → PATH，按路径去重（先出现的赢）。
pub fn candidates(
    exe: &str,
    pin: Option<&Path>,
    shims: &[PathBuf],
    commons: &[PathBuf],
    path_dirs: &[PathBuf],
) -> Vec<(PathBuf, Source)> {
    let mut out: Vec<(PathBuf, Source)> = Vec::new();
    let push = |p: PathBuf, s: Source, out: &mut Vec<(PathBuf, Source)>| {
        if out.iter().any(|(q, _)| q == &p) {
            return;
        }
        out.push((p, s));
    };
    if let Some(p) = pin {
        // pin 可以是目录（拼 exe）也可以是可执行文件本身。
        // 判据：已存在的目录 / 没有扩展名 → 当目录；有扩展名 → 当可执行文件。
        let as_dir = p.is_dir() || p.extension().is_none();
        let p = if as_dir { p.join(exe) } else { p.to_path_buf() };
        push(p, Source::Pin, &mut out);
    }
    for d in shims {
        push(d.join(exe), Source::Shim, &mut out);
    }
    for d in commons {
        push(d.join(exe), Source::Common, &mut out);
    }
    for d in path_dirs {
        push(d.join(exe), Source::Path, &mut out);
    }
    out
}

/// PATH 里的目录（只读，不改）。
pub fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default()
}

#[derive(Debug)]
enum RunErr {
    Timeout,
    Spawn,
}

/// `.cmd` / `.bat` 不能直接 spawn（CreateProcess 不认），要走 `cmd /C call`。
fn is_script(exe: &Path) -> bool {
    exe.extension()
        .map(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            e == "cmd" || e == "bat"
        })
        .unwrap_or(false)
}

/// 拼命令。`.exe` 一律 `Command::new`；`.cmd/.bat` 走 `cmd /C call "<quoted>" <args>`。
///
/// 为什么用 `call` 而不是 `cmd /c ""<path>" <args>"`：Rust 的 `Command` 会按
/// `CommandLineToArgvW` 规则给每个参数加引号，再套一层 `""…""` 会多出一对引号，
/// cmd 解析时就变成"命令找不到"。`call "<path>"` 让 cmd 自己按引号取路径，
/// 后面的参数各自成为独立 token，既固定形式也不会被路径里的空格截断。
fn build_command(exe: &Path) -> Command {
    if is_script(exe) {
        let comspec = std::env::var_os("COMSPEC")
            .unwrap_or_else(|| "C:\\Windows\\System32\\cmd.exe".into());
        let mut c = Command::new(comspec);
        c.arg("/C").arg("call").arg(exe);
        c
    } else {
        Command::new(exe)
    }
}

/// pin 里不允许出现的 shell 元字符。
///
/// `pin` 来自项目配置（`env.contract.json` 之类），而脚本类候选要经过 `cmd`。
/// 只要路径里能塞进 `& | > < ^ "` 这类字符，就等于把"配置"升级成"任意命令执行"。
/// 这里**先拒绝、再碰盘**：拒绝时连 spawn 都不会发生。
const FORBIDDEN_IN_PIN: &[char] = &['&', '|', '>', '<', '^', '"', '`', '$', ';', '\n', '\r'];

/// 去掉 Windows `canonicalize` 加的 `\\?\` 前缀（只影响显示，不影响判等）。
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    p.to_path_buf()
}

/// 校验 pin：必须是**已存在**的文件或目录，且不含 shell 元字符。
///
/// 返回规范化后的路径；不合格时返回给用户看的中文原因（进 `repro` 字段）。
pub fn validate_pin(pin: &Path) -> Result<PathBuf, String> {
    let shown = pin.to_string_lossy().to_string();
    if let Some(bad) = pin
        .to_string_lossy()
        .chars()
        .find(|c| FORBIDDEN_IN_PIN.contains(c))
    {
        return Err(format!("拒绝：pin 含 shell 元字符 {bad:?}：{shown}"));
    }
    if !pin.exists() {
        return Err(format!("拒绝：pin 指向的路径不存在：{shown}"));
    }
    match std::fs::canonicalize(pin) {
        Ok(p) => Ok(strip_verbatim(&p)),
        Err(e) => Err(format!("拒绝：pin 无法规范化（{e}）：{shown}")),
    }
}

/// 跑一次、最多等 `timeout`。不在这里做版本解析。
fn run_limited(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, RunErr> {
    let mut cmd = build_command(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|_| RunErr::Spawn)?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let mut out = String::new();
                let mut buf = Vec::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_end(&mut buf);
                }
                if let Some(mut s) = child.stderr.take() {
                    let mut eb = Vec::new();
                    let _ = s.read_to_end(&mut eb);
                    buf.extend_from_slice(&eb);
                }
                out.push_str(&String::from_utf8_lossy(&buf));
                return Ok(out);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunErr::Timeout);
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                let _ = child.kill();
                return Err(RunErr::Spawn);
            }
        }
    }
}

/// 从输出里抠第一个 `数字.数字` 段（如 `git version 2.56.0` → `2.56.0`）。
pub fn extract_version(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let v: String = chars[start..i].iter().collect();
            let v = v.trim_end_matches('.').to_string();
            if v.contains('.') && v.len() > 2 {
                return Some(v);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// 探测一个工具。`version_args` 例如 `["--version"]`；`exe` 是文件名（`git.exe`）。
///
/// `pin` 会先过 [`validate_pin`]：不合格直接 `Absent`，**不会 spawn 任何进程**。
pub fn probe_tool(
    name: &str,
    exe: &str,
    version_args: &[&str],
    pin: Option<&Path>,
    shims: &[PathBuf],
    commons: &[PathBuf],
) -> ToolProbe {
    let checked_pin = match pin {
        Some(p) => match validate_pin(p) {
            Ok(p) => Some(p),
            Err(reason) => {
                return ToolProbe {
                    name: name.to_string(),
                    status: Status::Absent,
                    version: None,
                    path: None,
                    source: None,
                    repro: Some(reason),
                };
            }
        },
        None => None,
    };
    let dirs = path_dirs();
    for (cand, source) in candidates(exe, checked_pin.as_deref(), shims, commons, &dirs) {
        if !cand.is_file() {
            continue;
        }
        match run_limited(&cand, version_args, PROBE_TIMEOUT) {
            Ok(out) => {
                if let Some(v) = extract_version(&out) {
                    return ToolProbe {
                        name: name.to_string(),
                        status: Status::Ok,
                        version: Some(v),
                        path: Some(cand),
                        source: Some(source),
                        repro: None,
                    };
                }
                // 跑了但给不出可解析版本：这个候选不算数，继续看下一个
            }
            Err(RunErr::Timeout) => {
                return ToolProbe {
                    name: name.to_string(),
                    status: Status::Timeout,
                    version: None,
                    path: Some(cand),
                    source: Some(source),
                    repro: None,
                };
            }
            Err(RunErr::Spawn) => {}
        }
    }
    ToolProbe::absent(name, exe, version_args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jw-env-{}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed),
            tag
        ));
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 假工具脚本：正常吐版本。
    fn write_ok(dir: &Path) -> PathBuf {
        let p = dir.join("fake-ok.cmd");
        std::fs::write(&p, "@echo off\r\necho faketool 1.2.3\r\n").expect("写脚本");
        p
    }

    /// 假工具脚本：卡住不返回（超过 2s）。
    fn write_slow(dir: &Path) -> PathBuf {
        let p = dir.join("fake-slow.cmd");
        std::fs::write(&p, "@echo off\r\nping -n 8 127.0.0.1 > nul\r\n").expect("写脚本");
        p
    }

    #[test]
    fn probe_ok_reports_version_and_source() {
        let dir = scratch("ok");
        write_ok(&dir);
        let r = probe_tool("faketool", "fake-ok.cmd", &["--version"], None, &[], &[dir.clone()]);
        assert_eq!(r.status, Status::Ok, "应判 ok：{r:?}");
        assert_eq!(r.version.as_deref(), Some("1.2.3"));
        assert_eq!(r.source, Some(Source::Common));
        assert!(r.path.as_ref().map(|p| p.is_file()).unwrap_or(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_timeout_when_the_tool_hangs() {
        let dir = scratch("slow");
        write_slow(&dir);
        let t0 = Instant::now();
        let r = probe_tool("slowtool", "fake-slow.cmd", &["--version"], None, &[], &[dir.clone()]);
        assert_eq!(r.status, Status::Timeout, "应判 timeout：{r:?}");
        assert!(r.version.is_none());
        // 不能把 8 秒等满：上限是 PROBE_TIMEOUT（2s）+ 余量
        assert!(t0.elapsed() < Duration::from_secs(4), "耗时 {:?}", t0.elapsed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_absent_when_nothing_is_there() {
        let dir = scratch("absent");
        let r = probe_tool("nope", "definitely-not-a-tool.exe", &["--version"], None, &[], &[dir.clone()]);
        assert_eq!(r.status, Status::Absent, "应判 absent：{r:?}");
        assert!(r.path.is_none() && r.version.is_none());
        assert_eq!(r.repro.as_deref(), Some("definitely-not-a-tool.exe --version"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pin_with_shell_metacharacters_is_rejected_without_spawning() {
        let dir = scratch("inject");
        // 名字里带 `&` 的文件在 Windows 上**合法**——正因为合法，才必须靠"拒绝元字符"挡住，
        // 不能指望"文件不存在"这种偶然。脚本内容是写一个标记文件：一旦被启动就能看出来。
        let evil = dir.join("nasty.cmd & calc");
        let marker = dir.join("marker.txt");
        std::fs::write(
            &evil,
            format!("@echo off\r\necho pwned> \"{}\"\r\n", marker.display()),
        )
        .expect("写注入用脚本");
        let r = probe_tool("evil", "nasty.cmd", &["--version"], Some(&evil), &[], &[dir.clone()]);
        assert_eq!(r.status, Status::Absent, "带元字符的 pin 必须判 absent：{r:?}");
        let repro = r.repro.clone().unwrap_or_default();
        assert!(repro.contains("元字符"), "repro 要写明拒绝原因：{repro}");
        assert!(!marker.exists(), "pin 被拒时不许启动任何进程（标记文件不该出现）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn candidates_follow_pin_shim_common_path_order() {
        let base = scratch("order");
        let pin = base.join("pin");
        let shim = base.join("shim");
        let common = base.join("common");
        let path = base.join("path");
        for d in [&pin, &shim, &common, &path] {
            std::fs::create_dir_all(d).expect("建候选目录");
        }
        // PATH 里故意再放一次 pin 目录：必须去重
        let got = candidates(
            "git.exe",
            Some(&pin),
            &[shim],
            &[common],
            &[path, pin.clone()],
        );
        let order: Vec<Source> = got.iter().map(|(_, s)| *s).collect();
        assert_eq!(order, vec![Source::Pin, Source::Shim, Source::Common, Source::Path]);
        assert_eq!(got.len(), 4, "重复目录要被去掉：{got:?}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
