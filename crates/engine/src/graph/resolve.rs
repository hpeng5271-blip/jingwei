//! 解析期：把**引用事实**变成**确定的关系**。
//!
//! ## 为什么单独一层
//!
//! 索引期每个文件独立处理（那是增量的前提，见设计 §6.2.1），
//! 所以那时只能说"这里引用了 `helper`"——**说不出它指谁**。
//! 跨文件的答案只有看到全图才有。这一层就是那一步。
//!
//! ## 自下而上，不是自顶向下
//!
//! rust-analyzer 讲 find-usages 的那篇点出了编译器与 IDE 的分工：
//!
//! > 编译器**自顶向下**：从语法树根开始遍历、维护一张符号表，最后为每个引用
//! > 记下它指向哪个定义；解析不了就报错。
//! > IDE **自下而上**：从那个引用出发，**往上**找父节点，遇到定义就返回——
//! > **它把无关的分支整个跳过去了。**
//!
//! 我们两边都要一点：
//!
//! - **自下而上**用于同文件：从引用出发，沿作用域链**从内往外**找第一个匹配；
//! - **全局查表**用于跨文件：只有当某个名字在全图谱里**唯一命中**时才连边。
//!
//! ## 三条出口，一条也不许含糊
//!
//! | 结果 | 落成什么 |
//! |---|---|
//! | **唯一命中** | 建边（`calls` / `references`），置信度语法层 |
//! | **命中多个** | **不猜**——记 `ambiguous`，由界面决定要不要显示 |
//! | **一个都没有** | 那是**外部符号**（标准库、别的 crate），记 `external`，**不是错误** |
//!
//! 最后那条最容易搞错：`std::mem::drop` 找不到定义不是缺陷，
//! 是我们本来就没索引标准库。把它算成"缺失"就是在制造误报——
//! 而这套系统最不许犯的错就是误报。

use std::collections::BTreeMap;

use jingwei_model::{Confidence, Edge, EdgeKind, Source};

use super::parse::RefFact;

/// 解析一轮的结果。**数字要能报到界面上**：
/// "连上了多少、有几个拿不准、有多少是外部的"——这三件事说法完全不同。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    /// 确定的关系：`(调用者 id, 被调者 id)`。
    pub resolved: Vec<(String, String)>,
    /// 命中多个候选——**不猜**。
    ///
    /// 记的是**名字本身**，不是个数。第一版记了个计数，
    /// 结果"46 条拿不准"这句话我一句都解释不了：**不知道该去看什么**。
    /// 记下名字，才有得查——这是这个项目反复学到的那条：
    /// **事实比计数有用**。
    pub ambiguous: Vec<String>,
    /// 一个候选都没有：**外部符号**（标准库、别的 crate、宏生成的）。
    pub external: usize,
    /// 压根没有调用者的（模块级引用），单独算一类——
    /// 它们不是"解析失败"，只是"不属于任何函数"。
    pub module_level: usize,
}

/// 符号表里的一条：id 加上它属于**哪个命名空间**。
///
/// 为什么必须带这个：《Rust Reference》说"名字被分进不同的命名空间，
/// 让不同命名空间里的实体可以同名而不冲突"。所以 `foo()` 这个**调用**
/// 只该在**值命名空间**里解析——同名的 struct / trait / mod 根本不该进候选。
///
/// 少了这一条，46 条"拿不准"里有相当一部分是**白白拿不准**的
/// （不是真的分不清，是我们压根没看名字的种类）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SymbolEntry {
    pub id: String,
    /// `value` / `type` / `module` / `other`（见 `parse::symbol_class`）。
    pub class: String,
}

/// 全局符号表：名字 → 所有同名符号。
///
/// **为什么是"所有"而不是"一个"**：同名符号在不同文件里很常见
/// （`new`、`from_str`、`main`）。只留一个的话，等于随机挑一个连上去——
/// 那就是在猜。留着全部，才有机会说"这个拿不准"。
pub type SymbolTable = BTreeMap<String, Vec<SymbolEntry>>;

/// 把一个文件里的引用解析成关系。
///
/// `own_file` 是这个文件的符号（按名字分组），用来做**就近解析**；
/// `all` 是全图谱的符号表，用来做**跨文件解析**。
pub fn resolve_file(
    rel: &str,
    refs: &[RefFact],
    own_file: &SymbolTable,
    imports: &[super::parse::ImportFact],
    all: &SymbolTable,
) -> Resolution {
    let mut out = Resolution::default();
    for r in refs {
        // ⓪ **先看这个文件自己写了什么 `use`**——那是它自己说出来的话，
        //    比任何推测都准：`use crate::util::winutil::which;` 就等于告诉了我们
        //    "本文件里的 `which` 是那个"。
        //
        //    这一步是这一轮唯一的新东西，**不再往细里打磨**——
        //    剩下的（`super::`、重导出、可见性）留给整体联调时一起收拾。
        if let Some(hit) = through_import(r, imports, all) {
            if let Some(caller) = r.scope.last() {
                if &hit != caller {
                    out.resolved.push((caller.clone(), hit));
                }
            } else {
                out.module_level += 1;
            }
            continue;
        }
        // ① 先在自己这个文件里找：沿作用域链**从内往外**找第一个匹配。
        //    这就是遮蔽规则——内层同名遮蔽外层。（stack-graphs 用一个符号栈
        //    在路径搜索时保证同一件事，我们这里退化成"从栈顶往下找"。）
        if let Some(hit) = nearest_in_scope(r, own_file) {
            if let Some(caller) = r.scope.last() {
                if &hit != caller {
                    out.resolved.push((caller.clone(), hit));
                }
            } else {
                out.module_level += 1;
            }
            continue;
        }

        // ② 自己文件里没有，去全图谱找。**只有唯一命中才连**——
        //    同名多个的话，凭什么挑那一个？
        //
        //    这里还没有做模块路径、`use` 语句、可见性的判断，
        //    所以"唯一命中"也仍然可能连错。**这不是终局**：
        //    等做到模块级解析时，这里要换成真正的路径匹配。
        //    现在这样做的代价是**宁可少连**，而不是乱连。
        // **只在值命名空间里找**：`foo()` 是调用，命中的必须是个能调的东西。
        // 同名的 struct / trait / mod 在这条路上一律不算候选。
        let values: Vec<&SymbolEntry> = all
            .get(&r.name)
            .map(|v| v.iter().filter(|e| e.class == "value").collect())
            .unwrap_or_default();
        match values.len() {
            1 => {
                if let Some(caller) = r.scope.last() {
                    if &values[0].id != caller {
                        out.resolved.push((caller.clone(), values[0].id.clone()));
                    }
                } else {
                    out.module_level += 1;
                }
            }
            0 => out.external += 1,
            // 值命名空间里也有多个——**这才是真的拿不准**，不猜；
            // 但把**名字**记下来，因为"46 条拿不准"这句话不告诉你该去看什么。
            // 记**名字 + 位置**：只有名字的话，看到 "which" 也不知道是哪一条，
            // 还得回去翻全文。位置是免费的，而且一眼能定位。
            _ => out.ambiguous.push(format!("{}:{}", rel, r.line)),
        }
    }
    out.resolved.sort();
    out.resolved.dedup();
    let _ = rel;
    out
}

/// 顺着本文件的 `use` 找：`which` → `crate::util::winutil::which` → 那个符号。
///
/// **路径 → 文件**这条映射是这里唯一的假设：`crate::a::b::name`
/// 对应文件 `a/b.rs` 里的 `name`。Rust 真实的模块树还要看 `mod` 声明在哪，
/// 但我们的目录结构和模块路径是对齐的（`src/graph/parse.rs` ↔ `crate::graph::parse`），
/// 所以这条假设在本项目里成立。
///
/// **它不成立的地方**：`pub use` 重导出、`#[path]` 属性、`mod.rs` 的层级。
/// 那些**留给整体联调**——现在这里只做"能确定的那一条路"。
fn through_import(
    r: &RefFact,
    imports: &[super::parse::ImportFact],
    all: &SymbolTable,
) -> Option<String> {
    let imp = imports.iter().find(|i| i.local == r.name)?;
    let segs: Vec<&str> = imp.path.split("::").filter(|s| !s.is_empty()).collect();
    // 只处理 `crate::…` 开头的（相对当前 crate 的路径）。
    // `super::` / `self::` 要结合当前模块位置算，留给整体联调。
    if segs.first() != Some(&"crate") || segs.len() < 2 {
        return None;
    }
    let body = &segs[1..];
    // 路径末段可能就是名字本身（`crate::a::b::which`），也可能指向模块
    // （`crate::a::b`）——两种情况都试一次。
    let (file_part, name_part) = if body.len() >= 2 {
        (body[..body.len() - 1].join("/"), body[body.len() - 1].to_string())
    } else {
        (body.join("/"), String::new())
    };
    let mut candidates = Vec::new();
    let mut hits: Vec<String> = Vec::new();
    if !name_part.is_empty() {
        candidates.push(format!("{file_part}.rs"));
    }
    // `use crate::graph::parse;` 这种：末段就是模块名
    let as_file = body.join("/");
    candidates.push(format!("{as_file}.rs"));
    candidates.push(format!("{as_file}/mod.rs"));

    for cand in candidates {
        let want_name = if name_part.is_empty() {
            r.name.clone()
        } else {
            name_part.clone()
        };
        // **用后缀匹配，不用全等**。
        //
        // 符号 id 里的路径是相对**索引根**的（`crates/engine/src/util/winutil.rs`），
        // 而 `use` 推出来的路径是相对 **crate 根**的（`util/winutil.rs`）——
        // 中间差一个 crate 根前缀。全等比对永远对不上，而后缀比对正好绕开这个差异。
        //
        // 这一条是实测出来的：改之前 `through_import` 一次都没命中，
        // 而每一步单看都是对的（use 抽对了、逻辑对、接线对）——
        // **差异藏在两个"路径"的参照系不同上**，光看代码看不出来。
        let tail = format!("/{cand}#{want_name}");
        if let Some(entries) = all.get(&want_name) {
            for e in entries {
                if e.id.ends_with(&tail) {
                    hits.push(e.id.clone());
                }
            }
        }
    }
    hits.sort();
    hits.dedup();
    // **恰好一个才连**：多个说明这条路径在不同 crate 里都有同名文件，
    // 而我们不知道当前在哪个 crate——那就回到"不猜"。
    match hits.len() {
        1 => Some(hits.remove(0)),
        _ => None,
    }
}

/// 从作用域链**从内往外**找第一个匹配。
///
/// `scope` 是从外往内压的栈，所以**末尾是最内层**——从后往前找就对了。
/// 找不到任何一个匹配时，退回"这个文件里有唯一同名"也算命中
/// （函数定义在作用域外、但同文件可见的情况）。
fn nearest_in_scope(r: &RefFact, own_file: &SymbolTable) -> Option<String> {
    let all = own_file.get(&r.name)?;
    // 同样只在值命名空间里找。
    let ids: Vec<&SymbolEntry> = all.iter().filter(|e| e.class == "value").collect();
    if ids.is_empty() {
        return None;
    }
    // 从最内层往外：谁的作用域前缀最长，谁就最近。
    // 作用域链里存的是符号 id（`symbol:a.rs#L::a`），而候选 id 也是同一种形状，
    // 所以"前缀"这个词是字面意思：`…#L::a` 是 `…#L::a::inner` 的前缀。
    for scope_id in r.scope.iter().rev() {
        if let Some(hit) = ids.iter().find(|e| e.id.starts_with(scope_id.as_str())) {
            return Some(hit.id.clone());
        }
    }
    // 不在任何已知作用域里，但这个文件里只有一个同名——那就是它。
    if ids.len() == 1 {
        Some(ids[0].id.clone())
    } else {
        None
    }
}

/// 把解析结果变成边。
///
/// **两端都是符号**，所以只可能是 `calls`（矩阵里 `calls => (Symbol, Symbol)`）。
pub fn to_edges(res: &Resolution, now: i64) -> Vec<Edge> {
    let mut out = Vec::new();
    for (src, dst) in &res.resolved {
        // 矩阵在构造期把关：不合法就返回 Err。这里已经保证两端都是符号，
        // 所以用 `from_row`（读路径）而不是 `new`（受检构造）——
        // 但**来源照样填**，因为"关系必有来源"这条线不能因为走了捷径就断。
        out.push(Edge::from_row(
            src.clone(),
            dst.clone(),
            EdgeKind::Calls,
            Source::TreeSitter,
            Confidence::High,
            now,
            None,
        ));
    }
    out
}

/// 给外部用的一句话（界面上的"这次解析连上了多少"）。
pub fn summary(res: &Resolution) -> String {
    format!(
        "连上 {} 条 · 拿不准 {} 条 · 外部符号 {} 个 · 模块级引用 {} 处",
        res.resolved.len(),
        res.ambiguous.len(),
        res.external,
        res.module_level
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn rf(name: &str, scope: &[&str]) -> RefFact {
        RefFact {
            name: name.into(),
            scope: scope.iter().map(|s| s.to_string()).collect(),
            line: 1,
            raw: name.into(),
        }
    }

    fn table(pairs: &[(&str, &[&str])]) -> SymbolTable {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    // 默认当成**值命名空间**（能调用的那种）——
                    // 引用都是从调用点抽出来的，本来就该在值命名空间里找。
                    v.iter()
                        .map(|s| SymbolEntry {
                            id: (*s).to_string(),
                            class: "value".to_string(),
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect()
    }

    /// **命名空间是白捡的确定性**。
    ///
    /// 一个 struct 和一个同名函数并存时，`foo()` 是调用，
    /// 它只可能指向那个函数——不该因为"有两个同名"就说拿不准。
    /// 《Rust Reference》管这叫"名字被分进不同的命名空间，
    /// 让不同命名空间里的实体可以同名而不冲突"。
    #[test]
    fn 同名的类型不该算作候选() {
        // 直接构造一个混合表：**同一个名字下，一个是值、一个是类型**。
        // （注意 `BTreeMap::extend` 是替换同 key，不是追加——第一版就是这么写错的。）
        let mut all: SymbolTable = BTreeMap::new();
        all.insert(
            "Foo".to_string(),
            vec![
                SymbolEntry {
                    id: "symbol:a.rs#Foo_fn".to_string(),
                    class: "value".to_string(),
                },
                SymbolEntry {
                    id: "symbol:a.rs#Foo_struct".to_string(),
                    class: "type".to_string(),
                },
            ],
        );
        let refs = vec![rf("Foo", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &SymbolTable::new(), &[], &all);
        assert_eq!(
            r.resolved,
            vec![(
                "symbol:a.rs#caller".to_string(),
                "symbol:a.rs#Foo_fn".to_string()
            )],
            "值命名空间里只有一个，那就是它"
        );
        assert!(r.ambiguous.is_empty());
    }

    #[test]
    fn 同文件里就近命中() {
        // 表的 key 是**名字**（解析时查的就是名字），值是所有同名符号的 id。
        let own = table(&[("helper", &["symbol:a.rs#helper"])]);
        let refs = vec![rf("helper", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &own, &[], &own);
        assert_eq!(
            r.resolved,
            vec![(
                "symbol:a.rs#caller".to_string(),
                "symbol:a.rs#helper".to_string()
            )]
        );
    }

    /// **唯一命中才跨文件连**——同名多个就不猜。
    /// 猜错一条边比漏一条糟得多：漏了只是图不全，错了是图在骗人。
    #[test]
    fn 跨文件同名不猜() {
        let own: SymbolTable = table(&[]);
        let all = table(&[(
            "new",
            &["symbol:a.rs#A::new", "symbol:b.rs#B::new"],
        )]);
        let refs = vec![rf("new", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &own, &[], &all);
        assert!(r.resolved.is_empty());
        assert_eq!(r.ambiguous, vec!["a.rs:1".to_string()], "现在记的是位置，能一眼定位");
    }

    /// 全图谱里**唯一**同名——可以连（保守，但这是"能确定"的那部分）。
    #[test]
    fn 跨文件唯一命中就连() {
        let own: SymbolTable = table(&[]);
        let all = table(&[("helper", &["symbol:b.rs#helper"])]);
        let refs = vec![rf("helper", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &own, &[], &all);
        assert_eq!(
            r.resolved,
            vec![(
                "symbol:a.rs#caller".to_string(),
                "symbol:b.rs#helper".to_string()
            )]
        );
    }

    /// **找不到定义是"外部符号"，不是错误**。
    /// `std::mem::drop` 找不到不是缺陷——我们本来就没索引标准库。
    /// 把它算成"缺失"就是在制造误报，而这套系统最不许犯的错就是误报。
    #[test]
    fn 找不到的是外部而不是缺失() {
        let own: SymbolTable = table(&[]);
        let all: SymbolTable = table(&[]);
        let refs = vec![rf("from_std", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &own, &[], &all);
        assert!(r.resolved.is_empty());
        assert_eq!(r.external, 1);
        assert!(r.ambiguous.is_empty(), "外部的不是拿不准——是压根不归我们管");
    }

    /// **顺着 `use` 找**：本文件写 `use crate::util::winutil::which;` 时，
    /// 那个 `which` 就确定指向 `util/winutil.rs#which`——这一步不需要看别的文件，
    /// 因为**这句话本身就写死了**。
    ///
    /// `git` 那条形状（`use crate::util::winutil::{run_cmd, which}`）
    /// 在 parse 里已经展开成两条，这里只面对"一个名字一个路径"。
    #[test]
    fn 顺着_use_能找到跨文件的定义() {
        use crate::graph::parse::ImportFact;
        let mut all: SymbolTable = BTreeMap::new();
        all.insert(
            "which".to_string(),
            vec![SymbolEntry {
                id: "symbol:util/winutil.rs#which".to_string(),
                class: "value".to_string(),
            }],
        );
        let imports = vec![ImportFact {
            local: "which".to_string(),
            path: "crate::util::winutil::which".to_string(),
        }];
        let refs = vec![rf("which", &["symbol:detect/ccpp.rs#probe_cc"])];
        let r = resolve_file("detect/ccpp.rs", &refs, &SymbolTable::new(), &imports, &all);
        assert_eq!(
            r.resolved,
            vec![(
                "symbol:detect/ccpp.rs#probe_cc".to_string(),
                "symbol:util/winutil.rs#which".to_string()
            )],
            "use 说得很清楚，不该拿不准：{:?}",
            r.ambiguous
        );
        assert!(r.ambiguous.is_empty());
    }
    /// 模块级引用（不在任何函数里）单独算一类，
    /// 不能跟"解析失败"混在一起说。
    #[test]
    fn 模块级引用单独计数() {
        let own = table(&[("compute", &["symbol:a.rs#compute"])]);
        let refs = vec![rf("compute", &[])];
        let r = resolve_file("a.rs", &refs, &own, &[], &own);
        assert_eq!(r.module_level, 1);
        assert!(r.resolved.is_empty(), "没有调用者就没有边");
    }
}
