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
    /// **文件内能确定的调用关系**：`(调用者 id, 被调者 id)`。
    ///
    /// 只收一种形状：**裸标识符**（`foo()`）且 `foo` 在本文件里有定义。
    /// `self.m()` / `Type::m()` / `crate::f()` 一律不收——
    /// 它们需要真正的符号解析（找接收者类型、模块路径、可见性），
    /// **猜错了比不收更糟**：图谱上会多出一条信誓旦旦的假边。
    ///
    /// 宏调用（`println!`）也不会被收进来——它是 `macro_invocation` 不是 `call_expression`，
    /// 这一条正好落在设计 §6.1 说的"`format!` 是宏不是函数"上，**语法层直接帮我们挡住了**。
    pub calls: Vec<(String, String)>,
    /// **解析不了的引用**——不只是计数，是事实本身。
    ///
    /// 这一条是读完 stack-graphs 之后改的。它那边的原话是：
    /// **索引期每个文件独立处理，抽出的"事实"包括"每个引用可能解析到的所有候选"**；
    /// 名字解析要等到**后面**（在图上做路径搜索）才能完成。
    ///
    /// 所以"解析不了"不该只留一个数字——**那个引用本身是有用的料**：
    /// 名字、在哪个函数里、第几行。跨文件解析那一步就靠它。
    pub references: Vec<RefFact>,
    /// 语法树里**没有**错误/缺失子树。
    ///
    /// 调用方拿它决定"能不能建调用关系"：`false` 时只出符号，不出关系。
    pub complete: bool,
}

/// 一条**解析不了的引用**。
///
/// 它记的是"这里引用了这个名字"，**不是**"它指向谁"——
/// 后者要等全局解析才说得出来，而那时需要看到不止这一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefFact {
    /// 引用的名字（`foo()` 里的 `foo`）。
    pub name: String,
    /// 在哪个函数里引用的（符号 id）。`None` = 不在函数体里。
    pub from: Option<String>,
    pub line: u32,
    /// 原始文本。`self.m()` / `Type::m()` 这种形状**原样留着**——
    /// 以后要做接收者类型推断时，需要看到完整的写法，而不是被我压扁的名字。
    pub raw: String,
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

    // 名字 → 符号 id。**同名只留第一个**：本文件里同名符号本来就少见
    // （真要同名，语法上多半是坏的），而随便挑一个总比让 id 为空好——
    // 但那种情况下 `complete` 会是 false，调用方据此不放关系。
    let mut by_name: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for s in &symbols {
        by_name.entry(s.name.as_str()).or_insert(s.id.as_str());
    }

    let mut calls = Vec::new();
    let mut refs = Vec::new();
    collect_calls(root, src, rel, None, None, &by_name, &mut calls, &mut refs);
    // 同一个调用点可能在多处出现（同一个函数调 `helper()` 两次）——
    // 图上只该有一条边：**边表达的是"有关系"，不是"关系发生了几次"**。
    calls.sort();
    calls.dedup();
    refs.sort_by_key(|r| (r.line, r.name.clone()));

    ParseOutcome {
        symbols,
        calls,
        references: refs,
        complete,
    }
}

/// 走一遍语法树，把**文件内能确定的调用**挑出来。
///
/// `current` 是"此刻在哪个函数里"——它是这套抽取的骨架：
/// 没有它，只能知道"这个文件里有调用"，说不出**谁**调用了谁。
#[allow(clippy::too_many_arguments)]
fn collect_calls(
    node: TsNode<'_>,
    src: &str,
    rel: &str,
    container: Option<&str>,
    current: Option<&str>,
    by_name: &std::collections::BTreeMap<&str, &str>,
    out: &mut Vec<(String, String)>,
    refs: &mut Vec<RefFact>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                // 进这个函数体时，把"当前函数"换成它自己的 id
                let own = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(src.as_bytes()).ok())
                    .map(|name| match container {
                        // **走 `node_id`，不要手写拼**——第一版手写成 `a.rs#caller`，
                        // 少了 `symbol:` 前缀，于是和图里真正的节点对不上，
                        // 而症状是"一条边都存不进去"（外键在拦），不报错只有结果为空。
                        Some(c) => {
                            jingwei_model::node_id(NodeKind::Symbol, &format!("{rel}#{c}::{name}"))
                        }
                        None => jingwei_model::node_id(NodeKind::Symbol, &format!("{rel}#{name}")),
                    });
                if let Some(body) = child.child_by_field_name("body") {
                    collect_calls(
                        body,
                        src,
                        rel,
                        container,
                        own.as_deref(),
                        by_name,
                        out,
                        refs,
                    );
                }
            }
            "impl_item" => {
                let ty = child
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(src.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                if let Some(body) = child.child_by_field_name("body") {
                    collect_calls(
                        body,
                        src,
                        rel,
                        ty.as_deref(),
                        current,
                        by_name,
                        out,
                        refs,
                    );
                }
            }
            "mod_item" => {
                if let Some(body) = child.child_by_field_name("body") {
                    collect_calls(body, src, rel, container, current, by_name, out, refs);
                }
            }
            "call_expression" => {
                let Some(f) = child.child_by_field_name("function") else {
                    continue;
                };
                let txt = f.utf8_text(src.as_bytes()).unwrap_or("").trim().to_string();
                // 只认裸标识符。带 `::` 或 `.` 的都要真正的符号解析，这里不猜。
                let is_plain = !txt.is_empty()
                    && txt.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !txt.starts_with(|c: char| c.is_ascii_digit());
                match (current, is_plain) {
                    (Some(caller), true) => match by_name.get(txt.as_str()) {
                        Some(callee) => {
                            // 自己调自己不算一条边——那是递归，不是关系。
                            if *callee != caller {
                                out.push((caller.to_string(), (*callee).to_string()));
                            }
                        }
                        // 本文件里没有这个名字：**可能是外部的**（标准库、别的 crate），
                        // 也可能是同 crate 别的文件里的——两种都要等全局解析才分得清。
                        None => refs.push(RefFact {
                            name: txt.clone(),
                            from: Some(caller.to_string()),
                            line: (f.start_position().row + 1) as u32,
                            raw: txt.clone(),
                        }),
                    },
                    // 复杂形状（`self.m()` / `Type::m()`）：解析不了，
                    // 但**原始写法留着**——以后做接收者类型推断时要用。
                    (Some(caller), false) => refs.push(RefFact {
                        name: txt.clone(),
                        from: Some(caller.to_string()),
                        line: (f.start_position().row + 1) as u32,
                        raw: txt.clone(),
                    }),
                    (None, _) => {} // 不在函数体里（比如常量里的调用），不算谁的调用
                }
            }
            // **其余节点一律下钻**。
            //
            // 第一版这里写的是 `_ => {}`，于是漏掉了一层：`call_expression` 通常
            // 嵌在 `expression_statement` 里，不下钻就永远看不见它——
            // 而症状是"一条边都抽不出来"，看起来像"这文件里没有调用"，
            // **不报错、不警告**。被测试当场抓住。
            _ => {
                collect_calls(child, src, rel, container, current, by_name, out, refs);
            }
        }
    }
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

    /// 文件内的调用要被抽出来——**这是图谱从"树"变成"网"的第一步**。
    #[test]
    fn 文件内调用被抽成边() {
        let src = r#"
fn helper() {}
fn caller() { helper(); helper(); }
"#;
        let r = parse_rust("a.rs", src, 1);
        assert!(r.complete);
        assert_eq!(
            r.calls,
            vec![(
                "symbol:a.rs#caller".to_string(),
                "symbol:a.rs#helper".to_string()
            )],
            "调了两次同一件事，图上只该有一条边"
        );
        assert!(r.references.is_empty(), "{:?}", r.references);
    }

    /// **不确定的不许猜**：`self.m()`、`Type::m()`、别的模块里的函数——
    /// 全都进 `references`（引用事实），不建边。
    /// 猜错一条边比漏一条边糟得多：漏了只是图不全，错了是图在骗人。
    #[test]
    fn 复杂的调用形状不猜只计数() {
        let src = r#"
struct S;
impl S { fn m(&self) {} }
fn use_them() {
    let s = S;
    s.m();
    std::mem::drop(s);
    println!("hi");
}
"#;
        let r = parse_rust("a.rs", src, 1);
        assert!(r.calls.is_empty(), "一个都不该猜出来：{:?}", r.calls);
        assert!(
            r.references.len() >= 2,
            "两处复杂调用要被记成引用事实，实际 {:?}",
            r.references
        );
        // 而且**原始写法要留着**——以后做接收者类型推断时要用
        assert!(
            r.references.iter().any(|rf| rf.raw.contains("s.m")),
            "`s.m()` 的原始文本该被留下：{:?}",
            r.references
        );
    }

    /// 宏调用不是函数调用——`println!` 不该被当成边。
    /// 这正是设计 §6.1 说的"`format!` 是宏不是函数"，语法层直接挡住了。
    #[test]
    fn 宏调用不算调用关系() {
        let src = "fn a() { println!(\"x\"); }\nfn println() {}\n";
        let r = parse_rust("a.rs", src, 1);
        assert!(r.calls.is_empty(), "宏不该被当成调用：{:?}", r.calls);
    }

    /// 自己调自己不算边——那是递归，不是"关系"。
    #[test]
    fn 递归不产生自环() {
        let src = "fn f() { f(); }\n";
        let r = parse_rust("a.rs", src, 1);
        assert!(r.calls.is_empty(), "{:?}", r.calls);
    }

    /// impl 里的方法互相调用，边要带上容器路径（跟符号 id 一致）。
    #[test]
    fn 容器里的调用带容器路径() {
        let src = r#"
struct L;
impl L {
    fn a(&self) { Self::b(); }
    fn b(&self) {}
}
"#;
        let r = parse_rust("a.rs", src, 1);
        // `Self::b()` 是 scoped 形状，按规矩不猜
        assert!(r.calls.is_empty(), "{:?}", r.calls);

        let src2 = r#"
struct L;
impl L {
    fn a(&self) { helper(); }
}
fn helper() {}
"#;
        let r2 = parse_rust("a.rs", src2, 1);
        assert_eq!(
            r2.calls,
            vec![(
                "symbol:a.rs#L::a".to_string(),
                "symbol:a.rs#helper".to_string()
            )],
            "{:?}",
            r2.calls
        );
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
