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

use super::imports::{collect_imports, ImportFact};

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
    /// **调用点的行号**（`(调用者, 被调者, 行)`）：重命名要按行改（债表 0.25）。
    /// 与 `calls` 一一对应；分开存是为了不动既有断言。
    pub call_sites: Vec<(String, String, u32, u32)>,
    /// **解析不了的引用**——不只是计数，是事实本身。
    ///
    /// 这一条是读完 stack-graphs 之后改的。它那边的原话是：
    /// **索引期每个文件独立处理，抽出的"事实"包括"每个引用可能解析到的所有候选"**；
    /// 名字解析要等到**后面**（在图上做路径搜索）才能完成。
    ///
    /// 所以"解析不了"不该只留一个数字——**那个引用本身是有用的料**：
    /// 名字、在哪个函数里、第几行。跨文件解析那一步就靠它。
    pub references: Vec<RefFact>,
    /// **本文件内能确定的"只用不调"**：`(使用者 id, 被用的符号 id)`。
    ///
    /// 典型：`Cfg.DEBUG` 这种属性访问——它不是调用（没有调用边），但**确实用了**。
    /// 债表 0：以前这类关系一条都不收，于是"改了它会影响谁"答不出来。
    /// **跨文件模块属性**（债 0.28 / D-04）：`mod.CONST` 这类"只用不调"的限定名。
    ///
    /// 与 `references` 分开存：那个是**调用**的候选，解析成功要建 `calls` 边；
    /// 这个解析成功建的是 `references` 边。混在一起会把"读常量"画成"调用"（踩过）。
    pub attr_refs: Vec<RefFact>,
    /// **同文件属性引用的位置**（债 0.26 的邻居）：`Cfg.DEBUG` 这种"只用不调"。
    /// 与 `refs_local` 一一对应，多带行列——重命名/LSP 要靠它落到那一列。
    pub ref_sites_local: Vec<(String, String, u32, u32)>,
    pub refs_local: Vec<(String, String)>,

    /// 本文件的 `use` 语句——索引期就能确定的"名字 → 路径"映射。
    ///
    /// **为什么它属于索引期**：`use crate::util::winutil::which;` 这句话本身
    /// 就把"本文件里的 `which` 指那条路径"写死了，**不需要看别的文件**。
    /// （跨文件的那一步是"路径 → 符号 id"——那个才要全图。）
    pub imports: Vec<ImportFact>,
    /// **装饰器注册**：`(文件 id, 被注册的符号 id)`。
    ///
    /// 只收"带调用的装饰器"（`@app.route("/")`、`@click.command()`）——
    /// `@staticmethod` 这类不带调用的不算"注册给外部框架"。
    /// 依据：D-20261019-02。
    pub registers: Vec<(String, String)>,

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
    /// 引用的**作用域链，从内到外**。空 = 不在任何函数体里。
    ///
    /// 为什么是"链"而不是"当前在哪个函数"：真实的作用域是**嵌套**的
    /// （函数 > 函数 > 闭包），而且有**遮蔽**——内层同名遮蔽外层。
    ///
    /// stack-graphs 把这件事编码成"图上的 push/pop 节点 + 搜索时维护符号栈"，
    /// 弹栈时栈顶不匹配就不许走那条路。我们抄它的**形状**：
    /// 记一条链，解析时**从内往外找第一个匹配**——那就是遮蔽规则的直接实现。
    pub scope: Vec<String>,
    pub line: u32,
    /// 原始文本。`self.m()` / `Type::m()` 这种形状**原样留着**——
    /// 以后要做接收者类型推断时，需要看到完整的写法，而不是被我压扁的名字。
    pub raw: String,
    /// **推断出的期望路径**（`T::m`）：接收者类型在本文件里可见时填上（2026-10-21）。
    /// 有它的话，resolve 可以按"全图谱里键正好是 `T::m` 的符号"去连——跨文件方法调用靠它。
    pub want_path: Option<String>,
}

/// 解析一段 Rust 源码。`rel` 是项目内的相对路径（用 / 分隔），`now` 是毫秒时间戳。
///
/// **不 panic**：起不来解析器、解析失败、文件不是 UTF-8，一律返回
/// `ParseOutcome { symbols: vec![], complete: false }`——
/// "这次没解析成"和"这个文件没有符号"是两件事，由 `complete` 区分。
pub fn parse_rust(rel: &str, src: &str, now: i64) -> ParseOutcome {
    parse_rust_with_hints(
        rel,
        src,
        now,
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeMap::new(),
    )
}

/// **带"全项目签名提示"再解析一遍**（2026-10-21）。
///
/// 为什么需要：`let x = make();` 里 `make` 的定义可能在**另一个文件**，而解析是按文件进行的。
/// 提示由索引层从库里（上一轮解析时落下的 `ret` / `fields`）或本轮其它文件的结果里收集，
/// **只在有的文件里出现"接收者类型看不出来"的方法调用时才重解析这些文件**——不是全量重来。
pub fn parse_rust_with_hints(
    rel: &str,
    src: &str,
    now: i64,
    hints: &std::collections::BTreeMap<String, String>,
    field_hints: &std::collections::BTreeMap<(String, String), String>,
) -> ParseOutcome {
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
    let mut scope: Vec<String> = Vec::new();
    collect(root, src, rel, now, None, &mut scope, &mut symbols);
    // **闭包也收成符号**——它们有名字、有作用域，而且是"能被调用的局部绑定"。
    // 不收的话，`let has = |s| ...` 后面那句 `has("x")` 就永远找不到定义。
    let mut closures: Vec<(String, Vec<String>, u32)> = Vec::new();
    collect_closures(root, src, &[], &mut closures);
    for (name, owner_scope, closure_line) in closures {
        let key = symbol_key(rel, &owner_scope, None, &name);
        let mut n = Node::new(
            NodeKind::Symbol,
            &key,
            name.as_str(),
            Source::TreeSitter,
            Confidence::High,
            now,
        );
        // `local` 是 SCIP 那套 `Descriptor.Suffix` 里的一个槽位，
        // 这里借它的语义：**局部绑定与全局项不是一回事**。
        n.data = Some(serde_json::json!({ "sym": "local" }));
        n.checksum = Some(crate::facts::stable_hash([
            NodeKind::Symbol.as_str(),
            name.as_str(),
            "local",
            rel,
        ]));
        // **用闭包自己的行号**（2026-10-19：位置回读发现这里一律写 1，300 条符号对不上）
        symbols.push(n.with_file(rel, closure_line.max(1)));
    }
    let imports = collect_imports(root, src);
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
    let mut call_sites: Vec<(String, String, u32, u32)> = Vec::new();
    let mut scope: Vec<String> = Vec::new();
    // **本文件内"路径 → id"的反查表**：`L::post` → `symbol:a.rs#L::post`。
    //
    // 为什么需要它：`self.m()` 这种形状，目标不是"某个名字"，而是
    // **"某个类型上的某个方法"**。只靠名字查会撞上不同 impl 里的同名方法，
    // 而路径（`L::m`）是本文件里唯一的。
    let mut by_path: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for s in &symbols {
        if let Some((_, p)) = s.id.split_once('#') {
            by_path.insert(p.to_string(), s.id.clone());
        }
    }
    let mut vars: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        // **`结构体.字段 → 类型`**（2026-10-21）：`self.field.m()` 要靠它。
    // 只看本文件里 `struct S { f: T }` 这种**字面写着**的字段；别的一律不猜。
    let mut fields: std::collections::BTreeMap<(String, String), String> =
        std::collections::BTreeMap::new();
    {
        let mut c = root.walk();
        let all: Vec<TsNode> = {
            let mut stack = vec![root];
            let mut v = Vec::new();
            while let Some(n) = stack.pop() {
                v.push(n);
                let mut cc = n.walk();
                for ch in n.children(&mut cc) {
                    stack.push(ch);
                }
            }
            v
        };
        let _ = &mut c;
        for n in all {
            if n.kind() != "struct_item" {
                continue;
            }
            let Some(sname) = n
                .child_by_field_name("name")
                .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                .map(|s| s.trim().to_string())
            else {
                continue;
            };
            let Some(list) = n.child_by_field_name("body") else {
                continue;
            };
            let mut lc = list.walk();
            for f in list.children(&mut lc) {
                if f.kind() != "field_declaration" {
                    continue;
                }
                let (Some(fname), Some(fty)) = (
                    f.child_by_field_name("name"),
                    f.child_by_field_name("type"),
                ) else {
                    continue;
                };
                let (Ok(fname), Ok(fty)) = (
                    fname.utf8_text(src.as_bytes()),
                    fty.utf8_text(src.as_bytes()),
                ) else {
                    continue;
                };
                if let Some(t) = last_type_ident(fty) {
                    fields.insert((sname.clone(), fname.trim().to_string()), t);
                }
            }
        }
    }
    // **别处定义的 `struct` 也算**（2026-10-21）：`struct` 与 `impl` 常分居两个文件，
    // 这时 `self.field.m()` 的字段类型要看另一个文件。提示由索引层给；**本文件的优先**。
    for (k, v) in field_hints {
        fields.entry(k.clone()).or_insert_with(|| v.clone());
    }
    // **函数的返回类型**（2026-10-21）：`let x = make();` 之后 `x.m()` 要靠它。
    // 两张表：`returns` = 原样返回的类型（`-> S`）；`returns_unwrapped` = 拆掉 `Option`/`Result`
    // 之后的那层（`-> Option<S>` → `S`）。**调用处拆没拆包装，决定查哪张表**——
    // 不拆包装就查 unwrapped 表，等于把 `Option<S>` 当 `S`，会连出假边。
    let mut returns: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut returns_unwrapped: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    // 先放"全项目提示"（来自别的文件），**本文件里的签名随后覆盖**——本文件更权威。
    for (k, v) in hints {
        returns.insert(k.clone(), v.clone());
    }
    {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            let mut c = n.walk();
            for ch in n.children(&mut c) {
                stack.push(ch);
            }
            if n.kind() != "function_item" {
                continue;
            }
            let (Some(name), Some(rt)) = (
                n.child_by_field_name("name"),
                n.child_by_field_name("return_type"),
            ) else {
                continue;
            };
            let (Ok(name), Ok(rt)) = (name.utf8_text(src.as_bytes()), rt.utf8_text(src.as_bytes()))
            else {
                continue;
            };
            let name = name.trim().to_string();
            if rt.contains('<') {
                // 泛型返回：只认 `Option`/`Result` 的第一层，别的（`Vec<Foo>`、`Box<T>`）不猜
                if let Some(inner) = unwrap_inner_type(rt) {
                    returns_unwrapped.insert(name, inner);
                }
                continue;
            }
            if let Some(t) = last_type_ident(rt) {
                returns.insert(name, t);
            }
        }
    }
    collect_calls(
        root,
        src,
        rel,
        None,
        &mut scope,
        &mut vars,
        &fields,
        &returns,
        &returns_unwrapped,
        &by_name,
        &by_path,
        &mut calls,
        &mut refs,
        &mut call_sites,
    );
    // 同一个调用点可能在多处出现（同一个函数调 `helper()` 两次）——
    // 图上只该有一条边：**边表达的是"有关系"，不是"关系发生了几次"**。
    calls.sort();
    calls.dedup();
    refs.sort_by_key(|r| (r.line, r.name.clone()));

    ParseOutcome {
        symbols,
        calls,
        call_sites,
        references: refs,
        imports,
        registers: Vec::new(),
        attr_refs: Vec::new(),
        ref_sites_local: Vec::new(),
        refs_local: Vec::new(),
        complete,
    }
}

/// 抽 `use` 语句。
///
/// 只处理**能确定**的三种形状：
/// - `use a::b::c;` → `c` 指向 `a::b::c`
/// - `use a::b::c as d;` → `d` 指向 `a::b::c`
/// - `use a::b::{c, d};` → 展开成两条
///
/// **不处理**的：`use a::*;`（通配——一个名字可能来自任何地方，
/// 记下来也没法用，反而会让人以为"这里有个确定的名字"）。
/// 宁可少收，也不收一条用不上的。
/// 走一遍语法树，把**文件内能确定的调用**挑出来。
///
/// `scope` 是**作用域栈**（从外到内），它是这套抽取的骨架：
/// 没有它，只能知道"这个文件里有调用"，说不出**谁**调用了谁。
///
/// 为什么是栈而不是"当前函数"：函数可以嵌在函数里，闭包又嵌在函数里，
/// 而**内层同名会遮蔽外层**。栈正好表达这件事——解析时从栈顶往下找第一个匹配。
#[allow(clippy::too_many_arguments)]
fn collect_calls(
    node: TsNode<'_>,
    src: &str,
    rel: &str,
    container: Option<&str>,
    scope: &mut Vec<String>,
    vars: &mut std::collections::BTreeMap<String, String>,
    fields: &std::collections::BTreeMap<(String, String), String>,
    returns: &std::collections::BTreeMap<String, String>,
    returns_unwrapped: &std::collections::BTreeMap<String, String>,
    by_name: &std::collections::BTreeMap<&str, &str>,
    by_path: &std::collections::BTreeMap<String, String>,
    out: &mut Vec<(String, String)>,
    refs: &mut Vec<RefFact>,
    sites: &mut Vec<(String, String, u32, u32)>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" | "function_signature_item" => {
                // 进这个函数体时**压栈**，出去时弹栈——作用域链就是这么来的。
                let own = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(src.as_bytes()).ok())
                    // **和 `collect` 用同一套拼法**（同一个 `symbol_key`）——
                    // 两边各写一份的话，边就会指向一个不存在的节点，
                    // 而症状是"一条边都存不进去"（外键在拦），不报错、只是结果为空。
                    .map(|name| {
                        jingwei_model::node_id(
                            NodeKind::Symbol,
                            &symbol_key(rel, scope, container, name),
                        )
                    });
                vars.clear();
                // 形参上写着的类型也算（`fn run(s: &S)`）——这是"字面可见"的第二种
                if let Some(params) = child.child_by_field_name("parameters") {
                    let mut pc = params.walk();
                    for p in params.children(&mut pc) {
                        if p.kind() != "parameter" {
                            continue;
                        }
                        let (Some(pat), Some(ty)) = (
                            p.child_by_field_name("pattern"),
                            p.child_by_field_name("type"),
                        ) else {
                            continue;
                        };
                        if pat.kind() != "identifier" {
                            continue;
                        }
                        let (Ok(v), Ok(t)) = (
                            pat.utf8_text(src.as_bytes()),
                            ty.utf8_text(src.as_bytes()),
                        ) else {
                            continue;
                        };
                        if let Some(t) = last_type_ident(t) {
                            vars.insert(v.trim().to_string(), t);
                        }
                    }
                }
                    if let Some(body) = child.child_by_field_name("body") {
                    if let Some(o) = &own {
                        scope.push(o.clone());
                    }
                    collect_calls(
                        body,
                        src,
                        rel,
                        container,
                        scope,
                        vars,
                        fields,
                        returns,
                        returns_unwrapped,
                        by_name,
                        by_path,
                        out,
                        refs,
                        sites,
                    );
                    if own.is_some() {
                        scope.pop();
                    }
                }
            }
            // trait 默认方法体里的调用：src 是那个方法（与收符号那趟用同一套 id 拼法）
            "trait_item" => {
                let tname = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(src.as_bytes()).ok())
                    .map(|s| s.trim().to_string());
                if let (Some(tname), Some(body)) = (tname, child.child_by_field_name("body")) {
                    // **不 push scope**：容器已经是 trait 名；两样都写会拼成 Gen::Gen::x
                    collect_calls(body, src, rel, Some(&tname), scope, vars, fields, returns, returns_unwrapped, by_name, by_path, out, refs, sites);
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
                        scope,
                        vars,
                        fields,
                        returns,
                        returns_unwrapped,
                        by_name,
                        by_path,
                        out,
                        refs,
                        sites,
                    );
                }
            }
            "mod_item" => {
                if let Some(body) = child.child_by_field_name("body") {
                    collect_calls(
                        body, src, rel, container, scope, vars, fields, returns, returns_unwrapped, by_name, by_path, out, refs, sites,
                    );
                }
            }
            // **变量名 → 类型名**（2026-10-21）：方法调用 `x.m()` 要靠它。
            // 只收"字面写着"的那两种：`let x: T = …` 与 `let x = T::new(…)` / `let x = T { … }`。
            "let_declaration" => {
                if let Some(pat0) = child.child_by_field_name("pattern") {
                    // `let mut s = …` 的 pattern 是 `mut_pattern`（里面才是 identifier）。
                    // 少了这一步，`let mut store = Store::open(..)?` 之后的 `store.m()` 全连不上（2026-10-21 实测）。
                    let pat = if pat0.kind() == "mut_pattern" {
                        pat0.child_by_field_name("pattern")
                            .or_else(|| pat0.named_child(0))
                            .unwrap_or(pat0)
                    } else {
                        pat0
                    };
                    if pat.kind() == "identifier" {
                        if let Ok(var) = pat.utf8_text(src.as_bytes()) {
                            let declared = child
                                .child_by_field_name("type")
                                .and_then(|t| t.utf8_text(src.as_bytes()).ok())
                                .and_then(last_type_ident);
                            let inferred = child
                                .child_by_field_name("value")
                                .and_then(|v| ty_of_initializer(v, src, returns, returns_unwrapped, false));
                            if let Some(ty) = declared.or(inferred) {
                                vars.insert(var.trim().to_string(), ty);
                            }
                        }
                    }
                }
                collect_calls(
                    child, src, rel, container, scope, vars, fields, returns, returns_unwrapped, by_name, by_path, out, refs, sites,
                );
            }
            "call_expression" => {
                let Some(f) = child.child_by_field_name("function") else {
                    continue;
                };
                // 调用点行号：重命名要靠它把"改哪一行"算准（债表 0.25）。
                let call_line = child.start_position().row as u32 + 1;
                // 被调者的**行内字节列**（tree-sitter Point.column）：绝对偏移会被 CRLF 归一化搞错。
                let call_byte = f.start_position().column as u32;
                let txt = f.utf8_text(src.as_bytes()).unwrap_or("").trim().to_string();

                // ① **`self.m()` —— 接收者类型就在眼前，不用推断**。
                //
                //    在 `impl L { fn a(&self) { self.b(); } }` 里，
                //    `self` 的类型**由它所在的 impl 块写着**（就是 `L`）。
                //    这不需要任何跨文件类型推断，是"局部就能确定"的那一类——
                //    而这正是方法调用里最常见的一种。
                if f.kind() == "field_expression" {
                    let recv = f
                        .child_by_field_name("value")
                        .and_then(|v| v.utf8_text(src.as_bytes()).ok())
                        .map(|s| s.trim().to_string());
                    let field = f
                        .child_by_field_name("field")
                        .and_then(|v| v.utf8_text(src.as_bytes()).ok())
                        .map(|s| s.trim().to_string());
                    if let (Some("self"), Some(m), Some(c)) = (recv.as_deref(), field.as_deref(), container) {
                        let path = format!("{c}::{m}");
                        if let Some(id) = by_path.get(&path) {
                            if let Some(caller) = scope.last() {
                                if id != caller {
                                    out.push((caller.clone(), id.clone()));
                                    sites.push((caller.clone(), id.clone(), call_line, call_byte));
                                }
                            }
                            continue;
                        }
                    }
                    // ①b **`x.m()`，而 `x` 的类型在本函数里看得到**（2026-10-21）：
                    //     `let x = Store::open()` / `let x: Store = …` / 形参 `x: Store`。
                    //     **只认"字面写着"的类型**：不看 import、不做跨文件推断——
                    //     看不懂就退回"引用"，让统计把它算进"没连上"，绝不猜。
                    if let (Some(rv), Some(m)) = (recv.as_deref(), field.as_deref()) {
                        // ①b-0 `self.field.m()`：字段类型在同一个 `struct` 定义里写着
                        if let Some(rest) = rv.strip_prefix("self.") {
                            if !rest.contains('.') {
                                if let Some(c) = container {
                                    if let Some(ty) = fields.get(&(c.to_string(), rest.to_string())) {
                                        let path = format!("{ty}::{m}");
                                        if !by_path.contains_key(&path) {
                                            // 跨文件（`self.field.m()`：字段类型在本文件写着，方法在别处）
                                            if let Some(caller) = scope.last() {
                                                refs.push(RefFact {
                                                    name: path.clone(),
                                                    scope: vec![caller.clone()],
                                                    line: call_line,
                                                    raw: txt.clone(),
                                                    want_path: Some(path.clone()),
                                                });
                                            }
                                            continue;
                                        }
                                        if let Some(id) = by_path.get(&path) {
                                            let mut done = false;
                                            if let Some(caller) = scope.last() {
                                                if id != caller {
                                                    out.push((caller.clone(), id.clone()));
                                                    sites.push((
                                                        caller.clone(),
                                                        id.clone(),
                                                        call_line,
                                                        call_byte,
                                                    ));
                                                    done = true;
                                                }
                                            }
                                            if done {
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if rv != "self" {
                            if let Some(ty) = vars.get(rv) {
                                                                let path = format!("{ty}::{m}");
                                if !by_path.contains_key(&path) {
                                    // **跨文件**：本文件里没有这个 `T::m`，但类型已经字面可见。
                                    // 把期望路径留给 resolve（全图按路径找；找不到就丢，会被计数）。
                                    if let Some(caller) = scope.last() {
                                        refs.push(RefFact {
                                            name: path.clone(),
                                            scope: vec![caller.clone()],
                                            line: call_line,
                                            raw: txt.clone(),
                                            want_path: Some(path.clone()),
                                        });
                                    }
                                    continue;
                                }
                                if let Some(id) = by_path.get(&path) {
                                    if let Some(caller) = scope.last() {
                                        if id != caller {
                                            out.push((caller.clone(), id.clone()));
                                            sites.push((
                                                caller.clone(),
                                                id.clone(),
                                                call_line,
                                                call_byte,
                                            ));
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                }
                // 只认裸标识符。带 `::` 或 `.` 的都要真正的符号解析，这里不猜。
                let is_plain = !txt.is_empty()
                    && txt.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !txt.starts_with(|c: char| c.is_ascii_digit());
                // 当前作用域 = 栈顶（最内层的那个函数）
                let caller = scope.last().cloned();
                match (caller, is_plain) {
                    (Some(caller), true) => match by_name.get(txt.as_str()) {
                        Some(callee) => {
                            // 自己调自己不算一条边——那是递归，不是关系。
                            if *callee != caller.as_str() {
                                sites.push((caller.clone(), (*callee).to_string(), call_line, call_byte));
                                out.push((caller, (*callee).to_string()));
                            }
                        }
                        // 本文件里没有这个名字：**可能是外部的**（标准库、别的 crate），
                        // 也可能是同 crate 别的文件里的——两种都要等全局解析才分得清。
                        None => refs.push(RefFact {
                            name: txt.clone(),
                            scope: scope.clone(),
                            line: (f.start_position().row + 1) as u32,
                            raw: txt.clone(),
                            want_path: None,
                        }),
                    },
                    // 复杂形状（`self.m()` / `Type::m()`）：解析不了，
                    // 但**原始写法留着**——以后做接收者类型推断时要用。
                    (Some(_), false) => refs.push(RefFact {
                        name: txt.clone(),
                        scope: scope.clone(),
                        line: (f.start_position().row + 1) as u32,
                        raw: txt.clone(),
                        want_path: None,
                    }),
                    // 不在函数体里（比如 `const X = compute();`）——
                    // **它仍然是个引用**，只是作用域是模块级。
                    // 丢掉它等于假装"这个文件里没有这个引用"，而解析期要靠它连关系。
                    (None, _) => refs.push(RefFact {
                        name: txt.clone(),
                        scope: scope.clone(),
                        line: (f.start_position().row + 1) as u32,
                        raw: txt.clone(),
                        want_path: None,
                    }),
                }
                // **嵌套调用也要下钻**（2026-10-21 实测抓到的真缺口）：
                // `store.save().ok()` / `foo(bar())` 里的内层调用**不会被看见**——
                // 外层 `call_expression` 先处理掉，match 臂之间不会"落下继续"。
                // 症状：`x.m()` 只要被 `.ok()`/`.unwrap()` 包一层，方法调用就整条丢。
                collect_calls(
                    child, src, rel, container, scope, vars, fields, returns, returns_unwrapped, by_name, by_path, out, refs,
                    sites,
                );
            }
            // **其余节点一律下钻**。
            //
            // 第一版这里写的是 `_ => {}`，于是漏掉了一层：`call_expression` 通常
            // 嵌在 `expression_statement` 里，不下钻就永远看不见它——
            // 而症状是"一条边都抽不出来"，看起来像"这文件里没有调用"，
            // **不报错、不警告**。被测试当场抓住。
            _ => {
                collect_calls(child, src, rel, container, scope, vars, fields, returns, returns_unwrapped, by_name, by_path, out, refs, sites);
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
    scope: &mut Vec<String>,
    out: &mut Vec<Node>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            // 有名字、且名字本身就是它的标识
            "function_item" | "function_signature_item" | "struct_item" | "enum_item"
            | "union_item" | "trait_item"
            | "type_item" | "const_item" | "static_item" | "mod_item" => {
                let own = push_symbol(child, src, rel, now, container, scope, out);
                // **函数可以嵌在函数里**，那时它自己就是一层作用域——
                // 内层的符号 id 要带上外层的路径，否则同名嵌套函数会撞成同一个节点。
                // （这一条是测试逼出来的：原来两个嵌套函数各自独立成 `a.rs#outer` 和 `a.rs#inner`。）
                if child.kind() == "function_item" {
                    if let Some(o) = own.as_ref() {
                        if let Some(body) = child.child_by_field_name("body") {
                            scope.push(o.clone());
                            collect(body, src, rel, now, container, scope, out);
                            scope.pop();
                        }
                    }
                }
                // **trait 的默认方法也是符号**（2026-10-19）：trait 只收了自己，
                // body 从没走过 → 默认方法缺席 → 有边指向它时外键违约、整项目索引失败。
                // 容器用 trait 名，与 impl 方法同形（`Trait::method`）。
                if child.kind() == "trait_item" {
                    // `own` 在前面已经被 function_item 那支借走过，这里只要读它的值
                    if let Some(o) = own.clone() {
                        if let Some(body) = child.child_by_field_name("body") {
                            let tname = o.rsplit('#').next().unwrap_or("").to_string();
                            // **不 push scope**：容器已经带了 trait 名，两样都写会拼成 Gen::Gen::x
                            collect(body, src, rel, now, Some(&tname), scope, out);
                        }
                    }
                }
                // `mod foo { … }` 里面的东西也要收——但容器路径不变（模块路径由 file_path 表达）
                if child.kind() == "mod_item" {
                    if let Some(body) = child.child_by_field_name("body") {
                        collect(body, src, rel, now, container, scope, out);
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
                    collect(body, src, rel, now, ty.as_deref(), scope, out);
                }
            }
            _ => {}
        }
    }
}

/// 返回这个符号自己的 id（嵌套函数要用它当下一层的作用域）。
fn push_symbol(
    item: TsNode<'_>,
    src: &str,
    rel: &str,
    now: i64,
    container: Option<&str>,
    scope: &[String],
    out: &mut Vec<Node>,
) -> Option<String> {
    let name_node = item.child_by_field_name("name")?;
    let Ok(name) = name_node.utf8_text(src.as_bytes()) else {
        return None;
    };
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let key = symbol_key(rel, scope, container, name);
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
    // **符号的子类要记下来**，因为 Rust 有命名空间——
    // 《Rust Reference》的原话是"名字被分进不同的命名空间，
    // 让不同命名空间里的实体可以同名而不冲突"。
    //
    // 具体到这里：`foo()` 是个**调用**，它只可能在**值命名空间**里解析，
    // 所以同名的 struct / trait / mod 根本不该进候选。
    // 不记这一条的话，46 条"拿不准"里有相当一部分是白白拿不准的。
    let mut data = serde_json::json!({ "sym": symbol_class(item.kind()) });
    // **函数签名里的返回类型**也存下来（2026-10-21）：下一次索引别的文件时要拿它做提示——
    // `let x = make();` 里 `make` 的定义常常在另一个文件。只存"简单返回类型"（无泛型），
    // 与解析期 `returns` 表的口径一致，避免库里存一个解析期不会用的东西。
    if item.kind() == "function_item" {
        if let Some(rt) = item.child_by_field_name("return_type") {
            if let Ok(text) = rt.utf8_text(src.as_bytes()) {
                if !text.contains('<') {
                    if let Some(t) = last_type_ident(text) {
                        data["ret"] = serde_json::Value::String(t);
                    }
                }
            }
        }
    }
    // **结构体字段的类型也存下来**（2026-10-21）：`struct` 与 `impl` 常分居两个文件，
    // 别处解析 `self.field.m()` 时要靠它。只存简单类型（无泛型），与解析期 `fields` 同口径。
    if item.kind() == "struct_item" {
        if let Some(list) = item.child_by_field_name("body") {
            let mut map = serde_json::Map::new();
            let mut lc = list.walk();
            for f in list.children(&mut lc) {
                if f.kind() != "field_declaration" {
                    continue;
                }
                let (Some(fname), Some(fty)) = (
                    f.child_by_field_name("name"),
                    f.child_by_field_name("type"),
                ) else {
                    continue;
                };
                let (Ok(fname), Ok(fty)) = (
                    fname.utf8_text(src.as_bytes()),
                    fty.utf8_text(src.as_bytes()),
                ) else {
                    continue;
                };
                if fty.contains('<') {
                    continue;
                }
                if let Some(t) = last_type_ident(fty) {
                    map.insert(fname.trim().to_string(), serde_json::Value::String(t));
                }
            }
            if !map.is_empty() {
                data["fields"] = serde_json::Value::Object(map);
            }
        }
    }
    n.data = Some(data);
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
    let id = n.id.clone();
    out.push(n.with_file(rel, line));
    Some(id)
}

/// 语法节点 → 符号子类。
///
/// 只分到**够用**为止：我们要的是"它在哪个命名空间"，
/// 不是一份完整的 Rust 语法分类表。
fn symbol_class(kind: &str) -> &'static str {
    match kind {
        // 值命名空间：可以被 `foo()` 这样调用
        "function_item" | "const_item" | "static_item" => "value",
        // 类型命名空间：不能直接当函数调
        "struct_item" | "enum_item" | "union_item" | "trait_item" | "type_item" => "type",
        // 模块：既不是值也不是类型，单独算
        "mod_item" => "module",
        _ => "other",
    }
}

/// **局部绑定也是一种符号**（SCIP 的 `Descriptor.Suffix.Local` 就是这个意思）。
///
/// 为什么必须收：`let has = |s: &str| ...;` 之后那句 `has("x")` 是个**调用**，
/// 而它指向的东西**在我们图谱里根本不存在**——于是解析期只能说"拿不准"。
/// 实测里 `launch.rs` 一口气贡献了七条这样的"拿不准"，全是同一个闭包。
///
/// **为什么只收闭包，不收普通 `let`**：我们要回答的是"谁调用了谁"，
/// 而能被调用的局部绑定只有闭包。把每个 `let x = 5` 都塞进图谱，
/// 只会让图变胖而不解决任何问题。
///
/// 返回 `(名字, 它的作用域链, 行号)`——作用域链**必须一起带**：
/// 局部符号的可见性只在本函数内，跨函数同名是**两回事**。
fn collect_closures(
    node: TsNode<'_>,
    src: &str,
    scope: &[String],
    out: &mut Vec<(String, Vec<String>, u32)>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // `let has = |s| ...` —— 闭包挂在 let 绑定上
        if child.kind() == "let_declaration" {
            let name = child
                .child_by_field_name("pattern")
                .and_then(|p| p.utf8_text(src.as_bytes()).ok())
                .map(|s| s.trim().to_string());
            let is_closure = child
                .child_by_field_name("value")
                .map(|v| v.kind() == "closure_expression")
                .unwrap_or(false);
            if let (Some(n), true) = (name, is_closure) {
                // 只收简单标识符：`let (a, b) = ...` 这种解构先跳过
                if !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    // **闭包的真实行号**（2026-10-19 位置回读抓到：原来一律写 1）
                    let line = (child.start_position().row as u32) + 1;
                    out.push((n, scope.to_vec(), line));
                }
            }
        }
        // 往下走时维护作用域：进函数体就压一层
        if child.kind() == "function_item" {
            let own = child
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(src.as_bytes()).ok());
            let mut inner = scope.to_vec();
            if let Some(o) = own {
                inner.push(o.to_string());
            }
            // **函数参数里"能被调用的那些"也是局部绑定**——
            // `fn launch(cb: &mut dyn FnMut(Event))` 之后那句 `cb(Event::Stage{..})`
            // 是一个**调用**，而它指向的东西就在签名里写着。
            //
            // 只收类型里带 `Fn` 的（`Fn` / `FnMut` / `FnOnce` / `dyn Fn`）：
            // 普通参数（`x: u32`）不会被当函数调，收进来只会让图谱变胖。
            if let Some(params) = child.child_by_field_name("parameters") {
                let mut pc = params.walk();
                for p in params.children(&mut pc) {
                    if p.kind() != "parameter" {
                        continue;
                    }
                    let ty = p
                        .child_by_field_name("type")
                        .and_then(|t| t.utf8_text(src.as_bytes()).ok())
                        .unwrap_or("");
                    if !ty.contains("Fn") {
                        continue;
                    }
                    let nm = p
                        .child_by_field_name("pattern")
                        .and_then(|n| n.utf8_text(src.as_bytes()).ok())
                        .map(|s| s.trim().to_string());
                    if let Some(nm) = nm {
                        if !nm.is_empty() && nm.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            let line = (child.start_position().row as u32) + 1;
                            out.push((nm, inner.clone(), line));
                        }
                    }
                }
            }
            if let Some(body) = child.child_by_field_name("body") {
                collect_closures(body, src, &inner, out);
            }
        } else if child.kind() == "closure_expression" {
            // 闭包里再套闭包：作用域不变（都算在同一个函数里），内容继续找
            if let Some(body) = child.child_by_field_name("body") {
                collect_closures(body, src, scope, out);
            }
        } else if child.child_count() > 0 {
            collect_closures(child, src, scope, out);
        }
    }
}

/// **符号 key 的唯一拼法**（`collect` 与 `collect_calls` 共用一份）。
///
/// 容器路径 = 外层作用域路径 + `container`（impl 的类型）+ 自己：
/// `src/lib.rs#Ledger::post` 与 `src/lib.rs#post` 是两个不同的符号；
/// 嵌套函数 `outer::inner` 同理。
///
/// **这条链就是 stack-graphs 里那个"作用域栈"的静态形态**——
/// 它保证"同一个名字在不同作用域下是不同的东西"。
///
/// 返回的是**裸 key**（`a.rs#outer::inner`），不带 `symbol:` 前缀——
/// `Node::new` 内部会走 `node_id` 加上去。这里返完整 id 的话会**双前缀**
/// （`symbol:symbol:a.rs#...`），而症状是"边指向一个不存在的节点"。
fn symbol_key(rel: &str, scope: &[String], container: Option<&str>, name: &str) -> String {
    let mut parts: Vec<&str> = scope.iter().map(|s| short_name(s)).collect();
    if let Some(c) = container {
        parts.push(c);
    }
    parts.push(name);
    format!("{rel}#{}", parts.join("::"))
}

/// 从符号 id 里取回短名（`symbol:a.rs#L::a` → `L::a`）。
/// 作用域栈里存的是 id，拼 key 时用得上的是短名。
fn short_name(id: &str) -> &str {
    id.split_once('#').map(|(_, rest)| rest).unwrap_or(id)
}

/// 把 `Option<T>` / `Result<T, E>` 拆出**第一层**：`Option<Store>` → `Store`。
///
/// 只认这两种容器（2026-10-21）：别的泛型返回（`Vec<Foo>`、`Box<T>`、`impl Trait`）**不猜**——
/// 拆错了会连出假边，而"查不了"是允许的答案。
fn unwrap_inner_type(text: &str) -> Option<String> {
    let t = text.trim();
    let t = t.strip_prefix("->").map(str::trim).unwrap_or(t);
    let inner = t
        .strip_prefix("Option<")
        .or_else(|| t.strip_prefix("Result<"))?;
    let first = inner.split([',', '>']).next()?.trim();
    if first.is_empty() {
        return None;
    }
    last_type_ident(first)
}

/// 类型文本里最后一个"看起来像类型"的标识符：`&Store` → `Store`，`Vec<Foo>` → `Foo`。
/// 首字母不是大写的就当没有（`&mut self`、生命周期那种不该进这张表）。
fn last_type_ident(text: &str) -> Option<String> {
    // **取"基类型"（第一段大写标识符），不是最后一个**（2026-10-21 实测）：按 rsplit 取最后一段时，
    // `Channel<TSend>` 会取到泛型参数 `TSend`，于是连出指向不存在节点的边、整次索引失败。
    // `&Store` / `Vec<Foo>` / `dyn Trait` 都该取**前段**。
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|s| {
            !s.is_empty()
                && s.chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
        })
        .map(|s| s.to_string())
}

/// 初始化表达式推出类型：`T::new(…)` / `T { … }`；别的形状一律不猜。
///
/// 2026-10-21 补一层**包装**：`T::open(p).ok()?`、`x.unwrap()`、`x.await` 这些在外层套了
/// `?`/`unwrap`/`await`，里头的类型没变——不拆掉的话，`let s = Store::open(p).ok()?`
/// 之后的 `s.save()` 就永远连不上（实测：自己仓库里 `store.sync_claims(...)` 就是这个形状）。
fn ty_of_initializer(
    v: TsNode<'_>,
    src: &str,
    returns: &std::collections::BTreeMap<String, String>,
    returns_unwrapped: &std::collections::BTreeMap<String, String>,
    unwrapped: bool,
) -> Option<String> {
    match v.kind() {
        "struct_expression" => v
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(src.as_bytes()).ok())
            .map(|s| s.trim().to_string()),
        // `expr?`：类型不变
        "try_expression" => v
            .child_by_field_name("value")
            .or_else(|| v.named_child(0))
            .and_then(|inner| ty_of_initializer(inner, src, returns, returns_unwrapped, true)),
        // `expr.await`：类型不变（外层是 Future，这里取被 await 的那层）
        "await_expression" => v
            .child_by_field_name("value")
            .or_else(|| v.named_child(0))
            .and_then(|inner| ty_of_initializer(inner, src, returns, returns_unwrapped, true)),
        "call_expression" => {
            let f = v.child_by_field_name("function")?;
            // `.unwrap()` / `.expect(..)` / `.unwrap_or(..)` 这类：剥掉，看接收者
            if f.kind() == "field_expression" {
                let name = f
                    .child_by_field_name("field")
                    .and_then(|n| n.utf8_text(src.as_bytes()).ok())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if matches!(
                    name.as_str(),
                    "unwrap" | "expect" | "unwrap_or" | "unwrap_or_else" | "unwrap_or_default" | "ok"
                ) {
                    let recv = f.child_by_field_name("value")?;
                    return ty_of_initializer(recv, src, returns, returns_unwrapped, true);
                }
            }
            let t = f.utf8_text(src.as_bytes()).ok()?;
            let first = t.split("::").next()?.trim();
            if first.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                Some(first.to_string())
            } else {
                // `let x = make();` / `let x = crate::a::make();` —— 类型来自**被调函数的返回类型**。
                // 首段是小写时按**最后一段**查（`crate::a::make` → `make`）；`obj.make()` 那种
                // 接收者不是路径，走不到这里（要看接收者类型，不猜）。
                let last = t.rsplit("::").next().unwrap_or(first).trim();
                if unwrapped {
                    returns_unwrapped.get(last).cloned()
                } else {
                    returns.get(last).cloned()
                }
            }
        }
        _ => None,
    }
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
    /// **trait 的默认方法也是符号**（2026-10-19，真 tauri 撞出来的）。
    ///
    /// 原来 `trait_item` 只收了自己、**从不走 body**，于是默认方法缺席；
    /// 而有调用边指向它 → 外键违约 → **整个项目索引失败**。
    /// 这条同时钉住两件事：方法在（带 trait 前缀），以及方法体里的调用是对的源。
    #[test]
    fn trait_的默认方法也要算符号() {
        let src = r#"
pub trait Gen {
    fn name(&self) -> String;
    fn describe(&self) -> String {
        helper(self.name())
    }
}
fn helper(s: String) -> String { s }
"#;
        let r = parse_rust("src/t.rs", src, 1000);
        assert!(r.complete, "这段源码没有语法错误");
        let ids: Vec<&str> = r.symbols.iter().map(|s| s.id.as_str()).collect();
        assert!(
            ids.contains(&"symbol:src/t.rs#Gen::name"),
            "trait 的方法声明要有位置：{ids:?}"
        );
        assert!(
            ids.contains(&"symbol:src/t.rs#Gen::describe"),
            "**默认方法必须进符号表**，否则有边指向它时外键会把整个索引领翻：{ids:?}"
        );
        // 默认方法体里的调用，源必须是那个方法 —— 不是 trait 本身
        assert!(
            r.calls.iter().any(|(s, d)| s == "symbol:src/t.rs#Gen::describe"
                && d == "symbol:src/t.rs#helper"),
            "默认方法里调 helper 的边要挂在 describe 上：{:?}",
            r.calls
        );
    }

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

    /// **作用域是链，不是"当前函数"**。
    ///
    /// 嵌套函数的引用要记下**从内到外**的整条链——因为解析时要按
    /// "从内往外找第一个匹配"来做遮蔽。这一条是读完 stack-graphs 之后加的：
    /// 它把作用域编码成图上的 push/pop 节点，我们抄它的形状。
    #[test]
    fn 引用记下从内到外的作用域链() {
        let src = r#"
fn outer() {
    fn inner() {
        somewhere_else();
    }
}
"#;
        let r = parse_rust("a.rs", src, 1);
        assert_eq!(r.references.len(), 1, "{:?}", r.references);
        let rf = &r.references[0];
        assert_eq!(rf.name, "somewhere_else");
        assert_eq!(
            rf.scope,
            vec![
                "symbol:a.rs#outer".to_string(),
                "symbol:a.rs#outer::inner".to_string()
            ],
            "**栈顶是最内层**——从外往里压栈，所以最内层在末尾。
             解析时从末尾往前找第一个匹配，那就是遮蔽规则。"
        );
    }

    /// 不在函数体里的引用（比如 `const X: i32 = f();`）作用域链是空的——
    /// **它不是"在某个函数里"的引用**，硬塞一个归属会造假。
    #[test]
    fn 函数体外的引用作用域链为空() {
        let src = "const X: i32 = compute();\n";
        let r = parse_rust("a.rs", src, 1);
        assert_eq!(r.references.len(), 1, "{:?}", r.references);
        assert!(r.references[0].scope.is_empty(), "{:?}", r.references[0]);
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
























