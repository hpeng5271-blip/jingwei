//! `jingwei impact <符号> | --file <路径>`：**改这里会牵动什么**（2026-10-21）。
//!
//! `explain` 回答"它是什么、被谁调"；`impact` 回答"往上一层一层往上，谁会受影响"。
//! 这是改代码前真正要问的那句，也是图谱比 grep 强的地方（grep 找不到"间接调用"）。
//!
//! **三态**：
//! - 有上游调用者 → 按层列出（第 1 层 = 直接调用它的；第 2 层 = 调用第 1 层的）；
//! - 没有任何调用者 → `查不了：静态 calls 边里没有谁调用它`（可能是入口/回调/死代码，**不猜**）；
//! - 符号/文件不在图谱里 → 查不了（exit 1）。
//!
//! **边界**（必须说清楚，否则会被当"全知"）：只看静态 `calls` 边。Python 的 `getattr`、
//! 字符串反射、装饰器动态注册、`__main__` 之外的动态派发都**不会**出现——那部分查不了。
//! 另外：**方法调用（`x.m()`）没进 calls 边**，所以方法符号常常查不出上游（债表有记录）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use jingwei_engine::graph::store::{Dir, Store};
use jingwei_engine::EdgeKind;

/// 一个受影响的符号：id + 最小层数。
type Affected = (String, usize);

pub fn cmd_impact(root: &Path, rest: &[String], json: bool) -> i32 {
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

    // `--file <路径>`：按文件问（实际动手时更常见）
    if let Some(pos) = rest.iter().position(|a| a == "--file") {
        let Some(path) = rest.get(pos + 1) else {
            eprintln!("--file 后面要跟项目内的相对路径");
            return 2;
        };
        return file_mode(&store, path, depth, json);
    }

    let Some(query) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("用法：jingwei impact <符号名|symbol:id> [--depth N]");
        eprintln!("      jingwei impact --file <路径> [--depth N]   （这个文件改了会牵动什么）");
        return 2;
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

    let by_depth = upstream(&store, &key, depth);
    let total: usize = by_depth.values().map(|v| v.len()).sum();
    if json {
        let v: Vec<_> = by_depth
            .iter()
            .map(|(d, ids)| serde_json::json!({"depth": d, "callers": ids}))
            .collect();
        println!(
            "{}",
            serde_json::json!({"symbol": key, "callers": v, "total": total})
        );
        return 0;
    }

    println!("改动影响面：{key}（往上游看 {depth} 层，只看静态 calls 边；同层按被调用次数降序）");
    if total == 0 {
        println!(
            "  查不了：静态 calls 边里没有谁调用它——可能是入口/回调/死代码，也可能调用关系是动态的"
        );
        println!("  （工具不猜：动态派发、反射、字符串调用都解析不出来；方法调用也没进 calls 边）");
        print_known(&store, std::slice::from_ref(&key));
        return 0;
    }
    print_layers(&store, &by_depth);
    let mut all_ids: Vec<String> = vec![key.clone()];
    for ids in by_depth.values() {
        all_ids.extend(ids.iter().cloned());
    }
    print_known(&store, &all_ids);
    print_boundary();
    0
}

/// 文件模式：**这个文件改了会牵动什么**。
///
/// 做法：取文件定义的所有符号，各自往上游走，按"最小层数"合并。
/// 同文件内的上游单独算（那是"一个文件里要一起改的地方"），跨文件的上游才是"牵动的别人"。
fn file_mode(store: &Store, raw: &str, depth: usize, json: bool) -> i32 {
    let rel = raw.trim_start_matches("./").replace('\\', "/");
    let file_id = format!("file:{rel}");
    if store.node(&file_id).map(|n| n.is_none()).unwrap_or(true) {
        println!("查不了：图谱里没有这个文件（先 jingwei index，或路径要写项目内相对路径）");
        return 1;
    }
    let symbols = store
        .neighbors(&file_id, EdgeKind::Defines, Dir::Out)
        .unwrap_or_default();
    if symbols.is_empty() {
        println!("查不了：这个文件在图谱里没有定义任何符号（可能只有数据/配置）");
        return 0;
    }

    let mut merged: BTreeMap<String, usize> = BTreeMap::new();
    for s in &symbols {
        for (d, ids) in upstream(store, s, depth) {
            for id in ids {
                let e = merged.entry(id).or_insert(d);
                if d < *e {
                    *e = d;
                }
            }
        }
    }
    // 同一个文件内的符号不算"被牵动的别人"（它们本来就在你的改动范围里）
    let (same_file, other): (Vec<Affected>, Vec<Affected>) = merged
        .into_iter()
        .partition(|(id, _)| id.starts_with(&format!("symbol:{rel}#")));

    // **排序**（2026-10-21，规则可复跑）：层数近的在前；同层按"被多少人调用"降序
    // （全局 calls 入边数——被调得越多，改它越容易伤到别处）；再同名。
    let mut ranked: Vec<(String, usize, usize)> = other
        .iter()
        .map(|(id, d)| {
            let indeg = store
                .neighbors(id, EdgeKind::Calls, Dir::In)
                .map(|v| v.len())
                .unwrap_or(0);
            (id.clone(), *d, indeg)
        })
        .collect();
    ranked.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)).then_with(|| a.0.cmp(&b.0)));

    if json {
        let v: Vec<_> = ranked
            .iter()
            .map(|(id, d, indeg)| serde_json::json!({"symbol": id, "depth": d, "in_degree": indeg}))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "file": rel, "symbols": symbols.len(),
                "affected": v, "same_file": same_file.len(),
            })
        );
        return 0;
    }
    println!("改动影响面：{rel}（{depth} 层，只看静态 calls 边）");
    println!(
        "  该文件定义 {} 个符号；牵动 **{}** 个文件外符号（同文件内还有 {} 个一起改）",
        symbols.len(),
        other.len(),
        same_file.len()
    );
    if !ranked.is_empty() {
        println!("  先看谁（排序：层数近的在前，同层按被调用次数降序）：");
        let mut files: BTreeSet<String> = BTreeSet::new();
        for (id, d, indeg) in ranked.iter().take(8) {
            let (f, name) = split_id(id);
            files.insert(f.clone());
            println!("    第 {d} 层 · 被 {indeg} 处调用　{name}（{f}）");
        }
        if ranked.len() > 8 {
            println!("    …等 {} 个", ranked.len());
        }
        println!("  牵动文件 {} 个：", files.len());
        for f in files.iter().take(8) {
            println!("    {f}");
        }
    } else {
        println!("  查不了：没有文件外的静态调用者——可能是入口/叶子，也可能调用关系是动态的");
    }
    let mut ids: Vec<String> = symbols.to_vec();
    ids.extend(other.iter().map(|(id, _)| id.clone()));
    print_known(store, &ids);
    print_boundary();
    0
}

/// 逐层往上游找 `calls` 边的入边。
fn upstream(store: &Store, key: &str, depth: usize) -> BTreeMap<usize, Vec<String>> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    seen.insert(key.to_string());
    let mut by_depth: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut frontier = vec![key.to_string()];
    for d in 1..=depth {
        let mut next = Vec::new();
        for id in &frontier {
            let Ok(srcs) = store.neighbors(id, EdgeKind::Calls, Dir::In) else {
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
    by_depth
}

fn print_layers(store: &Store, by_depth: &BTreeMap<usize, Vec<String>>) {
    for (d, ids) in by_depth {
        // 同层内按"被多少人调用"降序（规则与文件模式一致）：被调得越多，改它越容易伤到别处
        let mut ranked: Vec<(&String, usize)> = ids
            .iter()
            .map(|id| {
                let indeg = store
                    .neighbors(id, EdgeKind::Calls, Dir::In)
                    .map(|v| v.len())
                    .unwrap_or(0);
                (id, indeg)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        println!("  第 {d} 层（{} 个）:", ids.len());
        for (id, indeg) in ranked.iter().take(6) {
            let (file, name) = split_id(id);
            println!("    被 {indeg} 处调用　{name}（{file}）");
        }
        if ids.len() > 6 {
            println!("    …等 {} 个", ids.len());
        }
    }
}

/// 受影响的符号里，哪些被知识条目解释过（最多 5 条，超了报数）。
fn print_known(store: &Store, ids: &[String]) {
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
    known.sort();
    known.dedup();
    println!("  知识层（受影响的符号被这些主张解释过）：");
    for (sym, entry) in known.iter().take(5) {
        println!("    {sym} ← {entry}");
    }
    if known.len() > 5 {
        println!("    …等 {} 条", known.len());
    }
}

fn print_boundary() {
    println!("  边界：只看静态 calls 边；getattr / 反射 / 字符串调用、跨进程与外部调用都查不了");
}

/// `symbol:path#name` → (path, name)；不是这个形态就都给原串。
fn split_id(id: &str) -> (String, String) {
    let rest = id.strip_prefix("symbol:").unwrap_or(id);
    match rest.split_once('#') {
        Some((path, name)) => (path.to_string(), name.to_string()),
        None => (rest.to_string(), rest.to_string()),
    }
}



