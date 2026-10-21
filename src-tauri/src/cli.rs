//! **外壳吃 CLI 的协议**（2026-10-21）。
//!
//! ## 为什么让 GUI 去跑 CLI，而不是各调各的引擎
//!
//! 结论只有一份：`jingwei <动词> --json` 已经把「结论 + 三态 + 依据 + 覆盖度」算好了。
//! 如果界面再自己调引擎拼一遍，就又回到"同一个问题两种说法"——那正是这轮要消灭的。
//!
//! 所以这里的定位很窄：**把 CLI 的 JSON 原样搬给前端**，不解释、不加工、不补齐。
//! 需要补的字段，去 CLI 那边补；补在界面里就是第二份真源。
//!
//! ## 两种失败必须分开
//!
//! - CLI 正常退出/查不了（exit 1/2）但**stdout 是 JSON**：原样返回（里面已经有 state/reason）；
//! - 压根没跑起来（找不到 exe、不是 JSON、进程崩）：`Err`——那是"整件事没做成"。

use serde_json::Value;
use std::path::{Path, PathBuf};

/// 找同目录的 `jingwei.exe`；开发时退到 `target/{debug,release}`。
fn cli_exe() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "jingwei.exe"
    } else {
        "jingwei"
    };
    if let Ok(cur) = std::env::current_exe() {
        if let Some(dir) = cur.parent() {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    // 开发态：cargo 把 GUI 放在 target/debug，CLI 通常也在那儿
    for rel in ["target/debug", "target/release"] {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 跑一次 CLI，把它的 JSON 原样交给前端。
///
/// `args` 是动词及其参数（例如 `["next"]`、`["handover"]`）；`--root` 与 `--json` 由这里统一加，
/// 免得每个调用点各拼一遍。
#[tauri::command]
pub(crate) fn cli_json(root: String, args: Vec<String>) -> Result<Value, String> {
    let exe = cli_exe().ok_or_else(|| {
        "找不到同目录的 jingwei.exe（打包时它该和 GUI 放在一起；开发态要先 cargo build）".to_string()
    })?;
    let out = std::process::Command::new(&exe)
        .args(&args)
        .arg("--root")
        .arg(&root)
        .arg("--json")
        .output()
        .map_err(|e| format!("跑不起 jingwei（{}）：{e}", exe.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // **先看 stdout 是不是 JSON**：查不了时 CLI 会以 1/2 退出，但 JSON 是完整的。
    if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
        return Ok(v);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(format!(
        "jingwei {} 没给出 JSON（exit={:?}）：{}",
        args.join(" "),
        out.status.code(),
        stderr.trim()
    ))
}
