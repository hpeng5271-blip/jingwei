//! `jingwei symbol <符号>`：**反查**——这个符号被哪几条知识条目解释过（2026-10-20，pro 任务卡 C）。
//!
//! 这是"可操作图谱"的第一个真正查询原语：输入一个符号，输出围绕它的**主张 / 等级 / 来源**。
//! 同时兑现回收半边：条目引用了、但图谱里已经查不到的符号，标 `重编`——代码变了，条目该重编了。
//!
//! 两条不许破的规矩（pro 定的翻车点）：
//! 1. **精确匹配，不许子串**：条目的 `symbol:` 必须与图谱里的符号 id 或名字**整串相等**；
//!    拿"条目文本里出现过这个字符串"当命中，会把 `load` 撞进 `payload`。
//! 2. **重编必须可见**：凡"条目引用但图谱查不到"，一律标 `重编` 并给出待重编条数，
//!    不许只列条目、把回收半边悄悄丢掉。

use std::path::Path;

use crate::cmd_knowledge::{Entry, LEVELS};

/// 等级排序：一手 → 转写 → 二手 → 查不了（同等级按条目名稳定排序）。
fn level_rank(level: &str) -> usize {
    LEVELS.iter().position(|l| *l == level).unwrap_or(LEVELS.len())
}

/// 条目是否引用这个符号。`key` 可以是 `symbol:...` 的完整 id，也可以是一个名字。
/// **整串相等**才算，绝不做包含匹配。
pub fn refers_to(e: &Entry, key: &str) -> bool {
    e.symbols.iter().any(|s| s == key)
}

pub fn cmd_symbol(root: &Path, rest: &[String], json: bool) -> i32 {
    let Some(query) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("用法：jingwei symbol <符号名|symbol:id>   （反查：这个符号被哪几条知识条目解释过）");
        return 2;
    };

    let Some(store) = crate::cmd_knowledge::open_graph(root) else {
        println!("查不了：没有图谱（先 jingwei index）");
        return 1;
    };

    // 解析成**唯一**符号；同名多义时如实列出，不替用户挑（与 explain 同一口径）。
    let mut key = query.clone();
    if query.starts_with("symbol:") {
        if !store.node(query).map(|n| n.is_some()).unwrap_or(false) {
            key = query.clone(); // 不存在：留给下面报"查不了 + 待重编"
        }
    } else {
        match store.symbol_ids_by_name(query) {
            Ok(ids) if ids.len() > 1 => {
                println!("有 {} 个符号叫 `{query}`，用 id 指定：", ids.len());
                for id in ids {
                    println!("  {id}");
                }
                return 1;
            }
            Ok(ids) if ids.len() == 1 => key = ids[0].clone(),
            _ => {} // 找不到：同样走"查不了 + 待重编"
        }
    }

    let exists = store.node(&key).map(|n| n.is_some()).unwrap_or(false);

    // **走图边**（pro 任务卡 G）：主张层已经落库，这里不再回头 grep Markdown。
    // 查不到时用 `claim_edges` 的原样串反查——那条边还在，说明条目该重编了。
    if !exists {
        let refs = store
            .claims_referencing_missing(query)
            .or_else(|_| store.claims_referencing_missing(&key))
            .unwrap_or_default();
        if json {
            println!(
                "{}",
                serde_json::json!({"symbol": key, "exists": false, "entries": [], "recompile": refs.len()})
            );
            return 1;
        }
        println!("查不了：`{query}` 不在图谱里（可能改名/删了，或者还没索引）");
        if refs.is_empty() {
            return 1;
        }
        println!("待重编（{} 条）：", refs.len());
        for id in &refs {
            println!("  ! {id} —— 条目的 symbol 指向了一个图谱里没有的符号");
        }
        return 1;
    }

    let mut claims = store.claims_for_symbol(&key).unwrap_or_default();
    claims.sort_by(|a, b| level_rank(&a.level).cmp(&level_rank(&b.level)).then_with(|| a.id.cmp(&b.id)));

    if json {
        let v: Vec<_> = claims
            .iter()
            .map(|c| {
                serde_json::json!({
                    "entry": c.id,
                    "claim": c.claim,
                    "level": c.level,
                    "source": c.source,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({"symbol": key, "exists": true, "entries": v, "recompile": 0})
        );
        return 0;
    }

    println!("符号：{key}");
    if claims.is_empty() {
        println!("无条目解释（知识层里还没有围绕这个符号的主张）");
        return 0;
    }
    println!("条目：{} 条（按等级排：一手 → 转写 → 二手 → 查不了）", claims.len());
    for (i, c) in claims.iter().enumerate() {
        println!("  {}. [{}] {}", i + 1, c.level, c.claim);
        println!("     ← {}", c.source);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn e(name: &str, level: &str, symbols: &[&str]) -> Entry {
        let mut fields = BTreeMap::new();
        fields.insert("level".to_string(), level.to_string());
        fields.insert("claim".to_string(), format!("主张-{name}"));
        fields.insert("source".to_string(), "https://x".to_string());
        Entry {
            file: format!("{name}.md"),
            fields,
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn 引用是整串相等而不是包含() {
        let a = e("a", "一手", &["symbol:src/lib.rs#load"]);
        assert!(refers_to(&a, "symbol:src/lib.rs#load"));
        // 名字里含 load 的另一个符号、以及子串，都不许命中
        assert!(!refers_to(&a, "load"));
        assert!(!refers_to(&a, "symbol:src/lib.rs#payload"));
    }

    #[test]
    fn 等级排序一手在前二手最后() {
        // symbol 的输出顺序靠 level_rank：一手(0) < 转写(1) < 二手(2) < 查不了(3)
        assert!(level_rank("一手") < level_rank("转写"));
        assert!(level_rank("转写") < level_rank("二手"));
        assert!(level_rank("二手") < level_rank("查不了"));
        assert_eq!(level_rank("没写等级"), LEVELS.len());
    }
}
