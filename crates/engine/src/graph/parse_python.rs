//! Python 的语法树遍历。**第二门语言**（2026-10-16 加）。
//!
//! ## 为什么加它
//!
//! `graph/index.rs` 原来只收 `.rs`，于是**三个演示项目里两个（都是 Python）
//! 在图谱里一个节点都没有**：诊断挂不上文件、`window` 问不出"改了这里影响谁"。
//! 目标里"内核收口到能回答真实问题"这一条，对 Python 项目**从来没有成立过**。
//!
//! ## 为什么单独一个文件，而不是塞进 `parse.rs`
//!
//! 两条理由，第二条才是主要的：
//!
//! 1. `parse.rs` 已经 831 行，离门禁的 1000 行上限不远；加一门语言必然超。
//! 2. **两门语言的"什么算符号"根本不是一回事。** Rust 有 `impl` 块、
//!    `mod`、trait、生命周期；Python 有装饰器、`async def`、嵌套 class、
//!    以及"模块本身就是一层作用域"。把两套规则塞进一个 `collect` 里，
//!    改一门必然碰到另一门——这正是 `rundiag.rs` 那条教训
//!    （"按语言分家"、"一条通用规则必然有一边取错"）。
//!
//! ## 与 Rust 版**刻意保持一致**的地方
//!
//! 只收**裸标识符调用**（`foo()`），`self.m()` 靠容器路径（`C::m`）落地，
//! `Type.m()` / `mod.f()` 一律不收——**猜错了比不收更糟**，图谱上多一条
//! 信誓旦旦的假边比少一条边难查得多。这一条与 `parse.rs` 的文件头是同一条规矩。
//!
//! `complete` 的语义也一样：语法树里有 ERROR 就置 false，调用方据此**只出符号、
//! 不出关系**（Q8 实测出来的硬要求，见 `parse.rs` 文件头）。

use super::imports::ImportFact;
use super::parse::{ParseOutcome, RefFact};
use jingwei_model::{Confidence, Node, NodeKind, Source};
use tree_sitter::Node as TsNode;

/// 解析一段 Python 源码。签名与语义**刻意和 `parse_rust` 一致**。
///
/// **不 panic**：起不来解析器、解析失败、文件不是 UTF-8，一律返回
/// `ParseOutcome { symbols: vec![], complete: false }`——
/// "这次没解析成"和"这个文件没有符号"是两件事，由 `complete` 区分。
pub fn parse_python(rel: &str, src: &str, now: i64) -> ParseOutcome {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .is_err()
    {
        return ParseOutcome::default();
    }
    let Some(tree) = parser.parse(src, None) else {
        return ParseOutcome::default();
    };
    let root = tree.root_node();
    let complete = !root.has_error();

    let mut symbols = Vec::new();
    let mut scope: Vec<String> = Vec::new();
    // 容器栈：进 `class` 时压一个类型名，出时弹掉。
    // **它就是 Rust 那边的 `impl` 块**——`A.m` 与 `m` 是两个不同的符号。
    let mut container: Vec<String> = Vec::new();
    collect_defs(root, src, rel, now, &mut scope, &mut container, &mut symbols);

    let imports = collect_python_imports(root, src);

    // 名字 → 符号 id（本文件内）。同名只留第一个——与 Rust 那边同一套规矩。
    let mut by_name: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for s in &symbols {
        by_name.entry(s.name.as_str()).or_insert(s.id.as_str());
    }
    // 路径 → id（`C::m`）。`self.m()` 靠它落地。
    let mut by_path: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for s in &symbols {
        if let Some((_, p)) = s.id.split_once('#') {
            by_path.insert(p.to_string(), s.id.clone());
        }
    }

    let mut calls = Vec::new();
    let mut refs = Vec::new();
    let mut cscope: Vec<String> = Vec::new();
    let mut ccontainer: Vec<String> = Vec::new();
    collect_calls(
        root,
        src,
        rel,
        &mut cscope,
        &mut ccontainer,
        &imports,
        &by_name,
        &by_path,
        &mut calls,
        &mut refs,
    );

    symbols.sort_by(|a, b| a.id.cmp(&b.id));
    symbols.dedup_by(|a, b| a.id == b.id);
    calls.sort();
    calls.dedup();
    refs.sort_by_key(|r| (r.line, r.name.clone()));

    ParseOutcome {
        symbols,
        calls,
        references: refs,
        imports,
        complete,
    }
}

/// 符号 key 的拼法——**与 `parse.rs::symbol_key` 同一份格式**
/// （`rel#scope::Container::name`）。两份格式一旦分家，解析层就永远对不上。
fn symbol_key(rel: &str, scope: &[String], container: Option<&str>, name: &str) -> String {
    let mut parts: Vec<&str> = scope.iter().map(|s| short(s)).collect();
    if let Some(c) = container {
        parts.push(c);
    }
    parts.push(name);
    format!("{rel}#{}", parts.join("::"))
}

/// 从符号 **id** 里取回短名（`symbol:a.py#C::m` → `C::m`）。
///
/// **取 `#` 之后那一段，不是 `::` 的末段。** 前者才是"作用域路径"
/// （`C::m`），后者只剩 `m`——那会让 `A.m` 与 `B.m` 拼成同一个 key。
/// 与 `parse.rs::short_name` 必须**逐字一致**：两份一旦分家，
/// 嵌套符号的 id 就对不上，症状是"边指向一个不存在的节点"。
fn short(id: &str) -> &str {
    id.split_once('#').map(|(_, rest)| rest).unwrap_or(id)
}

/// 走一遍树，收 `def` / `class`（含嵌套与类里方法）。
fn collect_defs(
    n: TsNode<'_>,
    src: &str,
    rel: &str,
    now: i64,
    scope: &mut Vec<String>,
    container: &mut Vec<String>,
    out: &mut Vec<Node>,
) {
    let mut cursor = n.walk();
    for child in n.children(&mut cursor) {
        match child.kind() {
            // `def f()` / `async def f()`
            "function_definition" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let Some(name) = name else { continue };
                let key = symbol_key(rel, scope, container.last().map(|s| s.as_str()), &name);
                let mut node = Node::new(
                    NodeKind::Symbol,
                    &key,
                    name.clone(),
                    Source::TreeSitter,
                    Confidence::High,
                    now,
                );
                // `sym` 槽位与 Rust 那边一致：这个符号是哪一类东西。
                // Python 这边只分三种——`method`（在类里）/ `local`（在函数里）/ `other`。
                let class = if container.is_empty() {
                    if scope.is_empty() {
                        "other"
                    } else {
                        "local"
                    }
                } else {
                    "method"
                };
                node.data = Some(serde_json::json!({ "sym": class }));
                node.checksum = Some(crate::facts::stable_hash([
                    NodeKind::Symbol.as_str(),
                    name.as_str(),
                    class,
                    rel,
                ]));
                out.push(node);

                // 进函数体：里面的 `def` 是**嵌套函数**（`outer::inner`）
                if let Some(body) = child.child_by_field_name("body") {
                    // 推**完整 id**（带 `symbol:` 前缀）——与 `parse.rs` 的 `push_symbol`
                    // 一致，也让下游 `calls` 里的调用者 id 直接可用。
                    // `symbol_key` 拼 key 时会用 `short()` 把前缀和 `rel#` 都剥掉，
                    // 所以推 id 或推裸 key 拼出来的结果一样。
                    scope.push(format!("symbol:{key}"));
                    collect_defs(body, src, rel, now, scope, container, out);
                    scope.pop();
                }
            }
            // `class C:` / `class C(Base):`
            "class_definition" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let Some(name) = name else { continue };
                // **类自己也是一个符号**：`C` 能被 `C()` 调用。
                let key = symbol_key(rel, scope, container.last().map(|s| s.as_str()), &name);
                let mut node = Node::new(
                    NodeKind::Symbol,
                    &key,
                    name.clone(),
                    Source::TreeSitter,
                    Confidence::High,
                    now,
                );
                node.data = Some(serde_json::json!({ "sym": "class" }));
                node.checksum = Some(crate::facts::stable_hash([
                    NodeKind::Symbol.as_str(),
                    name.as_str(),
                    "class",
                    rel,
                ]));
                out.push(node);

                if let Some(body) = child.child_by_field_name("body") {
                    container.push(name);
                    collect_defs(body, src, rel, now, scope, container, out);
                    container.pop();
                }
            }
            // 缩进块是 Python 的结构，`if` / `for` / `try` 里面也可能有 `def`
            // （少见但合法：条件定义）。往下钻，别漏。
            "block" | "if_statement" | "for_statement" | "while_statement"
            | "try_statement" | "with_statement" | "else_clause" | "elif_clause"
            | "except_clause" | "finally_clause" | "decorated_definition" => {
                collect_defs(child, src, rel, now, scope, container, out);
            }
            _ => {}
        }
    }
}

/// 走一遍树，收「本文件内能确定的调用」与「解析不了的引用」。
#[allow(clippy::too_many_arguments)]
fn collect_calls(
    n: TsNode<'_>,
    src: &str,
    rel: &str,
    scope: &mut Vec<String>,
    container: &mut Vec<String>,
    imports: &[ImportFact],
    by_name: &std::collections::BTreeMap<&str, &str>,
    by_path: &std::collections::BTreeMap<String, String>,
    calls: &mut Vec<(String, String)>,
    refs: &mut Vec<RefFact>,
) {
    let mut cursor = n.walk();
    for child in n.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let key = symbol_key(rel, scope, container.last().map(|s| s.as_str()), &name);
                if let Some(body) = child.child_by_field_name("body") {
                    // 推**完整 id**（带 `symbol:` 前缀）——与 `parse.rs` 的 `push_symbol`
                    // 一致。`symbol_key` 拼 key 时会用 `short()` 把前缀和 `rel#` 都剥掉，
                    // 所以推 id 或推裸 key 拼出来的结果一样；但推 id 少一次转换，
                    // 而且下面 `caller` 直接就是 id，不用再加前缀。
                    scope.push(format!("symbol:{key}"));
                    collect_calls(
                        body, src, rel, scope, container, imports, by_name, by_path, calls, refs,
                    );
                    scope.pop();
                }
            }
            "class_definition" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                if let Some(body) = child.child_by_field_name("body") {
                    container.push(name);
                    collect_calls(
                        body, src, rel, scope, container, imports, by_name, by_path, calls, refs,
                    );
                    container.pop();
                }
            }
            "call" => {
                let Some(f) = child.child_by_field_name("function") else {
                    continue;
                };
                let text = f.utf8_text(src.as_bytes()).unwrap_or("").trim().to_string();
                let line = child.start_position().row as u32 + 1;

                match f.kind() {
                    // ① `self.m()` —— 和 Rust 一样，接收者类型就在眼前（所在 class）。
                    "attribute" => {
                        let recv = f
                            .child_by_field_name("object")
                            .and_then(|v| v.utf8_text(src.as_bytes()).ok())
                            .map(|s| s.trim().to_string());
                        let attr = f
                            .child_by_field_name("attribute")
                            .and_then(|v| v.utf8_text(src.as_bytes()).ok())
                            .map(|s| s.trim().to_string());
                        if let (Some("self"), Some(m), Some(c)) =
                            (recv.as_deref(), attr.as_deref(), container.last())
                        {
                            let path = format!("{c}::{m}");
                            if let Some(id) = by_path.get(&path) {
                                if let Some(caller) = scope.last() {
                                    // scope 里存的就是完整 id（带 `symbol:`），别再拼一次
                                    if *id != *caller {
                                        calls.push((caller.clone(), id.clone()));
                                    }
                                }
                            }
                        }
                        // **`Type.m()` / `mod.f()`：语法层看不出接收者是类型还是模块，
                        // 所以默认不收**（`obj.save()` 长得一模一样），与 `parse.rs` 同规矩。
                        //
                        // **但有一个例外，它不是猜**：接收者名字**是 `import` 引进来的**
                        // （`from hello_python import core` 之后的 `core.greet()`）——
                        // 那说明 `core` 是个模块，而这个事实**本文件里就写着**，
                        // 不需要任何跨文件推断。实测代价：`hello_python` 这个演示项目里
                        // `main.py` 调用 `core.greet()`，只靠"裸标识符"规则时
                        // **一条边都连不上**，于是"改了 greet 会影响谁"答不出来。
                        //
                        // 做法：把它变成一条**带 `::` 的引用**（`hello_python::core::greet`），
                        // 交给 `resolve::through_import`——那边已经有"路径 → 文件"的逻辑，
                        // 而且是"恰好一个候选才连"。
                        if let (Some(recv), Some(attr)) = (recv.as_deref(), attr.as_deref()) {
                            if let Some(imp) = imports.iter().find(|i| i.local == recv) {
                                refs.push(RefFact {
                                    name: format!("{}::{}", imp.path, attr),
                                    // **存完整 id，不是短名。**
                                    //
                                    // `resolve_file` 拿 `r.scope.last()` **直接当调用者 id**
                                    // 写进边里（`calls.push((caller.clone(), hit))`），
                                    // 所以这里必须是 `symbol:main.py#main`。
                                    // 第一版我写成 `short(s)` = `main`，于是索引在
                                    // 「写关系失败：main -calls-> symbol:…」上直接失败——
                                    // **是引擎自己报出来的**，不是我看出来的。
                                    // Rust 那边 `RefFact.scope` 存的也是完整 id
                                    // （`push_symbol` 返回 `n.id`），两门语言必须一致。
                                    scope: scope.clone(),
                                    line,
                                    raw: text.clone(),
                                });
                            }
                        }
                    }
                    // ② 裸标识符：`foo()`
                    "identifier" => {
                        if let Some(id) = by_name.get(text.as_str()) {
                            if let Some(caller) = scope.last() {
                                // scope 里存的就是完整 id，直接比、直接用
                                if caller.as_str() != *id {
                                    calls.push((caller.clone(), (*id).to_string()));
                                }
                            }
                        } else if !text.is_empty() {
                            // 本文件没定义 → **老实记成"解析不了"**，
                            // 跨文件那一步（`resolve`）拿着它去别处找。
                            refs.push(RefFact {
                                name: text.clone(),
                                // 同上面那条：**完整 id**，不是短名——
                                // `resolve_file` 直接拿它当调用者 id 写进边里。
                                scope: scope.clone(),
                                line,
                                raw: text.clone(),
                            });
                        }
                    }
                    _ => {}
                }
                // 参数里的调用也要收（`f(g())`）
                if let Some(args) = child.child_by_field_name("arguments") {
                    collect_calls(
                        args, src, rel, scope, container, imports, by_name, by_path, calls, refs,
                    );
                }
            }
            _ => {
                collect_calls(
                    child, src, rel, scope, container, imports, by_name, by_path, calls, refs,
                );
            }
        }
    }
}

/// 抽 `import` / `from ... import ...`，转成与 Rust `use` **同一形状**的事实。
///
/// | Python | `ImportFact.local` | `ImportFact.path` |
/// |---|---|---|
/// | `import a.b` | `a` | `a::b` |
/// | `import a.b as c` | `c` | `a::b` |
/// | `from a.b import f` | `f` | `a::b::f` |
/// | `from a.b import f as g` | `g` | `a::b::f` |
/// | `from . import f` | `f` | `::f`（相对：前导 `::` 表示"从当前包开始"） |
/// | `from .a import f` | `f` | `::a::f` |
/// | `from a.b import *` | —— | **不收**：通配符引进来多少个名字语法层不知道 |
///
/// **`import a.b` 的 `local` 是 `a` 不是 `b`**：Python 里 `import a.b` 之后
/// 只有 `a` 这个名字被绑定，用的时候要写 `a.b.f()`。这一条与 `from a.b import f`
/// 是**两种不同的绑定**——搞混了会让 `a.f()` 这种写法落到错的地方。
pub fn collect_python_imports(root: TsNode<'_>, src: &str) -> Vec<ImportFact> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() != "import_statement" && child.kind() != "import_from_statement" {
            continue;
        }
        let text = child.utf8_text(src.as_bytes()).unwrap_or("").trim().to_string();
        if child.kind() == "import_from_statement" {
            collect_from(&text, &mut out);
        } else {
            collect_plain(&text, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `from a.b import f, g as h`（也含前面那串点）。
fn collect_from(text: &str, out: &mut Vec<ImportFact>) {
    // `from` 与 `import` 之间是模块路径；`import` 之后是名字表。
    let body = text.strip_prefix("from ").unwrap_or(text);
    let Some((module_raw, names_raw)) = body.split_once(" import ") else {
        return;
    };
    let module_raw = module_raw.trim();
    // 前导点为相对导入：`.` 有几个就是往上几层。转成 `::` 前缀（`::a::b` = 从当前包起）。
    let dots = module_raw.chars().take_while(|c| *c == '.').count();
    let rest = &module_raw[dots..];
    let mut path_parts: Vec<String> = Vec::new();
    if dots > 0 {
        // 用空段表示"当前包"这一层——`resolve` 那一侧按段处理，空段被过滤掉，
        // 所以这里显式用 `"self"` 表示"当前包"，与 Rust 的 `self` 同义。
        for _ in 0..dots {
            path_parts.push("self".to_string());
        }
    }
    if !rest.is_empty() {
        path_parts.extend(rest.split('.').map(|s| s.trim().to_string()));
    }
    // 括号形式（`from a import (b, c)`）
    let names_raw = names_raw.trim().trim_start_matches('(').trim_end_matches(')');
    for one in names_raw.split(',') {
        let one = one.trim();
        if one.is_empty() || one == "*" {
            continue;
        }
        let (name, alias) = match one.split_once(" as ") {
            Some((n, a)) => (n.trim().to_string(), a.trim().to_string()),
            None => (one.to_string(), one.to_string()),
        };
        if name.is_empty() {
            continue;
        }
        let mut segs = path_parts.clone();
        segs.push(name);
        out.push(ImportFact {
            local: alias,
            path: segs.join("::"),
        });
    }
}

/// `import a.b, c as d`
fn collect_plain(text: &str, out: &mut Vec<ImportFact>) {
    let body = text.strip_prefix("import ").unwrap_or(text);
    for one in body.split(',') {
        let one = one.trim();
        if one.is_empty() {
            continue;
        }
        let (dotted, alias) = match one.split_once(" as ") {
            Some((n, a)) => (n.trim(), Some(a.trim().to_string())),
            None => (one, None),
        };
        let segs: Vec<&str> = dotted.split('.').filter(|s| !s.is_empty()).collect();
        if segs.is_empty() {
            continue;
        }
        // **`import a.b` 绑定的名字是首段 `a`**（见函数头那张表）。
        let local = alias.unwrap_or_else(|| segs[0].to_string());
        out.push(ImportFact {
            local,
            path: segs.join("::"),
        });
    }
}

#[cfg(test)]
#[path = "parse_python_tests.rs"]
mod tests;
