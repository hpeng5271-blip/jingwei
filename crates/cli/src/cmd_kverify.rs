//! `jingwei knowledge verify`：**让判据真的跑起来**（2026-10-20，pro 任务卡 I）。
//!
//! 为什么要有它：条目的 `check` 一直是散文——"严格校验"只到"写了判据"，没到"判据会被执行"。
//! 这一版只做最小闭环：支持 `check-run: <命令>`（期望 exit 0）与 `check-exit: <N>`（期望非 0），
//! 外加第二类判据——**`check` 里引用的测试名必须真的存在**。
//!
//! 三态（本命令的全部诚实性所在）：
//! - **P 通过**：命令跑完，退出码等于期望；
//! - **F 没过**：命令跑完，退出码不等于期望（**真问题**，exit=1）；
//! - **U 不可执行**：没写 `check-run`、命令不在白名单、环境跑不起来——**不算错**，
//!   因为"可执行判据"是**增量**：没写的人不该被判红（否则等于强制所有人立刻改造）。
//!
//! 安全边界（pro 的翻车点）：只跑**构建/测试类**命令（白名单前缀），跑前要求工作区干净——
//! verify 绝不能变成"点一下就跑 rm/mv/写文档"的危险操作。

use std::path::Path;
use std::process::Command;

use crate::cmd_knowledge::{load_entries, Entry};

#[derive(Debug, Default)]
pub struct Report {
    pub pass: Vec<String>,
    pub fail: Vec<String>,
    pub unrun: Vec<String>,
    /// 引用了不存在的测试名（算 F）。
    pub bad_tests: Vec<String>,
}

/// 只允许跑这些前缀的命令：都是"读 + 构建 + 测试"，不会有写/删副作用。
const RUN_WHITELIST: [&str; 5] = [
    "cargo ",
    "node ",
    "python ",
    "powershell -NoProfile -ExecutionPolicy Bypass -File tools\\",
    "powershell -noprofile -executionpolicy bypass -file tools\\",
];

fn allowed(cmd: &str) -> bool {
    let c = cmd.trim().to_ascii_lowercase();
    RUN_WHITELIST.iter().any(|p| c.starts_with(&p.to_ascii_lowercase()))
}

/// 工作区状态（一行一条）。用来判断"命令有没有动过工作区"。
fn status(root: &Path) -> Vec<String> {
    Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(root)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 工作区干净吗（只有干净才跑命令：脏工作区里跑测试，结论会被别人的改动污染）。
fn workspace_clean(root: &Path) -> bool {
    let out = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(root)
        .output();
    match out {
        Ok(o) => o.status.success() && o.stdout.iter().all(|b| b.is_ascii_whitespace()),
        Err(_) => false, // 连 git 都没有：判不了干净 → 一律不跑
    }
}

/// 跑一条命令，返回 (退出码, 是否算"环境跑不起来")。
///
/// **不经 shell**（直接 spawn 程序 + 参数）：一是没有注入面，二是中文参数能原样传过去。
/// 实测过走 `cmd /C` 的后果：`cargo test ... 没索引时_positions_…` 里的中文被 cmd 的代码页搞坏，
/// 命令直接 exit 101（"没有这个测试目标"），而同一个命令在终端里手敲是 exit 0——假红。
fn run_cmd(root: &Path, cmd: &str) -> (i32, bool, String) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let Some((prog, args)) = parts.split_first() else {
        return (-1, true, String::new());
    };
    match Command::new(prog).args(args).current_dir(root).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            // 失败时留最后几行 stderr：诊断"是判据没过还是命令本身错了"靠它
            let tail = err
                .lines()
                .filter(|l| !l.trim().is_empty())
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ");
            // -1（被信号杀）算"跑不起来"，不是"没过"
            (code, code == -1, tail)
        }
        Err(_) => (-1, true, String::new()), // 程序不在 PATH / 权限问题 → 环境错
    }
}

/// 所有能跑的测试名（`cargo test -- --list` 的路径末段）。
/// 跑不起来就返回 None —— 那时"测试名校验"只能算 U，不许算 F。
pub fn test_names(root: &Path) -> Option<Vec<String>> {
    let out = Command::new("cargo")
        .args(["test", "--workspace", "--all-targets", "--", "--list"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut names = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_suffix(": test") else {
            continue;
        };
        // 形态有 `target: test` 与 `mod::path::测试名: test`，两种都收
        names.push(rest.trim().to_string());
    }
    Some(names)
}

/// `check` 文本里**按约定形式**引用的测试名：反引号里的下划线标识符、以及 `--test <名>`。
/// 只认约定标记，不猜自由文本（pro 的要求）。
pub fn cited_test_names(e: &Entry) -> Vec<String> {
    let mut out = Vec::new();
    let check = e.fields.get("check").cloned().unwrap_or_default();
    for m in check.split('`').skip(1).step_by(2) {
        let t = m.trim();
        // Rust 测试函数的常见形态：全小写 + 下划线、长度够长（短词如 `source` 不当测试名）
        if t.len() >= 8
            && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && t.contains('_')
        {
            out.push(t.to_string());
        }
    }
    let mut it = check.split_whitespace().peekable();
    while let Some(w) = it.next() {
        if w == "--test" {
            if let Some(n) = it.peek() {
                let n = n.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                if !n.is_empty() {
                    out.push(n.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn run(root: &Path, allow_dirty: bool) -> (Report, bool) {
    let mut r = Report::default();
    if !allow_dirty && !workspace_clean(root) {
        return (r, false); // 调用方据此说"工作区不干净，先提交"
    }
    let names = test_names(root);
    let before = status(root);
    for e in load_entries(root) {
        // 第二类判据：引用的测试名必须真实存在（这是**存在性**判定，与环境无关）
        let mut missing = Vec::new();
        for t in cited_test_names(&e) {
            let ok = names
                .as_ref()
                .map(|all| all.iter().any(|x| x == &t || x.ends_with(&format!("::{t}"))))
                .unwrap_or(true); // 拿不到名单 → 判不了，不冤枉条目
            if !ok {
                missing.push(t);
            }
        }
        if !missing.is_empty() {
            r.bad_tests
                .push(format!("{} 引用的测试名不存在：{}", e.file, missing.join(" / ")));
            r.fail.push(e.file.clone());
            continue;
        }

        let Some(cmd) = e.fields.get("check-run").cloned().filter(|c| !c.trim().is_empty()) else {
            r.unrun.push(format!("{} 没有 check-run（不计错）", e.file));
            continue;
        };
        if !allowed(&cmd) {
            r.unrun.push(format!("{} 的命令不在白名单（只跑构建/测试类）：{cmd}", e.file));
            continue;
        }
        let want: i32 = e
            .fields
            .get("check-exit")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        let (code, env_fail, tail) = run_cmd(root, &cmd);
        if env_fail {
            r.unrun
                .push(format!("{} 跑不起来（环境）：{cmd}（exit={code}）", e.file));
            continue;
        }
        // **副作用检测**：跑完工作区变了 = 这条命令动了不该动的东西。比"禁止脏工作区"更硬。
        let after = status(root);
        if after != before {
            r.fail.push(format!(
                "{} 的命令改动了工作区（副作用）：`{cmd}`",
                e.file
            ));
            continue;
        }
        if code == want {
            r.pass
                .push(format!("{} `{cmd}` → exit={code}", e.file));
        } else {
            let hint = if tail.is_empty() {
                String::new()
            } else if tail.contains("拒绝访问") && tail.contains("jingwei.exe") {
                // 实测踩过：用 `target\debug\jingwei.exe` 跑 verify 时，cargo 想重编并替换这个 exe，
                // Windows 拒绝删正在运行的文件 → 命令 exit 101，看起来像"判据没过"，其实是自指。
                format!(
                    "（这是自指：cargo 想重编你正在用的这个 exe。改用装好的 jingwei 再跑。stderr：{tail}）"
                )
            } else {
                format!("（stderr 末行：{tail}）")
            };
            r.fail.push(format!(
                "{} 期望 exit={want}，实际 {code}：`{cmd}`{hint}",
                e.file
            ));
        }
    }
    (r, true)
}

/// 快照路径：`%LOCALAPPDATA%\jingwei\verify-last.json`。
///
/// 为什么落本地而不是仓库：这是**机器状态**（跑没跑、跑出什么），不是项目事实；
/// 进 git 会让每次手动跑都产生脏提交。但报告要引用**可复现的数字**，所以必须留一份快照。
pub fn snapshot_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("jingwei").join("verify-last.json")
}

fn git_head(root: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// 写快照：`{ts, head, dirty, pass, fail, unrun}`。写不了也不影响判定（只是少一份证据）。
fn write_snapshot(root: &Path, r: &Report, dirty: bool) {
    let v = serde_json::json!({
        "ts": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64).unwrap_or(0),
        "head": git_head(root),
        "dirty": dirty,
        "pass": r.pass,
        "fail": r.fail,
        "unrun": r.unrun,
    });
    let p = snapshot_path();
    if let Some(dir) = p.parent() {
        // 写不进去也不影响判定：这只是一份"最近一次跑过"的证据，不是结论本身。
        _ = std::fs::create_dir_all(dir);
    }
    _ = std::fs::write(&p, serde_json::to_string_pretty(&v).unwrap_or_default());
}

pub fn cmd_knowledge_verify(root: &Path, json: bool, allow_dirty: bool) -> i32 {
    let (r, ran) = run(root, allow_dirty);
    if !ran {
        println!("查不了：工作区不干净（`git status` 有改动）——先提交再跑；开发时要跑可加 --allow-dirty（会检测命令有没有动工作区）");
        return 2;
    }
    // **落盘**（pro 收口条件）：报告里的 6/0/10 必须能被接手者核对，而不是口头值。
    write_snapshot(root, &r, !workspace_clean(root));
    if json {
        println!(
            "{}",
            serde_json::json!({"pass": r.pass, "fail": r.fail, "unrun": r.unrun})
        );
    } else {
        let total = r.pass.len() + r.fail.len() + r.unrun.len();
        println!(
            "判据执行：通过 {} / 没过 {} / 不可执行 {}（共 {total} 条）",
            r.pass.len(),
            r.fail.len(),
            r.unrun.len()
        );
        for p in &r.pass {
            println!("  ✓ {p}");
        }
        for f in &r.fail {
            println!("  × {f}");
        }
        for u in &r.unrun {
            println!("  · {u}");
        }
        if r.pass.is_empty() && r.fail.is_empty() {
            println!("  · 还没有条目写 check-run（可执行判据是增量，不写不算错）");
        }
        println!("  （快照已写：{}）", snapshot_path().display());
    }
    if r.fail.is_empty() {
        0
    } else {
        1
    }
}
