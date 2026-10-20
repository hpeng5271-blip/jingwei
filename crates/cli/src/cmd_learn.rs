//! `jingwei learn <主题>`：把知识条目按**显式前置**排成一条能照着走的路径（2026-10-20，pro 任务卡 B）。
//!
//! 这是双头漏斗第一个**可见出口**：严筛过的条目（`docs/knowledge/*.md`）按 `prereq:` 排顺序，
//! 每一步都带「凭什么」——claim / level / source，有符号或决策的再带落点。
//!
//! 三条不许破的规矩（pro 定的翻车点）：
//! 1. **顺序只能来自 `prereq:`**（或将来图谱的依赖边），**不许按文件名/时间戳拍脑袋**；
//! 2. 每步必须引用 ≥1 条真条目；引用不到就**不生成**那一步，只报"查不了"；
//! 3. 被引用的条目过不了严筛（缺来源/等级）→ 该步标 `查不了`，整条命令 exit≠0，不许当绿。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::cmd_knowledge::{entry_stem, load_entries, why_rejected, Entry};

/// 一条路径步骤：**每步都必须带"凭什么"**（claim / level / source），否则不许生成。
#[derive(Debug, Clone)]
pub struct Step {
    pub entry: String,
    pub claim: String,
    pub level: String,
    pub source: String,
    pub symbols: Vec<String>,
    /// 该条目的前置（已解析成条目名）。
    pub prereq: Vec<String>,
    /// 过不了严筛时的原因；None = 这条可以当"已核实"。
    pub rejected: Option<String>,
}

/// 按主题取条目，再按 `prereq:` 排成 DAG 的拓扑序。
pub fn plan(root: &Path, topic: &str) -> Result<Vec<Step>, String> {
    let all: Vec<Entry> = load_entries(root);
    let want = topic.trim();

    // 主题匹配：`topic:` 可写多个，逗号分隔，精确匹配（不做模糊，模糊会把不相干的条目卷进来）。
    let picked: Vec<Entry> = all
        .into_iter()
        .filter(|e| {
            e.fields
                .get("topic")
                .map(|t| t.split(',').any(|x| x.trim() == want))
                .unwrap_or(false)
        })
        .collect();
    if picked.is_empty() {
        return Err(format!(
            "还没有 `topic: {want}` 的条目（先采集：格式见 docs/knowledge/README.md）"
        ));
    }

    let names: BTreeSet<String> = picked
        .iter()
        .map(|e| entry_stem(&e.file).to_string())
        .collect();
    let mut steps: BTreeMap<String, Step> = BTreeMap::new();
    for e in &picked {
        let stem = entry_stem(&e.file).to_string();
        let prereq: Vec<String> = e
            .fields
            .get("prereq")
            .map(|v| {
                v.split(',')
                    .map(|x| x.trim().trim_end_matches(".md").to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        for p in &prereq {
            if !names.contains(p) {
                // 前置指向了不存在（或不属于本主题）的条目：这一条**不能**猜顺序
                return Err(format!(
                    "{stem} 的前置 `{p}` 不在本主题的条目里——顺序查不了（别猜）"
                ));
            }
        }
        steps.insert(
            stem.clone(),
            Step {
                entry: stem,
                claim: e.fields.get("claim").cloned().unwrap_or_default(),
                level: e.fields.get("level").cloned().unwrap_or_default(),
                source: e.fields.get("source").cloned().unwrap_or_default(),
                symbols: e.symbols.clone(),
                prereq,
                rejected: why_rejected(root, e),
            },
        );
    }

    // Kahn：入度为 0 的先出；同一个入度的按名字排（只用来**稳定**输出，不用来决定先后）。
    let mut order: Vec<Step> = Vec::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    let total = steps.len();
    while order.len() < total {
        let ready: Vec<String> = steps
            .values()
            .filter(|s| !done.contains(&s.entry))
            .filter(|s| s.prereq.iter().all(|p| done.contains(p)))
            .map(|s| s.entry.clone())
            .collect();
        if ready.is_empty() {
            let left: Vec<String> = steps
                .keys()
                .filter(|k| !done.contains(*k))
                .cloned()
                .collect();
            return Err(format!(
                "顺序查不了：{} 互相前置成环（{}）",
                left.len(),
                left.join(" / ")
            ));
        }
        for name in ready {
            if let Some(s) = steps.get(&name) {
                order.push(s.clone());
                done.insert(name);
            }
        }
    }
    Ok(order)
}

pub fn cmd_learn(root: &Path, rest: &[String], json: bool) -> i32 {
    let Some(topic) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("用法：jingwei learn <主题>     （按 docs/knowledge 条目的 prereq 排顺序）");
        return 2;
    };

    let plan = match plan(root, topic) {
        Ok(p) => p,
        Err(e) => {
            // **查不了不是失败**：还没有条目是"还没采集"，exit 0 如实说。
            if e.contains("还没有") {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"topic": topic, "steps": [], "unknown": e})
                    );
                } else {
                    println!("查不了：{e}");
                }
                return 0;
            }
            // 顺序判不出来（环 / 前置不存在）才算失败：这时候任何顺序都是编的。
            eprintln!("{e}");
            return 1;
        }
    };

    let bad = plan.iter().filter(|s| s.rejected.is_some()).count();
    if json {
        let v: Vec<_> = plan
            .iter()
            .map(|s| {
                serde_json::json!({
                    "entry": s.entry,
                    "claim": s.claim,
                    "level": s.level,
                    "source": s.source,
                    "symbols": s.symbols,
                    "prereq": s.prereq,
                    "rejected": s.rejected,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({"topic": topic, "steps": v, "rejected": bad})
        );
    } else {
        println!(
            "主题：{topic} —— {} 步（顺序来自条目的 prereq，不是文件名）",
            plan.len()
        );
        for (i, s) in plan.iter().enumerate() {
            match &s.rejected {
                Some(why) => {
                    println!("  {}. ? {} —— 查不了：{why}", i + 1, s.claim);
                    println!("     ← {} · {}", s.level, s.source);
                }
                None => {
                    println!("  {}. {}", i + 1, s.claim);
                    let mut basis = format!("     ← {} · {}", s.level, s.source);
                    if !s.prereq.is_empty() {
                        basis.push_str(&format!("（前置：{}）", s.prereq.join(" / ")));
                    }
                    println!("{basis}");
                    if !s.symbols.is_empty() {
                        println!("     挂：{}", s.symbols.join(" / "));
                    }
                }
            }
        }
    }
    if bad > 0 {
        eprintln!("有 {bad} 步的条目过不了严筛（缺来源/等级）——这些步骤只能算查不了");
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-learn-{tag}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("docs").join("knowledge")).unwrap();
        d
    }

    /// `prereq` 传 `""` 或 `"prereq: x\n"`。
    fn entry(topic: &str, prereq: &str) -> String {
        format!(
            "---\nclaim: 主张\ntopic: {topic}\n{prereq}source: https://x\nlevel: 一手\ncheck: 跑一下\n---\n"
        )
    }

    fn put(root: &Path, name: &str, body: &str) {
        std::fs::write(root.join("docs").join("knowledge").join(name), body).unwrap();
    }

    #[test]
    fn 顺序跟着_prereq_走而不是文件名() {
        let root = tmp("order");
        // 文件名故意与正确顺序相反：z 在最前，a 在最后
        put(&root, "z-first.md", &entry("t", ""));
        put(&root, "a-second.md", &entry("t", "prereq: z-first\n"));
        put(&root, "m-third.md", &entry("t", "prereq: a-second\n"));
        let plan = plan(&root, "t").unwrap();
        let order: Vec<&str> = plan.iter().map(|s| s.entry.as_str()).collect();
        assert_eq!(order, vec!["z-first", "a-second", "m-third"]);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 没有这个主题的条目就是查不了() {
        let root = tmp("unknown");
        put(&root, "a.md", &entry("other", ""));
        let e = plan(&root, "没有这个主题").unwrap_err();
        assert!(e.contains("还没有"), "{e}");
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 前置成环就拒绝给顺序() {
        let root = tmp("cycle");
        put(&root, "a.md", &entry("t", "prereq: b\n"));
        put(&root, "b.md", &entry("t", "prereq: a\n"));
        let e = plan(&root, "t").unwrap_err();
        assert!(e.contains("环"), "{e}");
        _ = std::fs::remove_dir_all(&root);
    }
}
