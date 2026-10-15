//! C / C++ 那一摊：编译器、构建工具、以及它们之间最容易撞车的地方。
//!
//! 这里的坑和 Java 不一样：Java 是「装了几个」的问题，
//! C/C++ 是「装的是哪一家」的问题——GNU、LLVM、MSVC 三家的命令行参数、
//! 目标文件格式、运行时库全都不通用，混着用会报一堆看不懂的链接错误。

use crate::winutil::{run_cmd, which};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// 便携版工具链的常见落脚点：`<根>\<产品>\bin`。
/// 这些目录通常故意不进 PATH（免得几个版本互相打架），所以只能自己找。
fn dev_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for raw in [
        r"%SystemDrive%\SDK",
        r"%SystemDrive%\tools",
        r"D:\SDK",
        r"D:\tools",
        r"C:\SDK",
        r"C:\tools",
        r"D:\msys64",
        r"C:\msys64",
        r"C:\Program Files\LLVM\bin",
    ] {
        let root = crate::editors::expand(raw);
        if !root.is_dir() {
            continue;
        }
        v.push(root.clone());
        if let Ok(rd) = std::fs::read_dir(&root) {
            let mut n = 0;
            for e in rd.flatten() {
                if e.path().is_dir() && n < 32 {
                    n += 1;
                    v.push(e.path().join("bin"));
                    v.push(e.path().clone());
                }
            }
        }
    }
    v
}

/// Visual Studio 装的 MSVC **不在 PATH 上**：要 `cl.exe` 出现在命令行里，
/// 得先跑一次 `vcvarsall.bat`。所以「明明装了 VS，工具却说没有 C++ 编译器」
/// 是这一摊最经典的一种误报。
///
/// 微软为此提供了 `vswhere.exe`（位置固定、一直随 VS Installer 装），问它就行。
fn msvc_cl_from_vswhere() -> Option<PathBuf> {
    let vswhere = [
        r"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe",
        r"%ProgramFiles%\Microsoft Visual Studio\Installer\vswhere.exe",
    ]
    .iter()
    .map(|r| crate::editors::expand(r))
    .find(|p| p.is_file())?;

    let (ok, out) = run_cmd(
        &vswhere,
        &[
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ],
        Duration::from_secs(10),
    )?;
    if !ok {
        return None;
    }
    let root = PathBuf::from(out.trim());
    if !root.is_dir() {
        return None;
    }

    // 一个 VS 里可能并装多个工具集版本，取最大的那个（目录名就是版本号，字典序可用）。
    let msvc = root.join("VC").join("Tools").join("MSVC");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&msvc)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    let v = versions.pop()?;

    // Hostx64\x64 是 64 位宿主编 64 位目标；只有 32 位宿主的旧版退到 Hostx86。
    // arm64 目标这里不找——本工具的用法（教学/本地跑）用不上。
    [
        v.join("bin").join("Hostx64").join("x64").join("cl.exe"),
        v.join("bin").join("Hostx86").join("x64").join("cl.exe"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// 先看 PATH，再翻上面的目录，最后问 vswhere。工具不该因为「你没配环境变量」就装作没装。
fn find_tool(name: &str) -> Option<PathBuf> {
    which(name)
        .or_else(|| {
            dev_dirs().into_iter().find_map(|d| {
                let p = d.join(name);
                p.is_file().then_some(p)
            })
        })
        .or_else(|| {
            if name == "cl.exe" {
                msvc_cl_from_vswhere()
            } else {
                None
            }
        })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Compiler {
    pub id: String,
    pub exe: String,
    /// gnu / llvm / msvc
    pub family: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CcFacts {
    pub compilers: Vec<Compiler>,
    pub cmake: Option<String>,
    pub cmake_version: String,
    pub ninja: bool,
    pub make: bool,
    pub has_cmake_lists: bool,
    pub has_makefile: bool,
}

const CC: &[(&str, &str, &str, &str)] = &[
    // (id, exe, 家族, 版本参数)
    ("gcc", "gcc.exe", "gnu", "--version"),
    ("g++", "g++.exe", "gnu", "--version"),
    ("clang", "clang.exe", "llvm", "--version"),
    ("clang++", "clang++.exe", "llvm", "--version"),
    ("cl", "cl.exe", "msvc", ""),
    ("clang-cl", "clang-cl.exe", "llvm", "--version"),
];

pub fn probe_cc(root: &std::path::Path) -> CcFacts {
    let mut compilers = Vec::new();
    for (id, exe, family, flag) in CC {
        let Some(p) = find_tool(exe) else { continue };
        // cl.exe 不带 --version（它打印的是横幅），给了参数反而不认
        let version = if flag.is_empty() {
            String::new()
        } else {
            run_cmd(&p, &[flag], Duration::from_secs(12))
                .map(|(_, o)| o.lines().next().unwrap_or("").trim().to_string())
                .unwrap_or_default()
        };
        compilers.push(Compiler {
            id: (*id).to_string(),
            exe: p.to_string_lossy().to_string(),
            family: (*family).to_string(),
            version,
        });
    }
    let cmake = find_tool("cmake.exe");
    let cmake_version = cmake
        .as_ref()
        .and_then(|p| run_cmd(p, &["--version"], Duration::from_secs(12)))
        .map(|(_, o)| o.lines().next().unwrap_or("").trim().to_string())
        .unwrap_or_default();
    CcFacts {
        compilers,
        cmake: cmake.map(|p| p.to_string_lossy().to_string()),
        cmake_version,
        ninja: find_tool("ninja.exe").is_some(),
        make: find_tool("make.exe").is_some() || find_tool("mingw32-make.exe").is_some(),
        has_cmake_lists: root.join("CMakeLists.txt").is_file(),
        has_makefile: root.join("Makefile").is_file() || root.join("makefile").is_file(),
    }
}
