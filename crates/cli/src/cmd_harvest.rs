//! `jingwei harvest <文件>`：**双头漏斗的第一头**——从散文里抽「候选条目」进待确认区（2026-10-20，pro 任务卡 D）。
//!
//! 规矩（pro 定的两条防空壳）：
//! 1. **逐字摘录**：候选的 `claim` 必须是源文件那一行的**原文子串**，`source` 写成 `路径:行号`，
//!    事后任何一次 `knowledge check` 都会回去 grep 那一行——对不上就红（来源失效 / 被改写）。
//! 2. **候选不进库**：写到 `docs/knowledge/_inbox/`，**不计入**漏斗的"采集"；
//!    只有人跑 `jingwei knowledge add <id>` 并补上 `check`，才移进 `docs/knowledge/`。
//!
//! 抽取只认"已经被人整理过的行"（`- ` 列表项 / `|` 表格行），不做全文 NLP——
//! 自动总结会产出"看着对、原文没有"的句子，那正是这个项目最不许出现的东西。

use std::path::{Path, PathBuf};

/// 一条候选。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub line: u32,
    pub text: String,
}

/// 判断等级：按来源文件的类型推，**推不出来就写 二手**（宁低勿高）。
pub fn infer_level(path: &str) -> &'static str {
    let p = path.replace('\\', "/").to_lowercase();
    if p.contains("transcripts/") || p.contains("转写") {
        "转写"
    } else if p.contains("一手来源") {
        "一手"
    } else {
        "二手"
    }
}

/// 从正文里抽候选。
///
/// **默认只抽 `- ` 列表项**。表格行要显式 `--tables` 才抽——2026-10-20 实测：对 `一手来源.md`
/// （一张"来源 | 借什么 | 读哪"的表）抽出来的 10 条候选全是**来源名/链接**，不是主张。
/// 这就是 pro 说的"看起来抽了其实全是噪音"：表格第一列在有些文档里是主张，有些里是索引键，
/// 猜不出来的时候**宁可不抽**，让人用 `--tables` 明确要求。
pub fn extract(text: &str, max: usize, tables: bool) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        if out.len() >= max {
            break;
        }
        let line = raw.trim();
        if line.starts_with('|') {
            if !tables {
                continue;
            }
            // 表格：跳过 `|---|` 与表头（表头常含"来源""借什么"这类列名）
            if line.contains("---") || line.contains("要求 |") || line.contains("| 来源 |") {
                continue;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(|s| s.trim()).collect();
            if cells.len() < 2 {
                continue;
            }
            // 取第一列（主张/要点），第二列当补充不参与 claim
            let first = cells[0].trim_matches('*').trim();
            if first.chars().count() < 12 || first.chars().count() > 200 {
                continue;
            }
            out.push(Candidate {
                line: (i + 1) as u32,
                text: first.to_string(),
            });
            continue;
        }
        let Some(body) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else {
            continue;
        };
        let body = body.trim();
        let n = body.chars().count();
        if !(12..=200).contains(&n) {
            continue;
        }
        out.push(Candidate {
            line: (i + 1) as u32,
            text: body.to_string(),
        });
    }
    out
}

/// 候选的文件名：`_inbox/<源文件名去扩展>-L<行号>.md`（同一个源可以抽多次，按行号区分）。
///
/// **名字不能以 `_` 开头**：`docs/knowledge/` 顶层的 `_*` 与 `README.md` 被约定成"草稿/说明"，
/// 不参与严筛；候选要是带了 `_` 前缀，`knowledge add` 把它移进库之后会被当成草稿**静默跳过**——
/// 入库成功、漏斗数字不动，正是最难发现的那种错（2026-10-20 实测踩到）。
pub fn candidate_name(src: &Path, line: u32) -> String {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "src".into());
    let clean: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let clean = clean.trim_start_matches(['_', '-']).to_string();
    let clean = if clean.is_empty() { "src".to_string() } else { clean };
    format!("{clean}-L{line}.md")
}

/// `jingwei harvest --from-git [N]`：**从提交记录里发现新知识**（2026-10-21）。
///
/// 为什么是提交记录：这个仓库的提交信息按规矩写 `## 为什么` / `## 证据`，"为什么"正是
/// 知识衰减最快的那部分——它只在人的脑子里和 commit 里，不在代码里。
///
/// 规矩与文档采集一致：`claim` 用**提交标题原样**（逐字，不改写）、`source: git:<短 sha>`、
/// `check: 待验`（人补了才准 `add`）、写进 `_inbox/` **不计入采集**；同一 sha 抽过就跳过。
fn harvest_git(root: &Path, n: usize, json: bool) -> i32 {
    // `\x1e` 分隔提交、`\x1f` 分隔字段（标题里可能有空格/中文，不能用空格分列）
    let out = std::process::Command::new("git")
        .args([
            "log",
            &format!("-n{n}"),
            "--format=%h%x1f%s%x1f%b%x1e",
            "--no-merges",
        ])
        .current_dir(root)
        .output();
    let Ok(o) = out else {
        println!("查不了：这台机器上没有 git");
        return 0;
    };
    if !o.status.success() {
        println!("查不了：git log 跑不起来（这个目录是 git 仓库吗？）");
        return 0;
    }
    let text = String::from_utf8_lossy(&o.stdout).to_string();
    let inbox = root.join("docs").join("knowledge").join("_inbox");
    if let Err(e) = std::fs::create_dir_all(&inbox) {
        eprintln!("建不了 {}：{e}", inbox.display());
        return 2;
    }
    // 已抽过哪些 sha：扫 _inbox 与正式条目里的 `source: git:<sha>`
    let mut seen = String::new();
    for dir in [inbox.clone(), root.join("docs").join("knowledge")] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.filter_map(|e| e.ok()) {
                if let Ok(t) = std::fs::read_to_string(e.path()) {
                    seen.push_str(&t);
                }
            }
        }
    }

    let mut written = 0;
    let mut skipped = 0;
    let mut total = 0;
    for rec in text.split('\u{1e}') {
        let rec = rec.trim();
        if rec.is_empty() {
            continue;
        }
        let mut it = rec.split('\u{1f}');
        let (Some(sha), Some(subject)) = (it.next(), it.next()) else {
            continue;
        };
        let body = it.next().unwrap_or("").trim();
        total += 1;
        let sha = sha.trim();
        let subject = subject.trim();
        // 标题太短/太长都不是一句主张；`Merge` 之类在 --no-merges 后也不会出现
        if subject.chars().count() < 8 || subject.chars().count() > 120 {
            skipped += 1;
            continue;
        }
        let marker = format!("git:{sha}");
        if seen.contains(&marker) {
            skipped += 1;
            continue;
        }
        // 文件名：git-<sha>-<标题前 16 字>。**空格换成 `-`**——文件名带空格就没法在命令行里
        // 直接 `knowledge add <名字>`（实测踩到）；Windows 还不许结尾是空格/点。
        let slug: String = subject
            .chars()
            .take(16)
            .map(|c| {
                if c.is_whitespace() || "\\/:*?\"<>|".contains(c) {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        let slug = slug.trim_matches(|c: char| c == '-' || c == '.');
        let name = format!("git-{sha}-{slug}.md");
        let body_text = if body.is_empty() {
            String::new()
        } else {
            format!("\n提交正文：\n{body}\n")
        };
        let content = format!(
            "---\nclaim: {subject}\nsource: git:{sha}\nlevel: 一手\ncheck: 待验\ncandidate: true\n---\n\n来自提交 `{sha}`（标题原样，未改写）。{body_text}"
        );
        if std::fs::write(inbox.join(&name), content).is_ok() {
            written += 1;
        }
    }
    if json {
        println!(
            "{}",
            serde_json::json!({"source": "git log", "scanned": total, "candidates": written, "skipped": skipped})
        );
    } else {
        println!("看了最近 {total} 个提交，新抽 {written} 条候选（跳过 {skipped}：太短/太长/已抽过）→ docs/knowledge/_inbox/");
        if written > 0 {
            println!("确认方式：把 check 改成真判据后跑 `jingwei knowledge add <条目文件名>`");
        }
    }
    0
}

pub fn cmd_harvest(root: &Path, rest: &[String], json: bool) -> i32 {
    // `--git [N]`：从提交记录里发现新知识（第 N 默认为 8）。
    if let Some(pos) = rest.iter().position(|a| a == "--from-git") {
        let n = rest
            .get(pos + 1)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(8)
            .clamp(1, 50);
        return harvest_git(root, n, json);
    }
    let Some(arg) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("用法：jingwei harvest <docs/research 下的文件>   （抽候选条目到 docs/knowledge/_inbox/）");
        eprintln!("      jingwei harvest --from-git [N]                （从最近 N 个提交里发现候选，默认 8）");
        return 2;
    };
    let src = PathBuf::from(arg);
    let abs = if src.is_absolute() { src.clone() } else { root.join(&src) };
    let Ok(text) = std::fs::read_to_string(&abs) else {
        eprintln!("读不了 {arg}");
        return 2;
    };
    let rel = abs
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| arg.clone());

    let cands = extract(&text, 40, rest.iter().any(|a| a == "--tables"));
    if cands.is_empty() {
        println!(
            "没有可抽的候选（默认只认 `- ` 列表项；表格要加 --tables，长度 12–200）"
        );
        return 0;
    }

    let inbox = root.join("docs").join("knowledge").join("_inbox");
    if let Err(e) = std::fs::create_dir_all(&inbox) {
        eprintln!("建不了 {}：{e}", inbox.display());
        return 2;
    }
    let level = infer_level(&rel);
    let mut written = 0;
    for c in &cands {
        let name = candidate_name(&abs, c.line);
        let body = format!(
            "---\nclaim: {}\nsource: {}:{}\nlevel: {}\ncheck: 待验\ncandidate: true\n---\n\n原文（{}:{}）：\n{}\n",
            c.text, rel, c.line, level, rel, c.line, c.text
        );
        if std::fs::write(inbox.join(&name), body).is_ok() {
            written += 1;
        }
    }
    if json {
        println!(
            "{}",
            serde_json::json!({"source": rel, "candidates": written, "level": level, "inbox": "docs/knowledge/_inbox"})
        );
    } else {
        println!("本次候选 {written} 条 → docs/knowledge/_inbox/（等级推定：{level}）");
        println!("确认方式：补好 check 字段后跑 `jingwei knowledge add <条目文件名>`");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 只抽列表与表格行_且长度受限() {
        let text = "开头不是列表\n- 短\n- 这一条够长了，应该被抽出来当候选条目。\n| 要点 | 说明 |\n|---|---|\n| 这也是够长的表格主张，应该被抽出来 | 补充 |\n";
        let c = extract(text, 10, true);
        let texts: Vec<&str> = c.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(texts[0].contains("应该被抽出来当候选条目"));
        assert!(texts[1].contains("够长的表格主张"));
        assert_eq!(c[0].line, 3);
        assert_eq!(c[1].line, 6);
    }

    #[test]
    fn 默认不抽表格行_因为表格第一列常常不是主张() {
        // 一手来源.md 那张表的第一列是"来源名/链接"：抽出来全是噪音（2026-10-20 实测 10/10）
        let text = "| [SQLite WAL](https://www.sqlite.org/wal.html) `200` | 追加日志 + checkpoint | WAL 原理 |\n";
        assert!(extract(text, 10, false).is_empty(), "默认不该抽表格");
        assert_eq!(extract(text, 10, true).len(), 1, "--tables 才抽");
    }

    #[test]
    fn 候选名不能以_开头() {
        // `_tmp-harvest.md` 是草稿前缀；候选名要去掉它，否则入库后被当成草稿静默跳过
        let n = candidate_name(std::path::Path::new("docs/research/_tmp-harvest.md"), 3);
        assert_eq!(n, "tmp-harvest-L3.md");
        assert!(!n.starts_with('_'));
    }

    #[test]
    fn 等级按来源类型推定() {
        assert_eq!(infer_level("docs/research/transcripts/BV1x.txt"), "转写");
        assert_eq!(infer_level("docs/research/一手来源.md"), "一手");
        assert_eq!(infer_level("docs/research/原子能-转写要点.md"), "转写");
        assert_eq!(infer_level("docs/research/观点地图.md"), "二手");
    }
}

