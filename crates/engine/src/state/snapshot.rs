use crate::facts::Facts;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEEP: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

impl Change {
    pub fn describe(&self) -> String {
        match (&self.detail, &self.before, &self.after) {
            (Some(d), _, _) => format!("{}：{}", self.kind, d),
            _ => format!(
                "{}：{} → {}",
                self.kind,
                self.before.clone().unwrap_or_default(),
                self.after.clone().unwrap_or_default()
            ),
        }
    }
}

pub fn snapshots_dir(root: &Path) -> PathBuf {
    crate::data_dir(root).join("snapshots")
}

pub fn save(root: &Path, facts: &Facts) -> Result<PathBuf> {
    let dir = snapshots_dir(root);
    std::fs::create_dir_all(&dir).with_context(|| format!("建目录失败 {}", dir.display()))?;
    let stamp = facts.taken_at.replace(':', "-").replace('Z', "");
    let mut path = dir.join(format!("{}.json", stamp));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{}_{}.json", stamp, n));
    }
    std::fs::write(&path, serde_json::to_string_pretty(facts)? + "\n")?;
    prune(&dir);
    Ok(path)
}

fn prune(dir: &Path) {
    let all = list(dir);
    if all.len() <= KEEP {
        return;
    }
    for p in &all[..all.len() - KEEP] {
        // 修剪旧快照。删不掉只是多占一点磁盘，且这里能拿到的上下文不比磁盘状态更多。
        #[allow(clippy::let_underscore_must_use)]
        let _ = std::fs::remove_file(p);
    }
}

/// 按修改时间排序 —— 按文件名排会把 "…_2" 排到 "….json" 前面，新旧就反了
fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                let t = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                v.push((t, p));
            }
        }
    }
    v.sort_by_key(|(t, _)| *t);
    v.into_iter().map(|(_, p)| p).collect()
}

pub fn count(root: &Path) -> usize {
    list(&snapshots_dir(root)).len()
}

pub fn load(path: &Path) -> Result<Facts> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

pub fn load_latest(root: &Path) -> Option<(PathBuf, Facts)> {
    for p in list(&snapshots_dir(root)).iter().rev() {
        if let Ok(f) = load(p) {
            return Some((p.clone(), f));
        }
    }
    None
}

pub fn load_previous(root: &Path) -> Option<(PathBuf, Facts)> {
    let all = list(&snapshots_dir(root));
    if all.len() < 2 {
        return None;
    }
    let p = all[all.len() - 2].clone();
    load(&p).ok().map(|f| (p, f))
}

/// 快照差异里只显示解释器的尾部两段，整条绝对路径太长会把一行撑爆
fn short_exe(exe: &str) -> String {
    let parts: Vec<&str> = exe.rsplit(['\\', '/']).take(2).collect();
    match parts.len() {
        0 => exe.to_string(),
        1 => parts[0].to_string(),
        _ => format!("{}\\{}", parts[1], parts[0]),
    }
}

/// 只回答「变了什么」，不判断好坏
pub fn diff(old: &Facts, new: &Facts) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    {
        let mut ch = |kind: String, detail: Option<String>, before: Option<String>, after: Option<String>| {
            out.push(Change { kind, detail, before, after });
        };

        if old.system.path.hash != new.system.path.hash || old.system.path.count != new.system.path.count {
            ch(
                "system:path".into(),
                None,
                Some(format!("{} 条", old.system.path.count)),
                Some(format!("{} 条", new.system.path.count)),
            );
        }
        for (k, v) in &new.system.env {
            if old.system.env.get(k) != Some(v) {
                ch(
                    format!("env:{}", k),
                    None,
                    old.system.env.get(k).cloned(),
                    Some(v.clone()),
                );
            }
        }
        for (k, v) in &old.system.env {
            if !new.system.env.contains_key(k) {
                ch(format!("env:{}", k), None, Some(v.clone()), None);
            }
        }
        for (name, t) in &new.toolchain {
            let o = old.toolchain.get(name);
            let o_ok = o.map(|x| x.ok).unwrap_or(false);
            if o_ok != t.ok {
                ch(
                    format!("tool:{}", name),
                    None,
                    Some(if o_ok { "可用" } else { "不可用" }.into()),
                    Some(if t.ok { "可用" } else { "不可用" }.into()),
                );
            } else if o.and_then(|x| x.version.clone()) != t.version {
                ch(
                    format!("tool:{}", name),
                    None,
                    o.and_then(|x| x.version.clone()),
                    t.version.clone(),
                );
            }
        }
        for (key, a, b) in [
            ("python:version", old.python.version.clone(), new.python.version.clone()),
            ("python:exe", old.python.exe.clone(), new.python.exe.clone()),
            ("python:in_venv", old.python.in_venv.to_string(), new.python.in_venv.to_string()),
            ("python:isolated", old.python.isolated.to_string(), new.python.isolated.to_string()),
        ] {
            if a != b {
                ch(key.into(), None, Some(a), Some(b));
            }
        }
        // 换了解释器就没法比「已装包」了：那是两个不同的环境，
        // 逐条列出只会变成一屏「你少了 10 个包」的假警报。
        // 这种情况下真正有用的一句话是「依赖要按新解释器重装」。
        let oi: std::collections::BTreeSet<&String> = old.python.installed.iter().collect();
        let ni: std::collections::BTreeSet<&String> = new.python.installed.iter().collect();
        if old.python.exe != new.python.exe {
            let lost = oi.difference(&ni).count();
            let gained = ni.difference(&oi).count();
            if lost > 0 || gained > 0 || old.python.installed.len() != new.python.installed.len() {
                ch(
                    "包:不可比".into(),
                    Some(format!(
                        "解释器换了（{} → {}），已装包列表属于两个不同环境，不逐条对比；\
                         这个项目声明的依赖要按新解释器重装一遍（{} 项 → {} 项）",
                        short_exe(&old.python.exe),
                        short_exe(&new.python.exe),
                        old.python.installed.len(),
                        new.python.installed.len()
                    )),
                    None,
                    None,
                );
            }
        } else {
            for x in ni.difference(&oi).take(10) {
                ch("dep+".into(), Some((*x).clone()), None, None);
            }
            for x in oi.difference(&ni).take(10) {
                ch("dep-".into(), Some((*x).clone()), None, None);
            }
        }
        for (k, v) in &new.project.deps_status {
            if old.project.deps_status.get(k) != Some(v) {
                ch(
                    format!("dep-status:{}", k),
                    None,
                    Some(
                        old.project
                            .deps_status
                            .get(k)
                            .map(|b| if *b { "已装" } else { "未装" })
                            .unwrap_or("未知")
                            .into(),
                    ),
                    Some(if *v { "已装" } else { "未装" }.into()),
                );
            }
        }
        if old.project.contract_hash != new.project.contract_hash {
            ch(
                "project:contract_hash".into(),
                None,
                old.project.contract_hash.clone(),
                new.project.contract_hash.clone(),
            );
        }
        if old.project.venvs != new.project.venvs {
            ch(
                "project:venvs".into(),
                None,
                Some(format!("{:?}", old.project.venvs)),
                Some(format!("{:?}", new.project.venvs)),
            );
        }
    }
    out
}

/// 给人看的「切中要害的 6~8 行」
pub fn summary_lines(x: &Facts) -> Vec<(String, String)> {
    let mut v = Vec::new();
    v.push(("项目类型".into(), x.lang.name().to_string()));
    v.push((
        "解释器".into(),
        if x.python.found {
            format!("Python {}（{}）", x.python.version, x.python.exe)
        } else {
            "未找到".into()
        },
    ));
    let kind = if !x.python.found {
        "未知"
    } else if x.python.isolated || !x.python.pth_files.is_empty() {
        "嵌入式/便携（隔离模式）"
    } else if x.python.in_venv {
        "虚拟环境内"
    } else {
        "普通安装"
    };
    v.push(("环境类型".into(), kind.into()));
    v.push(("已装包".into(), format!("{} 个", x.python.installed.len())));
    v.push((
        "PATH".into(),
        format!("{} 条（hash {}）", x.system.path.count, x.system.path.hash),
    ));
    if !x.project.deps_status.is_empty() {
        let missing: Vec<String> = x
            .project
            .deps_status
            .iter()
            .filter(|(_, ok)| !**ok)
            .map(|(k, _)| k.clone())
            .collect();
        v.push((
            "依赖".into(),
            if missing.is_empty() {
                "都装了".into()
            } else {
                format!("缺 {}", missing.join(", "))
            },
        ));
    }
    v.push((
        "契约".into(),
        if x.project.contract.is_some() { "有".into() } else { "无".into() },
    ));
    let tools: Vec<String> = x
        .toolchain
        .iter()
        .filter(|(_, t)| t.ok)
        .map(|(k, t)| format!("{} {}", k, t.version.clone().unwrap_or_default()))
        .collect();
    v.push((
        "工具链".into(),
        if tools.is_empty() { "（无）".into() } else { tools.join(" · ") },
    ));
    v.push(("采集时间".into(), x.taken_at.clone()));
    v
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use crate::facts::Facts;

    fn facts(exe: &str, installed: &[&str]) -> Facts {
        let mut f = Facts::default();
        f.python.found = true;
        f.python.exe = exe.to_string();
        f.python.installed = installed.iter().map(|s| s.to_string()).collect();
        f
    }

    /// 换了解释器还逐条列「你少了这些包」，是一屏假警报。
    /// 这是本机装完 3.15 之后真实踩到的：一条 python:exe 的变化，
    /// 底下跟着 10 行 dep-，看着像项目被拆了。
    #[test]
    fn switching_interpreters_does_not_report_fake_package_loss() {
        let old = facts("D:\\old\\python.exe", &["rich==15.0.0", "six==1.16.0"]);
        let new = facts("C:\\new\\python.exe", &["pip==26.2.1"]);
        let d = diff(&old, &new);
        let kinds: Vec<String> = d.iter().map(|c| c.kind.clone()).collect();
        assert!(
            !kinds.iter().any(|k| k.starts_with("dep")),
            "换解释器时不该逐条列包，实际有：{:?}",
            kinds
        );
        let note = d
            .iter()
            .find(|c| c.kind == "包:不可比")
            .expect("应该给一条「解释器换了」的说明");
        assert!(note.describe().contains("重装"), "{}", note.describe());
    }

    /// 但同一个解释器下装了/卸了包，是必须报出来的
    #[test]
    fn same_interpreter_still_reports_real_package_changes() {
        let old = facts("C:\\same\\python.exe", &["rich==15.0.0"]);
        let new = facts("C:\\same\\python.exe", &["rich==15.0.0", "six==1.16.0"]);
        let d = diff(&old, &new);
        let kinds: Vec<String> = d.iter().map(|c| c.kind.clone()).collect();
        assert!(kinds.contains(&"dep+".to_string()), "{:?}", kinds);
    }
}
