//! Windows 上的一点脾气活。

/// 别让子进程弹控制台窗口。
///
/// 桌面外壳本身是 GUI 进程，它每拉起一个控制台程序（python -V、node -v、git……）
/// Windows 都会给那个子进程新建一个控制台窗口——屏幕上一串黑框闪过去，
/// 看着像中毒。CREATE_NO_WINDOW 把窗口拿掉，管道照常。
#[cfg(windows)]
pub fn hide_console(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut std::process::Command) {}

// ── 进程与查找：两个纯工具，放在这里是为了把依赖方向掰直 ──────────────
//
// 它们原本长在 `probe` 里。于是 `ccpp` / `jvm` 为了用它们必须依赖 `probe`，
// 而 `probe` 本身又要调 `ccpp::probe_cc` / `jvm::probe_jvm` ——两对模块就此互相依赖
// （《复审记录》Q6 抓到的三对循环里的两对）。
//
// 它们不涉及任何业务概念（"跑个命令"、"在 PATH 里找个 exe"），所以下沉到这里：
// **谁都该能用，而它谁都不依赖**（只用 textfile 解一下中文编码）。

/// 在 PATH 里找一个可执行文件。
pub fn which(exe: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let p = dir.join(exe);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 跑一个外部命令，带超时。返回 `(是否成功, 合并后的输出)`；超时或起不来返回 `None`。
///
/// **超时返回 `None` 而不是 `(false, "")`**：这两件事在界面上说法完全不同——
/// "没跑成"要人说清为什么，而"跑了但失败"要给退出码与输出。
pub fn run_cmd(
    program: &std::path::Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Option<(bool, String)> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let child = cmd.spawn().ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // 这里 `send` 失败是正常的：超时路径上接收端已经走了，结果没人要。
        #[allow(clippy::let_underscore_must_use)]
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => {
            let mut s = crate::util::textfile::decode_output(&out.stdout);
            s.push_str(&crate::util::textfile::decode_output(&out.stderr));
            Some((out.status.success(), s.trim().to_string()))
        }
        _ => None,
    }
}
