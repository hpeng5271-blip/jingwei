//! **跑失败了，到底缺哪个件？**（2026-10-18）
//!
//! ## 为什么要有这一层
//!
//! 昨晚的 `取舍记录.md` 已经把环境这件事定死：**不装、不提权、不接管系统**（T-01 / T-11）。
//! 那么产品能给的最高价值就剩一句话：**"你缺的不是 Python，是 MSVC 的链接器。"**
//!
//! 这一层只做这一件事：**把构建/运行的第一条错误行，翻译成"缺什么 + 怎么验"**。
//! 它不碰进程、不碰阶段、不读盘——纯函数，表驱动。
//!
//! ## 三条不可退让的规矩
//!
//! 1. **认不出来就返回 `None`**，由调用方退回"一条没有落点的症结"。
//!    理由与 `rundiag.rs` 同源：**一句指错方向的"缺 X"比不说更坏**。
//! 2. **每一行都带"怎么自己验"**——`verify` 是可以直接粘进终端的命令。
//!    没有自查的结论，本项目不许报（四底线之一）。
//! 3. **说的是"缺哪个件"，不是"去装什么"**。安装方式写清是"哪种形态"（解压 / 官方安装器 / 包管理器），
//!    但**不代跑**、**不给一键命令**（不生成修复命令，四底线之二）。
//!
//! ## 覆盖从哪来
//!
//! 这些都是**真报错原文**（Python 扩展 / node-gyp / Rust 链接 / JDK / C 头文件），
//! 不是想象的措辞。新增一条时请贴一条真实输出，别按印象写。

use std::sync::OnceLock;

/// 缺的那个"件"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingTool {
    /// 缺的是什么（一句话，人话）。
    pub what: &'static str,
    /// 为什么「只装语言本体不够」——这一句是本层存在的理由。
    pub why: &'static str,
    /// 我们建议的**装配形态**（不是命令）：解压 / 官方安装器 / 包管理器。
    pub shape: &'static str,
    /// **自己能验的**：粘进终端就能跑，输出能判断到底装没装。
    pub verify: &'static str,
    /// **固定的官方链接**（2026-10-18 加）：只放官方分发页，不放教程/博客/网盘。
    ///
    /// 为什么必须有：`shape` 只说"官方安装器"，用户还得自己搜——
    /// 而搜出来的第一个结果经常是第三方打包站，**那比不装更危险**。
    pub url: &'static str,
    /// **预期输出**：照 `verify` 跑完之后，看到什么才算装上了。
    ///
    /// 没有这一句，`verify` 只是"跑了个命令"——用户看着满屏输出不知道好不好。
    pub expect: &'static str,
}

struct Rule {
    needle: &'static str,
    tool: MissingTool,
}

/// 规则表。**顺序有意义**：更具体的在前（`Microsoft Visual C++ 14.0` 先于泛化的 `link.exe`）。
fn rules() -> &'static [Rule] {
    static T: OnceLock<Vec<Rule>> = OnceLock::new();
    T.get_or_init(|| {
        vec![
            Rule {
                needle: "Microsoft Visual C++ 14.0 or greater is required",
                tool: MissingTool {
                    what: "MSVC 生成工具（C++ 编译器 + Windows SDK）",
                    why: "Python 的 C 扩展和 node-gyp 都要现编 C 代码，光有解释器编不了",
                    shape: "官方安装器（只能安装，不能解压就位）",
                    url: "https://visualstudio.microsoft.com/visual-cpp-build-tools/",
                    expect: "vswhere 的输出里能看到 `installationPath`；同一个目录下有 `VC\\Tools\\MSVC\\*\\bin\\Hostx64\\x64\\cl.exe`",
                    verify: "& \"${env:ProgramFiles(x86)}\\Microsoft Visual Studio\\Installer\\vswhere.exe\" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath",
                },
            },
            Rule {
                needle: "Microsoft Visual C++ 14.0 required",
                tool: MissingTool {
                    what: "MSVC 生成工具（C++ 编译器 + Windows SDK）",
                    why: "带 C 扩展的 wheel 要现编；没有编译器就只剩报错",
                    shape: "官方安装器（只能安装，不能解压就位）",
                    url: "https://visualstudio.microsoft.com/visual-cpp-build-tools/",
                    expect: "vswhere 的输出里能看到 `installationPath`；`cl.exe` 在它下面的 VC\\Tools\\MSVC\\*\\bin里",
                    verify: "& \"${env:ProgramFiles(x86)}\\Microsoft Visual Studio\\Installer\\vswhere.exe\" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath",
                },
            },
            Rule {
                needle: "gyp ERR! find VS",
                tool: MissingTool {
                    what: "Visual Studio Build Tools（node-gyp 要它）",
                    why: "Node 的原生模块不是 JS，装的时候要编译 C/C++",
                    shape: "官方安装器",
                    url: "https://visualstudio.microsoft.com/visual-cpp-build-tools/",
                    expect: "vswhere 输出了 `installationPath`，且里面含 `BuildTools`",
                    verify: "& \"${env:ProgramFiles(x86)}\\Microsoft Visual Studio\\Installer\\vswhere.exe\" -latest -property installationPath",
                },
            },
            Rule {
                needle: "link.exe not found",
                tool: MissingTool {
                    what: "MSVC 链接器（link.exe）",
                    why: "rustup 只装 rustc/cargo，不装链接器；没有它 cargo 编不出可执行文件",
                    shape: "官方安装器（Build Tools）；或改用 gnu 工具链 + MinGW",
                    url: "https://rust-lang.github.io/rustup/installation/windows-msvc.html",
                    expect: "`cargo -Vv` 的 host 是 `x86_64-pc-windows-msvc`，且 `rustc --print target-libdir` 指向真实存在的目录",
                    verify: "rustc --print target-libdir; cargo -Vv",
                },
            },
            Rule {
                needle: "LNK1104",
                tool: MissingTool {
                    what: "MSVC 链接器 / 库路径（LIB）",
                    why: "编译器在，但链接阶段找不到库——通常是 Build Tools 没装全，或环境变量没注入",
                    shape: "官方安装器",
                    url: "https://learn.microsoft.com/cpp/build/reference/lib-reference",
                    expect: "同一份工程换到「开发者命令提示符」里能编过——说明缺的是环境变量，不是文件",
                    verify: "& \"${env:ProgramFiles(x86)}\\Microsoft Visual Studio\\Installer\\vswhere.exe\" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath",
                },
            },
            Rule {
                needle: "fatal error: stdio.h: No such file or directory",
                tool: MissingTool {
                    what: "C 编译器 + Windows SDK（头文件）",
                    why: "编译器与 CRT/SDK 是两套东西；只有编译器而没有头文件，照样编不了",
                    shape: "解压（MinGW-w64 / w64devkit）或 官方安装器（MSVC）",
                    url: "https://github.com/skeeto/w64devkit/releases",
                    expect: "`where.exe gcc` 有一行；`gcc --version` 打印 `gcc (GCC)`",
                    verify: "where.exe gcc; where.exe cl",
                },
            },
            Rule {
                needle: "JAVA_HOME is not set",
                tool: MissingTool {
                    what: "JDK（而不是 JRE），以及 JAVA_HOME",
                    why: "Maven / Gradle 找的是 JAVA_HOME；只装运行时装不出 `javac`",
                    shape: "解压（Adoptium Temurin zip）或 官方安装器",
                    url: "https://adoptium.net/temurin/releases/",
                    expect: "`java -version` 打印 `openjdk version 21`（或你装的版本）；`$env:JAVA_HOME` 非空；`where.exe javac` 有一行",
                    verify: "java -version; echo $env:JAVA_HOME; where.exe javac",
                },
            },
            Rule {
                needle: "Unable to locate a Java Runtime",
                tool: MissingTool {
                    what: "Java 运行时 / JDK",
                    why: "PATH 里没有任何 java；项目声明的版本再对也没用",
                    shape: "解压或 官方安装器",
                    url: "https://adoptium.net/temurin/releases/",
                    expect: "`java -version` 打印 `openjdk version` 或 `java version`——只有这一行才算装了运行时",
                    verify: "where.exe java; java -version",
                },
            },
            Rule {
                needle: "No module named pip",
                tool: MissingTool {
                    what: "pip（embeddable 版 Python 不带它）",
                    why: "embeddable 包是给「嵌进别人程序」用的，本来就没有包管理器",
                    shape: "换官方安装包，或自己 get-pip",
                    url: "https://pip.pypa.io/en/stable/installation/",
                    expect: "`python -m pip --version` 打印 `pip 2x.y` 并给出 python 路径",
                    verify: "python -m pip --version",
                },
            },
            Rule {
                needle: "gyp ERR! find Python",
                tool: MissingTool {
                    what: "Python（node-gyp 要它做构建脚本）",
                    why: "Node 的原生模块构建脚本是 Python 写的",
                    shape: "官方安装包（不要 Microsoft Store 版）",
                    url: "https://www.python.org/downloads/windows/",
                    expect: "`python -V` 打印 `Python 3.`；`where.exe python` 指到你**自己装的那个**（不是 WindowsApps 别名）",
                    verify: "python -V; where.exe python",
                },
            },
            Rule {
                needle: "error: linker `cc` not found",
                tool: MissingTool {
                    what: "C 链接器（cc / gcc）",
                    why: "这条通常出现在非 Windows 目标或交叉编译上",
                    shape: "系统包管理器",
                    url: "https://github.com/skeeto/w64devkit/releases",
                    expect: "`where.exe gcc`（或 `which cc`）有一行，且 `gcc --version` 能打印版本",
                    verify: "which cc; which gcc",
                },
            },
        ]
    })
}

/// 归一化：小写 + 去掉反引号与引号。
///
/// **为什么需要它**（2026-10-18 被我自己的测试抓出来的）：真报错是
/// ``error: linker `link.exe` not found``——中间有反引号，而表里写的是 `link.exe not found`，
/// 于是**规则写了却永远命中不了**。这正是这个仓库最熟的那种失效：**看起来在守，其实没守**。
/// 与其逐条去猜引号形态，不如两边都归一化一次。
fn norm(s: &str) -> String {
    s.to_lowercase().replace(['`', '"', '\''], "")
}

/// 给 `Outcome` 用的可序列化形态（`&'static str` 不好跨层带）。
///
/// **字段与 `MissingTool` 一一对应，不加不减**：多一个字段就意味着有第二个地方在猜。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MissingToolOut {
    pub what: String,
    pub why: String,
    pub shape: String,
    pub verify: String,
    /// 固定的官方链接（见 `MissingTool::url`）。
    pub url: String,
    /// 预期输出（见 `MissingTool::expect`）。
    pub expect: String,
}

impl From<&MissingTool> for MissingToolOut {
    fn from(t: &MissingTool) -> Self {
        Self {
            what: t.what.to_string(),
            why: t.why.to_string(),
            shape: t.shape.to_string(),
            verify: t.verify.to_string(),
            url: t.url.to_string(),
            expect: t.expect.to_string(),
        }
    }
}

/// 与 `first_missing` 同义，但返回可序列化的那份。
pub fn first_missing_owned<'a, I: IntoIterator<Item = &'a str>>(lines: I) -> Option<MissingToolOut> {
    first_missing(lines).map(MissingToolOut::from)
}

/// 从一行输出里认出「缺哪个件」。**认不出来就 `None`**（宁可不说）。
pub fn missing_toolchain(line: &str) -> Option<&'static MissingTool> {
    let l = norm(line);
    rules()
        .iter()
        .find(|r| l.contains(&norm(r.needle)))
        .map(|r| &r.tool)
}

/// 从一串输出里找**第一条**能认出来的（构建工具通常第一句就说清了缺什么）。
pub fn first_missing<'a, I: IntoIterator<Item = &'a str>>(lines: I) -> Option<&'static MissingTool> {
    lines.into_iter().find_map(missing_toolchain)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **每条规则都得有真报错原文撑着**。这里逐条钉住，防止"改了措辞就静默失效"。
    #[test]
    fn 十种真报错都能认出来() {
        let cases = [
            (
                "error: Microsoft Visual C++ 14.0 or greater is required. Get it with \"Microsoft C++ Build Tools\"",
                "MSVC 生成工具（C++ 编译器 + Windows SDK）",
            ),
            (
                "gyp ERR! find VS msvs_version not set from command line or npm config",
                "Visual Studio Build Tools（node-gyp 要它）",
            ),
            (
                "error: linker `link.exe` not found",
                "MSVC 链接器（link.exe）",
            ),
            (
                "fatal error LNK1104: cannot open file 'kernel32.lib'",
                "MSVC 链接器 / 库路径（LIB）",
            ),
            (
                "main.c:1:10: fatal error: stdio.h: No such file or directory",
                "C 编译器 + Windows SDK（头文件）",
            ),
            (
                "The JAVA_HOME environment variable is not defined correctly, JAVA_HOME is not set",
                "JDK（而不是 JRE），以及 JAVA_HOME",
            ),
            (
                "Unable to locate a Java Runtime.",
                "Java 运行时 / JDK",
            ),
            (
                "/usr/bin/python: No module named pip",
                "pip（embeddable 版 Python 不带它）",
            ),
            (
                "gyp ERR! find Python Python is not set from command line or npm config",
                "Python（node-gyp 要它做构建脚本）",
            ),
            (
                "error: linker `cc` not found",
                "C 链接器（cc / gcc）",
            ),
        ];
        for (line, want) in cases {
            let got = missing_toolchain(line).unwrap_or_else(|| panic!("认不出来：{line}"));
            assert_eq!(got.what, want, "认错了：{line}");
        }
    }

    /// **认不出来必须返回 None**——这是"不猜"的落点。
    ///
    /// 这几行都是**真失败但和缺件无关**的输出；把它们硬翻成"缺 X"就是答非所问。
    #[test]
    fn 认不出来时不猜() {
        for line in [
            "error[E0425]: cannot find value `x` in this scope",
            "AssertionError: 1 != 2",
            "npm ERR! 404 Not Found - GET https://registry.npmjs.org/nonexistent",
            "error: could not compile `foo` due to 3 previous errors",
            "",
        ] {
            assert!(
                missing_toolchain(line).is_none(),
                "这行不该被翻译成缺件：{line}"
            );
        }
    }

    /// 每一条都必须带**能粘进终端的自查**——没有自查的结论本项目不许报。
    #[test]
    fn 每条都带自查命令() {
        for r in rules() {
            assert!(!r.tool.verify.trim().is_empty(), "{} 没有自查命令", r.tool.what);
            assert!(
                !r.tool.why.trim().is_empty(),
                "{} 没说清「为什么光有本体不够」",
                r.tool.what
            );
        }
    }

    /// **顺序有意义**：更具体的规则必须先命中（否则泛化规则会抢走它）。
    #[test]
    fn 具体规则先于泛化规则() {
        let t = missing_toolchain("error: Microsoft Visual C++ 14.0 or greater is required").unwrap();
        assert!(
            t.what.contains("MSVC 生成工具"),
            "应命中「Microsoft Visual C++ 14.0」那条，而不是别的：{}",
            t.what
        );
    }

    /// 从一串输出里取**第一条**能认出来的。
    #[test]
    fn 从多行里取第一条() {
        let lines = [
            "> my-app@1.0.0 install",
            "gyp ERR! find VS msvs_version not set",
            "link.exe not found",
        ];
        let t = first_missing(lines).expect("该认出 node-gyp 那条");
        assert!(t.what.contains("Visual Studio Build Tools"), "{}", t.what);
    }

    /// **每条规则都要有官方链接与预期输出**（2026-10-18）。
    ///
    /// 为什么钉这条：`shape` 只说"官方安装器"，用户还得自己搜；
    /// 而搜出来的第一个经常是第三方打包站——**那比不装更危险**。
    /// 而没有 `expect`，"跑了个命令"就不等于"知道装没装上"。
    #[test]
    fn 每条缺件规则都给了官方链接与预期输出() {
        for r in rules() {
            assert!(
                r.tool.url.starts_with("https://"),
                "`{}` 的 url 必须是 https 官方页：{}",
                r.tool.what,
                r.tool.url
            );
            assert!(!r.tool.expect.trim().is_empty(), "`{}` 没写预期输出", r.tool.what);
            assert!(!r.tool.verify.trim().is_empty(), "`{}` 没写自查命令", r.tool.what);
        }
    }
}
