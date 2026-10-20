//! `jingwei knowledge check`：**知识条目的严筛**——双头漏斗最细的那道颈（2026-10-20，pro 任务卡 A）。
//!
//! 为什么要有它：采集与二次编制如果只靠文档自觉，条目会越写越像"感觉"，
//! 而这道颈的判据只有一句话：**没有来源、没有等级、没有判据的条目，不算过筛**；
//! **关联符号在图谱里查不到的，只能算"查不了"，不许算"挂上图谱"**（三态，不许压扁）。
//!
//! 条目格式（`docs/knowledge/*.md`，front-matter 用最简单的 `key: value`，不引 YAML 依赖）：
//!
//! ```text
//! ---
//! claim: 一句话主张
//! source: https://… 或 docs/…（仓库内相对路径必须真的存在）
//! level: 一手 | 转写 | 二手 | 查不了
//! check: 怎么验（命令或判据）
//! symbol: symbol:crates/engine/src/graph/store.rs#Store::apply_file_update
//! ---
//! ```
//!
//! 输出一张漏斗表：`采集 N / 过筛 M / 挂图谱 K / 缺口 J`，缺口逐条列名与原因。

use std::collections::BTreeMap;
use std::path::Path;

use jingwei_engine::graph::store::Store;

/// 等级枚举与三态对齐：`查不了` 就是三态里的那个"查不了"，不是"没有"。
pub const LEVELS: [&str; 4] = ["一手", "转写", "二手", "查不了"];
/// 一条条目要"过筛"必须齐全的四项。
const REQUIRED: [&str; 4] = ["claim", "source", "level", "check"];

#[derive(Debug, Default, Clone)]
pub struct Entry {
    pub file: String,
    pub fields: BTreeMap<String, String>,
    pub symbols: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub total: usize,
    /// 四字段齐全（来源/等级/判据/主张）。
    pub passed: usize,
    /// 关联符号全部在图谱里查得到。
    pub linked: usize,
    /// 缺字段（**不许算过筛**）。
    pub problems: Vec<String>,
    /// 三态里的"查不了"：符号查不到、或没有图谱。
    pub gaps: Vec<String>,
    pub no_graph: bool,
}

/// 解析一条条目。front-matter 之外的内容不参与判定（正文可以是笔记）。
pub fn parse_entry(file: &str, text: &str) -> Entry {
    let mut fields = BTreeMap::new();
    let mut symbols = Vec::new();
    // **BOM 不是空白**：PowerShell 写的文件常带它，`trim()` 不认（踩过）。
    let t = text.trim_start_matches('\u{feff}');
    let mut lines = t.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return Entry {
            file: file.to_string(),
            fields,
            symbols,
        };
    }
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
        if k == "symbol" || k == "symbols" {
            for s in v.split(',') {
                let s = s.trim();
                if !s.is_empty() {
                    symbols.push(s.to_string());
                }
            }
        } else {
            fields.insert(k, v);
        }
    }
    Entry {
        file: file.to_string(),
        fields,
        symbols,
    }
}

/// 符号能不能在图谱里查出来。`symbol:` 前缀按 id 查，否则按名字查（同名多义也算"有"）。
fn symbol_ok(store: &Store, s: &str) -> bool {
    if s.starts_with("symbol:") {
        store.node(s).map(|n| n.is_some()).unwrap_or(false)
    } else {
        store
            .symbol_ids_by_name(s)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }
}

/// 来源是 URL 就放行；**看起来像仓库内路径的，必须真的存在**（文档承诺的文件不存在 = 典型事故）；
/// 其余的（书名、论文标题、"查不了"）不是路径，放行。
fn source_ok(root: &Path, v: &str) -> bool {
    let s = v.trim().trim_matches('`').trim();
    if s.is_empty() {
        return false;
    }
    if s.starts_with("http://") || s.starts_with("https://") || s == "查不了" {
        return true;
    }
    let looks_path = s.contains('/')
        || [".md", ".rs", ".json", ".txt", ".toml"]
            .iter()
            .any(|e| s.ends_with(e));
    if looks_path {
        return root.join(s).exists();
    }
    true
}

/// 严筛一遍 `docs/knowledge/`。`store` 为 None = 没有图谱（图谱侧的判定只能"查不了"）。
pub fn evaluate(root: &Path, store: Option<&Store>) -> Report {
    let mut r = Report {
        no_graph: store.is_none(),
        ..Default::default()
    };
    let dir = root.join("docs").join("knowledge");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return r; // 还没有条目：采集 0
    };
    let mut files: Vec<std::path::PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
        .filter(|p| {
            // `README.md` 与 `_` 开头的是说明/草稿，不是条目（否则一条 README 就让漏斗永远红）。
            let n = p
                .file_name()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_default();
            n != "README.md" && !n.starts_with('_')
        })
        .collect();
    files.sort();

    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let e = parse_entry(&name, &text);
        r.total += 1;

        let mut missing: Vec<&str> = Vec::new();
        for k in REQUIRED {
            match e.fields.get(k) {
                Some(v) if !v.trim().is_empty() => {}
                _ => missing.push(k),
            }
        }
        // 等级必须是四个之一（不许自由发挥，否则三态就是标签）。
        let mut bad_level = None;
        if let Some(lv) = e.fields.get("level") {
            if !lv.is_empty() && !LEVELS.contains(&lv.as_str()) {
                bad_level = Some(lv.clone());
            }
        }
        // 来源是仓库内路径时，路径必须存在（文档承诺的文件不存在 = 典型事故）。
        let mut bad_source = false;
        if let Some(src) = e.fields.get("source") {
            if !missing.contains(&"source") && !source_ok(root, src) {
                bad_source = true;
            }
        }
        if !missing.is_empty() {
            r.problems.push(format!(
                "{} 缺 {}（没有这些就不算过筛）",
                e.file,
                missing.join(" / ")
            ));
            continue;
        }
        if let Some(lv) = bad_level {
            r.problems.push(format!(
                "{} 的 level = `{lv}`，只能是 一手/转写/二手/查不了",
                e.file
            ));
            continue;
        }
        if bad_source {
            r.problems
                .push(format!("{} 的 source 指向的仓库内路径不存在", e.file));
            continue;
        }
        r.passed += 1;

        if e.symbols.is_empty() {
            continue;
        }
        match store {
            None => r.gaps.push(format!(
                "{} 关联了 {} 个符号，但没有图谱——查不了（先 jingwei index）",
                e.file,
                e.symbols.len()
            )),
            Some(st) => {
                let bad: Vec<&String> =
                    e.symbols.iter().filter(|s| !symbol_ok(st, s)).collect();
                if bad.is_empty() {
                    r.linked += 1;
                } else {
                    r.gaps.push(format!(
                        "{} 关联的符号图谱里查不到：{}——查不了",
                        e.file,
                        bad.iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    ));
                }
            }
        }
    }
    r
}

pub fn cmd_knowledge(root: &Path, rest: &[String], json: bool) -> i32 {
    if rest.first().map(|s| s.as_str()) != Some("check") {
        eprintln!("用法：jingwei knowledge check     （严筛 docs/knowledge/*.md，输出漏斗表）");
        return 2;
    }
    let store = Store::open(&root.join(".jingwei").join("graph.db")).ok();
    let r = evaluate(root, store.as_ref());

    if json {
        let v = serde_json::json!({
            "total": r.total, "passed": r.passed, "linked": r.linked,
            "problems": r.problems, "gaps": r.gaps, "no_graph": r.no_graph,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        println!(
            "知识条目：采集 {} / 过筛 {} / 挂图谱 {} / 缺口 {}",
            r.total,
            r.passed,
            r.linked,
            r.problems.len() + r.gaps.len()
        );
        for p in &r.problems {
            println!("  × {p}");
        }
        for g in &r.gaps {
            println!("  ! {g}");
        }
        if r.total == 0 {
            println!("  · docs/knowledge/ 里还没有条目（先写一条再跑）");
        }
        if r.no_graph {
            println!("  · 没有图谱：符号侧一律「查不了」（先 jingwei index）");
        }
    }
    // 缺字段 / 等级错 / 来源不存在 = 红；缺口（查不了）只如实报，不算过筛也不算失败。
    if r.problems.is_empty() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-kn-{tag}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("docs").join("knowledge")).unwrap();
        d
    }

    fn put(root: &Path, name: &str, body: &str) {
        std::fs::write(root.join("docs").join("knowledge").join(name), body).unwrap();
    }

    #[test]
    fn 解析_front_matter_与符号() {
        let e = parse_entry(
            "a.md",
            "\u{feff}---\nclaim: 双源才敢说是\nsource: https://x\nlevel: 一手\ncheck: 命令\nsymbol: symbol:a.rs#f\nsymbols: g, h\n---\n正文",
        );
        assert_eq!(e.fields.get("claim").map(|s| s.as_str()), Some("双源才敢说是"));
        assert_eq!(e.fields.get("level").map(|s| s.as_str()), Some("一手"));
        assert_eq!(e.symbols, vec!["symbol:a.rs#f", "g", "h"]);
    }

    #[test]
    fn 缺来源或缺等级就不过筛() {
        let root = tmp("miss");
        put(&root, "ok.md", "---\nclaim: x\nsource: https://x\nlevel: 一手\ncheck: 跑一下\n---\n");
        put(&root, "nosrc.md", "---\nclaim: x\nlevel: 一手\ncheck: 跑一下\n---\n");
        put(&root, "nolevel.md", "---\nclaim: x\nsource: https://x\ncheck: 跑一下\n---\n");
        let r = evaluate(&root, None);
        assert_eq!(r.total, 3);
        assert_eq!(r.passed, 1, "{:?}", r.problems);
        assert_eq!(r.problems.len(), 2, "{:?}", r.problems);
        assert!(r.problems.iter().any(|p| p.contains("nosrc.md") && p.contains("source")));
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 等级只能是四个之一() {
        let root = tmp("level");
        put(&root, "bad.md", "---\nclaim: x\nsource: https://x\nlevel: 听说\ncheck: 跑一下\n---\n");
        let r = evaluate(&root, None);
        assert_eq!(r.passed, 0);
        assert!(r.problems[0].contains("听说"), "{:?}", r.problems);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 来源是仓库内路径时必须存在() {
        let root = tmp("src");
        put(&root, "gone.md", "---\nclaim: x\nsource: docs/nope.md\nlevel: 一手\ncheck: 跑一下\n---\n");
        let r = evaluate(&root, None);
        assert_eq!(r.passed, 0, "{:?}", r.problems);
        assert!(r.problems[0].contains("source"), "{:?}", r.problems);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 说明与草稿不算条目() {
        let root = tmp("skip");
        put(&root, "README.md", "# 说明\n没有 front-matter\n");
        put(&root, "_draft.md", "---\nclaim: 草稿\n---\n");
        put(
            &root,
            "real.md",
            "---\nclaim: x\nsource: https://x\nlevel: 一手\ncheck: 跑一下\n---\n",
        );
        let r = evaluate(&root, None);
        assert_eq!(r.total, 1, "README 与 _ 草稿不该计入采集");
        assert_eq!(r.passed, 1);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 没有图谱时关联符号算查不了而不是算挂上() {
        let root = tmp("nogph");
        put(
            &root,
            "s.md",
            "---\nclaim: x\nsource: https://x\nlevel: 一手\ncheck: 跑一下\nsymbol: symbol:a.rs#f\n---\n",
        );
        let r = evaluate(&root, None);
        assert_eq!(r.passed, 1);
        assert_eq!(r.linked, 0, "没有图谱就不许算挂上");
        assert_eq!(r.gaps.len(), 1);
        assert!(r.gaps[0].contains("查不了"));
        _ = std::fs::remove_dir_all(&root);
    }
}
