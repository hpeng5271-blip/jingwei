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
        // `data` 也要进指纹（2026-10-21）：符号节点上存了 `sym` 命名空间与 `ret` 返回类型，
        // 而这些**会被后来的解析当提示用**。不把它算进指纹的话，改了 data 的解析结果会被
        // "早期截断"判定为"没变"→ 新 data 永远写不进库（实测：`ret` 第一版就是这样丢了）。
        let data = s.data.as_ref().map(|d| d.to_string()).unwrap_or_default();
        parts.push(format!("S|{}|{}|{}", s.id, s.name, data));
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

#[cfg(test)]
mod tests {
    use super::*;
    use jingwei_model::{Confidence, Node, NodeKind, Source};

    fn sym(name: &str, data: Option<serde_json::Value>) -> Node {
        let mut n = Node::new(
            NodeKind::Symbol,
            &format!("a.rs#{name}"),
            name,
            Source::TreeSitter,
            Confidence::High,
            0,
        );
        n.data = data;
        n
    }

    /// **`data` 必须进指纹**（2026-10-21 实测抓到的）。
    ///
    /// 由来：符号节点上存了命名空间（`sym`）和返回类型（`ret`），而 `ret` 会被后续解析当提示用。
    /// 第一版指纹只看 id/名字，于是"只改 data"的解析结果被判成"没变"→ **早期截断吞掉新 data**，
    /// 症状是"提示永远是空的、数字死活不涨"。这条测试守的就是它。
    #[test]
    fn 改_data_会改变解析结果指纹() {
        let a = crate::graph::parse::ParseOutcome {
            symbols: vec![sym("f", Some(serde_json::json!({"sym":"value","ret":"S"})))],
            ..Default::default()
        };
        let sha_a = parsed_result_sha(&a);
        let b = crate::graph::parse::ParseOutcome {
            symbols: vec![sym("f", Some(serde_json::json!({"sym":"value","ret":"T"})))],
            ..Default::default()
        };
        let sha_b = parsed_result_sha(&b);
        assert_ne!(sha_a, sha_b, "data 变了，指纹必须跟着变");
    }

    /// 反过来：data 一样时，指纹要稳定（否则每轮都判"变了"、增量就废了）。
    #[test]
    fn 同样的结果指纹稳定() {
        let a = crate::graph::parse::ParseOutcome {
            symbols: vec![sym("f", Some(serde_json::json!({"sym":"value"})))],
            ..Default::default()
        };
        let b = crate::graph::parse::ParseOutcome {
            symbols: vec![sym("f", Some(serde_json::json!({"sym":"value"})))],
            ..Default::default()
        };
        assert_eq!(parsed_result_sha(&a), parsed_result_sha(&b));
    }
}
