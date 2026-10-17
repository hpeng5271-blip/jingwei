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
        // 依赖状态的变化：**按三态比，而且遍历两侧的并集。**
        //
        // 这一段原来读 `deps_status`（`BTreeMap<String, bool>`）。两个问题一起修了：
        //   ① 它**只遍历新侧**（`for (k, v) in &new.…`），而布尔图里"无法确认"的项
        //      压根不进去——所以某依赖从"已装"变成"无法确认"时，**差异里一行都不产生**
        //      （静默丢失）。同一个文件里 env 那段专门补过"只遍历新侧会漏掉删除"的第二个循环，
        //      这一段没有。现在按两侧并集比。
        //   ② 布尔图本身装不下三态（那个类型 2026-10-16 已删），所以以前这里
        //      只能说"已装/未装"，说不了"这下查不了了"。
        {
            let state_of = |x: &Facts| -> std::collections::BTreeMap<String, String> {
                x.project
                    .deps_detail
                    .iter()
                    .map(|d| {
                        let s = if d.ok {
                            format!("已装 {}", d.have)
                        } else if d.unknown {
                            // **"无法确认"后面要跟上是"谁的问题"**（2026-10-16）。
                            //
                            // 原来这里只有光秃秃四个字「无法确认」。而"无法确认"有两种：
                            // 用户那边没有（他没提供判据的入口），和我们这边查不到
                            // （问不到 pip / 那个语言我们没实现 / 文件读不出来）。
                            // **同一句"无法确认"会让用户不知道该做什么**——
                            // 一种是"你补上"，一种是"不怪你，等我们修"。
                            //
                            // 归属由 `DepDetail::unknown_why()` 给（那里逐个出口查过），
                            // **不在这里猜**：快照层看不到"为什么未知"，它只该转述。
                            format!("无法确认——{}", d.unknown_why().what_to_do())
                        } else {
                            "未装".to_string()
                        };
                        (d.name.clone(), s)
                    })
                    .collect()
            };
            let (os, ns) = (state_of(old), state_of(new));
            let names: std::collections::BTreeSet<&String> = os.keys().chain(ns.keys()).collect();
            for k in names {
                let (a, b) = (os.get(k), ns.get(k));
                if a != b {
                    ch(
                        format!("dep-status:{}", k),
                        None,
                        a.map(|s| s.as_str()).unwrap_or("（这次没声明）").to_string().into(),
                        b.map(|s| s.as_str()).unwrap_or("（这次没声明）").to_string().into(),
                    );
                }
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
    // **这一行原来只看布尔图（`deps_status`），于是把"无法确认"说成了"都装了"。**
    //
    // 2026-10-16 外部对照复核抓出：第 16 轮修掉的是 `ui/projects.js` 里那句
    // 「解释器、依赖、契约都对得上」，而**这条路径没被扫到**——它更底层，
    // 一路走到 CLI（`cmd_doctor` / `cmd_project`）和界面的 `#facts`。
    // 三种情形都会说错：
    //   - 一部分 true + 一部分 unknown → `missing` 为空 → 打印「依赖 都装了」；
    //   - 全部 unknown → `deps_status` 是空 map → **这行整个消失**（沉默）；
    //   - 而同屏的左边栏（`ui/shell.js`）会说「N 项无法确认」——**自相矛盾**。
    //
    // 现在按三态说话：unknown 单独列出来，**"都装了"只在真的都确认装了时出现**。
    let mut deps_ok = 0;
    let mut deps_missing: Vec<String> = Vec::new();
    let mut deps_unknown: Vec<String> = Vec::new();
    for d in &x.project.deps_detail {
        if d.ok {
            deps_ok += 1;
        } else if d.unknown {
            deps_unknown.push(d.name.clone());
        } else {
            deps_missing.push(d.name.clone());
        }
    }
    if deps_ok + deps_missing.len() + deps_unknown.len() > 0 {
        let mut parts: Vec<String> = Vec::new();
        if !deps_missing.is_empty() {
            parts.push(format!("缺 {}", deps_missing.join(", ")));
        }
        if !deps_unknown.is_empty() {
            // 「查不了」既不是"缺"也不是"都装了"——单列，并说清是哪几项。
            //
            // 2026-10-16：**再加"是谁的"**。原来这句只说"无法确认"，
            // 用户分不出"我没提供判据"和"它没查到"——而这两件事该做的事完全不同。
            // 归属从 `DepDetail::unknown_why()` 来（逐个出口查过，不是这里猜的）。
            let why = x
                .project
                .deps_detail
                .iter()
                .find(|d| d.unknown)
                .map(|d| d.unknown_why().what_to_do())
                .unwrap_or("");
            parts.push(format!(
                "{} 项无法确认（{}）——{}",
                deps_unknown.len(),
                deps_unknown.join(", "),
                why
            ));
        }
        if parts.is_empty() {
            parts.push("都装了".into());
        }
        v.push(("依赖".into(), parts.join("；")));
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

    /// **「无法确认」要说是谁的问题**（2026-10-16）。
    ///
    /// 同一个解释器、同一批依赖，唯一不同的是 `deps_detail` 里那一条 `unknown`。
    /// 快照那两处输出（变更卡的逐条状态、概览那行的汇总）**都要带上归属**——
    /// 光说"无法确认"，用户不知道是该他补点什么，还是等我们修。
    ///
    /// 这里同时钉两处，因为它们会各自坏：
    ///   ① `summary_lines`（概览那行）
    ///   ② `diff` 里那条 `dep?`（变更卡逐条）
    #[test]
    fn 无法确认的依赖在两处都要说清是谁的问题() {
        let mk = |unknown: bool| {
            let mut f = facts("C:\\same\\python.exe", &[]);
            f.project.deps_detail = vec![crate::facts::DepDetail {
                name: "rich".into(),
                want: ">=13".into(),
                have: String::new(),
                ok: false,
                unknown,
                from: "requirements.txt".into(),
            evidence: "pip list".into(),
            }];
            f
        };
        // 装了的那种（用来造"状态变了"）
        let mk_ok = || {
            let mut f = facts("C:\\same\\python.exe", &[]);
            f.project.deps_detail = vec![crate::facts::DepDetail {
                name: "rich".into(),
                want: ">=13".into(),
                have: "13.7.1".into(),
                ok: true,
                unknown: false,
                from: "requirements.txt".into(),
            evidence: "pip list".into(),
            }];
            f
        };
        // ① 概览那行
        let lines = summary_lines(&mk(true));
        let deps = lines
            .iter()
            .find(|(k, _)| k == "依赖")
            .map(|(_, v)| v.clone())
            .expect("该有「依赖」这一行");
        assert!(deps.contains("无法确认"), "{deps}");
        assert!(
            deps.contains("我们") || deps.contains("不怪你"),
            "「无法确认」必须说清是我们的失败，实际：{deps}"
        );
        // ② 变更卡：**让状态真的变化**（"已装"→"无法确认"），那条记录才会带上归属。
        //
        //    写这条测试时踩了一次：我第一版拿**同一份** facts 相减（`diff(&mk(true), &mk(true))`），
        //    得到空变更 → 断言直接失败在 `""` 上。原因是 `state_of` 那个字符串
        //    **只用于比较两侧状态**，不进任何输出——**没变化就没输出，也就没有断言对象**。
        //    所以测试必须构造"状态变了"，否则它测的是一个没有消费者的中间量。
        let known = mk_ok();
        let d_changed = diff(&known, &mk(true));
        let text = format!("{d_changed:?}");
        assert!(
            text.contains("不怪你") || text.contains("我们"),
            "从'已装'变成'无法确认'时，变更卡里的说法要带归属，实际：{text}"
        );
        // 而未装那侧**不许**被说成"无法确认"（那是把两种情形混成一种）
        let d_missing = diff(&known, &mk(false));
        let missing_text = format!("{d_missing:?}");
        assert!(
            missing_text.contains("未装"),
            "真的没装要报成'未装'：{missing_text}"
        );
        assert!(
            !missing_text.contains("无法确认"),
            "真的没装不该说成无法确认：{missing_text}"
        );
    }
}
