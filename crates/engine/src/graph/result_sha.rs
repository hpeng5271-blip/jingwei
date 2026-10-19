//! 解析结果指纹（从 `index.rs` 拆出，2026-10-20：那个文件贴到 1000 行上限）。
//!
//! 它只包含"会进图的东西"（符号 id / 调用 / 引用 / 导入），排序后哈希——
//! **不含行号**，所以注释移动不会导致重写。

/// 解析结果的指纹：**只包含会进图的东西**（符号 id / 调用 / 引用 / 导入），排序后哈希。
///
/// 为什么不含行号：行号会随注释移动，而图的结构不变——含进来就永远截断不了。
pub fn parsed_result_sha(p: &crate::graph::parse::ParseOutcome) -> String {
    let mut parts: Vec<String> = Vec::new();
    for s in &p.symbols {
        parts.push(format!("S|{}|{}", s.id, s.name));
    }
    for (a, b) in &p.calls {
        parts.push(format!("C|{a}|{b}"));
    }
    for r in &p.references {
        parts.push(format!("R|{}|{}", r.name, r.scope.first().cloned().unwrap_or_default()));
    }
    for i in &p.imports {
        parts.push(format!("I|{}|{}", i.local, i.path));
    }
    parts.sort();
    crate::facts::stable_hash(parts.iter().map(|s| s.as_str()))
}
