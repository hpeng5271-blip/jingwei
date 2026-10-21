//! `jingwei net`：看清"网络现在允许什么"，并给**人手放行**一个入口（2026-10-22）。
//!
//! 为什么单开一个命令而不是只靠环境变量：界面也要能放行，而界面已经统一通过
//! `cli_json` 调 CLI——**允许表只能有一处实现**（`sandbox_allow`），界面别自己写一份 JSON。
//!
//! 两个时长，口径不变：**本次会话**（`JINGWEI_ALLOW_NET` 前缀，不落盘）/ **24 小时**（落盘，到点失效）。
//! 没有"永远允许"。

use super::{c, BOLD, DIM, GREEN, RED, YELLOW};
use crate::sandbox_allow;
use crate::protocol::{envelope, print as print_protocol, Coverage, Evidence, State};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn until_text(secs: i64) -> String {
    if secs <= 0 {
        return "已过期".to_string();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("还剩 {h} 小时 {m} 分")
    } else {
        format!("还剩 {m} 分")
    }
}

pub(crate) fn cmd_net(rest: &[String], json: bool) -> i32 {
    let sub = rest.first().map(|s| s.as_str()).unwrap_or("status");
    let path = sandbox_allow::allow_path();
    match sub {
        "status" => {
            let granted = sandbox_allow::read_allows(&path)
                .into_iter()
                .filter(|(w, _)| w.starts_with("net:"))
                .collect::<Vec<_>>();
            if json {
                let rows: Vec<serde_json::Value> = granted
                    .iter()
                    .map(|(w, until)| {
                        serde_json::json!({
                            "host": w.trim_start_matches("net:"),
                            "until": until,
                            "left": (until - now()).max(0),
                        })
                    })
                    .collect();
                let v = envelope(
                    State::Yes,
                    format!(
                        "白名单 {} 个包仓库；当前额外允许 {} 个（最多 24 小时）",
                        sandbox_allow::REGISTRY_HOSTS.len(),
                        rows.len()
                    ),
                    vec![
                        Evidence::file(&path.display().to_string(), "允许表：坏文件当没有"),
                        Evidence::command("jingwei net allow <域名>", "放行一个域名（24 小时）"),
                    ],
                    Coverage::full(rows.len(), "列出的是尚未过期的那些"),
                    serde_json::json!({ "whitelist": sandbox_allow::REGISTRY_HOSTS, "allowed": rows }),
                );
                print_protocol(&v);
                return 0;
            }
            println!();
            println!("  {} 网络默认拒绝，只放行包仓库。", c(BOLD, "net"));
            println!("  {}", c(DIM, "白名单："));
            for h in sandbox_allow::REGISTRY_HOSTS {
                println!("    {}", h);
            }
            println!();
            if granted.is_empty() {
                println!("  {}", c(DIM, "没有额外的临时允许。"));
            } else {
                println!("  {}", c(BOLD, "临时允许（最多 24 小时）："));
                for (w, until) in &granted {
                    println!(
                        "    {}  {}",
                        w.trim_start_matches("net:"),
                        c(DIM, &until_text(*until - now()))
                    );
                }
            }
            println!();
            println!(
                "  {}",
                c(DIM, "放行：jingwei net allow <域名>    收回：jingwei net deny <域名>")
            );
            println!();
            0
        }
        "allow" => {
            let Some(raw) = rest.get(1) else {
                eprintln!("用法：jingwei net allow <域名>              （默认 24 小时）");
                eprintln!("      jingwei net allow <域名> --hours 2    （只放行 2 小时）");
                return 2;
            };
            let Some(host) = sandbox_allow::host_of(raw) else {
                // 认不出就不放行——把 "3.12" 这种当域名写进允许表，比拒绝更糟。
                println!(
                    "  {} 暂无法探知：`{raw}` 不像一个域名｜缺：形如 pypi.org 的主机名｜为什么：没有点或顶级域不是字母｜怎么补：把主机名写全（协议和路径会自动剥掉）",
                    c(RED, "✗")
                );
                return 2;
            };
            let hours = rest
                .iter()
                .position(|s| s == "--hours")
                .and_then(|i| rest.get(i + 1))
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|h| *h > 0 && *h <= 24)
                .unwrap_or(24);
            let ttl = hours * 3600;
            let mut rows = sandbox_allow::read_allows(&path)
                .into_iter()
                .filter(|(_, until)| *until > now())
                .filter(|(w, _)| w != &format!("net:{host}"))
                .map(|(w, u)| serde_json::json!({"what": w, "until": u}))
                .collect::<Vec<_>>();
            rows.push(serde_json::json!({"what": format!("net:{host}"), "until": now() + ttl}));
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&rows).unwrap_or_default()) {
                println!("  {} 写允许表失败：{e}", c(RED, "✗"));
                return 1;
            }
            println!(
                "\n  {} 已允许 {}（{}，到点自动失效）\n",
                c(GREEN, "✓"),
                c(BOLD, &host),
                until_text(ttl)
            );
            0
        }
        "deny" => {
            let Some(raw) = rest.get(1) else {
                eprintln!("用法：jingwei net deny <域名>");
                return 2;
            };
            let host = sandbox_allow::host_of(raw).unwrap_or_else(|| raw.to_ascii_lowercase());
            if sandbox_allow::remove_allow(&path, &format!("net:{host}"), now()) {
                println!("\n  {} 已收回 {} 的临时允许\n", c(GREEN, "✓"), host);
                0
            } else {
                println!("\n  {} {} 本来就没有临时允许\n", c(YELLOW, "·"), host);
                0
            }
        }
        other => {
            eprintln!("net 的子命令只有 status / allow / deny（不认识 `{other}`）");
            2
        }
    }
}
