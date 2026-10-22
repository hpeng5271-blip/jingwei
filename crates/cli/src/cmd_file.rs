//! `jingwei file <路径>` — 单文件聚焦分析（D-20261022-03 §四）。
//!
//! 为什么单独立一条命令：问"**这一个文件**怎么样"以前要拼 `explain` + `impact`，
//! 而文件是最小体量的逻辑与信息实验体——它该一次问清、一次答完。
//!
//! 五件事（都在现有能力内，没有新增采集）：
//! 1. 是什么：语言 / 行数 / 字节 / 图里的符号数
//! 2. 连着什么：这个文件依赖了谁（`DependsOn` 出边）
//! 3. 有没有问题：读不到就说查不了（诊断入口在 `check`，这里不重跑）
//! 4. 最近动过：有 `.git` 用 git，没有就用文件 mtime（basis 写清楚）
//! 5. 影响谁：`jingwei impact --file` 的职责，这里只给入口提示，不重复算
//!
//! **三态**：文件不存在 → `no`；文件在但图谱没建 → `unknown`（三字段齐全）；否则 `yes`。

use crate::protocol::{envelope, print, Coverage, Evidence, State};
use jingwei_engine::graph::store::{Dir, Store};
use std::path::{Path, PathBuf};

/// 语言：只按扩展名给**最保守**的答案；认不出就说"认不出"。
fn lang_of(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "Rust",
        "py" => "Python",
        "ts" => "TypeScript",
        "tsx" => "TypeScript(JSX)",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JavaScript(JSX)",
        "go" => "Go",
        "java" => "Java",
        "kt" => "Kotlin",
        "c" => "C",
        "cc" | "cpp" | "cxx" => "C++",
        "h" | "hpp" => "C/C++ 头文件",
        "md" => "Markdown",
        "toml" => "TOML",
        "json" => "JSON",
        "yml" | "yaml" => "YAML",
        _ => "认不出（按扩展名判断不了）",
    }
}

/// 相对路径（统一 `/` 分隔），与图库里的 `file:` 节点同名。
fn rel_of(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 最近改动：有 `.git` 用 git（判据是提交时间），否则用文件 mtime（判据是落盘时间）。
/// **两种判据不同，所以必须把 basis 一起说出去**。
fn recent_of(root: &Path, abs: &Path, rel: &str) -> (String, String) {
    if root.join(".git").exists() {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["log", "-1", "--format=%cs", "--"])
            .arg(rel)
            .output();
        if let Ok(o) = out {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                return (s, "git 最后一次提交时间（不是文件 mtime）".to_string());
            }
        }
        return (
            "查不了".to_string(),
            "有 .git，但这条路径没有提交记录".to_string(),
        );
    }
    match std::fs::metadata(abs).and_then(|m| m.modified()) {
        Ok(t) => {
            let secs = t
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (fmt_unix(secs), "文件落盘时间（没有版本库；按 UTC 显示）".to_string())
        }
        Err(_) => ("查不了".to_string(), "读不到文件时间".to_string()),
    }
}

/// 三字段（缺什么 / 为什么 / 怎么补）——`unknown` 时必带。
fn env3(json: bool, root: &Path, at: &str, what: &str, why: &str, how: &str, code: i32) -> i32 {
    if json {
        let v = envelope(
            State::Unknown,
            format!("{what}：{why}"),
            vec![Evidence::file(at, why)],
            Coverage::partial(0, None, why),
            serde_json::json!({
                "what": what, "why": why, "how": how, "root": root.display().to_string(),
            }),
        );
        print(&v);
    } else {
        println!("{what}：{why}");
        println!("  怎么补：{how}");
    }
    code
}

pub(crate) fn cmd_file(root: &Path, path: &str, json: bool) -> i32 {
    if path.trim().is_empty() {
        return env3(
            json,
            root,
            "(没有路径)",
            "查不了这个文件",
            "没给路径",
            "jingwei file <路径>（可相对项目根）",
            2,
        );
    }
    let p = Path::new(path);
    let abs: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };
    // 沙箱：只认项目内的文件（越界就明说，不猜）
    if crate::sandbox::ensure_within(root, &abs).is_err() {
        return env3(
            json,
            root,
            &rel_of(root, &abs),
            "查不了这个文件",
            &format!("`{}` 不在项目内", abs.display()),
            "给一个项目内的路径",
            2,
        );
    }
    if !abs.is_file() {
        return env3(
            json,
            root,
            &rel_of(root, &abs),
            "查不了这个文件",
            &format!("`{}` 不存在（或不是文件）", abs.display()),
            "确认路径；列目录用 `jingwei files` 或界面「编辑器」",
            2,
        );
    }

    let rel = rel_of(root, &abs);
    let bytes = std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
    let text = std::fs::read_to_string(&abs).unwrap_or_default();
    let lines = if text.is_empty() {
        0
    } else {
        text.lines().count()
    };
    let lang = lang_of(&abs);
    let (recent, recent_basis) = recent_of(root, &abs, &rel);

    // 图谱：有就读，没有就明确 unknown（不许把"没建图"说成"0 个符号"）
    let db = root.join(".jingwei").join("graph.db");
    let mut evidence = vec![Evidence::file(&rel, &format!("{lines} 行 / {bytes} 字节"))];
    let (symbols, deps, deps_sample, graph_note) = if db.exists() {
        match Store::open_readonly(&db) {
            Ok(store) => {
                let sym = store
                    .file_state(&rel)
                    .ok()
                    .flatten()
                    .and_then(|s| s.nodes_count);
                let id = format!("file:{rel}");
                let out = store
                    .neighbors(&id, jingwei_engine::EdgeKind::DependsOn, Dir::Out)
                    .unwrap_or_default();
                let sample: Vec<String> = out.iter().take(5).cloned().collect();
                evidence.push(Evidence::graph(
                    &db,
                    &format!("依赖出边 {} 条", out.len()),
                ));
                (sym, Some(out.len()), sample, "图谱已建".to_string())
            }
            Err(e) => {
                evidence.push(Evidence::file(&db.display().to_string(), "图库打开失败"));
                (None, None, Vec::new(), format!("图库打开失败：{e}"))
            }
        }
    } else {
        (None, None, Vec::new(), "图谱没建（先 jingwei index）".to_string())
    };

    let has_graph = symbols.is_some() || deps.is_some();
    let state = if has_graph { State::Yes } else { State::Unknown };
    let summary = if has_graph {
        format!("{lang} · {lines} 行 · 图里 {} 个符号", symbols.unwrap_or(0))
    } else {
        format!("{lang} · {lines} 行；符号与依赖查不了（图谱没建）")
    };

    if json {
        let v = envelope(
            state,
            summary,
            evidence,
            if has_graph {
                Coverage::full(1, "这个文件读完了；符号与依赖来自图谱")
            } else {
                Coverage::partial(1, None, "只读到文件本身；符号/依赖缺图谱")
            },
            serde_json::json!({
                "path": rel, "lang": lang, "lines": lines, "bytes": bytes,
                "symbols": symbols,
                "depends_on": deps.map(|n| serde_json::json!({"count": n, "sample": deps_sample})),
                "recent": {"value": recent, "basis": recent_basis},
                "graph": graph_note,
                // 图谱没建时，三字段必须齐全（不许只说"查不了"就完事）
                "what": if has_graph { serde_json::Value::Null } else { serde_json::json!("符号与依赖") },
                "why": if has_graph { serde_json::Value::Null } else { serde_json::json!(graph_note) },
                "how": if has_graph { serde_json::Value::Null } else { serde_json::json!("jingwei index（或界面「索引」）") },
                "impact_hint": format!("jingwei impact --file {rel}"),
            }),
        );
        print(&v);
        return 0;
    }

    println!("{}", summary);
    println!("  路径：{rel}");
    println!("  最近动过：{recent}（{recent_basis}）");
    match deps {
        Some(n) => {
            println!("  依赖：{n} 个");
            for d in &deps_sample {
                println!("    · {d}");
            }
            if n > deps_sample.len() {
                println!("    …还有 {} 个（jingwei impact --file {rel}）", n - deps_sample.len());
            }
        }
        None => println!("  依赖：查不了（{graph_note}）"),
    }
    println!("  诊断：这条命令不算诊断——要问题清单跑 `jingwei check`（或界面「检查」）");
    0
}

/// 把 unix 秒排成 `YYYY-MM-DD HH:MM`（本地近似：按 UTC 显示并在后面注明，不假装时区）。
/// 为什么要它：直接印 `1792661159` 人读不懂；但也不能印成"3 天前"这种算出来的模糊话。
fn fmt_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

/// Howard Hinnant 的 civil_from_days：把"1970 起的第几天"换成公历年月日。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
