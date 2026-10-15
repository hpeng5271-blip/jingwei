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
    pub ambiguous: usize,
    /// 一个候选都没有：**外部符号**（标准库、别的 crate、宏生成的）。
    pub external: usize,
    /// 压根没有调用者的（模块级引用），单独算一类——
    /// 它们不是"解析失败"，只是"不属于任何函数"。
    pub module_level: usize,
}

/// 全局符号表：名字 → 所有同名符号的 id。
///
/// **为什么是"所有"而不是"一个"**：同名符号在不同文件里很常见
/// （`new`、`from_str`、`main`）。只留一个的话，等于随机挑一个连上去——
/// 那就是在猜。留着全部，才有机会说"这个拿不准"。
pub type SymbolTable = BTreeMap<String, Vec<String>>;

/// 把一个文件里的引用解析成关系。
///
/// `own_file` 是这个文件的符号（按名字分组），用来做**就近解析**；
/// `all` 是全图谱的符号表，用来做**跨文件解析**。
pub fn resolve_file(
    rel: &str,
    refs: &[RefFact],
    own_file: &SymbolTable,
    all: &SymbolTable,
) -> Resolution {
    let mut out = Resolution::default();
    for r in refs {
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
        match all.get(&r.name) {
            Some(ids) if ids.len() == 1 => {
                if let Some(caller) = r.scope.last() {
                    if &ids[0] != caller {
                        out.resolved.push((caller.clone(), ids[0].clone()));
                    }
                } else {
                    out.module_level += 1;
                }
            }
            Some(_) => out.ambiguous += 1,
            None => out.external += 1,
        }
    }
    out.resolved.sort();
    out.resolved.dedup();
    let _ = rel;
    out
}

/// 从作用域链**从内往外**找第一个匹配。
///
/// `scope` 是从外往内压的栈，所以**末尾是最内层**——从后往前找就对了。
/// 找不到任何一个匹配时，退回"这个文件里有唯一同名"也算命中
/// （函数定义在作用域外、但同文件可见的情况）。
fn nearest_in_scope(r: &RefFact, own_file: &SymbolTable) -> Option<String> {
    let ids = own_file.get(&r.name)?;
    if ids.is_empty() {
        return None;
    }
    // 从最内层往外：谁的作用域前缀最长，谁就最近。
    // 作用域链里存的是符号 id（`symbol:a.rs#L::a`），而候选 id 也是同一种形状，
    // 所以"前缀"这个词是字面意思：`…#L::a` 是 `…#L::a::inner` 的前缀。
    for scope_id in r.scope.iter().rev() {
        if let Some(hit) = ids.iter().find(|id| id.starts_with(scope_id.as_str())) {
            return Some(hit.clone());
        }
    }
    // 不在任何已知作用域里，但这个文件里只有一个同名——那就是它。
    if ids.len() == 1 {
        Some(ids[0].clone())
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
        res.ambiguous,
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
                    v.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                )
            })
            .collect()
    }

    #[test]
    fn 同文件里就近命中() {
        // 表的 key 是**名字**（解析时查的就是名字），值是所有同名符号的 id。
        let own = table(&[("helper", &["symbol:a.rs#helper"])]);
        let refs = vec![rf("helper", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &own, &own);
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
        let r = resolve_file("a.rs", &refs, &own, &all);
        assert!(r.resolved.is_empty());
        assert_eq!(r.ambiguous, 1);
    }

    /// 全图谱里**唯一**同名——可以连（保守，但这是"能确定"的那部分）。
    #[test]
    fn 跨文件唯一命中就连() {
        let own: SymbolTable = table(&[]);
        let all = table(&[("helper", &["symbol:b.rs#helper"])]);
        let refs = vec![rf("helper", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &own, &all);
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
        let r = resolve_file("a.rs", &refs, &own, &all);
        assert!(r.resolved.is_empty());
        assert_eq!(r.external, 1);
        assert_eq!(r.ambiguous, 0, "外部的不是拿不准——是压根不归我们管");
    }

    /// 模块级引用（不在任何函数里）单独算一类，
    /// 不能跟"解析失败"混在一起说。
    #[test]
    fn 模块级引用单独计数() {
        let own = table(&[("compute", &["symbol:a.rs#compute"])]);
        let refs = vec![rf("compute", &[])];
        let r = resolve_file("a.rs", &refs, &own, &own);
        assert_eq!(r.module_level, 1);
        assert!(r.resolved.is_empty(), "没有调用者就没有边");
    }
}
