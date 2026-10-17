//! 扫盘时该跳过的目录：网络盘、联接点/软链、超长路径。
//!
//! 这三样都不罕见，而它们的**共同症状都是"卡在那里不动"**：
//!   · 网络盘断线时一次 `read_dir` 能挂几十秒；
//!   · 联接点指回上一层，扫一遍就是无限循环（`node_modules` 里很常见）；
//!   · 超过 260 字符的老接口直接报错，看起来像"这个目录不存在"。
//! 所以不是"遇到了再处理"，而是**进去之前先问一句**，并把原因记下来——
//! 用户看到"跳过了 3 个目录"总比看着进度条不动好。

use std::path::Path;

/// 老式 Win32 路径上限。超过它要么加 `\\?\` 前缀，要么别碰——这里选择不碰并说明。
const PATH_LIMIT: usize = 240;

#[derive(Debug, Clone, PartialEq)]
pub struct Skip {
    pub path: String,
    pub why: &'static str,
}

/// 这个目录该不该跳过。返回 None 表示可以进去。
pub fn should_skip(dir: &Path, root: &Path) -> Option<&'static str> {
    let s = dir.to_string_lossy();

    // 网络路径：UNC（\\server\share）与映射的网络盘
    if s.starts_with("\\\\") {
        return Some("网络路径（UNC），断线时会把扫描挂住");
    }
    if is_remote_drive(dir) {
        return Some("网络盘，断线时会把扫描挂住");
    }

    // 符号链接 / 联接点：绕开循环，也绕开"扫到项目外面去"
    if let Ok(md) = std::fs::symlink_metadata(dir) {
        if md.file_type().is_symlink() {
            return Some("符号链接或联接点，可能指回上一层造成循环");
        }
    }

    // 超长路径
    if s.len() > PATH_LIMIT {
        return Some("路径过长，老式文件接口处理不了");
    }

    // 别走到项目根的外面去（防止顺着软链跑飞）
    if !root.as_os_str().is_empty() && !s.starts_with(&*root.to_string_lossy()) {
        return Some("不在当前项目范围内");
    }
    None
}

#[cfg(windows)]
fn is_remote_drive(dir: &Path) -> bool {
    extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_UNKNOWN: u32 = 0;

    // 取盘符（"D:\\..." → "D:\\"）
    let s = dir.to_string_lossy();
    let b = s.as_bytes();
    if b.len() < 2 || b[1] != b':' {
        return false;
    }
    let drive: Vec<u16> = format!("{}:\\", b[0] as char)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `drive` 是以 NUL 结尾的 UTF-16 盘根串，在本行归还前一直存活；
    // 该 API 只读这个串。
    let t = unsafe { GetDriveTypeW(drive.as_ptr()) };
    t == DRIVE_REMOTE || t == DRIVE_UNKNOWN
}

#[cfg(not(windows))]
fn is_remote_drive(_dir: &Path) -> bool {
    false
}

// ------------------------------------------------------------------ 目录黑名单

/// **"不当源码扫描"的目录**——**唯一一处**（2026-10-17）。
///
/// ## 为什么要有这个常量
///
/// 它原来是**三份几乎一样的清单**，分别写在 `detect/lang.rs`、`graph/index.rs`、
/// `graph/watch.rs` 里。三份一样，但**判法不同**：
///
///   · `lang.rs` 把名字 `to_lowercase()` 之后比 —— 对；
///   · `index.rs` / `watch.rs` **直接比原名** —— 于是 Windows 上
///     `Target/`（大写 T）**漏过去了**，实测被当成源码索引进去
///     （`jingwei index` 对一个含 `Target/deep/x.rs` 的目录报 **2 个文件**，
///     而真源码只有 1 个）。
///
/// 而 Windows 的文件系统**不区分大小写**，所以 `Target` 和 `target`
/// 是**同一个目录**——一份清单、两种判法，结果就分叉。
///
/// ## 为什么 `__pycache__` 必须在这儿
///
/// `graph/parse_python_tests.rs` 的注释早就写着「`__pycache__` 之类不该被当成源码，
/// **这条由 `index.rs` 的目录黑名单管**」——而那份黑名单里**根本没有 `__pycache__`**。
/// 一条假注释，指着一条不存在的判据（实测：`__pycache__/x.py` 会被索引）。
///
/// ## 为什么 `.jingwei` 也在
///
/// 那是**我们自己的笔记目录**。`AGENTS.md` 记过一笔：
/// 「`.jingwei` 污染结论——我们自己的笔记成了这个项目最长的路径」。
/// **别把自己算进被观察的对象。**
pub(crate) const NOT_SOURCE_DIRS: &[&str] = &[
    ".git",
    ".jingwei",
    "node_modules",
    "target",
    "build",
    "dist",
    ".venv",
    "venv",
    "__pycache__",
];

/// 这个目录名是不是"不当源码扫描"的。
///
/// **判法只在这一处**：`to_lowercase()` 之后比。调用方不许自己再写一份
/// `matches!(name, "target" | ...)`——那正是分叉的起点。
pub(crate) fn is_not_source_dir(name: &str) -> bool {
    let n = name.to_lowercase();
    NOT_SOURCE_DIRS.contains(&n.as_str())
}
#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unc_paths_are_skipped_with_a_reason() {
        let r = PathBuf::from("C:\\work");
        let why = should_skip(Path::new("\\\\server\\share\\proj"), &r).unwrap();
        assert!(why.contains("网络"), "{}", why);
    }

    #[test]
    fn overlong_paths_are_skipped() {
        let r = PathBuf::from("C:\\work");
        let long = format!("C:\\work\\{}", "a".repeat(300));
        let why = should_skip(Path::new(&long), &r).unwrap();
        assert!(why.contains("过长"), "{}", why);
    }

    /// 正常的项目目录不许被误跳——跳过太多比不跳还糟（用户会以为项目没了）。
    #[test]
    fn ordinary_dirs_are_not_skipped() {
        let root = std::env::temp_dir().join(format!("jingwei-scan-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert_eq!(should_skip(&root.join("src"), &root), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
