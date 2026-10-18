//! **没有项目标记、但有源码的目录**：给不给建议？（2026-10-18，用户拍板「要」）
//!
//! ## 这个模块存在的理由
//!
//! 原来的规则是"**没有项目标记就一律沉默**"。它来自 5 个真库换来的教训：
//! `site-packages/<pkg>` 的清单在**上一层**的 `*.dist-info` 里，
//! 对它说"你没清单"是**对象错了**（不是用户的空白，是我们问错了）。
//!
//! 但那条规则把**你自己的脚本文件夹**也一起静音了：几个 `.py`、没清单，
//! 它同样听不到「你有源码但没有依赖清单」——而那正是它最该说的一句话。
//!
//! ## 用户拍板（2026-10-18）
//!
//! 问：「没有项目标记的目录，要不要给项目建议？」答：**要**。
//! 代价写清：**包管理器树里必须排除**，否则会对着别人的包狂报。
//!
//! ## 判据（两条，缺一不可）
//!
//! ```text
//! 1. **不在包管理器树里**：路径含 site-packages / dist-packages / node_modules /
//!    .cargo\registry / .rustup\toolchains / .venv\ / \venv\ / __pycache__
//!    —— 这些目录里的东西归**别人**管，我们的建议会在错误的层级响。
//! 2. **有可解析的源码**：只数 `.py` / `.rs`（我们能解析的两种）+ 常见前端/Java 扩展名，
//!    免得把"只有一个 README 的文件夹"也说成"你有源码"。
//! ```
//!
//! 两条都成立，才返回 `Some(UnmarkedSources)`；否则 `None`（保持沉默）。

use std::path::Path;

/// 包管理器树里的目录名（**任何一个出现在路径里就不给建议**）。
///
/// 为什么用"路径包含"而不是"父目录名相等"：`site-packages` 可能在很深的层级，
/// 而它下面还套着几十层包目录；只要祖先里有它，就说明这些文件不归用户管。
const PKG_TREE: &[&str] = &[
    "site-packages",
    "dist-packages",
    "node_modules",
    ".cargo\\registry",
    ".cargo/registry",
    ".rustup\\toolchains",
    ".rustup/toolchains",
    "__pycache__",
    "\\.venv\\",
    "/.venv/",
    "\\venv\\",
    "/venv/",
];

/// 我们会用来判断"有源码"的扩展名。
///
/// **刻意包含我们解析不了的语言**（ts/js/java）：那句话是"你有源码但没有清单"，
/// 和"我们能不能解析"是两件事——混在一起就变成了"我们不支持 = 你没有"。
const SRC_EXT: &[&str] = &[
    "py", "rs", "ts", "tsx", "js", "jsx", "mjs", "vue", "svelte", "java", "kt", "go", "c", "h",
    "cc", "cpp", "hpp", "cs", "rb", "php",
];

#[derive(Debug, Clone, PartialEq)]
pub struct UnmarkedSources {
    /// 数到的源码文件数。
    pub files: usize,
    /// 按扩展名从多到少（最多留 3 个，够说清"你有什么"）。
    pub langs: Vec<(String, usize)>,
    /// 这次只看了多少个子目录（**给读者一个规模感，避免"我扫了全盘"的误解**）。
    pub dirs: usize,
}

/// 这个目录是不是在**别人管的树**里（包管理器 / 虚拟环境 / 缓存）。
pub fn in_package_tree(root: &Path) -> bool {
    let p = root.to_string_lossy().replace('/', "\\").to_lowercase();
    PKG_TREE
        .iter()
        .any(|k| p.contains(&k.replace('/', "\\").to_lowercase()))
}

/// 数一下"没有项目标记但有源码"的情况。**返回 `None` 就是"保持沉默"。**
///
/// 上限 2000 个目录：这一条只用来决定"要不要给一句建议"，
/// 不值得为它做全量遍历（真实项目通常几十个目录）。
pub fn unmarked_sources(root: &Path) -> Option<UnmarkedSources> {
    if !root.is_dir() || in_package_tree(root) {
        return None;
    }
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    let mut files = 0usize;
    let mut dirs = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        dirs += 1;
        if dirs > 2000 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let ft = match e.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let name = e.file_name().to_string_lossy().to_string();
            if ft.is_dir() {
                // **跳过我们自己的数据目录**：`.jingwei` 里是图谱库，不是源码。
                if name == ".git" || name == ".jingwei" || name == "target" {
                    continue;
                }
                let child = e.path();
                if in_package_tree(&child) {
                    continue;
                }
                stack.push(child);
            } else if let Some(ext) = name.rsplit('.').next() {
                let ext = ext.to_lowercase();
                if SRC_EXT.contains(&ext.as_str()) {
                    files += 1;
                    *counts.entry(ext).or_insert(0) += 1;
                }
            }
        }
    }
    if files == 0 {
        return None;
    }
    let mut langs: Vec<(String, usize)> = counts.into_iter().collect();
    langs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    langs.truncate(3);
    Some(UnmarkedSources { files, langs, dirs })
}

impl UnmarkedSources {
    /// 给人看的一句："10 个 .py、2 个 .ts"。
    pub fn lang_list(&self) -> String {
        self.langs
            .iter()
            .map(|(e, n)| format!("{n} 个 .{e}"))
            .collect::<Vec<_>>()
            .join("、")
    }
}

#[cfg(test)]
mod tests {
    // 临时目录清理失败不影响断言，也不能改变结论——显式豁免（与 `guide.rs` 同一句）。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dd-unmarked-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// **有源码、没标记 → 给一句建议的依据**（用户拍板「要」的那个行为）。
    #[test]
    fn 没标记但有源码就算数() {
        let d = scratch("py");
        for n in ["a.py", "b.py", "c.py"] {
            fs::write(d.join(n), "print(1)\n").unwrap();
        }
        let u = unmarked_sources(&d).expect("有 3 个 .py 就该给建议");
        assert_eq!(u.files, 3);
        assert!(u.lang_list().contains("3 个 .py"), "{}", u.lang_list());
        let _ = fs::remove_dir_all(&d);
    }

    /// **包管理器树里一律沉默**——否则会对着别人的包狂报（5 个真库换来的教训）。
    #[test]
    fn 包管理器树里保持沉默() {
        let d = scratch("pkg");
        let sp = d.join("site-packages").join("somelib");
        fs::create_dir_all(&sp).unwrap();
        fs::write(sp.join("__init__.py"), "").unwrap();
        assert!(in_package_tree(&sp), "路径里含 site-packages 就该判成树里");
        assert!(unmarked_sources(&sp).is_none(), "树里不许给建议");
        let _ = fs::remove_dir_all(&d);
    }

    /// 没有源码就不算（只有一个 README 的文件夹不该被说成"你有源码"）。
    #[test]
    fn 没有源码就不算() {
        let d = scratch("empty");
        fs::write(d.join("README.md"), "# hi\n").unwrap();
        assert!(unmarked_sources(&d).is_none());
        let _ = fs::remove_dir_all(&d);
    }

    /// **我们解析不了的语言也要数**：那句话是"你有源码但没清单"，
    /// 与"我们能不能解析"是两件事（混起来就变成"我们不支持 = 你没有"）。
    #[test]
    fn 解析不了的语言也要数() {
        let d = scratch("ts");
        for n in ["a.ts", "b.ts", "c.java"] {
            fs::write(d.join(n), "// x\n").unwrap();
        }
        let u = unmarked_sources(&d).expect("ts/java 也算源码");
        assert_eq!(u.files, 3, "{:?}", u);
        let _ = fs::remove_dir_all(&d);
    }
}
