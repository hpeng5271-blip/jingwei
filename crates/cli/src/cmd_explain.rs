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
                "callers": ex.callers.iter().map(|(f, l, s)| serde_json::json!({"file": f, "line": l, "by": s})).collect::<Vec<_>>(),
                "referenced_by": ex.references.iter().map(|(f, l, s)| serde_json::json!({"file": f, "line": l, "by": s})).collect::<Vec<_>>(),
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
    // `calls` 是**边数**（= 有几个符号在调它），`callers.len()` 是**调用点行数**——
    // 同一个符号在循环/多个分支里调多次会有多条站点。两个数都报，免得"3 处"下面列 5 行对不上。
    println!(
        "被调用：{} 个符号 / {} 个调用点",
        ex.calls,
        ex.callers.len()
    );
    for (f, l, s) in ex.callers.iter().take(5) {
        // 调用点后面直接给**谁在调**：只给行号，读的人还得翻到那一行
        println!("  {f}:{l} ← {}", caller_name(s));
    }
    println!("被引用：{} 个符号 / {} 个引用点", ex.refs, ex.references.len());
    if ex.calls == 0 && ex.refs == 0 {
        println!("判定依据：查不了（这个符号在库里没有任何边；可能没解析到，也可能真没人用）");
        // **别把"静态连不准"读成"没人用"**（2026-10-21）：接收者类型是泛型参数时，
        // 调用点写的是 `x.m()` 而 `x` 的类型推不出来；那类调用**静态连就是猜**，宁可不连。
        // 不解释的话，用户会以为这个方法真的没人调用。
        if id.contains('<') {
            println!(
                "  注：这个符号的类型带泛型参数（`Type<..>::m`）——它的调用方多半是泛型派发，静态连不准；说「查不了」不等于「没人用」"
            );
        }
    } else {
        println!("判定依据：图谱的 calls {} / references {} 条边（可用 jingwei graph stats 复核）", ex.calls, ex.refs);
    }
    for (f, l, s) in ex.references.iter().take(5) {
        println!("  {f}:{l} ← {}", caller_name(s));
    }
    // **复用**（2026-10-20；2026-10-21 改成给短名）：`explain` 是最常用的入口，知识层却要人
    // 另外想起来敲 `symbol`。这里直接报"哪几条"——但**必须走图边**（`claims_for_symbol`），
    // 不读 Markdown：两处各读一份迟早漂，而 verify/stale 都以库为准。
    let names = knowledge_names(&store, &ex.id);
    if !names.is_empty() {
        let total = names.len();
        let shown: Vec<&str> = names.iter().take(3).map(|s| s.as_str()).collect();
        if total > 3 {
            println!("知识层：{total} 条主张 —— {} 等 {total} 条", shown.join(" · "));
        } else {
            println!("知识层：{total} 条主张 —— {}", shown.join(" · "));
        }
    }
    // **改动影响面**（2026-10-21）：上面给的是**直接**调用者；间接的（第 2 层往上）在 impact 里。
    // 这里只报个数并指路——不重复打印清单，免得两个命令各说一套。
    let up = crate::cmd_impact::upstream(&store, &ex.id, 2);
    let indirect: usize = up.values().map(|v| v.len()).sum();
    let direct = ex.calls.max(0) as usize;
    if indirect > direct {
        println!(
            "改动影响面：往上游 2 层共 {indirect} 个符号（直接 {direct} 个）（jingwei impact {} 看清单与排序）",
            ex.id
        );
    }
    0
}

/// 调用者 id → 名字：`symbol:path#name` 取 `name`；`file:path` 取路径末段。
fn caller_name(id: &str) -> String {
    if let Some(rest) = id.strip_prefix("symbol:") {
        return rest.split_once('#').map(|(_, n)| n.to_string()).unwrap_or_else(|| rest.to_string());
    }
    if let Some(rest) = id.strip_prefix("file:") {
        return rest.rsplit('/').next().unwrap_or(rest).to_string();
    }
    id.to_string()
}

/// 短名：去 `.md`、去前导 `YYYY-MM-DD-`，剩下的就是"topic-序号-标题"那种能一眼睛看懂的形态。
pub(crate) fn short_name(id: &str) -> String {
    let s = id.strip_suffix(".md").unwrap_or(id);
    let b = s.as_bytes();
    let dated = s.len() > 11
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'-'
        && s[..4].chars().all(|c| c.is_ascii_digit());
    if dated {
        s[11..].to_string()
    } else {
        s.to_string()
    }
}

/// 这个符号被哪些主张解释过（**走图边**），按等级排（一手在前），返回短名。
fn knowledge_names(store: &Store, symbol_id: &str) -> Vec<String> {
    let mut claims = store.claims_for_symbol(symbol_id).unwrap_or_default();
    claims.sort_by(|a, b| {
        crate::cmd_symbol::level_rank(&a.level)
            .cmp(&crate::cmd_symbol::level_rank(&b.level))
            .then_with(|| a.id.cmp(&b.id))
    });
    claims.into_iter().map(|c| short_name(&c.id)).collect()
}

