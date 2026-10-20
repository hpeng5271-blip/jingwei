//! `jingwei impact <符号>`：**改这里会牵动什么**（2026-10-21）。
//!
//! `explain` 回答"它是什么、被谁调"；`impact` 回答"往上一层一层往上，谁会受影响"。
//! 这是改代码前真正要问的那句，也是图谱比 grep 强的地方（grep 找不到"间接调用"）。
//!
//! **三态**：
//! - 有上游调用者 → 按层列出（第 1 层 = 直接调用它的；第 2 层 = 调用第 1 层的）；
//! - 没有任何调用者 → `查不了：静态 calls 边里没有谁调用它`（可能是入口/回调/死代码，**不猜**）；
//! - 符号不在图谱里 → 查不了（exit 1）。
//!
//! **边界**（必须说清楚，否则会被当"全知"）：只看静态 `calls` 边。Python 的 `getattr`、
//! 字符串反射、装饰器动态注册、`__main__` 之外的动态派发都**不会**出现——那部分查不了。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn cmd_impact(root: &Path, rest: &[String], json: bool) -> i32 {
    let Some(query) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("用法：jingwei impact <符号名|symbol:id>   （往上游看：谁会受你改动影响）");
        return 2;
    };
    let depth: usize = rest
        .iter()
        .position(|a| a == "--depth")
        .and_then(|i| rest.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(2)
        .clamp(1, 4);

    let Some(store) = crate::cmd_knowledge::open_graph(root) else {
        println!("查不了：没有图谱（先 jingwei index）");
        return 1;
    };

    // 解析成唯一符号（与 explain / symbol 同一口径：同名多义列出来，不替人挑）
    let key = if query.starts_with("symbol:") {
        if store.node(query).map(|n| n.is_some()).unwrap_or(false) {
            query.to_string()
        } else {
            println!("查不了：`{query}` 不在图谱里（可能改名/删了）");
            return 1;
        }
    } else {
        match store.symbol_ids_by_name(query) {
            Ok(ids) if ids.len() == 1 => ids[0].clone(),
            Ok(ids) if ids.is_empty() => {
                println!("查不了：图谱里没有叫 `{query}` 的符号（先 index，或名字拼错了）");
                return 1;
            }
            Ok(ids) => {
                println!("有 {} 个符号叫 `{query}`，用 id 指定：", ids.len());
                for id in ids {
                    println!("  {id}");
                }
                return 1;
            }
            Err(e) => {
                eprintln!("查符号失败：{e}");
                return 1;
            }
        }
    };

    // 逐层往上找调用者（`calls` 边的入边）。
    let mut seen: BTreeSet<String> = BTreeSet::new();
    seen.insert(key.clone());
    let mut by_depth: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut frontier = vec![key.clone()];
    for d in 1..=depth {
        let mut next = Vec::new();
        for id in &frontier {
            let Ok(srcs) = store.neighbors(id, jingwei_engine::EdgeKind::Calls, jingwei_engine::graph::store::Dir::In) else {
                continue;
            };
            for s in srcs {
                if seen.insert(s.clone()) {
                    next.push(s);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort();
        by_depth.insert(d, next.clone());
        frontier = next;
    }

    let total: usize = by_depth.values().map(|v| v.len()).sum();
    if json {
        let v: Vec<_> = by_depth
            .iter()
            .map(|(d, ids)| serde_json::json!({"depth": d, "callers": ids}))
            .collect();
        println!("{}", serde_json::json!({"symbol": key, "callers": v, "total": total}));
        return 0;
    }

    println!("改动影响面：{key}（往上游看 {depth} 层，只看静态 calls 边）");
    if total == 0 {
        println!("  查不了：静态 calls 边里没有谁调用它——可能是入口/回调/死代码，也可能调用关系是动态的");
        println!("  （工具不猜：动态派发、反射、字符串调用都解析不出来）");
        // 没有上游 ≠ 没有知识：这条代码自己可能被条目解释过，那才是最该看的"为什么"。
        print_known(&store, std::slice::from_ref(&key));
        return 0;
    }
    let mut files: BTreeSet<String> = BTreeSet::new();
    for (d, ids) in &by_depth {
        println!("  第 {d} 层（{} 个）:", ids.len());
        for id in ids.iter().take(6) {
            let (file, name) = split_id(id);
            files.insert(file.clone());
            println!("    {name}（{file}）");
        }
        if ids.len() > 6 {
            println!("    …等 {} 个", ids.len());
        }
    }
    println!("  牵动文件 {} 个：", files.len());
    for f in files.iter().take(8) {
        println!("    {f}");
    }
    if files.len() > 8 {
        println!("    …等 {} 个", files.len());
    }
    // **知识层交叉**（2026-10-21）：受影响的符号如果被条目解释过，一并列出来。
    // 一次查询同时回答"谁会坏"和"这条代码背后记着什么为什么"。
    let mut all_ids: Vec<String> = vec![key.clone()];
    for ids in by_depth.values() {
        all_ids.extend(ids.iter().cloned());
    }
    print_known(&store, &all_ids);
    println!("  边界：只看静态 calls 边；getattr / 反射 / 字符串调用、跨进程与外部调用都查不了");
    0
}

/// 受影响的符号里，哪些被知识条目解释过（最多 5 条，超了报数）。
fn print_known(store: &jingwei_engine::graph::store::Store, ids: &[String]) {
    let mut known: Vec<(String, String)> = Vec::new();
    for id in ids {
        let Ok(cs) = store.claims_for_symbol(id) else {
            continue;
        };
        for c in cs {
            let (_, name) = split_id(id);
            known.push((name, crate::cmd_explain::short_name(&c.id)));
        }
    }
    if known.is_empty() {
        return;
    }
    println!("  知识层（受影响的符号被这些主张解释过）：");
    for (sym, entry) in known.iter().take(5) {
        println!("    {sym} ← {entry}");
    }
    if known.len() > 5 {
        println!("    …等 {} 条", known.len());
    }
}

/// `symbol:path#name` → (path, name)；不是这个形态就都给原串。
fn split_id(id: &str) -> (String, String) {
    let rest = id.strip_prefix("symbol:").unwrap_or(id);
    match rest.split_once('#') {
        Some((path, name)) => (path.to_string(), name.to_string()),
        None => (rest.to_string(), rest.to_string()),
    }
}

