//! 项目边界：**这个目录到底是不是一个项目**。
//!
//! ## 为什么单独一个文件
//!
//! `project.rs` 早就超了 1000 行上限、登记在案（只许降不许升）。这一册的主题
//! 与它里面那些检查（忽略规则、依赖清单、体积）**不是一类**：那些问"这个项目有什么毛病"，
//! 这里问"**你看的这个东西是不是一个项目**"——后者更上层。
//!
//! ## 它是拿什么换来的
//!
//! 2026-10-16 在真窗口里打开 `D:\projects\jingwei-demo`（三个演示项目的父目录）时抓到的：
//! 界面上写着「项目类型 Python」「环境看着没问题」，而依赖清单里并排躺着
//! `hello_python/requirements.txt` 和 `hello_rust/Cargo.toml`——
//! **一个"项目"的体检里混着两个别的项目**，而那些结论一条都不该算在这一层头上。

use crate::understand::project::it;
use jingwei_model::{Finding, Severity};
use std::path::Path;

/// 这个目录里是不是装着**好几项目**（它自己不是一个项目，是个容器）。
///
/// ## 为什么要有这条判据
///
/// 2026-10-16 打开 `D:\projects\jingwei-demo`（里面是三个演示项目）时抓到的：
/// 界面上写着「项目类型 Python」「环境看着没问题」，而依赖清单里并排列着
/// `hello_python/requirements.txt` 和 `hello_rust/Cargo.toml`——
/// **一个"项目"的体检里混着两个别的项目**，而它们各自的结论都不该算在这一层头上。
///
/// 它的成因在 `manifests` 收集那里：`rel.matches('/').count() > 1` 允许**一层**子目录
/// 的清单（那是给 monorepo 用的），所以子项目的清单会被收进来。那条规矩本身是对的
/// （monorepo 真的需要），缺的是**一句告诉用户"你打开的是装项目的目录"的话**。
///
/// 判据：**直接子目录里有 ≥2 个各自带项目标记**。1 个不算——那种情况多半是
/// 一个项目里带一个示例，硬报就成了误报；而这个项目最不许犯的就是误报。
pub(crate) fn child_projects(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        // 产物目录与工具自己的目录不算项目——**判据只有一处**（`util::scan`）。
        if crate::util::scan::is_not_source_dir(&name) {
            continue;
        }
        let dir = e.path();
        let Ok(inner) = std::fs::read_dir(&dir) else {
            continue;
        };
        // **"什么算项目标记"只有一处判据**（`project::manifest_kind`）。
        // 原来这里直接查 `MARKERS` 常量，于是漏了 `.csproj` / `.sln` / `.fsproj`、
        // `settings.gradle.kts`、`requirements-dev.txt`——
        // **一个装着 .NET 项目的父目录认不出自己是「容器目录」**。
        let has_marker = inner.flatten().any(|f| {
            let n = f.file_name().to_string_lossy().to_string();
            crate::understand::project::manifest_kind(&n).is_some()
        });
        if has_marker {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// 把"这是个装项目的目录"变成一条能找到出路的结论。
///
/// 四要素齐全（这个项目的硬规矩）：
/// **事实**（找到哪几个子项目）· **自查**（一条能复制的命令）· **边界**（什么时候不用管）·
/// **下一步**（往哪走）——注意第四样**不是修复命令**，是"你该去看哪里"。
pub(crate) fn projects_dir_item(root: &Path) -> Option<Finding> {
    let kids = child_projects(root);
    if kids.len() < 2 {
        return None;
    }
    let shown: Vec<String> = kids.iter().take(6).cloned().collect();
    let more = if kids.len() > shown.len() {
        format!(" 等 {} 个", kids.len())
    } else {
        String::new()
    };
    Some(it(
        "projects-dir-not-a-project",
        // 报"提示"而不是"要处理"：这个目录本身没有毛病，
        // 只是**你看错了对象**。把它报成错误就是误报。
        Severity::Info,
        crate::understand::project::G_DEPS,
        format!("这是个装着 {} 个项目的目录，不是一个项目", kids.len()),
        vec![
            format!("里面有：{}{}", shown.join("、"), more),
            "本页的「项目类型」「依赖清单」是把它们**混在一起**看的——比如同时列出一个 Rust 项目的 Cargo.toml 和一个 Python 项目的 requirements.txt".into(),
        ],
        vec!["Get-ChildItem -Directory | Select-Object Name".into()],
        "如果你就是想把这一堆放在一起看（比如它是一份教学材料），那不用管这一条。",
        "想看某一个项目的真实状态，在「概览」页的项目列表里**直接点它那一行**——\
         每条结论只有落在正确的项目上才算数。",
    ))
}
