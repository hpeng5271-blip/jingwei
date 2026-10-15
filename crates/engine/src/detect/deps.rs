//! 依赖：把「清单里声明的」和「机器上实际的」摆成两列。
//!
//! 为什么单独一个模块：每种语言的清单长得都不一样。拿 Python 那套去读
//! 别的语言，界面就会一本正经地显示「未声明依赖」——那不是简洁，是撒谎。
//!
//! 另一条底线：**确认不了就说确认不了**。C++ 的包可能来自系统包管理器、
//! vcpkg、Conan 或者源码树，我们没有可靠办法知道它装没装；
//! 那就显示「无法确认」，绝不报成「未安装」。

use crate::lang::Lang;
use std::path::Path;

/// 一条依赖边：谁把谁捎带进来了。
/// 这就是 EMI 那条「recipe tree」的形状——把一件复杂的事拆开，
/// 让你看见每一步、以及最后到底拉进来多少个包。
#[derive(Debug, Clone, PartialEq)]
pub struct TreeEdge {
    pub from: String,
    pub to: String,
}

/// 从 Cargo.lock 读依赖图。
///
/// 为什么不用猜：Cargo.lock 里每个 `[[package]]` 都写着自己的 `dependencies`，
/// 间接依赖是**记在案的事实**，不是推断出来的。别的语言没有这么一份现成的东西
/// （npm 要 node_modules/*/package.json、Maven 要跑一次 dependency:tree），
/// 所以这一层先只对 Rust 做实，其余语言留空——**不做假数据**。
pub fn rust_tree(root: &Path) -> (Vec<TreeEdge>, usize) {
    let Some(text) = find_up(root, "Cargo.lock", 3).and_then(|p| std::fs::read_to_string(p).ok())
    else {
        return (Vec::new(), 0);
    };
    let mut pkgs: Vec<(String, Vec<String>)> = Vec::new();
    let mut cur: Option<(String, Vec<String>)> = None;
    let mut in_deps = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            if let Some(p) = cur.take() {
                pkgs.push(p);
            }
            cur = Some((String::new(), Vec::new()));
            in_deps = false;
            continue;
        }
        let Some(p) = cur.as_mut() else { continue };
        if line.starts_with("dependencies = [") {
            in_deps = true;
            continue;
        }
        if in_deps {
            if line.starts_with(']') {
                in_deps = false;
                continue;
            }
            let name = line
                .trim_matches(|c| c == '"' || c == ',' || c == ' ' || c == '\t')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                p.1.push(name);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("name = ") {
            p.0 = rest.trim().trim_matches('"').to_string();
        }
    }
    if let Some(p) = cur.take() {
        pkgs.push(p);
    }
    let total = pkgs.len();
    let mut edges = Vec::new();
    for (name, deps) in &pkgs {
        if name.is_empty() {
            continue;
        }
        for d in deps {
            edges.push(TreeEdge { from: name.clone(), to: d.clone() });
        }
    }
    (edges, total)
}

/// 汇总「声明 → 实际」两列。两条取数的路径（GUI 的完整探测、CLI 的快速探测）
/// 都走这里，免得同一个界面在两处各算一遍、算出不一样的结果。
///
/// `py_installed` 为空 = 这次没有 pip 清单（快速路径）：
/// 那种情况下 Python 依赖标「无法确认」，**不猜成未安装**。
pub fn collect(
    root: &Path,
    langs: &[Lang],
    contract: Option<&crate::contract::Contract>,
    py_installed: &[String],
) -> (Vec<crate::facts::DepDetail>, std::collections::BTreeMap<String, bool>) {
    let mut decls = declared_all(root, langs);
    if let Some(c) = contract {
        for d in c.deps.required.iter() {
            if !decls.iter().any(|x| &x.name == d) {
                decls.push(Decl {
                    name: d.clone(),
                    want: String::new(),
                    from: "env.contract.json".into(),
                    lang: Lang::Unknown,
                    kind: DepKind::Declared,
                });
            }
        }
    }
    let mut detail = Vec::new();
    let mut status = std::collections::BTreeMap::new();
    for d in decls {
        let v = if d.lang == Lang::Python && py_installed.is_empty() {
            Verdict::Unknown
        } else {
            installed(root, d.lang, &d.name, py_installed)
        };
        let (ok, unknown, have) = match v {
            Verdict::Yes(ver) => (true, false, ver.unwrap_or_default()),
            Verdict::No => (false, false, String::new()),
            Verdict::Unknown => (false, true, String::new()),
        };
        status.insert(d.name.clone(), ok);
        detail.push(crate::facts::DepDetail {
            name: d.name,
            want: d.want,
            have,
            ok,
            unknown,
            from: d.from,
        });
    }
    (detail, status)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    /// 依赖名（Rust 用 crate 名、Java 用 groupId:artifactId）
    pub name: String,
    /// 清单里写的版本。空 = 没写
    pub want: String,
    /// 从哪个文件看出来的
    pub from: String,
    /// 这条来自哪门语言的清单。查「装没装」的方式由它决定。
    pub lang: Lang,
    /// 运行时依赖还是只在开发期用的。清单里就写着（dependencies /
    /// devDependencies / dev-dependencies），照着读就行——Mod Menu 的做法
    /// 是「把库从正经内容里筛出去」，同一条道理：混在一起就永远看不清。
    pub kind: DepKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DepKind {
    Runtime,
    Dev,
    /// 契约里额外声明、清单里没有的
    Declared,
}

impl DepKind {
    pub fn id(&self) -> &'static str {
        match self {
            DepKind::Runtime => "runtime",
            DepKind::Dev => "dev",
            DepKind::Declared => "declared",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// 装上了（可能带版本号）
    Yes(Option<String>),
    /// 明确没装
    No,
    /// 这个语言没有可靠的确认手段
    Unknown,
}

fn read(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join(name)).ok()
}

/// 从当前目录往上找某个文件。
///
/// 为什么必须往上找：workspace 的成员（crates/engine 这种）自己目录里
/// 没有 Cargo.lock，锁文件在工作区根上。只看当前目录，就会把已经装好的
/// 依赖报成「未安装」——一个误报就够让整栏不可信了。
fn find_up(root: &Path, name: &str, max_hops: usize) -> Option<std::path::PathBuf> {
    let mut cur = Some(root);
    for _ in 0..=max_hops {
        let dir = cur?;
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
        cur = dir.parent();
    }
    None
}

/// 声明了哪些依赖（只按一门语言）。
pub fn declared(root: &Path, lang: Lang) -> Vec<Decl> {
    let out = match lang {
        Lang::Python => python_decls(root),
        Lang::Web => node_decls(root),
        Lang::Java => java_decls(root),
        Lang::Unknown if root.join("Cargo.toml").is_file() => rust_decls(root),
        Lang::Cpp => cpp_decls(root),
        _ => Vec::new(),
    };
    // 顺序固定：清单里的顺序不等于人想看的顺序，而且测试和界面都要稳定
    let mut sorted = out;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted.dedup_by(|a, b| a.name == b.name);
    sorted
}

/// 复合项目的依赖：按检测到的**所有**语言取并集。
/// 只挑一个「主语言」是不行的——一个 Tauri 项目同时有 Cargo.toml 和
/// package.json，只报一边就等于告诉用户「另一边不存在」。
pub fn declared_all(root: &Path, langs: &[Lang]) -> Vec<Decl> {
    let mut list: Vec<Lang> = langs.to_vec();
    // Rust 在 lang::Lang 里没有独立变体，用清单文件的存在性补上
    if root.join("Cargo.toml").is_file() && !list.contains(&Lang::Unknown) {
        list.push(Lang::Unknown);
    }
    let mut out: Vec<Decl> = Vec::new();
    for l in list {
        for d in declared(root, l) {
            if !out.iter().any(|x| x.name == d.name) {
                out.push(d);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── Rust：Cargo.toml 的 [dependencies] 一族 ─────────────────
fn rust_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "Cargo.toml") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let kind = match section.as_str() {
            "dependencies" => DepKind::Runtime,
            "dev-dependencies" | "build-dependencies" => DepKind::Dev,
            _ => continue,
        };
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let name = k.trim().to_string();
        let v = v.trim();
        let want = if v.starts_with('{') {
            // 行内表：{ version = "1.0", features = [...] }
            v.trim_matches(|c| c == '{' || c == '}')
                .split(',')
                .find_map(|kv| {
                    let (k2, v2) = kv.split_once('=')?;
                    if k2.trim() == "version" {
                        Some(v2.trim().trim_matches('"').to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        } else {
            v.trim_matches('"').to_string()
        };
        if !name.is_empty() {
            out.push(Decl { name, want, from: format!("Cargo.toml · {}", section), lang: Lang::Unknown, kind });
        }
    }
    out
}

// ── Node：package.json 的两组依赖 ──────────────────────────
fn node_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "package.json") else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, kind) in [("dependencies", DepKind::Runtime), ("devDependencies", DepKind::Dev)] {
        let from = key;
        if let Some(obj) = json.get(key).and_then(|v| v.as_object()) {
            for (name, want) in obj {
                out.push(Decl {
                    name: name.clone(),
                    want: want.as_str().unwrap_or("").to_string(),
                    from: format!("package.json · {}", from),
                    lang: Lang::Web,
                    kind,
                });
            }
        }
    }
    out
}

// ── Java：pom.xml 的 <dependency>（手写扫描，不引 XML 库）──
fn java_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "pom.xml") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<dependency>") {
        let tail = &rest[start + "<dependency>".len()..];
        let Some(end) = tail.find("</dependency>") else {
            break;
        };
        let block = &tail[..end];
        let tag = |name: &str| -> String {
            let open = format!("<{}>", name);
            match block.find(&open) {
                Some(i) => {
                    let t = &block[i + open.len()..];
                    t.split('<').next().unwrap_or("").trim().to_string()
                }
                None => String::new(),
            }
        };
        let g = tag("groupId");
        let a = tag("artifactId");
        let v = tag("version");
        if !a.is_empty() {
            let scope = tag("scope");
            out.push(Decl {
                name: if g.is_empty() { a } else { format!("{}:{}", g, a) },
                want: v,
                from: if scope.is_empty() { "pom.xml".to_string() } else { format!("pom.xml · {}", scope) },
                lang: Lang::Java,
                kind: if scope == "test" { DepKind::Dev } else { DepKind::Runtime },
            });
        }
        rest = &tail[end + "</dependency>".len()..];
    }
    out
}

// ── C++：CMakeLists 里的 find_package ───────────────────────
fn cpp_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "CMakeLists.txt") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let lower = line.to_lowercase();
        if !lower.starts_with("find_package(") {
            continue;
        }
        let inner = line
            .trim_start_matches(|c: char| c != '(')
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
        let name = inner.split_whitespace().next().unwrap_or("").to_string();
        if !name.is_empty() {
            out.push(Decl { name, want: String::new(), from: "CMakeLists.txt".into(), lang: Lang::Cpp, kind: DepKind::Runtime });
        }
    }
    out
}

// ── Python：requirements.txt（由 contract 那侧统一解析）────
fn python_decls(root: &Path) -> Vec<Decl> {
    crate::contract::Contract::declared_deps_spec(root)
        .into_iter()
        .map(|(name, want)| Decl { name, want, from: "requirements.txt".into(), lang: Lang::Python, kind: DepKind::Runtime })
        .collect()
}

/// 装没装。`py_installed` 是 pip list 的结果（`名字==版本`），只在 Python 下用。
pub fn installed(root: &Path, lang: Lang, name: &str, py_installed: &[String]) -> Verdict {
    if root.join("Cargo.toml").is_file() && lang != Lang::Python {
        // Rust：Cargo.lock 记着实际锁定的版本。没有 lock 就是没装过依赖。
        let Some(lock_path) = find_up(root, "Cargo.lock", 3) else {
            return Verdict::No;
        };
        let Ok(lock) = std::fs::read_to_string(&lock_path) else {
            return Verdict::Unknown;
        };
        let needle = format!("name = \"{}\"", name);
        if !lock.lines().any(|l| l.trim() == needle) {
            return Verdict::No;
        }
        let mut lines = lock.lines();
        while let Some(l) = lines.next() {
            if l.trim() == needle {
                if let Some(v) = lines.next() {
                    let v = v.trim();
                    if let Some(rest) = v.strip_prefix("version = ") {
                        return Verdict::Yes(Some(rest.trim_matches('"').to_string()));
                    }
                }
            }
        }
        return Verdict::Yes(None);
    }
    match lang {
        Lang::Web => {
            if root.join("node_modules").join(name).is_dir() {
                Verdict::Yes(None)
            } else {
                Verdict::No
            }
        }
        Lang::Java => {
            let Some(home) = std::env::var_os("USERPROFILE") else {
                return Verdict::Unknown;
            };
            let repo = std::path::PathBuf::from(home).join(".m2").join("repository");
            if !repo.is_dir() {
                return Verdict::Unknown;
            }
            let (g, a) = match name.split_once(':') {
                Some((g, a)) => (g, a),
                None => ("", name),
            };
            let dir = if g.is_empty() {
                repo.join(a)
            } else {
                repo.join(g.replace('.', "/")).join(a)
            };
            if dir.is_dir() {
                Verdict::Yes(None)
            } else {
                Verdict::No
            }
        }
        Lang::Cpp => Verdict::Unknown,
        _ => {
            let norm = crate::contract::norm_name(name);
            py_installed
                .iter()
                .find_map(|s| {
                    let (n, v) = s.split_once("==")?;
                    if crate::contract::norm_name(n) == norm {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                })
                .map(|v| Verdict::Yes(Some(v)))
                .unwrap_or(Verdict::No)
        }
    }
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jingwei-deps-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rust_deps_come_from_all_three_sections() {
        let d = tmp("rust");
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"x\"\n\n[dependencies]\nserde = { version = \"1.0\", features = [\"derive\"] }\nanyhow = \"1\"\n\n[dev-dependencies]\npretty_assertions = \"1.4\"\n",
        )
        .unwrap();
        let got = declared(&d, Lang::Unknown);
        let names: Vec<&str> = got.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["anyhow", "pretty_assertions", "serde"]);
        assert_eq!(got[2].want, "1.0");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rust_lock_decides_installed() {
        let d = tmp("lock");
        std::fs::write(d.join("Cargo.toml"), "[dependencies]\nserde = \"1\"\n").unwrap();
        assert_eq!(installed(&d, Lang::Unknown, "serde", &[]), Verdict::No);
        std::fs::write(
            d.join("Cargo.lock"),
            "[[package]]\nname = \"serde\"\nversion = \"1.0.197\"\n",
        )
        .unwrap();
        assert_eq!(
            installed(&d, Lang::Unknown, "serde", &[]),
            Verdict::Yes(Some("1.0.197".into()))
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 成员目录自己不带锁文件，锁在工作区根上——向上找不到就会误报「未安装」。
    #[test]
    fn rust_lock_is_looked_up_in_parent_dirs() {
        let root = tmp("lockup");
        std::fs::write(root.join("Cargo.lock"), "[[package]]\nname = \"serde\"\nversion = \"1.0.197\"\n").unwrap();
        let member = root.join("crates").join("engine");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(member.join("Cargo.toml"), "[dependencies]\nserde = \"1\"\n").unwrap();
        assert_eq!(
            installed(&member, Lang::Unknown, "serde", &[]),
            Verdict::Yes(Some("1.0.197".into()))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 复合项目：一个 Tauri 仓库同时有 Cargo.toml 和 package.json，
    /// 只报一边就等于告诉用户「另一边不存在」。
    #[test]
    fn composite_projects_merge_every_manifest() {
        let d = tmp("composite");
        std::fs::write(d.join("Cargo.toml"), "[dependencies]\nserde = \"1\"\n").unwrap();
        std::fs::write(d.join("package.json"), r#"{"devDependencies":{"vite":"^5"}}"#).unwrap();
        let got = declared_all(&d, &[Lang::Web]);
        let names: Vec<&str> = got.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["serde", "vite"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn node_deps_read_both_groups() {
        let d = tmp("node");
        std::fs::write(
            d.join("package.json"),
            r#"{"dependencies":{"vue":"^3.4.0"},"devDependencies":{"vite":"^5.0.0"}}"#,
        )
        .unwrap();
        let got = declared(&d, Lang::Web);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "vite");
        assert_eq!(got[0].from, "package.json · devDependencies");
        // 没装（没有 node_modules）
        assert_eq!(installed(&d, Lang::Web, "vue", &[]), Verdict::No);
        std::fs::create_dir_all(d.join("node_modules").join("vue")).unwrap();
        assert_eq!(installed(&d, Lang::Web, "vue", &[]), Verdict::Yes(None));
        let _ = std::fs::remove_dir_all(&d);
    }


    /// 传递依赖必须读得出来，而且不许猜：Cargo.lock 里写着什么就是什么。
    #[test]
    fn rust_tree_reads_the_lock_graph() {
        let d = tmp("tree");
        std::fs::write(
            d.join("Cargo.lock"),
            "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\n \"serde\",\n \"walkdir 2.5.0\",\n]\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.229\"\ndependencies = [\n \"serde_derive\",\n]\n\n[[package]]\nname = \"serde_derive\"\nversion = \"1.0.229\"\n",
        )
        .unwrap();
        let (edges, total) = rust_tree(&d);
        assert_eq!(total, 3);
        assert!(edges.contains(&TreeEdge { from: "app".into(), to: "serde".into() }));
        assert!(edges.contains(&TreeEdge { from: "app".into(), to: "walkdir".into() }));
        assert!(edges.contains(&TreeEdge { from: "serde".into(), to: "serde_derive".into() }));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 没有锁文件时不许编：返回空，界面就什么都不显示。
    #[test]
    fn rust_tree_without_lock_is_empty() {
        let d = tmp("tree-none");
        let (edges, total) = rust_tree(&d);
        assert!(edges.is_empty());
        assert_eq!(total, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 快速路径拿不到 pip 清单时，Python 依赖必须标「无法确认」。
    /// 标成「未安装」就是误报——误报比空着更糟，它会让人不信这一栏。
    #[test]
    fn python_without_pip_list_is_unknown_not_missing() {
        let d = tmp("py-unknown");
        std::fs::write(d.join("requirements.txt"), "rich>=13.0.0\n").unwrap();
        let (detail, status) = collect(&d, &[Lang::Python], None, &[]);
        assert_eq!(detail.len(), 1);
        assert!(detail[0].unknown, "没有 pip 清单时必须标成无法确认");
        assert!(!detail[0].ok);
        assert_eq!(status.get("rich"), Some(&false));
        let (d2, _) = collect(&d, &[Lang::Python], None, &["rich==13.7.1".to_string()]);
        assert!(d2[0].ok);
        assert_eq!(d2[0].have, "13.7.1");
        assert!(!d2[0].unknown);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn java_deps_merge_group_and_artifact() {
        let d = tmp("java");
        std::fs::write(
            d.join("pom.xml"),
            "<project><dependencies><dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.9</version></dependency></dependencies></project>",
        )
        .unwrap();
        let got = declared(&d, Lang::Java);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "org.slf4j:slf4j-api");
        assert_eq!(got[0].want, "2.0.9");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// C++ 的包可能来自系统、vcpkg、Conan 或源码树——查不到就必须说「无法确认」，
    /// 报成「未安装」就是误报，而误报会让人不再相信这个工具。
    #[test]
    fn cpp_is_never_reported_as_missing() {
        let d = tmp("cpp");
        std::fs::write(d.join("CMakeLists.txt"), "find_package(OpenCV REQUIRED)\n").unwrap();
        let got = declared(&d, Lang::Cpp);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "OpenCV");
        assert_eq!(installed(&d, Lang::Cpp, "OpenCV", &[]), Verdict::Unknown);
        let _ = std::fs::remove_dir_all(&d);
    }
}
