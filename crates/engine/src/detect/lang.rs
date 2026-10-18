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
    /// Rust：`Cargo.toml` 或 `.rs` 源码。
    ///
    /// **这个变体是补上的，而且补得晚。** 在它之前，"Rust 项目"这件事在整个
    /// `detect` 层是**不存在的**：`hello_rust`（有 Cargo.toml、有 src/main.rs）
    /// 的 `langs` 是**空数组**、`lang` 是 `Unknown`。三处后果，一处比一处隐蔽：
    ///
    ///   1. 界面「项目类型」那栏是空的；
    ///   2. `understand/project.rs` 里那句 `l == "Rust"` **永不成立**（一句死判断）；
    ///   3. 最糟的：`understand/rules.rs` 把"`langs` 为空且 `lang` 是 Unknown"
    ///      当作"可能是 Python 项目"，于是**对一个纯 Rust 项目跑了全套 Python 规则**
    ///      （解释器、venv、编码、依赖……每条都可能变成假警报）。
    ///
    /// 而 `detect/deps.rs` 里留着一句自己承认的绕道：「Rust 在 `lang::Lang` 里
    /// 没有独立变体，用清单文件的存在性补上」——把 `Unknown` 塞进语言列表冒充 Rust。
    /// 一个枚举少一个变体，另外三处就得各自绕一次。这就是那三次绕道的总账。
    Rust,
    Cpp,
    /// Node / 前端那一类：Vue、React、纯 npm 项目
    Web,
    #[default]
    Unknown,
}


impl Lang {
    /// **这个生态里"装依赖"的原生命令**（2026-10-18，真 Spring petclinic 换的）。
    ///
    /// 原来 `guide` 对所有语言都写 `jingwei env install`，而它走契约里的
    /// `commands.install`——**Maven 项目没有那份契约**，用户敲下去只得到一句
    /// 「契约里没有声明 install 命令」。**提示把用户引进了死路。**
    ///
    /// 放在 `Lang` 上而不是 `guide` 里：这是**这门语言的事实**，不是某个消费点的排版。
    pub fn install_hint(self) -> &'static str {
        match self {
            Lang::Python => "jingwei env install（或 python -m pip install -r requirements.txt）",
            Lang::Java => "mvn dependency:go-offline（没装 Maven 就先装 mvn）",
            Lang::Rust => "cargo fetch",
            Lang::Web => "npm install",
            _ => "按这个项目的生态装依赖",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "Python",
            Lang::Java => "Java",
            Lang::Rust => "Rust",
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
            // 判法只有一处（`util::scan::is_not_source_dir`）。
            // 这里早就转了小写、而 `index.rs` / `watch.rs` 没转——**同一份清单、两种判法**。
            if crate::util::scan::is_not_source_dir(&name) {
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
    // **`Makefile` 不是语言标记**（2026-10-18，真 `psf/requests` 抓到的）：它放在这里，
    // 于是**所有带 Makefile 的 Python 项目都被判成 C/C++**（requests 的`make test`就是）。
    // 只有 `CMakeLists.txt` 留在强标记里——那才是 C/C++ 的清单文件。
    if has(root, &["CMakeLists.txt"]) {
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
    // Rust：构建文件优先于"有没有 .rs 文件"——一个仓库里带个示例 .rs 不该叫 Rust 项目。
    if has(root, &["Cargo.toml"]) {
        return Lang::Rust;
    }
    if has_ext(root, &["py"]) {
        return Lang::Python;
    }
    if has_ext(root, &["java"]) {
        return Lang::Java;
    }
    if has_ext(root, &["rs"]) {
        return Lang::Rust;
    }
    if has_ext(root, &["c", "cc", "cpp", "cxx", "h", "hpp"]) {
        return Lang::Cpp;
    }
    if has_ext(root, &["js", "ts", "vue", "jsx", "tsx"]) {
        return Lang::Web;
    }
    // **弱标记放最后**：没有别的语言清单、只有 Makefile/meson，才当 C/C++。
    if has(root, &["Makefile", "makefile", "meson.build"]) {
        return Lang::Cpp;
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

    /// **`Makefile` 不是语言标记**（2026-10-18，真 `psf/requests` 抓到的）。
    ///
    /// 修之前：`detect_one` 把 `Makefile` 和 `CMakeLists.txt` 并列当 C/C++ 强标记，
    /// 于是**所有带 `make test` 的 Python 项目都被判成「C / C++ + Python（混合项目）」**。
    /// 后果不只是标签难看：主语言错 → 依赖/规则全按 C++ 走。
    #[test]
    fn makefile不是语言标记() {
        // ① Python 清单 + Makefile → Python（requests 的形状）
        let d = scratch("make-py");
        std::fs::write(d.join("Makefile"), "test:\n\tpython -m pytest").unwrap();
        std::fs::write(d.join("pyproject.toml"), "[project]\nname=\"x\"\n").unwrap();
        assert_eq!(detect(&d), Lang::Python, "有 pyproject 就是 Python，Makefile 只是构建工具");
        let _ = std::fs::remove_dir_all(&d);

        // ② 只有 Makefile + C 源码 → C/C++（弱标记兜底）
        let e = scratch("make-c");
        std::fs::write(e.join("Makefile"), "all:\n\tcc main.c").unwrap();
        std::fs::write(e.join("main.c"), "int main(){}").unwrap();
        assert_eq!(detect(&e), Lang::Cpp);
        let _ = std::fs::remove_dir_all(&e);

        // ③ `CMakeLists.txt` 仍是强标记
        let f = scratch("cmake");
        std::fs::write(f.join("CMakeLists.txt"), "project(x)").unwrap();
        assert_eq!(detect(&f), Lang::Cpp);
        let _ = std::fs::remove_dir_all(&f);
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

    /// **Rust 项目要认出来。**
    ///
    /// 这条是拿一次真事故换的：`hello_rust`（有 `Cargo.toml`、有 `src/main.rs`）的
    /// `langs` 是**空数组**、`lang` 是 `Unknown`——因为 `Lang` 枚举里压根没有 Rust 变体。
    /// 后果里最隐蔽的一条：`understand/rules.rs` 把"`langs` 为空且 `lang` 是 Unknown"
    /// 当成"可能是 Python 项目"，于是**对一个纯 Rust 项目跑了全套 Python 规则**。
    /// 所以这里不只断言"认出来"，还断言**不能被当成 Python**。
    #[test]
    fn a_rust_project_is_rust_and_not_python() {
        let d = scratch("rust");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(d.join("src").join("main.rs"), "fn main(){}").unwrap();
        assert_eq!(detect(&d), Lang::Rust, "有 Cargo.toml 就该认成 Rust 项目");
        assert!(detect_all(&d).contains(&Lang::Rust), "langs 里要有 Rust");
        assert!(
            !detect_all(&d).contains(&Lang::Python),
            "一个纯 Rust 项目不许被当成 Python——那会让整套 Python 规则跑在它身上"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 只有 `.rs` 源码、没有构建文件时也要认出来（判据不能只挂在 Cargo.toml 上）。
    #[test]
    fn rust_source_alone_is_enough() {
        let d = scratch("rustsrc");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src").join("lib.rs"), "pub fn f(){}").unwrap();
        assert_eq!(detect(&d), Lang::Rust);
        let _ = std::fs::remove_dir_all(&d);
    }
}
