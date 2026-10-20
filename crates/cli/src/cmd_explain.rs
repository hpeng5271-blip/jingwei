//! `jingwei explain <名字|symbol:id>`（2026-10-20）：一个符号**到底是什么**。
//!
//! 为什么要它：图谱页能看到形状，但"这个函数是谁在调、被谁引用"以前只能靠编辑器
//! 的 LSP 或翻 SQL。理解层的第一个动词应该是"解释"，不是"看图"。
//!
//! **三态**：名字在库里但查不到定义 → 说"图谱里没有"；同名多义 → 列出来让你指定，
//! 不替你挑。

use std::path::Path;

use jingwei_engine::graph::{query, store::Store};

pub fn cmd_explain(root: &Path, name: &str, json: bool) -> i32 {
    let store: Store = match query::open_for_query(root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("开图谱失败：{e}（先跑 jingwei index）");
            return 2;
        }
    };

    let id = if name.starts_with("symbol:") {
        name.to_string()
    } else {
        match store.symbol_ids_by_name(name) {
            Ok(v) if v.len() == 1 => v[0].clone(),
            Ok(v) if v.is_empty() => {
                eprintln!("图谱里没有叫 `{name}` 的符号（先 index，或名字拼错了）");
                return 2;
            }
            Ok(v) => {
                eprintln!("有 {} 个符号叫 `{name}`，用 id 指定：", v.len());
                for x in &v {
                    eprintln!("  {x}");
                }
                return 2;
            }
            Err(e) => {
                eprintln!("查符号失败：{e}");
                return 2;
            }
        }
    };

    let Some(ex) = (match query::explain_symbol(&store, &id) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("读图谱失败：{e}");
            return 2;
        }
    }) else {
        eprintln!("图谱里没有这个符号：{id}");
        return 2;
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": ex.id, "name": ex.name, "file": ex.file, "line": ex.line,
                "calls": ex.calls, "references": ex.refs,
                "callers": ex.callers.iter().map(|(f, l)| serde_json::json!({"file": f, "line": l})).collect::<Vec<_>>(),
                "referenced_by": ex.references.iter().map(|(f, l)| serde_json::json!({"file": f, "line": l})).collect::<Vec<_>>(),
            }))
            .unwrap_or_default()
        );
        return 0;
    }

    println!("符号：{}  ({})", ex.name, ex.id);
    match (&ex.file, ex.line) {
        (Some(f), Some(l)) => println!("在哪：{f}:{l}"),
        (Some(f), None) => println!("在哪：{f}（行号未知）"),
        _ => println!("在哪：查不了（图谱里没记文件）"),
    }
    println!("被调用：{} 处", ex.calls);
    for (f, l) in ex.callers.iter().take(5) {
        println!("  {f}:{l}");
    }
    println!("被引用：{} 处", ex.refs);
    if ex.calls == 0 && ex.refs == 0 {
        println!("判定依据：查不了（这个符号在库里没有任何边；可能没解析到，也可能真没人用）");
    } else {
        println!("判定依据：图谱的 calls {} / references {} 条边（可用 jingwei graph stats 复核）", ex.calls, ex.refs);
    }
    for (f, l) in ex.references.iter().take(5) {
        println!("  {f}:{l}");
    }
    // **复用**（2026-10-20）：`explain` 是最常用的入口，知识层却要人另外想起来敲 `symbol`。
    // 这里只报"有几条"，不重复打印内容——内容仍在 `jingwei symbol <id>`，免得两处各说一套。
    if let Some(n) = knowledge_count(root, &ex.id) {
        match n {
            0 => {}
            _ => println!("知识层：{n} 条主张解释过它 → jingwei symbol {}", ex.id),
        }
    }
    0
}

/// 知识条目里挂在这个符号上的条数。`None` = 知识层不可用（不算错，只是不显示）。
fn knowledge_count(root: &Path, symbol_id: &str) -> Option<usize> {
    let entries = crate::cmd_knowledge::load_entries(root);
    if entries.is_empty() {
        return None;
    }
    Some(
        entries
            .iter()
            .filter(|e| crate::cmd_symbol::refers_to(e, symbol_id))
            .count(),
    )
}
