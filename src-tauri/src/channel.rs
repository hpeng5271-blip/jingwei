//! **常驻通道**（2026-10-22，D-20261022-03 Q8：只接详情页 + 看门狗）。
//!
//! pro 的裁定原文：「C（只接详情页）+ 惰性启动 + 崩了自动回退到 spawn 的看门狗；
//! 否则 71 ms 换不来可靠性」。
//!
//! 所以这里只做三件事：
//! 1. **惰性启动**：第一次真的要取数时才拉进程；
//! 2. **一问一答**：写一行、读一行（协议与 `jingwei serve` 一致）；
//! 3. **崩了就丢**：任何 IO 错误都把进程杀掉并清空，让前端回退到 spawn（下一次请求会重新拉）。
//!
//! 不做：并发请求、请求排队、超时重试（都不在这条路径的收益里）。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::{Mutex, OnceLock};

struct Chan {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

fn slot() -> &'static Mutex<Option<Chan>> {
    static SLOT: OnceLock<Mutex<Option<Chan>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 找到与外壳同目录的 `jingwei.exe`；找不到就不启动（前端回退 spawn）。
fn jingwei_exe() -> Option<PathBuf> {
    let mut dir = std::env::current_exe().ok()?;
    dir.pop();
    let exe = dir.join(if cfg!(windows) { "jingwei.exe" } else { "jingwei" });
    exe.exists().then_some(exe)
}

fn spawn(root: &str) -> Result<Chan, String> {
    let exe = jingwei_exe().ok_or_else(|| "找不到同目录的 jingwei.exe".to_string())?;
    let mut child = std::process::Command::new(exe)
        .args(["serve", "--root", root])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("起不来：{e}"))?;
    let stdin = child.stdin.take().ok_or("拿不到 stdin")?;
    let stdout = child.stdout.take().ok_or("拿不到 stdout")?;
    Ok(Chan { child, stdin, stdout: BufReader::new(stdout) })
}

/// 惰性启动：已经在跑就直接用；没在跑就拉一个。
#[tauri::command]
pub fn channel_start(root: String) -> Result<bool, String> {
    let mut g = slot().lock().map_err(|_| "锁坏了".to_string())?;
    if g.is_some() {
        return Ok(false);
    }
    *g = Some(spawn(&root)?);
    Ok(true)
}

/// 一问一答。**失败就把进程丢掉**（前端会回退到 spawn）。
#[tauri::command]
pub fn channel_request(root: String, method: String) -> Result<serde_json::Value, String> {
    let mut g = slot().lock().map_err(|_| "锁坏了".to_string())?;
    if g.is_none() {
        *g = Some(spawn(&root)?);
    }
    let chan = g.as_mut().ok_or("通道没起来")?;
    let req = serde_json::json!({"id": 1, "method": method});
    let out = write_read(chan, &req);
    if out.is_err() {
        // 看门狗：任何 IO 问题都不留半死进程
        if let Some(mut c) = g.take() {
            c.child.kill().ok();
            c.child.wait().ok();
        }
    }
    out
}

fn write_read(chan: &mut Chan, req: &serde_json::Value) -> Result<serde_json::Value, String> {
    let line = serde_json::to_string(req).map_err(|e| e.to_string())?;
    chan.stdin.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    chan.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
    chan.stdin.flush().map_err(|e| e.to_string())?;
    let mut buf = String::new();
    chan.stdout.read_line(&mut buf).map_err(|e| e.to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(buf.trim()).map_err(|e| format!("响应不是 JSON：{e}"))?;
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
    } else {
        Err(v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("通道报错")
            .to_string())
    }
}

/// 关掉（切换离开详情页时调用）。重复调用无害。
#[tauri::command]
pub fn channel_stop() -> Result<(), String> {
    let mut g = slot().lock().map_err(|_| "锁坏了".to_string())?;
    if let Some(mut c) = g.take() {
        c.child.kill().ok();
        c.child.wait().ok();
    }
    Ok(())
}
