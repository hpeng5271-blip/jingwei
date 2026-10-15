//! Node / 前端项目的**前四个阶段**：运行时、命令从哪来、依赖装没装、跑哪条脚本。
//!
//! ## 为什么不能走 Python 那一条
//!
//! Python 那条路的第 1 阶段就是"找一个能用的解释器"，找不到就报
//! 「没有可用的 Python，无法启动」。拿它去开一个 Vue 项目，用户看到的是
//! 一句莫名其妙的话——**答非所问比不回答更糟**，因为它把人引到错的方向上。
//!
//! ## 这一条路里"确定"与"不确定"的边界
//!
//! | 事 | 凭什么 | 有多确定 |
//! |---|---|---|
//! | 用哪个包管理器 | `package.json` 的 `packageManager`，或锁文件 | 确定（多个锁文件并存时**不猜**） |
//! | 跑哪条脚本 | `package.json` 的 `scripts` 里真的写着 | 确定 |
//! | 依赖装没装 | `node_modules/<名字>` / PnP | **PnP 下确认不了**（映射表在 `.pnp.cjs` 里，要跑 JS 才读得出） |
//! | Node 版本对不对 | 项目写的 pin vs `node -v` | **只按主版本号比**，范围表达式不解析 |
//!
//! 最后两行是这一层最该守住的地方：能说清楚的就说清楚，说不清楚的说"说不清楚"。

use super::{Event, LaunchOptions, Prepared, Purpose, StageStatus};
use crate::detect::{deps, lang::Lang, web};
use crate::understand::contract::Contract;
use crate::util::winutil::{run_cmd, which};
use anyhow::{anyhow, Result};
use std::path::Path;
use std::time::Duration;

/// 常驻类脚本：跑起来之后**不会退**，所以"到点没就绪"不等于失败。
const LIVE_SCRIPTS: [&str; 4] = ["dev", "start", "serve", "preview"];

pub(crate) fn prepare(
    root: &Path,
    opts: &LaunchOptions,
    contract: &Option<Contract>,
    contract_err: &Option<String>,
    cb: &mut dyn FnMut(Event),
) -> Result<Prepared> {
    let facts = web::probe_web(root)
        .ok_or_else(|| anyhow!("不是一个 Node 项目（根目录下没有 package.json）"))?;

    // ---- 1 检查运行环境 ----
    let t = std::time::Instant::now();
    let node = find_node();
    let (pm_name, pm_exe) = match &facts.package_manager {
        Some(p) => (p.clone(), which(&format!("{p}.cmd")).or_else(|| which(p))),
        None => (String::new(), None),
    };
    let node_ver = node.as_ref().and_then(|n| version_of(n));
    let pm_ver = pm_exe.as_ref().and_then(|p| version_of(p));
    let (s1, d1) = if node.is_none() {
        (
            StageStatus::Fail,
            "没找到 node（PATH 里没有）。装一个 Node.js 再回来——这是所有前端项目的底座".to_string(),
        )
    } else if pm_name.is_empty() {
        (
            StageStatus::Fail,
            format!(
                "看不出用哪个包管理器（{}）。装依赖、跑脚本都要靠它",
                facts
                    .pm_from
                    .clone()
                    .unwrap_or_else(|| "没有锁文件，也没有 packageManager 字段".into())
            ),
        )
    } else if pm_exe.is_none() {
        (
            StageStatus::Fail,
            format!("这个项目用 {pm_name}，但 PATH 里没有 {pm_name}"),
        )
    } else {
        let mut d = format!(
            "node {} · {} {}",
            node_ver.clone().unwrap_or_else(|| "（读不出小版本）".into()),
            pm_name,
            pm_ver.clone().unwrap_or_default()
        );
        // 项目写了版本要求就摆出来对一对。**只比主版本号**——范围表达式
        // （`^20.11`、`>=18 <21`）不做完整解析，所以措辞不能像在断言。
        if let Some((pin, from, want_major)) = wanted_major(&facts) {
            let got_major = node_ver.as_deref().and_then(major_of);
            d.push_str(&format!(" · 项目要求 {pin}（{from}）"));
            if let (Some(w), Some(g)) = (want_major, got_major) {
                if w != g {
                    d.push_str(&format!("——你这里是主版本 {g}，按主版本比不一致"));
                }
            }
        }
        (StageStatus::Ok, d)
    };
    cb(Event::Stage {
        index: 1,
        total: super::TOTAL_STAGES,
        title: "检查运行环境".into(),
        status: s1,
        detail: d1,
        ms: t.elapsed().as_millis(),
    });

    // ---- 2 校验项目契约 ----
    let t = std::time::Instant::now();
    let script = pick_script(&facts, opts.purpose);
    let (s2, d2) = match (contract, contract_err) {
        (Some(c), _) if c.run_command().is_some() && opts.purpose == Purpose::Run => (
            StageStatus::Ok,
            format!(
                "{} · run: {}",
                c.name.clone().unwrap_or_else(|| "（未命名）".into()),
                c.run_command().unwrap_or("")
            ),
        ),
        (Some(_), _) if opts.purpose == Purpose::Run => (
            StageStatus::Note,
            "契约里没有 run 命令，按 package.json 的 scripts 推".to_string(),
        ),
        (None, Some(e)) => (StageStatus::Fail, format!("契约解析失败：{e}")),
        (None, _) => (
            StageStatus::Note,
            match (&script, opts.purpose) {
                (Some((k, v)), _) => format!("没有 env.contract.json；就用 scripts 里的 {k}：{v}"),
                // 装依赖本来就不该在 scripts 里找——那句「没有可用的脚本」
                // 会让人以为项目缺了点什么，而它什么都不缺。
                (None, Purpose::Install) => {
                    "没有 env.contract.json；装依赖用包管理器自己的子命令".to_string()
                }
                (None, _) => format!(
                    "没有 env.contract.json；scripts 里也没有 {}",
                    want_keys(opts.purpose)
                ),
            },
        ),
        (Some(_), _) => (StageStatus::Ok, "按任务跑，不用契约里的启动命令".to_string()),
    };
    cb(Event::Stage {
        index: 2,
        total: super::TOTAL_STAGES,
        title: "校验项目契约".into(),
        status: s2,
        detail: d2,
        ms: t.elapsed().as_millis(),
    });

    // ---- 3 解析依赖声明 ----
    let t = std::time::Instant::now();
    let declared = deps::declared(root, Lang::Web);
    let mut missing = Vec::new();
    let mut unknown = Vec::new();
    for d in &declared {
        match deps::installed(root, Lang::Web, &d.name, &[]) {
            deps::Verdict::No => missing.push(d.name.clone()),
            deps::Verdict::Unknown => unknown.push(d.name.clone()),
            deps::Verdict::Yes(_) => {}
        }
    }
    let (s3, d3) = if declared.is_empty() {
        (StageStatus::Note, "package.json 里没有声明依赖".to_string())
    } else if !missing.is_empty() {
        (
            StageStatus::Note,
            format!(
                "{} 项里缺 {} 项：{} · 依赖位置：{}",
                declared.len(),
                missing.len(),
                head(&missing, 6),
                facts.link_kind
            ),
        )
    } else if !unknown.is_empty() {
        (
            StageStatus::Note,
            format!(
                "{} 项都已声明；但有 {} 项**确认不了**（{}）——{}",
                declared.len(),
                unknown.len(),
                facts.link_kind,
                facts.link_from
            ),
        )
    } else {
        (
            StageStatus::Ok,
            format!("{} 项都已装 · {}", declared.len(), facts.link_kind),
        )
    };
    cb(Event::Stage {
        index: 3,
        total: super::TOTAL_STAGES,
        title: format!("解析依赖声明（{} 项）", declared.len()),
        status: s3,
        detail: d3,
        ms: t.elapsed().as_millis(),
    });

    // ---- 4 准备运行时 ----
    let t = std::time::Instant::now();
    // **装依赖没有"脚本"这一步**：`pnpm install` 是包管理器自己的子命令。
    // 把它塞进 scripts 那条路，会让"补装依赖"这个按钮去跑别的脚本。
    let install_only = opts.purpose == Purpose::Install;
    let script_key = script.clone().map(|(k, _)| k);
    let Some(key) = script_key.clone().or_else(|| {
        if install_only {
            Some(String::new())
        } else {
            None
        }
    }) else {
        // 没有脚本可用时，**不硬编一条**——把"这里有什么"如实列出来更有用。
        let keys: Vec<String> = facts.scripts.iter().map(|(k, _)| k.clone()).collect();
        cb(Event::Stage {
            index: 4,
            total: super::TOTAL_STAGES,
            title: "准备运行时".into(),
            status: StageStatus::Fail,
            detail: format!(
                "找不到可跑的脚本（要 {}）。package.json 里有这些：{}",
                want_keys(opts.purpose),
                if keys.is_empty() {
                    "一个 scripts 都没写".to_string()
                } else {
                    keys.join(" / ")
                }
            ),
            ms: t.elapsed().as_millis(),
        });
        return Err(anyhow!(
            "package.json 里没有 {} 脚本：{}",
            want_keys(opts.purpose),
            if keys.is_empty() {
                "它一个 scripts 都没写".to_string()
            } else {
                format!("它有的是 {}", keys.join(" / "))
            }
        ));
    };
    let exe = pm_exe.ok_or_else(|| anyhow!("PATH 里没有 {pm_name}"))?;
    let (run_cmd, args, why) = if install_only {
        (
            format!("{pm_name} install"),
            vec!["install".to_string()],
            "包管理器自己的子命令，不走 scripts".to_string(),
        )
    } else {
        let body = script.clone().map(|(_, v)| v).unwrap_or_default();
        (
            format!("{pm_name} run {key}"),
            vec!["run".to_string(), key.clone()],
            format!("脚本原文：{body}"),
        )
    };
    cb(Event::Stage {
        index: 4,
        total: super::TOTAL_STAGES,
        title: "准备运行时".into(),
        status: StageStatus::Ok,
        detail: format!("{} · {}（{}）", exe.display(), run_cmd, why),
        ms: t.elapsed().as_millis(),
    });

    let live = !install_only && LIVE_SCRIPTS.contains(&key.as_str());
    Ok(Prepared {
        program: exe,
        args,
        // Node 侧不需要替用户设环境变量：那几个（NODE_ENV 之类）
        // 该由项目的 scripts 自己写——我们设了反而会盖掉它的意思。
        env: Vec::new(),
        run_cmd,
        // **常驻服务不退是正常的**：到点了还在跑，只能说"还没在输出里看到监听地址"，
        // 不能说它失败。说失败会让用户去查一个根本不存在的故障。
        alive_unready: if live { StageStatus::Note } else { StageStatus::Fail },
        url_watch: live && opts.purpose == Purpose::Run,
        ready_rule: if live && opts.purpose == Purpose::Run {
            "输出里出现监听地址（localhost / 127.0.0.1）".to_string()
        } else {
            "退出码 0".to_string()
        },
    })
}

/// 这个用途下该找哪几个脚本，按优先级排。
fn want_keys(purpose: Purpose) -> String {
    match purpose {
        Purpose::Task => "test".to_string(),
        Purpose::Build => "build".to_string(),
        Purpose::Run => "dev 或 start".to_string(),
        // 装依赖不是"跑个脚本"——它是包管理器自己的子命令。
        Purpose::Install => "（不需要脚本，用包管理器装）".to_string(),
    }
}

/// 从 scripts 里挑一条。
///
/// **顺序是固定的事实，不是偏好**：`dev` 是开发时用的（带热更新），
/// `start` 通常指向生产启动。跑开发就该先给 `dev`。
fn pick_script(facts: &web::WebFacts, purpose: Purpose) -> Option<(String, String)> {
    let want: &[&str] = match purpose {
        Purpose::Task => &["test"],
        Purpose::Build => &["build"],
        Purpose::Run => &["dev", "start"],
        Purpose::Install => &[],
    };
    for key in want {
        if let Some((k, v)) = facts.scripts.iter().find(|(k, _)| k == key) {
            return Some((k.clone(), v.clone()));
        }
    }
    None
}

/// `node` 在哪儿。Windows 上有 `node.exe`，也有不带后缀的（有些包装器）。
fn find_node() -> Option<std::path::PathBuf> {
    which("node.exe").or_else(|| which("node"))
}

/// 跑一次 `-v` 拿版本号。拿不到就返回 `None`（**不编一个**）。
fn version_of(exe: &Path) -> Option<String> {
    let (ok, out) = run_cmd(exe, &["-v"], Duration::from_secs(5))?;
    if !ok {
        return None;
    }
    let line = out.lines().find(|l| !l.trim().is_empty())?.trim().to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// 取版本号的第一个数字组：`v20.11.0` → `20`，`9.1.0` → `9`。
fn major_of(s: &str) -> Option<u32> {
    let digits: String = s
        .trim_start_matches('v')
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 项目写下的 Node 版本要求里最该先看的那一条 + 它的主版本号。
///
/// **为什么只取一条**：`.nvmrc` 和 `engines.node` 常常不一样，
/// 界面那一栏会把它们**一处一条**全列出来（那才是完整事实）；
/// 这里只是启动时顺嘴提一句，所以挑"最具体的那一条"——
/// 有精确版本号的不拿范围表达式。
fn wanted_major(facts: &web::WebFacts) -> Option<(String, String, Option<u32>)> {
    let exact = facts
        .node_pins
        .iter()
        .find(|p| p.spec.chars().all(|c| c.is_ascii_digit() || c == '.' || c == 'v'))
        .or_else(|| facts.node_pins.first())?;
    let digits: String = exact
        .spec
        .trim_start_matches('v')
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let major = digits.parse().ok();
    Some((exact.spec.clone(), exact.from.clone(), major))
}

fn head(v: &[String], n: usize) -> String {
    if v.len() <= n {
        v.join(", ")
    } else {
        format!("{} 等 {} 项", v[..n].join(", "), v.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts_with(scripts: &[(&str, &str)]) -> web::WebFacts {
        web::WebFacts {
            scripts: scripts
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            ..Default::default()
        }
    }

    /// `dev` 优先于 `start`：开发时跑的那个带热更新，是"我要开始干活"该用的。
    #[test]
    fn 先挑_dev_再挑_start() {
        let f = facts_with(&[("start", "node server.js"), ("dev", "vite")]);
        assert_eq!(pick_script(&f, Purpose::Run).unwrap().0, "dev");
        let only_start = facts_with(&[("start", "node server.js")]);
        assert_eq!(pick_script(&only_start, Purpose::Run).unwrap().0, "start");
    }

    #[test]
    fn 任务只认_test_脚本() {
        let f = facts_with(&[("dev", "vite"), ("test", "vitest run")]);
        assert_eq!(pick_script(&f, Purpose::Task).unwrap().0, "test");
        let no_test = facts_with(&[("dev", "vite")]);
        assert!(pick_script(&no_test, Purpose::Task).is_none());
    }

    #[test]
    fn 主版本号取得出来() {
        assert_eq!(major_of("v20.11.0"), Some(20));
        assert_eq!(major_of("18.19.1"), Some(18));
        assert_eq!(major_of("(读不出来)"), None);
    }

    /// 版本要求里，**有精确号的那条优先**——范围表达式（`>=18`）说不出具体要哪个。
    #[test]
    fn 优先用带精确版本的那条要求() {
        let mut f = facts_with(&[]);
        f.node_pins = vec![
            web::Pin {
                from: "package.json · engines.node".into(),
                spec: ">=18".into(),
            },
            web::Pin {
                from: ".nvmrc".into(),
                spec: "20.11.0".into(),
            },
        ];
        let (spec, from, major) = wanted_major(&f).unwrap();
        assert_eq!(spec, "20.11.0");
        assert_eq!(from, ".nvmrc");
        assert_eq!(major, Some(20));
    }

    #[test]
    fn 常驻脚本才在输出里等监听地址() {
        assert!(LIVE_SCRIPTS.contains(&"dev"));
        assert!(LIVE_SCRIPTS.contains(&"preview"));
        assert!(!LIVE_SCRIPTS.contains(&"build"), "构建是一次性的，跑完就退");
        assert!(!LIVE_SCRIPTS.contains(&"test"));
    }
}
