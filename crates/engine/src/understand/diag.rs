//! 真实编译器诊断：把「跑一次检查」的结果变成能点、能跳的条目。
//!
//! 为什么先做这个，而不是先写一套常驻语言服务器（LSP）：
//! `cargo check --message-format=json` 吐出来的就是**带文件名、行列、
//! 错误码的真编译器诊断**。同样的信息，不用实现 JSON-RPC 握手、
//! 不用维护长连接、不用管 server 进程崩了怎么办。
//! 代价是：它在你保存/手动触发时才跑一次，不是打字时就飘红。
//! 这一步的价值（说得清、点得到、跳得准）占了 LSP 的八成，
//! 成本占两成不到；等这一层用顺了再上常驻连接也不迟。
//!
//! 红线照旧：**报告里的每一个字都要能自己验证**。所以每条诊断都带
//! 原始的错误码和位置，界面上点一下就能跳到那一行。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diag {
    /// error / warning / note
    pub level: String,
    /// 相对项目根的文件路径（统一用 / 分隔，界面直接用）
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub message: String,
    /// 错误码，例如 E0308。空 = 编译器没给
    #[serde(default)]
    pub code: String,
}

/// 一行 `cargo check --message-format=json` 输出 → 一条诊断。
///
/// 拆成纯函数是为了能测：拉进程那部分不好测，解析这一部分必须测。
pub fn parse_cargo_line(line: &str, root: &Path) -> Option<Diag> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("reason")?.as_str()? != "compiler-message" {
        return None;
    }
    let m = v.get("message")?;
    let level = m.get("level")?.as_str()?.to_string();
    if level != "error" && level != "warning" {
        return None;
    }
    let spans = m.get("spans")?.as_array()?;
    let span = spans.iter().find(|s| s.get("is_primary").and_then(|p| p.as_bool()) == Some(true))
        .or_else(|| spans.first())?;
    let raw = span.get("file_name")?.as_str()?;
    // 依赖目录里的报错与你无关；混进来会把"我的错"淹掉
    if raw.contains("registry") || raw.contains("\\.cargo\\") || raw.contains("/.cargo/") {
        return None;
    }
    let p = PathBuf::from(raw);
    let rel = p
        .strip_prefix(root)
        .map(|r| r.to_path_buf())
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/");
    Some(Diag {
        level,
        file: rel,
        line: span.get("line_start").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        col: span.get("column_start").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        message: m.get("message").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        code: m
            .get("code")
            .and_then(|c| c.get("code"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// 对 Rust 项目跑一次 `cargo check`，把诊断收回来。
/// 超时可给，因为第一次跑要编译依赖，可能很久——那种时候宁可说"没跑完"，
/// 也不要挂住界面。
pub fn rust_check(root: &Path, timeout: Duration) -> (Vec<Diag>, Option<String>) {
    let mut cmd = Command::new("cargo");
    cmd.arg("check")
        .arg("--message-format=json")
        .current_dir(root)
        .stdout(Stdio::piped())
        // stderr 必须留着：cargo 起不来（没有依赖、路径不合法、找不到
        // workspace 根）时它只在 stderr 里说话，而 stdout 一行诊断都没有。
        // 丢掉 stderr 的后果我踩过：界面显示「没问题」，其实是一次都没跑成。
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = cmd.spawn() else {
        return (Vec::new(), Some("没找到 cargo：这个项目要装 Rust 工具链才能检查".into()));
    };
    let mut out = String::new();
    let mut err = String::new();
    let mut reader = child.stdout.take().map(|s| {
        use std::io::Read;
        let mut s = s;
        let mut buf = String::new();
        // 读不到输出 ≠ 命令没有输出。两件事混成一件，排查时就会往错的方向走。
        if let Err(e) = s.read_to_string(&mut buf) {
            crate::dlog::w("9004", &format!("读检查器 stdout 失败：{e}"));
        }
        buf
    });
    let mut err_reader = child.stderr.take().map(|s| {
        use std::io::Read;
        let mut s = s;
        let mut buf = String::new();
        if let Err(e) = s.read_to_string(&mut buf) {
            crate::dlog::w("9004", &format!("读检查器 stderr 失败：{e}"));
        }
        buf
    });
    // 等进程（带超时）：超时就杀掉，别把界面拖住
    let started = std::time::Instant::now();
    let mut done = false;
    while started.elapsed() < timeout {
        if let Ok(Some(_)) = child.try_wait() {
            done = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    if !done {
        // 超时后收掉这个进程。杀不掉通常意味着它刚好自己退了（正是我们要的结果），
        // 但如果是别的原因，日志里得有这一笔。
        if let Err(e) = child.kill() {
            crate::dlog::w("9005", &format!("超时后杀检查器进程失败：{e}"));
        }
        return (Vec::new(), Some("检查超时（第一次跑要编译依赖，慢是正常的）".into()));
    }
    if let Some(r) = reader.as_mut() {
        out = std::mem::take(r);
    }
    if let Some(r) = err_reader.as_mut() {
        err = std::mem::take(r);
    }
    let mut list: Vec<Diag> = out.lines().filter_map(|l| parse_cargo_line(l, root)).collect();
    // 一条诊断都没解析出来，但进程确实报了错 → 多半是"检查根本没跑成"
    // （依赖没装、路径不合法、找不到 workspace 根）。
    // 这时候绝不能当作「没问题」——把 cargo 自己说的话原样带回去。
    if list.is_empty() && !err.trim().is_empty() {
        let first = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        return (list, Some(format!("检查没跑完：{}", first)));
    }
    // 同一个位置重复报（cargo 会把同一诊断派发多次）→ 去重
    list.dedup_by(|a, b| a.file == b.file && a.line == b.line && a.col == b.col && a.message == b.message);
    list.sort_by(|a, b| (a.file.as_str(), a.line, a.col).cmp(&(b.file.as_str(), b.line, b.col)));
    (list, None)
}

// ── Python：ruff 的 JSON 输出 ───────────────────────────────
//
// 为什么选 ruff 而不是 pyflakes/pylint：它是现在事实上的标准，
// 输出结构化 JSON，跑得又快到可以随手点一下。
// 没装的话**不给替代品**——`py_compile` 只能查语法，报出来的东西
// 和 ruff 不是一个量级，凑上去只会让人以为"我的代码没问题"。
// 那种时候如实说「没找到 ruff，装它才能检查」，并把命令给到。
pub fn parse_ruff_json(text: &str, root: &Path) -> Vec<Diag> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for it in arr {
        let raw = it.get("filename").and_then(|x| x.as_str()).unwrap_or("");
        if raw.is_empty() {
            continue;
        }
        let p = PathBuf::from(raw);
        let file = p
            .strip_prefix(root)
            .map(|r| r.to_path_buf())
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/");
        let loc = it.get("location").cloned().unwrap_or_default();
        out.push(Diag {
            level: "warning".into(),   // ruff 默认全是 lint 提示，不阻断构建
            file,
            line: loc.get("row").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            col: loc.get("column").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            message: it.get("message").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            code: it.get("code").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        });
    }
    out
}

/// 跑 ruff。返回 (诊断, 说明)；说明非空表示"这次没查成"，界面必须显示它。
pub fn python_check(root: &Path, ruff: &str, timeout: Duration) -> (Vec<Diag>, Option<String>) {
    let out = run_capture(
        ruff,
        &["check", "--output-format=json", "--no-cache", "--exit-zero", "."],
        root,
        timeout,
    );
    match out {
        Err(e) => (Vec::new(), Some(e)),
        Ok((stdout, stderr, code)) => {
            let mut list = parse_ruff_json(&stdout, root);
            list.sort_by(|a, b| (a.file.as_str(), a.line, a.col).cmp(&(b.file.as_str(), b.line, b.col)));
            let note = if list.is_empty() && code != 0 && !stderr.trim().is_empty() {
                Some(format!("检查没跑完：{}", first_line(&stderr)))
            } else {
                None
            };
            (list, note)
        }
    }
}

// ── TypeScript：tsc 的文本输出 ─────────────────────────────
//
// tsc 只有文本，但格式稳定：`src/a.ts(3,5): error TS2322: 消息`。
// 只在项目里真有 tsconfig.json 时才跑——没有配置就 tsc 自己都不知道
// 该按什么规则查，硬跑只会吐一堆与你无关的错。
pub fn parse_tsc_text(text: &str, root: &Path) -> Vec<Diag> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((head, rest)) = line.split_once("): ") else { continue };
        let Some(open) = head.rfind('(') else { continue };
        let (file_raw, pos) = (&head[..open], &head[open + 1..]);
        let Some((ln, col)) = pos.split_once(',') else { continue };
        let (Ok(ln), Ok(col)) = (ln.trim().parse::<u32>(), col.trim().parse::<u32>()) else {
            continue;
        };
        let (level, msg) = if let Some(m) = rest.strip_prefix("error ") {
            ("error", m)
        } else if let Some(m) = rest.strip_prefix("warning ") {
            ("warning", m)
        } else {
            continue;
        };
        // 错误码在消息开头，形如 TS2322: ...
        let (code, msg) = match msg.split_once(": ") {
            Some((c, m)) if c.starts_with("TS") => (c.to_string(), m.to_string()),
            _ => (String::new(), msg.to_string()),
        };
        let p = PathBuf::from(file_raw.trim());
        let file = p
            .strip_prefix(root)
            .map(|r| r.to_path_buf())
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(Diag { level: level.into(), file, line: ln, col, message: msg, code });
    }
    out
}

pub fn typescript_check(root: &Path, timeout: Duration) -> (Vec<Diag>, Option<String>) {
    if !root.join("tsconfig.json").is_file() {
        return (Vec::new(), None);   // 没配置就不跑，静默即可
    }
    match run_capture("npx", &["tsc", "--noEmit", "--pretty", "false"], root, timeout) {
        Err(e) => (Vec::new(), Some(e)),
        Ok((stdout, stderr, _)) => {
            let mut list = parse_tsc_text(&format!("{}\n{}", stdout, stderr), root);
            list.sort_by(|a, b| (a.file.as_str(), a.line, a.col).cmp(&(b.file.as_str(), b.line, b.col)));
            (list, None)
        }
    }
}

fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

/// 起一个进程、把 stdout/stderr 都收回来。带超时；超时杀掉并如实回报。
fn run_capture(
    prog: &str,
    args: &[&str],
    root: &Path,
    timeout: Duration,
) -> Result<(String, String, i32), String> {
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = cmd.spawn() else {
        return Err(format!("没找到 {}：装它才能做这一步检查", prog));
    };
    let mut o = child.stdout.take();
    let mut e = child.stderr.take();
    let ho = std::thread::spawn(move || {
        use std::io::Read;
        let mut s = String::new();
        if let Some(h) = o.as_mut() {
            if let Err(e) = h.read_to_string(&mut s) {
                crate::dlog::w("9004", &format!("读 stdout 失败：{e}"));
            }
        }
        s
    });
    let he = std::thread::spawn(move || {
        use std::io::Read;
        let mut s = String::new();
        if let Some(h) = e.as_mut() {
            if let Err(e) = h.read_to_string(&mut s) {
                crate::dlog::w("9004", &format!("读 stderr 失败：{e}"));
            }
        }
        s
    });
    let started = std::time::Instant::now();
    let code;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => { code = st.code().unwrap_or(-1); break; }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    if let Err(e) = child.kill() {
                        crate::dlog::w("9005", &format!("超时后杀进程失败：{e}"));
                    }
                    return Err("检查超时".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("检查出错：{}", e)),
        }
    }
    let stdout = ho.join().unwrap_or_default();
    let stderr = he.join().unwrap_or_default();
    Ok((stdout, stderr, code))
}

/// 按项目里有的东西决定跑哪些检查，把结果和"没跑成"的说明一起带回去。
pub fn check_all(root: &Path, timeout: Duration) -> (Vec<Diag>, Vec<String>) {
    let mut list = Vec::new();
    let mut notes = Vec::new();
    if root.join("Cargo.toml").is_file() {
        let (d, n) = rust_check(root, timeout);
        list.extend(d);
        if let Some(n) = n { notes.push(n); }
    }
    if crate::lang::detect_all(root).contains(&crate::lang::Lang::Python) {
        let ruff = which::find("ruff");
        match ruff {
            Some(p) => {
                let (d, n) = python_check(root, &p, timeout);
                list.extend(d);
                if let Some(n) = n { notes.push(n); }
            }
            None => notes.push(
                "没找到 ruff：Python 的静态检查要它（pip install ruff；装完再点一次运行检查）".into(),
            ),
        }
    }
    if root.join("tsconfig.json").is_file() {
        let (d, n) = typescript_check(root, timeout);
        list.extend(d);
        if let Some(n) = n { notes.push(n); }
    }
    list.sort_by(|a, b| (a.file.as_str(), a.line, a.col).cmp(&(b.file.as_str(), b.line, b.col)));
    (list, notes)
}

/// 找可执行文件：先看 PATH，再退到 python -m 里的常见位置。
mod which {
    use std::path::PathBuf;

    pub fn find(name: &str) -> Option<String> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            let cand: PathBuf = dir.join(format!("{}.exe", name));
            if cand.is_file() {
                return Some(cand.to_string_lossy().to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    /// ruff 的 JSON 是按这个形状吐的：一个数组，每项带 code/message/filename/location。
    #[test]
    fn parses_ruff_json_with_position_and_code() {
        let text = r#"[{"code":"F401","message":"`os` imported but unused","filename":"D:\\proj\\app.py","location":{"row":3,"column":8},"end_location":{"row":3,"column":10}}]"#;
        let got = parse_ruff_json(text, Path::new("D:\\proj"));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].code, "F401");
        assert_eq!(got[0].file, "app.py");
        assert_eq!((got[0].line, got[0].col), (3, 8));
        assert_eq!(got[0].level, "warning");   // lint 提示不阻断构建
    }

    /// tsc 只有文本，但格式稳定。格式变了宁可解析不出来，也不要瞎猜位置。
    #[test]
    fn parses_tsc_text_lines() {
        let text = "src/a.ts(3,5): error TS2322: Type 'string' is not assignable to type 'number'.\nnot a diagnostic line\n";
        let got = parse_tsc_text(text, Path::new("D:\\proj"));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].code, "TS2322");
        assert_eq!(got[0].file, "src/a.ts");
        assert_eq!((got[0].line, got[0].col), (3, 5));
        assert_eq!(got[0].level, "error");
    }

    /// 真编译器吐出来的那一行长什么样，就按它的形状测。
    /// 这一段字符串是从 `cargo check --message-format=json` 的输出里抄的结构。
    #[test]
    fn parses_a_compiler_message_with_position() {
        let line = r#"{"reason":"compiler-message","message":{"level":"error","code":{"code":"E0308"},"message":"mismatched types","spans":[{"file_name":"src/main.rs","line_start":3,"column_start":9,"is_primary":true}]}}"#;
        let d = parse_cargo_line(line, Path::new("D:/x")).expect("应该解析出来");
        assert_eq!(d.level, "error");
        assert_eq!(d.code, "E0308");
        assert_eq!(d.file, "src/main.rs");
        assert_eq!((d.line, d.col), (3, 9));
        assert_eq!(d.message, "mismatched types");
    }

    /// 依赖目录里的报错不该混进来——那是别人的错，和你没关系。
    /// 混进来的后果很具体：你自己的错会被淹掉。
    #[test]
    fn dependency_errors_are_dropped() {
        let line = r#"{"reason":"compiler-message","message":{"level":"error","message":"x","spans":[{"file_name":"C:\\Users\\me\\.cargo\\registry\\src\\foo\\lib.rs","line_start":1,"column_start":1,"is_primary":true}]}}"#;
        assert!(parse_cargo_line(line, Path::new("D:/x")).is_none());
    }

    /// note / 非编译事件不要收：界面上只放"要你动手的"。
    #[test]
    fn notes_and_other_reasons_are_ignored() {
        let note = r#"{"reason":"compiler-message","message":{"level":"note","message":"n","spans":[{"file_name":"src/main.rs","line_start":1,"column_start":1,"is_primary":true}]}}"#;
        assert!(parse_cargo_line(note, Path::new("D:/x")).is_none());
        let build = r#"{"reason":"build-finished","success":true}"#;
        assert!(parse_cargo_line(build, Path::new("D:/x")).is_none());
        assert!(parse_cargo_line("这不是 json", Path::new("D:/x")).is_none());
    }

    /// 绝对路径要变成相对项目根的路径，界面才显示得干净
    #[test]
    fn absolute_paths_become_relative() {
        let line = r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused","spans":[{"file_name":"D:\\proj\\src\\a.rs","line_start":2,"column_start":5,"is_primary":true}]}}"#;
        let d = parse_cargo_line(line, Path::new("D:\\proj")).unwrap();
        assert_eq!(d.file, "src/a.rs");
        assert_eq!(d.level, "warning");
        assert_eq!(d.code, "");
    }
}
