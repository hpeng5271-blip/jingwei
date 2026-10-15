//! 项目生成：从模板长出一个能立刻跑起来的 Python 项目骨架。
//!
//! 生成的入口一律是「自定位」的（把项目根塞进 sys.path），
//! 因为便携版/嵌入式 Python 的隔离模式不会把当前目录放进 sys.path——
//! 这类解释器上，不自定位的入口一 import 自己的包就挂。

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Script,
    Package,
    Cli,
}

/// 新建项目时选哪门语言。**不同语言给的东西完全不同**——
/// 不是把 Python 那套换个后缀名就完事：清单文件、构建命令、忽略规则、
/// 目录约定、契约里的三条命令，每一门都得对。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Python,
    Rust,
    Node,
    Java,
    Cpp,
}

impl Lang {
    pub fn parse(s: &str) -> Option<Lang> {
        match s.to_lowercase().as_str() {
            "" | "python" | "py" => Some(Lang::Python),
            "rust" | "rs" => Some(Lang::Rust),
            "node" | "js" | "ts" | "前端" => Some(Lang::Node),
            "java" => Some(Lang::Java),
            "cpp" | "c++" | "c" => Some(Lang::Cpp),
            _ => None,
        }
    }
    pub fn id(&self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::Rust => "rust",
            Lang::Node => "node",
            Lang::Java => "java",
            Lang::Cpp => "cpp",
        }
    }
}

impl Template {
    pub fn parse(s: &str) -> Option<Template> {
        match s.to_lowercase().as_str() {
            "script" | "脚本" => Some(Template::Script),
            "package" | "pkg" | "包" => Some(Template::Package),
            "cli" | "命令行" => Some(Template::Cli),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Template::Script => "script",
            Template::Package => "package",
            Template::Cli => "cli",
        }
    }
}

pub struct NewOptions {
    pub dir: PathBuf,
    pub name: String,
    pub template: Template,
    /// 哪门语言。默认 Python（老调用不写这个字段时保持原样）。
    pub lang: Lang,
    pub python_exe: Option<PathBuf>,
    /// 为什么做它。写下来就是第一条记录，也是日后唯一的「原始意图」证据。
    pub purpose: Option<String>,
}

#[derive(Debug, Default)]
pub struct NewReport {
    pub created: Vec<String>,
    pub note: Vec<String>,
}

/// 用户敲的名字 → 能当包名/目录名的名字。
/// 中文名很常见，但它没法当包名，所以兜底改成 ascii 安全名。
pub fn safe_name(raw: &str) -> String {
    let mut s: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() || s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        s = format!("app_{}", s);
    }
    s
}

pub fn template_help() -> &'static str {
    "script   单个 .py 文件，最快能跑\n             package  一个包 + 测试（默认）\n             cli      包 + argparse 子命令 + 测试"
}

/// 项目名得能当包名用，所以只能是标识符的样子
pub fn check_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("项目名不能为空");
    }
    let ok_first = name
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_')
        .unwrap_or(false);
    if !ok_first || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("项目名只能是字母/数字/下划线，且不能以数字开头（它要当包名用）");
    }
    Ok(())
}

pub fn create(opts: &NewOptions) -> Result<NewReport> {
    if opts.lang != Lang::Python {
        return create_native(opts);
    }
    let root = &opts.dir;
    let name = opts.name.trim().to_string();
    check_name(&name)?;
    if root.exists() && std::fs::read_dir(root).map(|d| d.count() > 0).unwrap_or(false) {
        bail!("{} 里已经有东西了，换个名字或先清空它", root.display());
    }
    std::fs::create_dir_all(root).with_context(|| format!("建目录失败 {}", root.display()))?;

    let mut rep = NewReport::default();
    let pkg = opts.template != Template::Script;

    let files: Vec<(String, String)> = if pkg {
        vec![
            (
                format!("{}/__init__.py", name),
                format!("\"\"\"{} 包。\"\"\"\n", name),
            ),
            (format!("{}/core.py", name), CORE.replace("__NAME__", &name)),
            (
                "main.py".to_string(),
                if opts.template == Template::Cli {
                    CLI_ENTRY.replace("__NAME__", &name)
                } else {
                    ENTRY.replace("__NAME__", &name)
                },
            ),
            ("tests/__init__.py".to_string(), String::new()),
            (
                "tests/test_core.py".to_string(),
                TEST.replace("__NAME__", &name),
            ),
        ]
    } else {
        vec![(
            "main.py".to_string(),
            SCRIPT.replace("__NAME__", &name),
        )]
    };

    for (rel, body) in files {
        let p = root.join(&rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, body).with_context(|| format!("写 {} 失败", p.display()))?;
        rep.created.push(rel);
    }
    for (rel, body) in [
        ("requirements.txt", REQUIREMENTS),
        ("README.md", README_MD),
        (".gitignore", GITIGNORE),
    ] {
        let p = root.join(rel);
        std::fs::write(&p, body.replace("__NAME__", &name))?;
        rep.created.push(rel.to_string());
    }

    let purpose = opts.purpose.as_ref().map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    let ct = crate::understand::contract::Contract {
        schema_version: Some(1),
        name: Some(name.clone()),
        description: Some(
            purpose
                .clone()
                .unwrap_or_else(|| "一句话说清这个项目是干什么的".into()),
        ),
        runtime: crate::understand::contract::Runtime {
            python: Some(">=3.10".into()),
            python_exe: opts.python_exe.as_ref().map(|p| p.to_string_lossy().to_string()),
        },
        deps: crate::understand::contract::Deps::default(),
        commands: crate::understand::contract::Commands {
            install: Some("python -m pip install -r requirements.txt".into()),
            build: Some("python -m compileall -q .".into()),
            run: Some("python main.py".into()),
            test: if pkg {
                Some("python -m unittest discover -s tests -t . -v".into())
            } else {
                None
            },
        },
        log_files: Vec::new(),
        ready_when: Some(crate::understand::contract::ReadyWhen {
            port: None,
            log_match: None,
            exit_code: Some(0),
            timeout_s: Some(60),
        }),
        manual: Vec::new(),
    };
    ct.save(root)?;
    rep.created.push("env.contract.json".into());
    if let Some(px) = &opts.python_exe {
        rep.note.push(format!("契约已绑定解释器 {}", px.display()));
    }
    if let Some(p) = &purpose {
        // README 第一句就换成真正的原因，占位说明没有存在价值
        let readme = root.join("README.md");
        if let Ok(text) = std::fs::read_to_string(&readme) {
            std::fs::write(
                &readme,
                text.replace("一句话说清它解决什么问题。", p),
            )?;
        }
        crate::state::notes::save(
            root,
            &crate::state::notes::NoteInput {
                slug: None,
                kind: "goal".into(),
                title: "为什么做它".into(),
                body: p.clone(),
                tags: Vec::new(),
            },
        )?;
        rep.note
            .push("已把「为什么做它」记成第一条记录（jingwei notes 看）".into());
    }
    Ok(rep)
}

/// 非 Python 的模板：清单、入口、忽略规则、契约四条命令，全都按那门语言来。
fn create_native(opts: &NewOptions) -> Result<NewReport> {
    let root = &opts.dir;
    let name = opts.name.trim().to_string();
    check_name(&name)?;
    if root.exists() && std::fs::read_dir(root).map(|d| d.count() > 0).unwrap_or(false) {
        bail!("{} 里已经有东西了，换个名字或先清空它", root.display());
    }
    std::fs::create_dir_all(root).with_context(|| format!("建目录失败 {}", root.display()))?;
    let n = name.clone();
    let (files, commands): (Vec<(String, String)>, crate::understand::contract::Commands) = match opts.lang {
        Lang::Rust => (
            vec![
                (
                    "Cargo.toml".into(),
                    format!(
                        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
                        n
                    ),
                ),
                (
                    "src/main.rs".into(),
                    format!(
                        "//! {} —— 用 jingwei 起的 Rust 小项目。\nfn main() {{\n    println!(\"hello from {}\");\n}}\n",
                        n, n
                    ),
                ),
                (".gitignore".into(), "/target\n".into()),
            ],
            crate::understand::contract::Commands {
                install: Some("cargo fetch".into()),
                build: Some("cargo build".into()),
                run: Some("cargo run".into()),
                test: Some("cargo test".into()),
            },
        ),
        Lang::Node => (
            vec![
                (
                    "package.json".into(),
                    format!(
                        "{{\n  \"name\": \"{}\",\n  \"version\": \"0.1.0\",\n  \"type\": \"module\",\n  \"scripts\": {{\n    \"start\": \"node index.js\",\n    \"test\": \"node --test\"\n  }}\n}}\n",
                        n
                    ),
                ),
                (
                    "index.js".into(),
                    format!("// {} —— 用 jingwei 起的 Node 小项目。\nconsole.log('hello from {}');\n", n, n),
                ),
                (".gitignore".into(), "node_modules/\n".into()),
            ],
            crate::understand::contract::Commands {
                install: Some("npm install".into()),
                build: None,
                run: Some("npm start".into()),
                test: Some("npm test".into()),
            },
        ),
        Lang::Java => (
            vec![
                (
                    "pom.xml".into(),
                    format!(
                        "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>dev.jingwei</groupId>\n  <artifactId>{}</artifactId>\n  <version>0.1.0</version>\n  <properties>\n    <maven.compiler.source>17</maven.compiler.source>\n    <maven.compiler.target>17</maven.compiler.target>\n    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>\n  </properties>\n</project>\n",
                        n
                    ),
                ),
                (
                    "src/main/java/Main.java".into(),
                    format!("// {} —— 用 jingwei 起的 Java 小项目。\npublic class Main {{\n    public static void main(String[] args) {{\n        System.out.println(\"hello from {}\");\n    }}\n}}\n", n, n),
                ),
                (".gitignore".into(), "target/\n".into()),
            ],
            crate::understand::contract::Commands {
                install: Some("mvn -q dependency:go-offline".into()),
                build: Some("mvn -q -DskipTests package".into()),
                run: Some("mvn -q compile exec:java -Dexec.mainClass=Main".into()),
                test: Some("mvn test".into()),
            },
        ),
        Lang::Cpp => (
            vec![
                (
                    "CMakeLists.txt".into(),
                    format!(
                        "cmake_minimum_required(VERSION 3.16)\nproject({} CXX)\nset(CMAKE_CXX_STANDARD 17)\nadd_executable({} main.cpp)\n",
                        n, n
                    ),
                ),
                (
                    "main.cpp".into(),
                    format!("// {} —— 用 jingwei 起的 C++ 小项目。\n#include <iostream>\n\nint main() {{\n    std::cout << \"hello from {}\" << std::endl;\n    return 0;\n}}\n", n, n),
                ),
                (".gitignore".into(), "build/\n".into()),
            ],
            crate::understand::contract::Commands {
                install: Some("cmake -S . -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON".into()),
                build: Some("cmake --build build".into()),
                // 跑的是刚构建出来的那个可执行文件。留空的话 C++ 项目在
                // 「一条龙」里永远只能走到构建——建出来了却点不动「运行」，很别扭。
                run: Some(format!("build/{}.exe", n)),
                test: Some("ctest --test-dir build".into()),
            },
        ),
        Lang::Python => unreachable!(),
    };

    let mut rep = NewReport::default();
    for (rel, body) in files {
        let p = root.join(&rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, body)?;
        rep.created.push(rel);
    }
    std::fs::write(
        root.join("README.md"),
        format!(
            "# {}\n\n{}\n",
            name,
            opts.purpose.clone().unwrap_or_else(|| "一句话说清这个项目是干什么的".into())
        ),
    )?;
    rep.created.push("README.md".into());

    let ct = crate::understand::contract::Contract {
        schema_version: Some(1),
        name: Some(name.clone()),
        description: opts.purpose.clone(),
        commands,
        ready_when: Some(crate::understand::contract::ReadyWhen {
            exit_code: Some(0),
            timeout_s: Some(120),
            ..Default::default()
        }),
        ..Default::default()
    };
    ct.save(root)?;
    rep.created.push("env.contract.json".into());
    rep.note.push(format!(
        "按 {} 模板建的；契约里四条命令都写好了，点「运行」页的流水线直接跑",
        opts.lang.id()
    ));
    Ok(rep)
}

const CORE: &str = r#""""__NAME__ 的核心逻辑。

这里只放「算和判断」，不放打印和输入——
这样它才容易被测试，也才容易被别的文件复用。
"""


def greet(who: str) -> str:
    """最小示例：把输入变成输出，不碰任何外部资源。"""
    return f"你好，{who}！"
"#;

const ENTRY: &str = r#""""__NAME__ 的入口：只负责把命令行和 core 接起来。"""

import pathlib
import sys

# 自定位入口。便携版/嵌入式版的 Python 跑在隔离模式下，
# 当前目录不会进 sys.path——少了这几行，import 自己的包就会失败。
ROOT = pathlib.Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from __NAME__ import core  # noqa: E402


def main() -> int:
    print(core.greet("world"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
"#;

const CLI_ENTRY: &str = r#""""__NAME__：带子命令的命令行工具。"""

import argparse
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from __NAME__ import core  # noqa: E402


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="__NAME__", description="一句话说清它是干什么的")
    sub = p.add_subparsers(dest="cmd")
    hello = sub.add_parser("hello", help="打个招呼")
    hello.add_argument("who", nargs="?", default="world")
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    # 不带子命令直接跑也要有合理行为——这是新手拿到项目后第一件会做的事
    if args.cmd in (None, "hello"):
        print(core.greet(getattr(args, "who", None) or "world"))
        return 0
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
"#;

const SCRIPT: &str = r#""""__NAME__：一个能马上跑起来的脚本。

想让它长大：拆成「包 + 入口 + 测试」，模板见 jingwei new --help。
"""


def main() -> int:
    print("你好，__NAME__！")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
"#;

const TEST: &str = r#""""__NAME__ 的测试。测试放 tests/，被测代码放包里。"""

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from __NAME__ import core  # noqa: E402


class TestCore(unittest.TestCase):
    def test_greet_keeps_the_name(self):
        self.assertIn("world", core.greet("world"))


if __name__ == "__main__":
    unittest.main()
"#;

const REQUIREMENTS: &str = r#"# 第三方依赖写在这里，一行一个，例如：
# requests>=2.31
#
# 装它们：  jingwei env install
# 临时停用某一行：前面加 # 就行。
"#;

const GITIGNORE: &str = r#"__pycache__/
*.py[cod]
.venv/
venv/
env/
# 快照是机器的事实，体积大、随时可重建，不进版本库；
# 但 notes.jsonl 是你自己的思路，留着——它是这个项目最值钱的东西。
.jingwei/*
!.jingwei/notes.jsonl
build/
dist/
*.egg-info/
.pytest_cache/
.mypy_cache/
.ruff_cache/
"#;

const README_MD: &str = r#"# __NAME__

一句话说清它解决什么问题。

## 跑起来

    jingwei run

（不想用工具也行：`python main.py`）

## 体检

    jingwei doctor

它回答的是「环境为什么和上次不一样」：解释器、依赖、PATH、契约有没有变。

## 测试

    jingwei test

## 目录

    main.py             入口（自定位，便携版 Python 上也能跑）
    env.contract.json   环境契约：用哪个解释器、装什么、怎么跑、什么算成功
    requirements.txt    第三方依赖
"#;

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::path::Path;
    use std::time::Duration;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jingwei-scaffold-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_that_cannot_be_package_names_get_fixed() {
        assert_eq!(safe_name("my_tool"), "my_tool");
        assert_eq!(safe_name("记账工具"), "____");
        assert_eq!(safe_name("2fast"), "app_2fast");
        assert!(check_name("my_tool").is_ok());
        assert!(check_name("2fast").is_err());
        assert!(check_name("").is_err());
    }

    /// 生成的 Python 必须真的能编译。
    /// 这条测试是写它的理由本身：模板里一个没闭合的三引号，
    /// 命令行看起来一切正常，直到新手第一次 run 才炸。
    #[test]
    fn every_template_compiles() {
        let Some((py, _)) = crate::detect::probe::first_working(Path::new("."), None, 3) else {
            return; // 这台机器没有 Python，跳过（本机测试环境的正常情况）
        };
        for t in [Template::Script, Template::Package, Template::Cli] {
            let d = tmp(t.id());
            create(&NewOptions {
                dir: d.clone(),
                name: "probe_pkg".into(),
                template: t,
                lang: Lang::Python,
                python_exe: None,
                purpose: Some("测试用".into()),
            })
            .unwrap();
            let (ok, out) = crate::util::winutil::run_cmd(
                &py,
                &["-m", "compileall", "-q", &d.to_string_lossy()],
                Duration::from_secs(90),
            )
            .unwrap_or((false, "跑不起来".into()));
            assert!(ok, "模板 {} 生成的代码编译不过：\n{}", t.id(), out);
            // 顺手验证记录真的落盘了
            assert_eq!(crate::state::notes::count(&d), 1, "{} 模板没记下目的", t.id());
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    /// 每种语言都得长出它自己的骨架。
    ///
    /// 这条测试盯着一个具体的坑：把 Python 那套（requirements.txt + main.py）
    /// 换个后缀当成别的语言——清单文件错一个名字，那条工具链整条都不认，
    /// 而界面上看起来「项目已经建好了」，新手会卡死在第一步。
    #[test]
    fn every_language_gets_its_own_skeleton() {
        for (lang, manifest, entry) in [
            (Lang::Rust, "Cargo.toml", "src/main.rs"),
            (Lang::Node, "package.json", "index.js"),
            (Lang::Java, "pom.xml", "src/main/java/Main.java"),
            (Lang::Cpp, "CMakeLists.txt", "main.cpp"),
        ] {
            let d = tmp(&format!("native_{}", lang.id()));
            let rep = create(&NewOptions {
                dir: d.clone(),
                name: "probe_app".into(),
                template: Template::Package,
                lang,
                python_exe: None,
                purpose: None,
            })
            .unwrap();
            assert!(d.join(manifest).is_file(), "{} 没生成 {}", lang.id(), manifest);
            assert!(d.join(entry).is_file(), "{} 没生成 {}", lang.id(), entry);
            assert!(!d.join("requirements.txt").exists(), "{} 不该有 Python 的清单", lang.id());
            // 契约里至少要有「装 / 跑」两步，否则一条龙走到一半就断
            let (ct, _) = crate::understand::contract::Contract::load(&d);
            let ct = ct.unwrap_or_else(|| panic!("{} 契约没落盘", lang.id()));
            assert!(ct.commands.install.is_some(), "{} 契约缺 install", lang.id());
            assert!(ct.commands.run.is_some(), "{} 契约缺 run", lang.id());
            assert!(ct.commands.test.is_some(), "{} 契约缺 test", lang.id());
            assert!(rep.created.iter().any(|f| f == "env.contract.json"));
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}
