//! 机器与语言那一摊：`jingwei spec`（这机子什么配置）、`jingwei web`（Node / 前端）。
//!
//! 两个都不需要项目也能跑——"这机子什么配置"本身就是常见问题，
//! 不该逼人先打开一个项目。
//!
//! **为什么从 main.rs 拆出来**：门禁管着"单文件不许超 1000 行、
//! 登记在案的长文件只许降不许升"，main.rs 撞线了。拆的边界按领域走，
//! 不按"大小平均切"——同一个文件里的东西该回答同一类问题。

use super::{c, pad, BOLD, DIM, GREEN, YELLOW};
use crate::protocol::{envelope, print as print_protocol, Coverage, Evidence, State};

pub(crate) fn cmd_spec(json: bool) -> i32 {
    let facts = jingwei_engine::machine::spec_report();
    let notes = jingwei_engine::machine::spec_judgements(&facts);
    jingwei_engine::dlog::i(
        "6000",
        &format!("spec_facts={} judgements={}", facts.len(), notes.len()),
    );
    if json {
        // **协议**（2026-10-21）：机器信息也要能回答"凭什么 / 覆盖多少"。
        // 来源不一致时 `state=no`（**并列摆出来，不选边**）——与"三态不许压扁"是同一条。
        let bad = facts.iter().filter(|f| !f.agree).count();
        let multi = facts.iter().filter(|f| f.sources.len() > 1).count();
        let state = if facts.is_empty() {
            State::Unknown
        } else if bad > 0 {
            State::No
        } else {
            State::Yes
        };
        let summary = if facts.is_empty() {
            "暂无法探知：这台机器的配置一项都没读到｜缺：CPU/内存/磁盘等基础信息｜为什么：几种探法（WMI / 注册表 / 系统接口）都没返回｜怎么补：换普通权限再跑一次，或看 dlog 里的原始错误".to_string()
        } else if bad > 0 {
            format!(
                "读到 {} 项，其中 {bad} 项**来源之间不一致**（并列摆着，不替你选边）；多来源印证 {multi} 项",
                facts.len()
            )
        } else {
            format!("读到 {} 项机器配置；多来源印证 {multi} 项", facts.len())
        };
        let v = envelope(
            state,
            summary,
            vec![
                Evidence::command("jingwei spec", "同一条命令可复跑；每项的来源与分歧都在 facts 里"),
                Evidence::file("dlog", "读不到的原因记在调试日志里"),
            ],
            Coverage::partial(
                facts.len(),
                Some(facts.len()),
                "读到几项就报几项；来源不一致的项不会被压成一个值",
            ),
            serde_json::json!({ "facts": facts, "judgements": notes }),
        );
        print_protocol(&v);
        return 0;
    }
    println!();
    println!("  {}  ·  这台机器", c(BOLD, "jingwei"));
    println!();
    for f in &facts {
        // 多来源印证的标 ✓；单来源不标；有分歧的标 ! 并把两边说法都摆出来
        let mark = if !f.agree {
            c(YELLOW, "!")
        } else if f.sources.len() > 1 {
            c(GREEN, "✓")
        } else {
            " ".into()
        };
        println!("  {} {} {}", mark, pad(&f.label, 12), c(BOLD, &f.value));
        if let Some(n) = &f.note {
            println!("      {}", c(DIM, n));
        }
        if !f.agree {
            for d in &f.detail {
                println!("      {} {}", c(DIM, "来源之间不一致："), d);
            }
        }
    }
    println!();
    println!("  {}", c(DIM, "✓ = 两个独立来源印证过；空着 = 只有一个来源拿得到"));
    if !notes.is_empty() {
        println!();
        println!("  {}", c(BOLD, "从这些数据能看出来的"));
        for n in &notes {
            println!("    · {}", n);
        }
    }
    println!();
    0
}

// ------------------------------------------------------------------ web

/// Node / 前端那一摊：包管理器、依赖装到哪去了、版本要求写在哪。
///
/// **每一条都印出凭据**（`←` 后面那段）。界面上的悬停提示与这里是同一份数据——
/// 命令行不给凭据的话，"它凭什么这么说"就没法自己复核，那这份输出就只能信、不能查。
pub(crate) fn cmd_web(root: &std::path::Path, json: bool) -> i32 {
    let Some(w) = jingwei_engine::web::probe_web(root) else {
        if json {
            println!("{}", serde_json::json!({ "web": null }));
        } else {
            println!();
            println!("  {} 不是 Node 项目（这个目录里没有 package.json）", c(DIM, "·"));
            println!("  Node 项目的判据只有一条：根目录下有 package.json。");
        }
        return 0;
    };
    jingwei_engine::dlog::i(
        "6100",
        &format!(
            "web_probe pm={:?} link={} pins={}",
            w.package_manager,
            w.link_kind,
            w.node_pins.len()
        ),
    );
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "web": w })).unwrap_or_default()
        );
        return 0;
    }
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    println!();
    println!("  {}  ·  {}  ·  Node / 前端", c(BOLD, "jingwei"), name);
    println!();
    let pm = match (&w.package_manager, &w.pm_pinned) {
        (Some(p), Some(v)) => format!("{p} {v}"),
        (Some(p), None) => p.clone(),
        (None, _) => "还看不出来".to_string(),
    };
    println!(
        "  {} {}   {}",
        pad("包管理器", 10),
        c(BOLD, &pm),
        c(DIM, &format!("← {}", w.pm_from.clone().unwrap_or_default()))
    );
    println!(
        "  {} {}   {}",
        pad("依赖位置", 10),
        w.link_kind,
        c(DIM, &format!("← {}", w.link_from))
    );
    for p in &w.node_pins {
        println!(
            "  {} {}   {}",
            pad("Node 要求", 10),
            p.spec,
            c(DIM, &format!("← {}", p.from))
        );
    }
    if let Some(ws) = &w.workspaces {
        println!("  {} {}   {}", pad("工作区", 10), ws, c(DIM, "← monorepo"));
    }
    if !w.frameworks.is_empty() {
        println!("  {} {}", pad("框架", 10), w.frameworks.join(" / "));
    }
    if w.typescript {
        println!("  {} 有 tsconfig", pad("TypeScript", 10));
    }
    for (i, (k, v)) in w.scripts.iter().enumerate() {
        let label = if i == 0 { "脚本" } else { "" };
        println!("  {} {} {}", pad(label, 10), pad(k, 10), c(DIM, v));
    }
    if !w.notes.is_empty() {
        println!();
        for n in &w.notes {
            println!("  {} {}", c(YELLOW, "!"), n);
        }
    }
    println!();
    println!(
        "  {}",
        c(DIM, "依赖状态看 jingwei doctor；这里解释的是「为什么它是那个状态」。")
    );
    0
}

