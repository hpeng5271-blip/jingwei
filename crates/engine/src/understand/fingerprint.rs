//! **项目指纹**（2026-10-19）：判断"两份目录是不是同一个项目"。
//!
//! 设计与取舍（学习调研的结论，一手来源见 `docs/process/研究对象.md`）：
//! - **身份 = 内容 + 命名，不含环境**：绝对路径、盘符、用户名、mtime 一律不进指纹
//!   （SPDX 的 PackageVerificationCode 只对内容做哈希、明确不含路径与时间）；
//! - **路径要绑进去**：只哈希内容会让"两个文件互换名字"不改变指纹，
//!   所以每个元素是 `H(相对路径 ‖ 文件内容哈希)`，排序后再整体哈希（Merkle 列表）；
//! - **先排除 vendored/generated**：`node_modules` / `target` / `dist` / `vendor` 之类
//!   会把指纹拖成"只反映依赖树"，与项目本身无关；
//! - **有上限**：超过上限就标 `truncated`——**截断必须说出来**，不能假装是全量指纹。
//!
//! 两级用法：`hash` 判"完全同一份"；`lockfile` 与 `markers` 作为**证据**展示
//! （"为什么算同一个"），不做静默合并。

use std::path::Path;

/// 指纹 + 形成它的证据。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    /// 精确指纹（相同 = 内容与相对路径完全一致）
    pub hash: String,
    /// 参与计算的文件数
    pub files: u64,
    /// 参与计算的字节数
    pub bytes: u64,
    /// 是否因为上限被截断（截断时 `hash` 只代表"看过的那一部分"）
    pub truncated: bool,
    /// 标记文件的 `名字: 内容哈希`（稳定的身份特征）
    pub markers: Vec<String>,
    /// lockfile 的内容哈希（跨机器稳定：它钉的是精确版本 + 校验和）
    pub lockfile: Option<String>,
}

const MAX_FILES: u64 = 2000;

fn is_vendor(name: &str) -> bool {
    let n = name.to_lowercase();
    matches!(
        n.as_str(),
        "node_modules" | "target" | "dist" | "build" | "vendor" | "vendors" | ".git" | "__pycache__" | ".venv" | "venv" | "bower_components"
    )
}

/// 算一个目录的项目指纹。**只读**，不会写任何东西。
pub fn of(root: &Path) -> Fingerprint {
    of_detailed(root).0
}

/// 指纹 + 文件集合（集合给 Jaccard 用，不塞进 JSON）。
pub fn of_detailed(root: &Path) -> (Fingerprint, Vec<String>) {
    let mut out = Fingerprint::default();
    let mut items: Vec<(String, String)> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut bytes = 0u64;

    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if ft.is_dir() {
                if name.starts_with('.') || is_vendor(&name) {
                    continue;
                }
                stack.push(e.path());
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            if out.files >= MAX_FILES {
                out.truncated = true;
                break;
            }
            let Ok(data) = std::fs::read(e.path()) else { continue };
            let sha = crate::facts::stable_hash([&String::from_utf8_lossy(&data)]);
            let rel = e
                .path()
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| name.clone());
            // 标记文件：名字也要进证据（身份的一部分）
            if is_marker(&name) {
                out.markers.push(format!("{name}:{sha}"));
            }
            if is_lockfile(&name) {
                out.lockfile = Some(sha.clone());
            }
            bytes += data.len() as u64;
            items.push((rel, sha));
            out.files += 1;
        }
        if out.files >= MAX_FILES {
            out.truncated = true;
            break;
        }
    }

    // **排序后**再算：readdir 顺序不是稳定的（学习调研点名的坑）
    items.sort();
    let joined: Vec<String> = items.iter().map(|(p, h)| format!("{p}\u{1}{h}")).collect();
    out.hash = crate::facts::stable_hash(joined.iter().map(|s| s.as_str()));
    out.bytes = bytes;
    out.markers.sort();
    let set: Vec<String> = items.iter().map(|(p, h)| format!("{p}\u{1}{h}")).collect();
    (out, set)
}

/// **文件集合**：每个元素是 `相对路径\u{1}内容哈希`。
///
/// 与 `of()` 的关系：`of()` 是"把集合排序后整体哈希"，适合判"完全同一份"；
/// 集合本身用来算 **Jaccard**，判"同源不同版本"（官方文档/源码里那套）。
pub fn set_of(root: &Path) -> Vec<String> {
    of_detailed(root).1
}

/// **Jaccard 相似度**：交集 / 并集。空集返回 0（"比不了"不是"完全一样"）。
pub fn jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let sa: std::collections::BTreeSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let sb: std::collections::BTreeSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

fn is_marker(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "setup.py"
            | "requirements.txt"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "cmakelists.txt"
            | "gemfile"
            | "composer.json"
            | "pubspec.yaml"
    )
}

fn is_lockfile(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "poetry.lock" | "go.sum"
    )
}

#[cfg(test)]
mod tests {
    // 临时目录清理失败不影响断言，也不能改变结论——显式豁免，别让 lint 在整 crate 失声
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-fp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建目录失败");
        d
    }

    /// **同一份内容、不同路径 → 指纹必须相同**（路径不进指纹，相对路径进）。
    #[test]
    fn 同内容不同位置指纹相同() {
        let a = scratch("a");
        let b = scratch("b");
        for d in [&a, &b] {
            std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
            std::fs::write(d.join("main.rs"), "fn main() {}\n").unwrap();
        }
        let fa = of(&a);
        let fb = of(&b);
        assert_eq!(fa.hash, fb.hash, "内容与相对结构相同 → 同一个项目");
        assert_eq!(fa.files, 2, "两个文件");
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    /// **改一个字节 → 指纹必须变**（否则去重会把两份不同的东西并成一个）。
    #[test]
    fn 改一个字节指纹就变() {
        let d = scratch("c");
        std::fs::write(d.join("a.rs"), "fn a() {}\n").unwrap();
        let before = of(&d).hash;
        std::fs::write(d.join("a.rs"), "fn a() { }\n").unwrap();
        let after = of(&d).hash;
        assert_ne!(before, after, "内容变了指纹必须变");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **vendored 目录不进指纹**：否则 node_modules 一变，整份指纹跟着变。
    #[test]
    fn 依赖目录不影响指纹() {
        let d = scratch("v");
        std::fs::write(d.join("package.json"), "{}").unwrap();
        let before = of(&d).hash;
        std::fs::create_dir_all(d.join("node_modules/x")).unwrap();
        std::fs::write(d.join("node_modules/x/i.js"), "x").unwrap();
        assert_eq!(before, of(&d).hash, "node_modules 不该改变项目指纹");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **同源不同版本**：共享大部分文件、各多一个 → Jaccard 在 0.8 附近，
    /// 但精确指纹必须不同（否则"版本不同"会被当成"完全同一份"）。
    #[test]
    fn jaccard_同源不同版本() {
        let a = scratch("ja");
        let b = scratch("jb");
        for i in 0..8 {
            for d in [&a, &b] {
                std::fs::write(d.join(format!("f{i}.rs")), format!("fn f{i}() {{}}")).unwrap();
            }
        }
        std::fs::write(a.join("only_a.rs"), "fn a_only() {}").unwrap();
        std::fs::write(b.join("only_b.rs"), "fn b_only() {}").unwrap();

        let sa = set_of(&a);
        let sb = set_of(&b);
        let j = jaccard(&sa, &sb);
        assert!((j - 8.0 / 10.0).abs() < 1e-9, "8 共有 / 10 并集 = 0.8，实际 {j}");
        assert_ne!(of(&a).hash, of(&b).hash, "版本不同 → 精确指纹必须不同");
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    /// 完全不相关 → 0；空集 → 0（"比不了"不是"完全一样"）。
    #[test]
    fn jaccard_不相关是零() {
        let a = scratch("jc");
        let b = scratch("jd");
        std::fs::write(a.join("x.rs"), "fn x() {}").unwrap();
        std::fs::write(b.join("y.rs"), "fn y() {}").unwrap();
        assert_eq!(jaccard(&set_of(&a), &set_of(&b)), 0.0);
        assert_eq!(jaccard(&[], &[]), 0.0);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
