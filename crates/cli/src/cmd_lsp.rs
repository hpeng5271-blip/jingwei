//! `jingwei lsp`：**stdio 上的最小 LSP**，由编辑器拉起。
//!
//! 为什么是这样一个命令而不是一个服务：**编辑器不会替你开端口、也不会替你装服务**。
//! 它只会执行一条命令、然后通过 stdin/stdout 说话——所以这一层必须薄到
//! "启动、转发、退出"三件事就完。
//!
//! 边界（编辑器里要自己配的那段）：
//! ```text
//! command = jingwei     args = ["lsp"]     transport = stdio
//! ```
//! 还**没做**：workspace 热更新、hover、补全、诊断推送（见 `docs/reference/债表.md`）。

/// 跑 stdio 循环；`0` = 正常退出（EOF 或 `exit` 通知）。
pub fn cmd_lsp(root: &std::path::Path) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    // **起手先说一句**：编辑器那边看不到服务端日志，这一行是唯一能确认"它真的起来了"的痕迹。
    // 走 stderr —— LSP 的 stdout 只能放消息，混一个字就会把协议弄坏。
    eprintln!("jingwei lsp: root = {}", root.display());
    match jingwei_engine::lsp::serve(root, stdin.lock(), stdout.lock()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("jingwei lsp: {e}");
            1
        }
    }
}
