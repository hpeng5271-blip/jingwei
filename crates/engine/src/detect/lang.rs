//! 这个目录是什么项目。
//!
//! 判据只看文件，不解析内容——每多花一毫秒都在打开项目的第一帧上。
//! 知道是哪门语言，后续探测才能只做该做的：打开 Python 项目不该去 spawn java。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Lang {
    Python,
    Java,
    Cpp,
    /// Node / 前端那一类：Vue、React、纯 npm 项目
    Web,
    #[default]
    Unknown,
}


impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "Python",
            Lang::Java => "Java",
            Lang::Cpp => "C / C++",
            Lang::Web => "前端 / Node",
            Lang::Unknown => "还没认出来",
        }
    }
}

/// 认一些文件夹名。复合项目里 backend / frontend 这种命名几乎是惯例，
/// 光看名字就能少扫很多目录。
fn root_markers(root: &Path) -> Vec<Lang> {
    let mut out = Vec::new();
    let mut check = |p: &Path| {
        let l = detect_one(p);
        if l != Lang::Unknown && !out.contains(&l) {
            out.push(l);
        }
    };
    check(root);
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut n = 0;
        for e in rd.flatten() {
            if !e.path().is_dir() || n >= 16 {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if matches!(
                name.as_str(),
                ".git" | "node_modules" | "target" | "build" | ".venv" | "venv" | "dist"
            ) {
                continue;
            }
            n += 1;
            check(&e.path());
        }
    }
    out
}

/// 复合项目：一个仓库里既是 Java 后端又是 Vue 前端。
/// 这个数组就是「这个项目由哪些部分组成」，顺序按重要性排。
pub fn detect_all(root: &Path) -> Vec<Lang> {
    if !root.is_dir() {
        return Vec::new();
    }
    root_markers(root)
}

/// 主类型：复合项目里挑一个最该先看的。
pub fn detect(root: &Path) -> Lang {
    detect_all(root).first().copied().unwrap_or(Lang::Unknown)
}

fn detect_one(root: &Path) -> Lang {
    if !root.is_dir() {
        return Lang::Unknown;
    }
    // 构建文件一出现就不会认错，所以先看它
    if has(root, &["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle"]) {
        return Lang::Java;
    }
    if has(root, &["CMakeLists.txt", "Makefile", "makefile", "meson.build"]) {
        return Lang::Cpp;
    }
    if has(
        root,
        &[
            "package.json",
            "vite.config.js",
            "vite.config.ts",
            "vue.config.js",
            "next.config.js",
        ],
    ) {
        return Lang::Web;
    }
    if has(root, &["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"]) {
        return Lang::Python;
    }
    if has_ext(root, &["py"]) {
        return Lang::Python;
    }
    if has_ext(root, &["java"]) {
        return Lang::Java;
    }
    if has_ext(root, &["c", "cc", "cpp", "cxx", "h", "hpp"]) {
        return Lang::Cpp;
    }
    if has_ext(root, &["js", "ts", "vue", "jsx", "tsx"]) {
        return Lang::Web;
    }
    Lang::Unknown
}

fn has(root: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| root.join(n).is_file())
}

/// 有没有这种后缀的源码。只扫两层、遇到就停——深挖对大项目太贵。
fn has_ext(root: &Path, exts: &[&str]) -> bool {
    let Ok(rd) = std::fs::read_dir(root) else {
        return false;
    };
    let mut subdirs = Vec::new();
    let hit = |p: &Path| {
        p.extension()
            .and_then(|s| s.to_str())
            .map(|x| exts.contains(&x.to_lowercase().as_str()))
            .unwrap_or(false)
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_file() {
            if hit(&p) {
                return true;
            }
        } else if p.is_dir() && subdirs.len() < 24 {
            let n = e.file_name().to_string_lossy().to_lowercase();
            // 这些目录里全是别人的东西，别去数
            if !matches!(
                n.as_str(),
                ".git" | "target" | "build" | "node_modules" | ".venv" | "venv"
            ) {
                subdirs.push(p);
            }
        }
    }
    for d in subdirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            if rd.flatten().any(|e| hit(&e.path())) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pl-lang-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn build_files_win_over_loose_source_files() {
        let d = scratch("both");
        std::fs::write(d.join("pom.xml"), "<project/>").unwrap();
        std::fs::write(d.join("helper.py"), "print(1)").unwrap();
        assert_eq!(detect(&d), Lang::Java, "有 pom.xml 就该认成 Java 项目");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn detects_by_source_extension_too() {
        let d = scratch("src");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src").join("main.cpp"), "int main(){}").unwrap();
        assert_eq!(detect(&d), Lang::Cpp);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_folder_is_not_claimed_to_be_anything() {
        let d = scratch("empty");
        assert_eq!(detect(&d), Lang::Unknown);
        let _ = std::fs::remove_dir_all(&d);
    }
}
