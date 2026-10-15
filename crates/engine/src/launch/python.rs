//! Python 项目的**前四个阶段**：解释器、契约、依赖、启动命令。
//!
//! ## 为什么这一层要按语言分家
//!
//! 六个阶段里，后两个（拉起进程、等待就绪）跟语言毫无关系；
//! 前四个问的是同一组问题，但答案的**来源完全不同**：
//!
//! | 阶段 | Python | Node |
//! |---|---|---|
//! | 1 检查运行环境 | 找一个能用的解释器（虚拟环境 / 隔离模式） | `node` 在不在、项目用哪个包管理器 |
//! | 2 校验项目契约 | 契约里的 run 命令 | 契约，或者 `package.json` 的 scripts |
//! | 3 解析依赖声明 | pip 清单 vs 实际安装 | `package.json` 声明 vs 实际安装（PnP 下**确认不了**） |
//! | 4 准备运行时 | 解释器绝对路径 | 运行时 + 包管理器 + **将执行哪条脚本、凭什么选它** |
//!
//! 混在一个函数里的话，Node 项目打开就报"没有可用的 Python"——
//! 那不是简洁，是答非所问。

use super::{split_command, Event, LaunchOptions, Prepared, StageStatus, TOTAL_STAGES};
use crate::detect::probe;
use crate::understand::contract::Contract;
use anyhow::{anyhow, Result};
use std::path::Path;
use std::time::Instant;

pub(crate) fn prepare(
    root: &Path,
    opts: &LaunchOptions,
    contract: &Option<Contract>,
    contract_err: &Option<String>,
    cb: &mut dyn FnMut(Event),
) -> Result<Prepared> {
    // ---- 1 检查运行环境 ----
    let t = Instant::now();
    let picked = probe::first_working(root, contract.as_ref(), 5);
    let py_path = picked.as_ref().map(|(p, _)| p.clone());
    let pyf = picked.map(|(_, f)| f).unwrap_or_default();
    // 契约明确绑了一个解释器，而它不在了——工具必须说出来。
    // 悄悄换一个更省事，但那等于把用户写下的决定吞掉：
    // 他以为自己在用 A，实际跑的是 B，这种误会最难查。
    let bound_gone = contract
        .as_ref()
        .and_then(|c| c.runtime.python_exe.as_ref())
        .filter(|b| !Path::new(b).is_file())
        .cloned();
    let isolated = pyf.isolated || !pyf.pth_files.is_empty();
    let (s1, d1) = if !pyf.found {
        (StageStatus::Fail, "没找到可用的 Python 解释器".to_string())
    } else if let Some(gone) = &bound_gone {
        (
            StageStatus::Note,
            format!(
                "契约绑定的解释器不在了（{}），临时改用 {}",
                gone, pyf.exe
            ),
        )
    } else if isolated {
        (
            StageStatus::Note,
            format!("Python {} · {} · 嵌入式（隔离模式）", pyf.version, pyf.exe),
        )
    } else {
        (
            StageStatus::Ok,
            format!("Python {} · {}", pyf.version, pyf.exe),
        )
    };
    cb(Event::Stage {
        index: 1,
        total: TOTAL_STAGES,
        title: "检查运行环境".into(),
        status: s1,
        detail: d1,
        ms: t.elapsed().as_millis(),
    });

    // ---- 2 校验项目契约 ----
    let t = Instant::now();
    let (s2, d2) = match (&contract, &contract_err) {
        (Some(c), _) => (
            StageStatus::Ok,
            format!(
                "{} · run: {}",
                c.name.clone().unwrap_or_else(|| "（未命名）".into()),
                c.run_command().unwrap_or("（未声明）")
            ),
        ),
        (None, Some(e)) => (StageStatus::Fail, format!("解析失败：{}", e)),
        (None, None) => (
            StageStatus::Note,
            "没有 env.contract.json（用 jingwei init 生成）".into(),
        ),
    };
    cb(Event::Stage {
        index: 2,
        total: TOTAL_STAGES,
        title: "校验项目契约".into(),
        status: s2,
        detail: d2,
        ms: t.elapsed().as_millis(),
    });

    // ---- 3 解析依赖声明 ----
    let t = Instant::now();
    let wanted = Contract::wanted_deps(root, contract.as_ref());
    let installed_norm: Vec<String> = pyf
        .installed
        .iter()
        .map(|s| crate::understand::contract::norm_name(s.split("==").next().unwrap_or("")))
        .collect();
    let missing: Vec<String> = wanted
        .iter()
        .filter(|d| !installed_norm.contains(&crate::understand::contract::norm_name(d)))
        .cloned()
        .collect();
    cb(Event::Stage {
        index: 3,
        total: TOTAL_STAGES,
        title: format!("解析依赖声明（{} 项）", wanted.len()),
        status: if missing.is_empty() {
            StageStatus::Ok
        } else {
            StageStatus::Note
        },
        detail: if missing.is_empty() {
            "都已安装".into()
        } else {
            format!("缺 {} 项：{}", missing.len(), missing.join(", "))
        },
        ms: t.elapsed().as_millis(),
    });

    // ---- 4 准备解释器 ----
    let t = Instant::now();
    let exe = py_path.clone().ok_or_else(|| anyhow!("没有可用的 Python，无法启动"))?;
    cb(Event::Stage {
        index: 4,
        total: TOTAL_STAGES,
        title: "准备解释器".into(),
        status: StageStatus::Ok,
        detail: format!("{}（绝对路径，不需要 activate）", exe.display()),
        ms: t.elapsed().as_millis(),
    });

    // ---- 5 启动进程 ----
    let run_cmd = opts
        .run_override
        .clone()
        .or_else(|| contract.as_ref().and_then(|c| c.run_command().map(String::from)))
        .ok_or_else(|| anyhow!("契约里没有声明 run 命令（用 jingwei init 生成一份）"))?;
    let (program, args) = split_command(&run_cmd, &exe);

    Ok(Prepared {
        program,
        args,
        // 这几个环境变量是 Python 特有的：UTF-8 与不缓冲。
        // 少了它们，中文路径上的输出会乱码、进度会攒到进程退出才出来。
        env: vec![
            ("PYTHONUTF8".into(), "1".into()),
            ("PYTHONIOENCODING".into(), "utf-8".into()),
            ("PYTHONUNBUFFERED".into(), "1".into()),
            ("PYTHONPATH".into(), root.to_string_lossy().to_string()),
        ],
        run_cmd,
        // Python 脚本跑不完通常是**卡住了**，所以到点还没就绪该报失败。
        // （Node 的 dev server 是另一回事：它本来就不退，见 `node.rs`。）
        alive_unready: StageStatus::Fail,
        // 只有常驻服务才有"输出里出现监听地址"这回事，脚本项目不需要。
        url_watch: false,
        ready_rule: "契约里的 ready_when（没写就按「退出码 0」）".to_string(),
    })
}
