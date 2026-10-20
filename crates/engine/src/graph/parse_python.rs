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
        // **类属性不进"可被调用"的名字表**：`X = 1` 之后 `X()` 是调用一个值，
        // 不是调用我们的符号；放进去会凭空多出调用边（flask 实测 +16 条）。
        let is_attr = s
            .data
            .as_ref()
            .and_then(|d| d.get("sym"))
            .and_then(|v| v.as_str())
            == Some("attr");
        if is_attr {
            continue;
        }
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
    let mut call_sites: Vec<(String, String, u32, u32)> = Vec::new();
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
        &mut call_sites,
        None,
    );

    let mut registers: Vec<(String, String)> = Vec::new();
    let mut ref_sites_local: Vec<(String, String, u32, u32)> = Vec::new();
    let mut attr_refs: Vec<RefFact> = Vec::new();
    let aliases: std::collections::BTreeSet<String> = imports.iter().map(|i| i.local.clone()).collect();
    collect_attr_refs(root, src, rel, &by_path, &aliases, &mut ref_sites_local, &mut attr_refs);
    let refs_local: Vec<(String, String)> = ref_sites_local
        .iter()
        .map(|(a, b, _, _)| (a.clone(), b.clone()))
        .collect();
    collect_registers(root, src, rel, &by_name, &mut registers);

    symbols.sort_by(|a, b| a.id.cmp(&b.id));
    symbols.dedup_by(|a, b| a.id == b.id);
    calls.sort();
    calls.dedup();
    refs.sort_by_key(|r| (r.line, r.name.clone()));

    ParseOutcome {
        symbols,
        calls,
        call_sites,
        references: refs,
        imports,
        registers,
        attr_refs,
        ref_sites_local,
        refs_local,
        complete,
    }
}


/// 模块级常量的名字判据：`[A-Z][A-Z0-9_]*`（全大写）。
///
/// 只收全大写是**故意窄**：`x = 1` 这种普通变量在真项目里到处都是，
/// 收进来会让"谁被谁用"这张表被噪音淹没（pro 的判据，见 D-04）。
fn is_const_name(name: &str) -> bool {
    let mut it = name.chars();
    match it.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    it.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// 值判据：**字面量**才收。`True` / `None` / 数字 / 字符串（含相邻拼接）。
///
/// 不含调用、下标、属性访问——那些的"值"要运行时才知道，收成符号就是编。
fn is_literal(n: TsNode<'_>) -> bool {
    matches!(
        n.kind(),
        "true" | "false" | "none" | "integer" | "float" | "string" | "concatenated_string"
    )
}

/// 收集**属性访问**（债表 0）：`Cfg.DEBUG` 这类"只用不调"。
///
/// 只收**本文件内可解析**的：接收者名字 + 属性名能在 `by_path`（`C::X`）里找到。
/// 跨文件的（`os.sep`）不在这里编——那是 `resolve` 的活，猜错了比不收更糟。
/// 只在**符号体内**收（`scope` 非空）：模块级属性访问是 import 期副作用，
/// 与 D-20261019-02 的规矩一致。
fn collect_attr_refs(
    n: TsNode<'_>,
    src: &str,
    rel: &str,
    by_path: &std::collections::BTreeMap<String, String>,
    aliases: &std::collections::BTreeSet<String>,
    out: &mut Vec<(String, String, u32, u32)>,
    refs: &mut Vec<RefFact>,
) {
    let mut scope: Vec<String> = Vec::new();
    walk_attr(n, src, rel, by_path, aliases, &mut scope, out, refs);
}

#[allow(clippy::too_many_arguments)]
fn walk_attr(
    n: TsNode<'_>,
    src: &str,
    rel: &str,
    by_path: &std::collections::BTreeMap<String, String>,
    aliases: &std::collections::BTreeSet<String>,
    scope: &mut Vec<String>,
    out: &mut Vec<(String, String, u32, u32)>,
    refs: &mut Vec<RefFact>,
) {
    let mut cursor = n.walk();
    for child in n.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let key = symbol_key(rel, &[], None, name);
                if let Some(body) = child.child_by_field_name("body") {
                    scope.push(format!("symbol:{key}"));
                    walk_attr(body, src, rel, by_path, aliases, scope, out, refs);
                    scope.pop();
                }
            }
            "attribute" => {
                let is_call_fn = child
                    .parent()
                    .map(|p| p.kind() == "call")
                    .unwrap_or(false);
                if !is_call_fn {
                    if let Some(caller) = scope.last() {
                        let recv = child
                            .child_by_field_name("object")
                            .and_then(|v| v.utf8_text(src.as_bytes()).ok())
                            .map(|s| s.trim().to_string());
                        let attr = child
                            .child_by_field_name("attribute")
                            .and_then(|v| v.utf8_text(src.as_bytes()).ok())
                            .map(|s| s.trim().to_string());
                        if let (Some(recv), Some(attr)) = (recv, attr) {
                            if let Some(id) = by_path.get(&format!("{recv}::{attr}")) {
                                let pos = child
                                    .child_by_field_name("attribute")
                                    .map(|a| a.start_position())
                                    .unwrap_or_else(|| child.start_position());
                                out.push((
                                    caller.clone(),
                                    id.clone(),
                                    (pos.row + 1) as u32,
                                    pos.column as u32,
                                ));
                            } else if aliases.contains(&recv) {
                                // **跨文件模块属性**（债 0.28 / D-04）：只记事实，解析在 resolve。
                                refs.push(RefFact {
                                    name: format!("{recv}::{attr}"),
                                    scope: vec![caller.clone()],
                                    line: (child.start_position().row + 1) as u32,
                                    raw: format!("{recv}.{attr}"),
                                });
                            }
                        }
                    }
                }
                walk_attr(child, src, rel, by_path, aliases, scope, out, refs);
            }
            _ => walk_attr(child, src, rel, by_path, aliases, scope, out, refs),
        }
    }
}
/// 收集**装饰器注册**（D-20261019-02）：`@app.route"/"` / `@click.command()` 这类，
/// 把紧跟其后的函数"注册给外部框架"。
///
/// 只收**带调用**的装饰器（文本里有 `(`）：`@staticmethod` 不算"注册"。
/// 只认顶层定义——嵌套在函数里的装饰器不是路由入口，收了会污染角色。
fn collect_registers<'a>(
    root: TsNode<'_>,
    src: &str,
    rel: &str,
    by_name: &std::collections::BTreeMap<&'a str, &'a str>,
    out: &mut Vec<(String, String)>,
) {
    let file_id = format!("file:{rel}");
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() != "decorated_definition" {
            continue;
        }
        let Some(def) = child.child_by_field_name("definition") else {
            continue;
        };
        let Some(name) = def
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(src.as_bytes()).ok())
        else {
            continue;
        };
        // 装饰器本身：有 "(...)" 才算"注册给框架"
        let mut has_call = false;
        let mut dc = child.walk();
        for d in child.children(&mut dc) {
            if d.kind() == "decorator" {
                let txt = d.utf8_text(src.as_bytes()).unwrap_or("");
                if txt.contains('(') {
                    has_call = true;
                }
            }
        }
        if !has_call {
            continue;
        }
        if let Some(id) = by_name.get(name) {
            out.push((file_id.clone(), (*id).to_string()));
        }
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
                // **位置回读要能核到 Python 符号**（2026-10-19，债表 0.13c）：
                // 不写 file_path / line，`verify.rs` 会直接跳过——图谱里有 1,580 个符号
                // 却报"核了 0 个"，等于 Python 侧完全没有位置校验。
                node.file_path = Some(rel.to_string());
                node.line = Some(child.start_position().row as u32 + 1);
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
                // **位置回读要能核到 Python 符号**（2026-10-19，债表 0.13c）：
                // 不写 file_path / line，`verify.rs` 会直接跳过——图谱里有 1,580 个符号
                // 却报"核了 0 个"，等于 Python 侧完全没有位置校验。
                node.file_path = Some(rel.to_string());
                node.line = Some(child.start_position().row as u32 + 1);
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
            // **模块级常量**（D-20261020-04 A 路线，2026-10-20）：
            // `DEBUG = True` 这类**顶层、全大写、值是字面量**的赋值收成 `sym:"const"`。
            // 三条同时满足才收——判据宽一点就会把 `x = os.environ[...]` 这类
            // 运行时才定的东西也收成符号，而那正是"猜"。
            //
            // 为什么需要它：跨文件 `cfg.DEBUG` 要能连到 `cfg.py` 里的 `DEBUG`，
            // 而"符号不存在"让解析有答案也没落点（卡点见 D-04）。
            "expression_statement" if container.is_empty() && scope.is_empty() => {
                let Some(assign) = child.named_child(0) else {
                    continue;
                };
                if assign.kind() != "assignment" {
                    continue;
                }
                let (Some(left), Some(right)) = (
                    assign.child_by_field_name("left"),
                    assign.child_by_field_name("right"),
                ) else {
                    continue;
                };
                if left.kind() != "identifier" {
                    continue;
                }
                let Some(name) = left.utf8_text(src.as_bytes()).ok() else {
                    continue;
                };
                if !is_const_name(name) || !is_literal(right) {
                    continue;
                }
                let key = symbol_key(rel, &[], None, name);
                let mut node = Node::new(
                    NodeKind::Symbol,
                    &key,
                    name.to_string(),
                    Source::TreeSitter,
                    Confidence::High,
                    now,
                );
                node.file_path = Some(rel.to_string());
                node.line = Some(assign.start_position().row as u32 + 1);
                node.data = Some(serde_json::json!({ "sym": "const" }));
                node.checksum = Some(crate::facts::stable_hash([
                    NodeKind::Symbol.as_str(),
                    name,
                    "const",
                    rel,
                ]));
                out.push(node);
            }
            // **类体里的 `NAME = ...` 也是符号**（2026-10-19，债表 0）：
            // `class Cfg: DEBUG = True` 之后 `Cfg.DEBUG` 才有东西可指。
            // 只收**简单标识符**赋值；`self.x = 1` / `a[0] = 1` 不是类属性。
            "expression_statement" if !container.is_empty() => {
                let Some(assign) = child.named_child(0) else {
                    continue;
                };
                if assign.kind() != "assignment" {
                    continue;
                }
                let Some(left) = assign.child_by_field_name("left") else {
                    continue;
                };
                if left.kind() != "identifier" {
                    continue;
                }
                let Some(name) = left.utf8_text(src.as_bytes()).ok() else {
                    continue;
                };
                if name.is_empty() {
                    continue;
                }
                let key = symbol_key(rel, scope, container.last().map(|s| s.as_str()), name);
                let mut node = Node::new(
                    NodeKind::Symbol,
                    &key,
                    name.to_string(),
                    Source::TreeSitter,
                    Confidence::High,
                    now,
                );
                node.file_path = Some(rel.to_string());
                node.line = Some(assign.start_position().row as u32 + 1);
                node.data = Some(serde_json::json!({ "sym": "attr" }));
                node.checksum = Some(crate::facts::stable_hash([
                    NodeKind::Symbol.as_str(),
                    name,
                    "attr",
                    rel,
                ]));
                out.push(node);
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
    call_sites: &mut Vec<(String, String, u32, u32)>,
    module_caller: Option<&str>,
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
                        body, src, rel, scope, container, imports, by_name, by_path, calls, refs, call_sites, module_caller,
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
                        body, src, rel, scope, container, imports, by_name, by_path, calls, refs, call_sites, module_caller,
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
                                    call_sites.push((caller.clone(), id.clone(), line, f.start_position().column as u32));
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
                            // 模块级调用：D-20261019-02。__main__ 块里没有符号作用域，
                            // 调用者是文件本身——来源是事实：脚本被执行时顶层语句确实会跑。
                            let caller = scope
                                .last()
                                .cloned()
                                .or_else(|| module_caller.map(|s| s.to_string()));
                            if let Some(caller) = caller {
                                // scope 里存的就是完整 id，直接比、直接用
                                if caller.as_str() != *id {
                                    call_sites.push((caller.clone(), (*id).to_string(), line, f.start_byte() as u32));
                                    calls.push((caller, (*id).to_string()));
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
                        args, src, rel, scope, container, imports, by_name, by_path, calls, refs, call_sites, module_caller,
                    );
                }
            }
            "if_statement" => {
                // 条件取**整个 if 节点的文本**：`__name__ == "__main__"` 在语法里是
                // comparison_operator，字段名在不同 grammar 版本里不一定叫 condition——
                // 这里要的是"这是不是那条判据"，不是取子节点。
                let cond = child.utf8_text(src.as_bytes()).unwrap_or("");
                if cond.contains("__name__") && cond.contains("__main__") {
                    // tree-sitter-python 的 if 语句里，块叫 consequence，不叫 body
                    // （field 名写错时 child_by_field_name 返回 None，症状是「什么都没发生」）。
                    let body = child
                        .child_by_field_name("consequence")
                        .or_else(|| child.child_by_field_name("body"));
                    if let Some(body) = body {
                        let fid = format!("file:{rel}");
                        collect_calls(
                            body, src, rel, scope, container, imports, by_name, by_path, calls,
                            refs, call_sites, Some(fid.as_str()),
                        );
                    }
                } else {
                    collect_calls(
                        child, src, rel, scope, container, imports, by_name, by_path, calls, refs,
                        call_sites, module_caller,
                    );
                }
            }
            _ => {
                collect_calls(
                    child, src, rel, scope, container, imports, by_name, by_path, calls, refs, call_sites, module_caller,
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
