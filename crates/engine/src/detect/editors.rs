//! 找出这台机器上装了哪些编辑器和终端，并用它打开项目。
//!
//! 为什么不写死几个路径：JetBrains 装在哪、VS Code 是用户版还是系统版，
//! 每台机器都不一样。这里按「PATH → 常见安装位置 → JetBrains 目录扫描」三层找，
//! 找到什么就列什么。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    /// editor（编辑器）/ terminal（终端）/ shell（命令行）/ files（文件管理器）
    pub kind: String,
    pub exe: String,
}

struct Spec {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    /// 直接在 PATH 里找这两个名字
    exes: &'static [&'static str],
    /// JetBrains 那种 `厂商目录\<产品>\bin\<exe>` 的布局
    under_product: bool,
    /// 明确写出来的安装位置，%VAR% 会展开
    paths: &'static [&'static str],
}

const SPECS: &[Spec] = &[
    Spec {
        id: "code",
        name: "VS Code",
        kind: "editor",
        exes: &["code.cmd", "code.exe"],
        under_product: false,
        paths: &[
            r"%LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe",
            r"%PROGRAMFILES%\Microsoft VS Code\Code.exe",
            r"%PROGRAMFILES(X86)%\Microsoft VS Code\Code.exe",
        ],
    },
    Spec { id: "idea", name: "IntelliJ IDEA", kind: "editor", exes: &["idea64.exe"], under_product: true, paths: &[] },
    Spec { id: "pycharm", name: "PyCharm", kind: "editor", exes: &["pycharm64.exe"], under_product: true, paths: &[] },
    Spec { id: "webstorm", name: "WebStorm", kind: "editor", exes: &["webstorm64.exe"], under_product: true, paths: &[] },
    Spec {
        id: "cursor",
        name: "Cursor",
        kind: "editor",
        exes: &["cursor.exe"],
        under_product: false,
        paths: &[r"%LOCALAPPDATA%\Programs\cursor\Cursor.exe"],
    },
    Spec {
        id: "subl",
        name: "Sublime Text",
        kind: "editor",
        exes: &["subl.exe"],
        under_product: false,
        paths: &[r"%PROGRAMFILES%\Sublime Text\subl.exe"],
    },
    Spec {
        id: "npp",
        name: "Notepad++",
        kind: "editor",
        exes: &["notepad++.exe"],
        under_product: false,
        paths: &[r"%PROGRAMFILES%\Notepad++\notepad++.exe"],
    },
    Spec { id: "wt", name: "Windows Terminal", kind: "terminal", exes: &["wt.exe"], under_product: false, paths: &[] },
    Spec { id: "pwsh", name: "PowerShell", kind: "shell", exes: &["pwsh.exe", "powershell.exe"], under_product: false, paths: &[] },
    Spec { id: "cmd", name: "命令提示符", kind: "shell", exes: &["cmd.exe"], under_product: false, paths: &[] },
    Spec { id: "explorer", name: "文件资源管理器", kind: "files", exes: &["explorer.exe"], under_product: false, paths: &[] },
];

pub fn expand(raw: &str) -> PathBuf {
    let mut s = raw.to_string();
    for (k, v) in std::env::vars() {
        if k == "PROGRAMFILES(X86)" {
            s = s.replace("%PROGRAMFILES(X86)%", &v);
        } else {
            s = s.replace(&format!("%{}%", k), &v);
        }
    }
    PathBuf::from(s)
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// JetBrains 的安装根目录，`厂商目录/产品/bin/xxx64.exe`
fn product_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for raw in [
        r"%PROGRAMFILES%\JetBrains",
        r"%LOCALAPPDATA%\Programs\JetBrains",
        r"%LOCALAPPDATA%\JetBrains\Toolbox\apps",
    ] {
        let p = expand(raw);
        if p.is_dir() {
            roots.push(p);
        }
    }
    roots
}

pub fn detect() -> Vec<App> {
    let mut out = Vec::new();
    for spec in SPECS {
        let mut found: Option<PathBuf> = None;
        for exe in spec.exes {
            if let Some(p) = which(exe) {
                found = Some(p);
                break;
            }
        }
        if found.is_none() {
            for raw in spec.paths {
                let p = expand(raw);
                if p.is_file() {
                    found = Some(p);
                    break;
                }
            }
        }
        if found.is_none() && spec.under_product {
            'outer: for root in product_roots() {
                let Ok(rd) = std::fs::read_dir(&root) else { continue };
                for e in rd.flatten() {
                    // Toolbox 的目录还要再深一层（ch-0\...\bin）
                    for candidate in [e.path().join("bin"), e.path().join("ch-0")] {
                        for exe in spec.exes {
                            let p = candidate.join(exe);
                            if p.is_file() {
                                found = Some(p);
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
        if let Some(exe) = found {
            out.push(App {
                id: spec.id.into(),
                name: spec.name.into(),
                kind: spec.kind.into(),
                exe: exe.to_string_lossy().to_string(),
            });
        }
    }
    out
}

/// 打开项目。这里**故意不加** CREATE_NO_WINDOW——
/// 要的就是把编辑器/终端的窗口弹出来给人用。
pub fn open(id: &str, path: &Path) -> Result<()> {
    let app = detect()
        .into_iter()
        .find(|a| a.id == id)
        .or_else(|| detect().into_iter().find(|a| a.id == "explorer"))
        .with_context(|| format!("找不到可用的「{}」，也没找到文件管理器", id))?;
    let p = path.to_string_lossy().to_string();
    let mut c = Command::new(&app.exe);
    match app.id.as_str() {
        "wt" => {
            c.arg("-d").arg(&p);
        }
        "cmd" => {
            c.arg("/K").arg(format!("cd /d \"{}\"", p));
        }
        "pwsh" => {
            c.arg("-NoExit")
                .arg("-Command")
                .arg(format!("Set-Location -LiteralPath '{}'", p.replace('\'', "''")));
        }
        _ => {
            c.arg(&p);
        }
    }
    c.spawn()
        .with_context(|| format!("启动 {} 失败", app.exe))?;
    Ok(())
}

// ---------------------------------------------------------------- 偏好

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("jingwei").join("settings.json"))
}

fn read_settings() -> serde_json::Value {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

pub fn default_open() -> Option<String> {
    read_settings()
        .get("defaultOpen")
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn set_default_open(id: Option<&str>) -> Result<()> {
    let Some(p) = settings_path() else {
        bail!("找不到 LOCALAPPDATA，没法存偏好");
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut v = read_settings();
    match id {
        Some(id) => v["defaultOpen"] = serde_json::Value::String(id.to_string()),
        None => {
            v.as_object_mut().map(|o| o.remove("defaultOpen"));
        }
    }
    std::fs::write(&p, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}
