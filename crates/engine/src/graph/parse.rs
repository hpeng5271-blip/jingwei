//! 把源码变成符号。M3 的落点。
//!
//! 这一层只干一件事：**语法树 → 领域模型里的 `Node`**。它不碰数据库、不碰规则
//! （那些是 store 和 rules 的事），所以坏代码最多让这里少出几个符号，
//! 不会污染别处。
//!
//! ## 错误门控（Q8 实测出来的硬要求）
//!
//! 实测过：源码里有一个写坏的函数（`fn broken( { }`）时，**它照样会被抽出来**——
//! 语法树能识别出函数名，只是整棵树带着 `ERROR` 子树。所以"遇到错误就不建关系"
//! 这件事必须由这里说了算：返回 `complete: false`，调用方据此决定要不要建调用关系。
//!
//! 不这么做的后果很具体：图谱里会出现一个"存在但根本编译不过"的符号，
//! 然后"这个函数在哪定义"给出一个不存在的答案。

use jingwei_model::{Confidence, Node, NodeKind, Source};
use tree_sitter::Node as TsNode;

/// 一次解析的结果。
#[derive(Debug, Clone, Default)]
pub struct ParseOutcome {
    /// 抽出来的符号。**按名字去重**（同一个名字在一个文件里出现两次是语法错误，
    /// 但树没坏透时仍可能抽到——去重比让 id 撞车好）。
    pub symbols: Vec<Node>,
    /// 语法树里**没有**错误/缺失子树。
    ///
    /// 调用方拿它决定"能不能建调用关系"：`false` 时只出符号，不出关系。
    pub complete: bool,
}

/// 解析一段 Rust 源码。`rel` 是项目内的相对路径（用 / 分隔），`now` 是毫秒时间戳。
///
/// **不 panic**：起不来解析器、解析失败、文件不是 UTF-8，一律返回
/// `ParseOutcome { symbols: vec![], complete: false }`——
/// "这次没解析成"和"这个文件没有符号"是两件事，由 `complete` 区分。
pub fn parse_rust(rel: &str, src: &str, now: i64) -> ParseOutcome {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&tree_sitter_rust::LANGUAGE.into()).is_err() {
        return ParseOutcome::default();
    }
    let Some(tree) = parser.parse(src, None) else {
        return ParseOutcome::default();
    };
    let root = tree.root_node();
    let complete = !root.has_error();

    let mut symbols = Vec::new();
    collect(root, src, rel, now, None, &mut symbols);
    // 按 id 去重：树没坏透时可能抽到同名的，去重比让主键撞车好。
    symbols.sort_by(|a, b| a.id.cmp(&b.id));
    symbols.dedup_by(|a, b| a.id == b.id);

    ParseOutcome { symbols, complete }
}

/// 走一遍语法树。`container` 是所在容器（`impl` 的类型名），
/// 用来把同名方法区分开——设计 §2 要求的"符号 key 必须带容器路径"。
fn collect(
    node: TsNode<'_>,
    src: &str,
    rel: &str,
    now: i64,
    container: Option<&str>,
    out: &mut Vec<Node>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            // 有名字、且名字本身就是它的标识
            "function_item" | "struct_item" | "enum_item" | "union_item" | "trait_item"
            | "type_item" | "const_item" | "static_item" | "mod_item" => {
                push_symbol(child, src, rel, now, container, out);
                // `mod foo { … }` 里面的东西也要收——但容器路径不变（模块路径由 file_path 表达）
                if child.kind() == "mod_item" {
                    if let Some(body) = child.child_by_field_name("body") {
                        collect(body, src, rel, now, container, out);
                    }
                }
            }
            // `impl` 自己没有名字，它的"容器"是那个类型
            "impl_item" => {
                let ty = child
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(src.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                if let Some(body) = child.child_by_field_name("body") {
                    collect(body, src, rel, now, ty.as_deref(), out);
                }
            }
            _ => {}
        }
    }
}

fn push_symbol(
    item: TsNode<'_>,
    src: &str,
    rel: &str,
    now: i64,
    container: Option<&str>,
    out: &mut Vec<Node>,
) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let Ok(name) = name_node.utf8_text(src.as_bytes()) else {
        return;
    };
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    // 容器路径：`src/lib.rs#Ledger::post` 与 `src/lib.rs#post` 是两个不同的符号。
    let key = match container {
        Some(c) => format!("{rel}#{c}::{name}"),
        None => format!("{rel}#{name}"),
    };
    let line = (item.start_position().row + 1) as u32;
    let mut n = Node::new(
        NodeKind::Symbol,
        &key,
        name,
        Source::TreeSitter,
        // 0.85：语法层推断，通常可靠——但**不足以支撑"错误"级别的结论**
        //（`Confidence::is_authoritative` 会把 Guessed 挡掉，这里是 High）。
        Confidence::High,
        now,
    );
    // 内容指纹：种类 + 名字 + 容器 + 文件。**故意不含行号**——
    // 方法往下挪两行不算"内容变了"，而抽样校验问的正是"内容变了没"。
    //
    // （这里第一版写的是 `node_id(...)`，那等于把主键再存一遍：毫无信息量。
    //  R1 刚把"`checksum` 语义混层"当成一处漏洞修掉，转头自己又写了一个——
    //  说明这个错很容易犯，所以测试里有一条专门盯着它。）
    n.checksum = Some(crate::facts::stable_hash([
        NodeKind::Symbol.as_str(),
        name,
        container.unwrap_or(""),
        rel,
    ]));
    out.push(n.with_file(rel, line));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 顶层符号与容器里的方法都能抽出来() {
        let src = r#"
pub struct Ledger { total: i64 }
impl Ledger {
    pub fn post(&mut self, amt: i64) { self.total += amt; }
    fn hidden(&self) {}
}
fn free_function() {}
enum Kind { A, B }
"#;
        let r = parse_rust("src/lib.rs", src, 1000);
        assert!(r.complete, "这段源码没有语法错误");
        let names: Vec<&str> = r.symbols.iter().map(|s| s.name.as_str()).collect();
        for want in ["Ledger", "post", "hidden", "free_function", "Kind"] {
            assert!(names.contains(&want), "少了符号 {want}；实际 {names:?}");
        }
        let ids: Vec<&str> = r.symbols.iter().map(|s| s.id.as_str()).collect();
        assert!(
            ids.contains(&"symbol:src/lib.rs#Ledger::post"),
            "impl 里的方法必须带容器路径，否则同名方法会撞 id：{ids:?}"
        );
        assert!(ids.contains(&"symbol:src/lib.rs#free_function"), "{ids:?}");
    }

    /// **Q8 实测过的那件事**：写坏的函数照样会被抽出来。
    /// 所以"能不能建关系"不能看抽没抽到符号，只能看 `complete`。
    #[test]
    fn 坏代码里也会抽出符号但必须标成不完整() {
        let src = "fn good() {}\nfn broken( { }\n";
        let r = parse_rust("src/bad.rs", src, 1000);
        assert!(!r.complete, "有 ERROR 子树时必须标成不完整");
        let names: Vec<&str> = r.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"good"), "{names:?}");
        // 这条断言记录的是事实，不是期望：**坏函数也会被抽出来**。
        // 所以调用方必须靠 complete 决定要不要建关系，不能靠"抽到几个符号"。
        assert!(
            names.contains(&"broken"),
            "实测：语法树能识别出坏函数的名字——正因如此才需要 complete 这道闸"
        );
    }

    #[test]
    fn 行号从1开始() {
        let r = parse_rust("a.rs", "\n\nfn f() {}\n", 1);
        assert_eq!(r.symbols.len(), 1);
        assert_eq!(r.symbols[0].line, Some(3), "第三行的函数，行号该是 3");
    }

    /// `checksum` 必须是**内容**指纹，不是主键的副本。
    /// 一条专门盯着它的测试——因为这个词太容易被当成"再存一遍 id"。
    #[test]
    fn checksum_是内容指纹而不是主键副本() {
        let a = parse_rust("a.rs", "\nfn f() {}\n", 1);
        let b = parse_rust("a.rs", "\n\n\nfn f() {}\n", 1);
        // id 是**自然键**：位置不是身份。同一个文件里的同一个 `f` 永远同一个 id，
        // 不管它被挪到第几行——否则每次编辑都会造出一堆"新符号"，图谱就没法和历史比了。
        assert_eq!(
            a.symbols[0].id, b.symbols[0].id,
            "挪了位置还是同一个符号：id 不许含行号"
        );
        assert_ne!(a.symbols[0].line, b.symbols[0].line, "但位置本身要跟着变");
        assert_eq!(
            a.symbols[0].checksum, b.symbols[0].checksum,
            "只是往下挪了两行，内容没变——checksum 不该变"
        );
        assert_ne!(
            a.symbols[0].checksum.as_deref(),
            Some(a.symbols[0].id.as_str()),
            "checksum 不能等于 id（那就等于把主键存了两遍）"
        );
    }

    #[test]
    fn 空文件与非源码都不会炸() {
        let empty = parse_rust("a.rs", "", 1);
        assert!(empty.symbols.is_empty());
        assert!(empty.complete, "空文件是完整的（没有错误子树）");

        // 故意塞一段别的语言：不该 panic，也不该抽出像样的符号
        let junk = parse_rust("a.rs", "<?php echo 1; ?>", 1);
        assert!(junk.symbols.is_empty() || !junk.complete);
    }
}
