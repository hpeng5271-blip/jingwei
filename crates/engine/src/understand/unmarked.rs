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

/// **别人的安装树**：这些路径下就算有源码，也不该劝人加清单（2026-10-18 实测后加的）。
///
/// 实测（真机 40 个目录）：不加这条时，`C:\Program Files\Application Verifier`、
/// `D:\VSBT\Common7`、`C:\Users\cjz\AppData\Roaming\Code` 全都被建议"补一个 pyproject.toml"——
/// 那三个地方没有一个该有 pyproject。**这类目录的清单在安装包/产品仓库里，不在这里。**
const FOREIGN_TREE: &[&str] = &[
    "\\program files\\",
    "\\program files (x86)\\",
    "\\appdata\\roaming\\",
    "\\appdata\\local\\programs\\",
    "\\windows\\",
    "\\microsoft visual studio\\",
    "\\windows kits\\",
];

/// **已安装产品**的判据：目录里 DLL/EXE 多到不像源码夹，就不再劝它加清单。
///
/// 实测依据：`Application Verifier` 只有 1 个 `.h`，但它装着整套 exe/dll——
/// 光看源码数量会把它当成"刚起步的项目"。
const BINARY_EXT: &[&str] = &["dll", "exe", "pdb", "lib", "obj", "jar"];
const BINARY_LIMIT: usize = 5;
/// **至少 3 个**源码文件才开口（实测：临时夹具里 1 个 `.py` 也被劝过加清单）。
const MIN_FILES: usize = 3;

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
/// 这个目录是不是在**别人的安装树**里（Program Files / AppData / Windows / VS 安装目录）。
pub fn in_foreign_tree(root: &Path) -> bool {
    let p = root.to_string_lossy().replace('/', "\\").to_lowercase();
    let p = p + "\\";
    FOREIGN_TREE.iter().any(|k| p.contains(k))
}

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
    // **别人的安装树一律沉默**（实测：Program Files / AppData / VS 安装目录会被误报）。
    if !root.is_dir() || in_package_tree(root) || in_foreign_tree(root) {
        return None;
    }
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    let mut files = 0usize;
    // **已安装产品**：DLL/EXE 多到这个数，就不是"人在这里写代码"，是"别人装了个东西"。
    let mut binaries = 0usize;
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
                if BINARY_EXT.contains(&ext.as_str()) {
                    binaries += 1;
                }
                if SRC_EXT.contains(&ext.as_str()) {
                    files += 1;
                    *counts.entry(ext).or_insert(0) += 1;
                }
            }
        }
    }
    // **两条下限**（都是实测换的）：
    // ① 少于 3 个源码文件：临时夹具里 1 个 `.py` 也被劝过加清单，那是噪声；
    // ② DLL/EXE 超过 5 个：那是"已安装的产品"，清单在它的安装包里。
    if files < MIN_FILES || binaries > BINARY_LIMIT {
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

    /// **该补哪种清单**（按源码里最多的那门语言排序，2026-10-18 加）。
    ///
    /// 原来三句建议都是「`pyproject.toml` / `package.json` / `Cargo.toml` 任一」——
    /// 对每一种项目都念同一串。真机实测（`@deepseek-ai` 那个 1 万多个 js/ts 的包）：
    /// 一个纯 `.ts/.js` 目录被告知先补 `pyproject.toml`。**那不是建议，那是噪音。**
    ///
    /// 判据：看 `langs` 里数量最多的那个扩展名——它决定这个项目"本来该有什么清单"。
    /// 认不出来时（比如 .go/.java 这类我们还不解析的）退回那串"任一"，
    /// **不猜**：给错的清单比给一串候选更糟。
    pub fn manifest_hint(&self) -> &'static str {
        let top = self
            .langs
            .iter()
            .max_by_key(|(_, n)| *n)
            .map(|(e, _)| e.as_str())
            .unwrap_or("");
        match top {
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => "`package.json`",
            "py" => "`pyproject.toml`",
            "rs" => "`Cargo.toml`",
            _ => "`pyproject.toml` / `package.json` / `Cargo.toml` 任一",
        }
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

    /// **别人的安装树里一律沉默**（2026-10-18 真机实测后加的）。
    ///
    /// 实测（40 个真目录）：不设这条时 `C:\Program Files\Application Verifier`、
    /// `D:\VSBT\Common7`、`C:\Users\<名>\AppData\Roaming\Code` 都被劝"补一个清单"——
    /// 那些地方的"清单"在安装包/产品仓库里，用户在这里补是补错了地方。
    #[test]
    fn 安装树里保持沉默() {
        // **夹具的名字必须是「Program Files」（带空格）**：判据匹配的是这个串。
        // 第一版我写成 `ProgramFiles`，测试当场红——**夹具照判据写，不是照感觉写**。
        let base = std::env::temp_dir().join("dd-foreign-tree");
        let d = base.join("Program Files").join("SomeTool");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&d).unwrap();
        for n in ["a.py", "b.py", "c.py"] {
            fs::write(d.join(n), "x=1\n").unwrap();
        }
        assert!(in_foreign_tree(&d), "路径里有 Program Files 就该判成安装树");
        assert!(unmarked_sources(&d).is_none(), "安装树里不许给建议");
        let _ = fs::remove_dir_all(&base);
    }

    /// **已安装的产品**（DLL/EXE 一堆）不许劝加清单——它只有 1 个 `.h` 也不该被当成新项目。
    #[test]
    fn 装好的产品不给建议() {
        let d = scratch("installed");
        fs::write(d.join("only.h"), "// x\n").unwrap();
        for i in 0..6 {
            fs::write(d.join(format!("lib{i}.dll")), b"\0").unwrap();
        }
        assert!(unmarked_sources(&d).is_none(), "DLL 一堆 = 已安装的产品，不该给建议");
        let _ = fs::remove_dir_all(&d);
    }

    /// **少于 3 个源码文件不开口**：实测临时夹具里 1 个 `.py` 也被劝过加清单。
    #[test]
    fn 太少的源码不开口() {
        let d = scratch("tiny");
        fs::write(d.join("one.py"), "x=1\n").unwrap();
        assert!(unmarked_sources(&d).is_none(), "1 个 .py 不值得劝人加清单");
        fs::write(d.join("two.py"), "y=1\n").unwrap();
        assert!(unmarked_sources(&d).is_none(), "2 个也不够");
        fs::write(d.join("three.py"), "z=1\n").unwrap();
        assert!(unmarked_sources(&d).is_some(), "3 个才开口");
        let _ = fs::remove_dir_all(&d);
    }

    /// **该补哪种清单：按源码里最多的那门语言说**（2026-10-18，真机换的）。
    ///
    /// 拿到一个 1 万多个 js/ts 的真实 npm 包（`@deepseek-ai`）跑 `guide`，它说：
    /// 「补一个清单（`pyproject.toml` / `package.json` / `Cargo.toml` 任一）」——
    /// 对一个纯 TS 目录先念 `pyproject.toml`，**那不是建议，是噪音**。
    #[test]
    fn 清单建议按语言排序() {
        let mk = |langs: Vec<(&str, usize)>| UnmarkedSources {
            files: langs.iter().map(|(_, n)| *n).sum(),
            langs: langs.into_iter().map(|(e, n)| (e.to_string(), n)).collect(),
            dirs: 1,
        };
        assert_eq!(mk(vec![("ts", 8), ("js", 7)]).manifest_hint(), "`package.json`");
        assert_eq!(mk(vec![("py", 3)]).manifest_hint(), "`pyproject.toml`");
        assert_eq!(mk(vec![("rs", 5)]).manifest_hint(), "`Cargo.toml`");
        // 认不出来的语言**不猜**：给一串候选，别给一个错的。
        assert_eq!(
            mk(vec![("go", 9)]).manifest_hint(),
            "`pyproject.toml` / `package.json` / `Cargo.toml` 任一"
        );
        // 混合时看最多的那个：py 多过 ts 就提 pyproject。
        assert_eq!(mk(vec![("py", 6), ("ts", 2)]).manifest_hint(), "`pyproject.toml`");
    }
}
