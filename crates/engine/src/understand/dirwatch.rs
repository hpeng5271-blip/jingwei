//! **变更监听（Win32 kernel 版）**（2026-10-20，D-20261020-03 第 1 步）。
//!
//! 只做一件事：把"根目录下发生了什么"写进持久 journal（`watch.jsonl`），
//! 给 `discover`/`scan-changed` 用"只扫变过的目录"。
//!
//! **为什么不用 USN/MFT**：那要开卷句柄 `\\.\C:`，**需要管理员**——
//! 本项目的边界是"绿色免安装、不提权"。`ReadDirectoryChangesW` 不需要提权，
//! 代价是：**进程活着才收得到事件**；没在跑时"没有记录"≠"没有变化"。
//!
//! **缓冲区溢出**（`ERROR_NOTIFY_ENUM_DIR`）：变更太多时 Windows 会丢事件。
//! 处理方式是**写一行 `overflow`**，读的一侧据此退回全量——比"漏了还当最新"安全。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;

/// journal 的默认位置（与 `tools/watch.mjs` 同一份）。
pub fn default_journal() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("jingwei").join("watch.jsonl")
}

/// 动作编号 → 人类可读（Win32 的 `FILE_ACTION_*`）。
fn action_name(a: u32) -> &'static str {
    match a {
        1 => "added",
        2 => "removed",
        3 => "modified",
        4 => "renamed_old",
        5 => "renamed_new",
        _ => "unknown",
    }
}

/// 把 `FILE_NOTIFY_INFORMATION` 缓冲区解析成 `(动作, 相对路径)` 列表。
///
/// **纯函数**，不碰系统调用——这样它能被单测钉住（真正的监听没法在 CI 里稳定复现）。
pub fn parse_notify(buf: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    loop {
        if off + 12 > buf.len() {
            break;
        }
        let next = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap_or([0; 4])) as usize;
        let action = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap_or([0; 4]));
        let name_len = u32::from_le_bytes(buf[off + 8..off + 12].try_into().unwrap_or([0; 4])) as usize;
        let name_start = off + 12;
        if name_len % 2 != 0 || name_start + name_len > buf.len() {
            break;
        }
        let units: Vec<u16> = buf[name_start..name_start + name_len]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        out.push((action, String::from_utf16_lossy(&units)));
        if next == 0 {
            break;
        }
        off += next;
    }
    out
}

/// 写一行 journal。**失败不 panic**：监听线程不该因为磁盘满而把整个进程带走，
/// 但要把原因打到 stderr（静默失败是这个项目最反对的事）。
fn append_line(journal: &Path, value: serde_json::Value) {
    if let Some(dir) = journal.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "{value}");
        }
        Err(e) => eprintln!("journal 写不进去（{}）：{e}", journal.display()),
    }
}

#[cfg(windows)]
mod win {
    use super::{action_name, append_line};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
        FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };

    pub fn watch_root(root: &Path, journal: &Path, stop: Arc<AtomicBool>) {
        let wide: Vec<u16> = root
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            append_line(
                journal,
                serde_json::json!({ "ts": 0, "event": "watch-error", "root": root.display().to_string(), "message": "CreateFileW 失败" }),
            );
            return;
        }

        append_line(
            journal,
            serde_json::json!({ "ts": now_ms(), "event": "watch-start", "root": root.display().to_string() }),
        );

        let filter = FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_LAST_WRITE
            | FILE_NOTIFY_CHANGE_SIZE;

        // 64KB 是常见上限内的稳妥值；单条记录最长 ~64KB（Windows 限制）。
        let mut buf = vec![0u8; 64 * 1024];
        while !stop.load(Ordering::Relaxed) {
            let mut bytes: u32 = 0;
            let ok = unsafe {
                ReadDirectoryChangesW(
                    handle,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    buf.len() as u32,
                    1, // bWatchSubtree
                    filter,
                    &mut bytes,
                    std::ptr::null_mut(),
                    None,
                )
            };
            if ok == 0 {
                // 溢出/句柄失效：**明说**，读的一侧据此退回全量。
                append_line(
                    journal,
                    serde_json::json!({ "ts": now_ms(), "event": "overflow", "root": root.display().to_string() }),
                );
                break;
            }
            for (action, rel) in super::parse_notify(&buf[..bytes as usize]) {
                append_line(
                    journal,
                    serde_json::json!({
                        "ts": now_ms(),
                        "event": action_name(action),
                        "root": root.display().to_string(),
                        "path": root.join(rel.replace('\\', "/")).display().to_string(),
                    }),
                );
            }
        }
        unsafe { CloseHandle(handle) };
    }

    fn now_ms() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }
}

/// 起监听：每个根一个线程；`stop` 置位后**下一次事件/调用返回时**收手。
///
/// 为什么不做得更"优雅"（`CancelIoEx`）：那需要另一个句柄池与 Overlapped，
/// 而这里唯一的用法是"Ctrl-C 杀进程"——**进程退出时 journal 已经逐条落盘**，
/// 不需要在退出路径上再赌一次系统调用。
pub fn watch_roots(roots: &[PathBuf], journal: &Path, stop: Arc<AtomicBool>) -> Result<()> {
    if roots.is_empty() {
        anyhow::bail!("没有要监听的目录");
    }
    for r in roots {
        if !r.is_dir() {
            anyhow::bail!("不是目录：{}", r.display());
        }
    }
    #[cfg(windows)]
    {
        let mut handles = Vec::new();
        for r in roots {
            let r = r.clone();
            let j = journal.to_path_buf();
            let s = stop.clone();
            handles.push(std::thread::spawn(move || win::watch_root(&r, &j, s)));
        }
        for h in handles {
            let _ = h.join();
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (roots, journal, stop);
        anyhow::bail!("这个平台没实现变更监听（只做了 Windows）")
    }
}

/// journal 路径（给人看一句话）。
pub fn journal_hint(p: &Path) -> String {
    format!("journal：{}", p.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条 `FILE_NOTIFY_INFORMATION` 记录。
    fn rec(next: u32, action: u32, name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = Vec::new();
        v.extend(next.to_le_bytes());
        v.extend(action.to_le_bytes());
        v.extend(((units.len() * 2) as u32).to_le_bytes());
        for u in units {
            v.extend(u.to_le_bytes());
        }
        while v.len() % 4 != 0 {
            v.push(0);
        }
        v
    }

    #[test]
    fn 解析单条记录() {
        let buf = rec(0, 1, "a.rs");
        let got = parse_notify(&buf);
        assert_eq!(got, vec![(1u32, "a.rs".to_string())]);
    }

    #[test]
    fn 解析两条_按_next_串联() {
        let mut a = rec(0, 3, "src/lib.rs");
        let b = rec(0, 2, "旧.txt");
        let next = a.len() as u32;
        a[0..4].copy_from_slice(&next.to_le_bytes());
        a.extend(b);
        let got = parse_notify(&a);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].1, "src/lib.rs");
        assert_eq!(got[1].1, "旧.txt");
        assert_eq!(got[1].0, 2);
    }

    #[test]
    fn 半条记录也不崩() {
        let buf = rec(0, 1, "a.rs");
        for cut in 0..buf.len() {
            let _ = parse_notify(&buf[..cut]);
        }
    }

    #[test]
    fn 中文与动作名() {
        assert_eq!(action_name(1), "added");
        assert_eq!(action_name(5), "renamed_new");
        let buf = rec(0, 4, "改名前的文件.rs");
        let got = parse_notify(&buf);
        assert_eq!(got[0].1, "改名前的文件.rs");
    }
}
