//! 建项目的「配方」：模板 + 用哪个解释器 + 建不建虚拟环境。
//!
//! 为什么值得存：新手建第二个项目时，想要的其实和第一个一模一样。
//! 每次都重新选一遍解释器、重新纠结要不要虚拟环境，纯属消耗耐心。
//! 存成一条配置档，下次点一下就沿用。

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// script / package / cli
    pub template: String,
    /// python / rust / node / java / cpp。空 = 当成 python（老配置档没有这一项）
    #[serde(default)]
    pub lang: String,
    /// 解释器的绝对路径。空 = 让工具自己挑
    #[serde(default)]
    pub python_exe: Option<String>,
    #[serde(default = "yes")]
    pub make_venv: bool,
}

fn yes() -> bool {
    true
}

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("jingwei").join("profiles.json"))
}

pub fn list() -> Vec<Profile> {
    file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_all(all: &[Profile]) -> Result<()> {
    let Some(p) = file() else {
        bail!("找不到 LOCALAPPDATA，没法存配置档");
    };
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(all)?)?;
    Ok(())
}

/// 同名就覆盖——配置档是拿来用的，不是拿来考古的
pub fn save(profile: &Profile) -> Result<Vec<Profile>> {
    let name = profile.name.trim();
    if name.is_empty() {
        bail!("配置档得有个名字");
    }
    let mut all = list();
    all.retain(|p| p.name != name);
    let mut p = profile.clone();
    p.name = name.to_string();
    all.push(p);
    all.sort_by(|a, b| a.name.cmp(&b.name));
    write_all(&all)?;
    Ok(all)
}

pub fn remove(name: &str) -> Result<Vec<Profile>> {
    let mut all = list();
    all.retain(|p| p.name != name);
    write_all(&all)?;
    Ok(all)
}
