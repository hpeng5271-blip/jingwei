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

pub use super::resolve_pyattr::{attr_pairs, resolve_attr_refs};
use super::resolve_pyattr::through_python_module_attr;
use super::parse::RefFact;
use super::resolve_imports::through_import;

/// 解析一轮的结果。**数字要能报到界面上**：
/// "连上了多少、有几个拿不准、有多少是外部的"——这三件事说法完全不同。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    /// 确定的关系：`(调用者 id, 被调者 id)`。
    pub resolved: Vec<(String, String)>,
    /// **调用点** `(调用者, 被调者, 行号)`：LSP 要跳到调用行，不是定义行（2026-10-19）。
    pub sites: Vec<(String, String, u32)>,
    /// 命中多个候选——**不猜**。
    ///
    /// 记的是**名字本身**，不是个数。第一版记了个计数，
    /// 结果"46 条拿不准"这句话我一句都解释不了：**不知道该去看什么**。
    /// 记下名字，才有得查——这是这个项目反复学到的那条：
    /// **事实比计数有用**。
    pub ambiguous: Vec<String>,
    /// 一个候选都没有：**外部符号**（标准库、别的 crate、宏生成的）。
    pub external: usize,
    /// **本文件写了 `use`，但那条路径我们接不上**——名字 + 位置。
    ///
    /// 为什么必须单独一类：它现在被算进 `external` 里，而两者是完全不同的事。
    /// 外部符号是"不归我们管"（`Vec` / `println!` 本来就没索引标准库）；
    /// 这一类是"**归我们管、我们没接上**"——比如 `super::` 相对路径、
    /// 重导出链、可见性。把它们混在一起报，等于把**自己的漏接**
    /// 伪装成"外部"，而这类错在这套系统里最不该犯：它看起来一切正常。
    ///
    /// 这一条是这一轮学到的：`ambiguous` 降到 0 之后，剩下的数字是
    /// "外部 4155 个"，而那个数字**没法判断图是不是真的全**——
    /// 因为它把"标准库"和"我们漏了"加在了一起。
    pub unmapped_imports: Vec<String>,
    /// 名字在**图谱里有**，但它不落在值命名空间里——位置。
    ///
    /// 两种可能，都得看：要么是调用点真的写错了（把结构体当函数调），
    /// 要么是我们的命名空间判断没跟上。**不猜**，报出来给人看。
    pub namespace_mismatch: Vec<String>,
    /// 压根没有调用者的（模块级引用），单独算一类——
    /// 它们不是"解析失败"，只是"不属于任何函数"。
    pub module_level: usize,
    /// **点号形（方法调用 `x.m()`）却没连上的引用数**（2026-10-21 量化用）。
    /// 这是"已知盲点有多大"的量尺：有数才谈得上修不修。
    pub method_unresolved: usize,
    /// 点号形引用**总数**（含已连上的）：和上面那个一起看才知道"是没连"还是"根本没进来"。
    pub method_refs: usize,
    /// 点号形里**目标看起来是我们自己的符号**（末段能在值命名空间里找到）却没连上的条数。
    /// 这才是"我们的缺口"；`vec.push` / `iter.map` 那些是别人的库，不算。
    pub method_unresolved_own: usize,
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
    imports: &[super::imports::ImportFact],
    all: &SymbolTable,
) -> Resolution {
    let mut out = Resolution::default();
    for r in refs {
        if r.raw.contains('.') {
            out.method_refs += 1;
        }
        // ⓪− **Python 的模块属性**（债 0.28 / D-04）：`mod.CONST` / `pkg.mod.CONST`。
        //      前缀必须正好是本文件的某条 import（不许全图按名猜）；
        //      模块路径 → 文件按 `pkg::cfg` ↔ `pkg/cfg.py` 的约定走。
        if rel.ends_with(".py") && r.name.contains("::") {
            if let Some(hit) = through_python_module_attr(r, imports, all) {
                if let Some(caller) = r.scope.last() {
                    if &hit != caller {
                        out.sites.push((caller.clone(), hit.clone(), r.line));
                        out.resolved.push((caller.clone(), hit));
                    }
                } else {
                    out.module_level += 1;
                }
                continue;
            }
        }
        // ⓪ **先看这个文件自己写了什么 `use`**——那是它自己说出来的话，
        //    比任何推测都准：`use crate::util::winutil::which;` 就等于告诉了我们
        //    "本文件里的 `which` 是那个"。
        //
        //    这一步是这一轮唯一的新东西，**不再往细里打磨**——
        //    剩下的（`super::`、重导出、可见性）留给整体联调时一起收拾。
        if let Some(hit) = through_import(r, imports, all, rel) {
            if let Some(caller) = r.scope.last() {
                if &hit != caller {
                    out.sites.push((caller.clone(), hit.clone(), r.line));
                    out.resolved.push((caller.clone(), hit));
                }
            } else {
                out.module_level += 1;
            }
            continue;
        }
        // ⓪.5 **写了 use 却没接上**：那是我们漏了，不是"外部"。
        //      走这一步之前 `through_import` 已经失败，所以这里只判"有没有这句 use"。
        if imports.iter().any(|i| i.local == r.name) {
            out.unmapped_imports.push(format!("{}:{} {}", rel, r.line, r.name));
            continue;
        }
        // ① 先在自己这个文件里找：沿作用域链**从内往外**找第一个匹配。
        //    这就是遮蔽规则——内层同名遮蔽外层。（stack-graphs 用一个符号栈
        //    在路径搜索时保证同一件事，我们这里退化成"从栈顶往下找"。）
        if let Some(hit) = nearest_in_scope(r, own_file) {
            if let Some(caller) = r.scope.last() {
                if &hit != caller {
                    out.sites.push((caller.clone(), hit.clone(), r.line));
                    out.resolved.push((caller.clone(), hit));
                }
            } else {
                out.module_level += 1;
            }
            continue;
        }

        // ①.6 **模块路径 `mod::item`：先按"兄弟/子模块文件"落，再谈全图唯一**（2026-10-21）。
        //
        //     为什么不靠 ①.5 的末段唯一命中：那个规则在**同类名字有多个**时会拒绝连
        //     （"凭什么挑那一个"）。而 `info::command(...)` 的写法**自己已经说了是哪个文件**，
        //     不该被别的同名 `command` 连累。实测（tauri）：`lib.rs:283` 的 `info::command(options)?`
        //     一条边都没有，于是 `impact` 对那整个模块只能报"查不了"。
        //
        //     只认**最窄的一种**：`a::b` 里 `a` 是小写标识符（模块名约定），先把 `a` 落到
        //     本文件同目录的 `a.rs` / `a/mod.rs`；落到了、且那个文件里真有 `b`，才连。
        //     `crate::` / `super::` / `self::` 一律先不管——那几条要模块树，留给后续。
        if let Some(hit) = through_module_path(rel, r, all) {
            if let Some(caller) = r.scope.last() {
                if &hit != caller {
                    out.sites.push((caller.clone(), hit.clone(), r.line));
                    out.resolved.push((caller.clone(), hit));
                }
            } else {
                out.module_level += 1;
            }
            continue;
        }
        // ①.7 **带前缀的模块路径**：`crate::a::b` / `self::b` / `super::b`（2026-10-21）。
        //     不知道 crate 根时按路径后缀找候选，并要求全图唯一；多个候选就放弃（宁少勿错）。
        if r.name.starts_with("crate::") || r.name.starts_with("self::") || r.name.starts_with("super::") {
            if let Some(hit) = through_prefixed_path(rel, r, all) {
                if let Some(caller) = r.scope.last() {
                    if &hit != caller {
                        out.sites.push((caller.clone(), hit.clone(), r.line));
                        out.resolved.push((caller.clone(), hit));
                    }
                } else {
                    out.module_level += 1;
                }
                continue;
            }
        }

        // ①.5 **限定名先拆掉路径，按末段查。**（2026-10-16 实测抓出的真缺口）
        //
        // `cmd_doctor.rs` 里的 `rules::run_all(...)` / `snapshot::load_latest(...)`
        // 原来**一条边都连不上**——整个项目的跨文件调用只有 80 条。
        // 根因：parser 把**原始写法**记进 `RefFact.name`（留着是对的，以后要做接收者推断），
        // 而这里直接拿它查表，**表的键是短名** → 永远查不到 → 全落进"外部符号"。
        //
        // 为什么"只取末段"就够：**下一步的"唯一命中才连"自带安全性**——
        // `load_latest` 全图只有一个 → 连；`new` 有几十个 → 记进 `ambiguous`，不连。
        // 歧义由既有规则兜住，这里只需让**本来是唯一命中的那些**别被路径前缀挡在门外。
        // 修后跨文件调用 80 → 265（`tools/graph_probe.mjs` 量的）。
        let lookup: &str = match r.name.rsplit_once("::") {
            Some((_, last)) if !last.is_empty() => last,
            _ => r.name.as_str(),
        };

        // ② 自己文件里没有，去全图谱找。**只有唯一命中才连**——
        //    同名多个的话，凭什么挑那一个？
        //
        //    这里还没有做模块路径、`use` 语句、可见性的判断，
        //    所以"唯一命中"也仍然可能连错。**这不是终局**：
        //    等做到模块级解析时，这里要换成真正的路径匹配。
        //    现在这样做的代价是**宁可少连**，而不是乱连。
        // **只在值命名空间里找**：`foo()` 是调用，命中的必须是个能调的东西。
        // 同名的 struct / trait / mod 在这条路上一律不算候选。
        // **值命名空间 = `value` + `local`**：闭包是能被调用的局部绑定，
        // 它在符号空间里的位置和函数一样（只是可见范围小得多）。
        let values: Vec<&SymbolEntry> = all
            .get(lookup)
            .map(|v| {
                v.iter()
                    .filter(|e| e.class == "value" || e.class == "local")
                    .collect()
            })
            .unwrap_or_default();
        match values.len() {
            1 => {
                if let Some(caller) = r.scope.last() {
                    if &values[0].id != caller {
                        out.resolved.push((caller.clone(), values[0].id.clone()));
                    out.sites.push((caller.clone(), values[0].id.clone(), r.line));
                    }
                } else {
                    out.module_level += 1;
                }
            }
             0 => {
                    if r.raw.contains('.') {
                        out.method_unresolved += 1;
                        if method_targets_own(r, all) {
                            out.method_unresolved_own += 1;
                        }
                    }
                // **名字在图谱里、但不在值命名空间**——别把它混进"外部"。
                // 外部是"不归我们管"，这个是"有，但不是能调的东西"。
                // 用 `lookup`（末段）而不是 `r.name`：限定名的情况下
                // 表里有的也是短名（与上面那段同一个道理）。
                if all.contains_key(lookup) {
                    out.namespace_mismatch.push(format!("{}:{} {}", rel, r.line, r.name));
                } else {
                    out.external += 1;
                }
            }
            // 值命名空间里也有多个——**这才是真的拿不准**，不猜；
            // 但把**名字**记下来，因为"46 条拿不准"这句话不告诉你该去看什么。
            // 记**名字 + 位置**：只有名字的话，看到 "which" 也不知道是哪一条，
            // 还得回去翻全文。位置是免费的，而且一眼能定位。
            _ => {
                if r.raw.contains('.') {
                    out.method_unresolved += 1;
                        if method_targets_own(r, all) {
                            out.method_unresolved_own += 1;
                        }
                }
                out.ambiguous.push(format!("{}:{}", rel, r.line))
            }
        }
    }
    out.resolved.sort();
    out.resolved.dedup();
    let _ = rel;
    out
}

/// 点号形引用的末段是不是"我们自己的值符号"（能调的东西）。
fn method_targets_own(r: &RefFact, all: &SymbolTable) -> bool {
    let last = match r.raw.rsplit_once('.') {
        Some((_, l)) => l,
        None => return false,
    };
    all.get(last)
        .map(|v| v.iter().any(|e| e.class == "value" || e.class == "local"))
        .unwrap_or(false)
}

/// `mod::item` 的窄解析：把 `mod` 落到**本文件同目录**的 `mod.rs` / `mod/mod.rs`，
/// 再看那个文件里有没有 `item`。
///
/// 只处理最窄的一种形状（见调用处的注释）；`crate::` / `super::` / `self::` 前缀一律不接。
fn through_module_path(rel: &str, r: &RefFact, all: &SymbolTable) -> Option<String> {
    let (module, item) = r.name.split_once("::")?;
    if module.is_empty()
        || item.is_empty()
        || item.contains("::")
        || !module.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        || matches!(module, "crate" | "super" | "self")
    {
        return None;
    }
    let dir = match rel.rsplit_once('/') {
        Some((d, _)) => format!("{d}/"),
        None => String::new(),
    };
    let entries = all.get(item)?;
    for cand in [format!("{dir}{module}.rs"), format!("{dir}{module}/mod.rs")] {
        for e in entries
            .iter()
            .filter(|e| e.class == "value" || e.class == "local")
        {
            if id_is_symbol_in(&e.id, &cand, item) {
                return Some(e.id.clone());
            }
        }
    }
    None
}


/// **带前缀的模块路径**：`crate::a::b` / `super::b` / `self::b`（2026-10-21）。
///
/// 与上面那条窄规则的区别：这里不知道 crate 根在哪，所以**按路径后缀**找候选文件
/// （`a.rs` / `a/mod.rs`），并要求"全图只有一个候选"——同后缀多个就放弃（宁少勿错）。
/// - `crate::a::b`：任意 `a.rs` / `a/mod.rs` 里的 `b`
/// - `self::b`：**本文件**里的 `b`
/// - `super::b`：本文件所在模块的**父模块**（`src/foo/bar.rs` → `src/foo.rs` / `src/foo/mod.rs`）
fn through_prefixed_path(rel: &str, r: &RefFact, all: &SymbolTable) -> Option<String> {
    let (prefix, rest) = r.name.split_once("::")?;
    let mut segs = rest.split("::");
    let (a, b) = match (segs.next(), segs.next(), segs.next()) {
        (Some(a), Some(b), None) => (Some(a), b),
        (Some(b), None, None) => (None, b),
        _ => return None, // 三段以上不猜
    };
    if b.is_empty() {
        return None;
    }
    let dir = match rel.rsplit_once('/') {
        Some((d, _)) => d.to_string(),
        None => String::new(),
    };
    let mut cands: Vec<String> = Vec::new();
    match prefix {
        "self" => {
            if a.is_none() {
                cands.push(rel.to_string());
            }
        }
        "super" => match a {
            // super::b：父模块文件
            None => {
                let parent = match dir.rsplit_once('/') {
                    Some((p, m)) => format!("{p}/{m}"),
                    None => dir.clone(),
                };
                if !parent.is_empty() {
                    cands.push(format!("{parent}.rs"));
                    cands.push(format!("{parent}/mod.rs"));
                }
            }
            // super::a::b：父模块下的 a 模块
            Some(a) => {
                let parent = match dir.rsplit_once('/') {
                    Some((p, m)) => format!("{p}/{m}/"),
                    None => String::new(),
                };
                cands.push(format!("{parent}{a}.rs"));
                cands.push(format!("{parent}{a}/mod.rs"));
            }
        },
        "crate" => {
            let a = a?; // crate::b（根上的 b）不猜：根文件可能是 lib.rs 也可能 main.rs
            // 后缀匹配：任意目录下的 a.rs / a/mod.rs
            cands.push(format!("{a}.rs"));
            cands.push(format!("/{a}.rs"));
            cands.push(format!("/{a}/mod.rs"));
            cands.push(format!("{a}/mod.rs"));
        }
        _ => return None,
    }
    let entries = all.get(b)?;
    let mut hit: Option<&String> = None;
    for e in entries
        .iter()
        .filter(|e| e.class == "value" || e.class == "local")
    {
        for cand in &cands {
            if id_is_symbol_in(&e.id, cand, b) {
                if hit.is_some() && hit != Some(&e.id) {
                    return None; // 多个候选：放弃
                }
                hit = Some(&e.id);
            }
        }
    }
    hit.cloned()
}


/// 这个符号 id 是不是"`file` 里的 `name`"？
///
/// **用后缀匹配，不用全等**：符号 id 里的路径是相对**索引根**的
/// （`crates/engine/src/util/winutil.rs`），而 `use` 推出来的路径是相对
/// **crate 根**的（`util/winutil.rs`）——中间差一个 crate 根前缀。
/// 全等比对永远对不上，后缀比对正好绕开这个差异（两个参照系都是目录树，
/// 尾部一定是同一段）。
///
/// **按整段比，不做字符串的宽松包含**：`util/winutil.rs` 必须配到
/// `/util/winutil.rs` 这样一段完整的路径，不能配到 `myutil/winutil.rs`。
/// 少这一条，图里会长出**指向错文件的边**——而错的边比没有边糟得多。
pub(crate) fn id_is_symbol_in(id: &str, file: &str, name: &str) -> bool {
    let Some(rest) = id.strip_prefix("symbol:") else {
        return false;
    };
    let Some((path, key)) = rest.split_once('#') else {
        return false;
    };
    // 键要和名字一致（`L::post` 这种带容器的键，只有名字就是它的时候才算）。
    if key != name && !key.ends_with(&format!("::{name}")) {
        return false;
    }
    path == file || path.ends_with(&format!("/{file}"))
}

/// 一个"模块目录 + 子路径 + 名字"的四种落法。
///
/// 以 `crate::util::winutil::which` 为例（模块目录 `util`，子路径 `winutil`，名字 `which`）：
///
/// | 候选 | 什么情况下是它 |
/// |---|---|
/// | `util/winutil.rs` | `winutil` 是个模块文件——**最常见的那一种** |
/// | `util/winutil/mod.rs` | `winutil` 是个目录模块 |
/// | `util/winutil/which.rs` | `which` 本身是个子模块（`use` 引的是模块而不是成员） |
/// | `util/winutil/which/mod.rs` | 同上，目录式 |
///
/// 前两条和后两条是**两种完全不同的解释**（名字在 `winutil` 里 vs 名字**就是** `winutil` 的子模块），
/// 必须都给出来——只给一种的话，`use crate::a::b;` 这种引模块的写法永远落不下来。
pub(crate) fn landing(base: &str, joined: &str, name: &str) -> Vec<(String, String)> {    let dir = match (base.trim_matches('/'), joined) {
        ("", "") => String::new(),
        ("", j) => j.to_string(),
        (b, "") => b.to_string(),
        (b, j) => format!("{b}/{j}"),
    };
    if dir.is_empty() {
        // 基准和子路径都是空 → 只剩"名字"这一层信息，推不出文件。
        return vec![(format!("{name}.rs"), name.to_string())];
    }
    vec![
        // 名字定义在 `dir` 这个模块里（`dir` 是模块文件或目录模块）
        (format!("{dir}.rs"), name.to_string()),
        (format!("{dir}/mod.rs"), name.to_string()),
        // **crate 根的模块文件是 `main.rs` / `lib.rs`，不叫 `mod.rs`**。
        // 少了这两条，`use super::{c, pad}`（从 `src/cmd_clean.rs` 往 crate 根找）
        // 这一类永远接不上——实测自己索引自己时报出 11 处，全是这个形状。
        (format!("{dir}/main.rs"), name.to_string()),
        (format!("{dir}/lib.rs"), name.to_string()),
        // 名字本身是 `dir` 底下的一个子模块
        (format!("{dir}/{name}.rs"), name.to_string()),
        (format!("{dir}/{name}/mod.rs"), name.to_string()),
        // ── Python（2026-10-16 加的第二门语言）────────────────────────────
        //
        // Python 的模块树是**目录 = 包**，包用 `__init__.py` 标记：
        // `from hello_python.core import hello` → `hello_python/core.py` 里的 `hello`。
        //
        // **为什么把两种语言的后缀放在同一张表里、而不是按语言分支**：
        // `through_import` 的规矩是"**恰好一个**候选才连边，多个就回到不猜"。
        // 所以多给几条候选**不会造出假边**——最坏情况是同一段路径在两门语言里
        // 都命中，于是 `hits.len() > 1`，那条边不建（no edge，而不是 wrong edge）。
        // 这比"按语言分支"少一层状态：`landing` 不用知道自己在处理哪门语言。
        (format!("{dir}.py"), name.to_string()),
        (format!("{dir}/__init__.py"), name.to_string()),
        (format!("{dir}/{name}.py"), name.to_string()),
        (format!("{dir}/{name}/__init__.py"), name.to_string()),
    ]
}

/// 从作用域链**从内往外**找第一个匹配。
///
/// `scope` 是从外往内压的栈，所以**末尾是最内层**——从后往前找就对了。
/// 找不到任何一个匹配时，退回"这个文件里有唯一同名"也算命中
/// （函数定义在作用域外、但同文件可见的情况）。
fn nearest_in_scope(r: &RefFact, own_file: &SymbolTable) -> Option<String> {
    let all = own_file.get(&r.name)?;
    // 同样只在值命名空间里找。
    let ids: Vec<&SymbolEntry> = all
        .iter()
        .filter(|e| e.class == "value" || e.class == "local")
        .collect();
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
        "连上 {} 条 · 拿不准 {} 条 · use 没接上 {} 处 · 命名空间不符 {} 处 · 外部符号 {} 个 · 模块级引用 {} 处",
        res.resolved.len(),
        res.ambiguous.len(),
        res.unmapped_imports.len(),
        res.namespace_mismatch.len(),
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
        use crate::graph::imports::ImportFact;
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

    /// **必须是 `use` 把它接上的，不是"全图唯一同名"兜住的。**
    ///
    /// 为什么单独写这一条：上面那条"顺着 use 能找到跨文件的定义"，
    /// 名字在全图里**只有一个**，所以就算 `through_import` 一次都没命中，
    /// 后面那条"唯一命中就连"也会把它连上——**测试是绿的，机制却是坏的**。
    /// 这正是这个项目反复学到的那件事：断言对了，不等于那件事做对了。
    ///
    /// 所以这里放**两个同名符号**：兜底那一条会判"拿不准"，
    /// 只有真的读懂 `use` 才能给出唯一答案。
    #[test]
    fn 同名两个时只能靠_use_定夺() {
        use crate::graph::imports::ImportFact;
        let mut all: SymbolTable = BTreeMap::new();
        all.insert(
            "which".to_string(),
            vec![
                SymbolEntry {
                    id: "symbol:util/winutil.rs#which".to_string(),
                    class: "value".to_string(),
                },
                SymbolEntry {
                    id: "symbol:tools/which.rs#which".to_string(),
                    class: "value".to_string(),
                },
            ],
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
            "两个同名时兜底会判拿不准，能连上就证明是 use 定的"
        );
        assert!(r.ambiguous.is_empty(), "不该走到「拿不准」那一步");
    }

    /// **`super::` 要按"当前模块在哪"算**——这是一整个形状族，不是一条特例。
    ///
    /// `sys/machine/settings.rs` 是 `machine::settings`，它的 `super` 是 `machine`，
    /// 而 `machine` 这个模块**自己的文件就是 `machine/mod.rs`**。
    /// 弄错的话症状是"`super::` 永远差一层目录"，而每一处单看都像对的。
    #[test]
    fn super_按当前模块算() {
        use crate::graph::imports::ImportFact;
        let mut all: SymbolTable = BTreeMap::new();
        all.insert(
            "reg".to_string(),
            vec![SymbolEntry {
                id: "symbol:sys/machine/mod.rs#reg".to_string(),
                class: "value".to_string(),
            }],
        );
        let imports = vec![ImportFact {
            local: "reg".to_string(),
            path: "super::reg".to_string(),
        }];
        let refs = vec![rf("reg", &["symbol:sys/machine/settings.rs#system_settings"])];
        let r = resolve_file(
            "sys/machine/settings.rs",
            &refs,
            &SymbolTable::new(),
            &imports,
            &all,
        );
        assert_eq!(
            r.resolved,
            vec![(
                "symbol:sys/machine/settings.rs#system_settings".to_string(),
                "symbol:sys/machine/mod.rs#reg".to_string()
            )],
            "super 要落到父模块自己的文件上：{:?}",
            r.unmapped_imports
        );
    }

    /// **`mod.rs` 与非 `mod.rs` 差一层**——`graph/mod.rs` 的 `super` 才是 `graph` 的上级，
    /// 而 `graph/parse.rs` 的 `super` 就是 `graph`。
    #[test]
    fn 同目录的兄弟模块也算相对路径() {
        use crate::graph::imports::ImportFact;
        let mut all: SymbolTable = BTreeMap::new();
        all.insert(
            "collect_imports".to_string(),
            vec![SymbolEntry {
                id: "symbol:crates/engine/src/graph/imports.rs#collect_imports".to_string(),
                class: "value".to_string(),
            }],
        );
        let imports = vec![ImportFact {
            local: "collect_imports".to_string(),
            path: "super::imports::collect_imports".to_string(),
        }];
        let refs = vec![rf(
            "collect_imports",
            &["symbol:crates/engine/src/graph/parse.rs#parse_rust"],
        )];
        let r = resolve_file(
            "crates/engine/src/graph/parse.rs",
            &refs,
            &SymbolTable::new(),
            &imports,
            &all,
        );
        assert_eq!(
            r.resolved.len(),
            1,
            "`super::imports::x` 是同级模块，要能连上：{:?}",
            r.unmapped_imports
        );
    }

    /// **裸路径的两种解释都要试**：`pub use ram::{...}` 是**子模块**
    /// （Rust 2018 的 uniform paths），`use jingwei_engine::...` 是**crate 名**。
    #[test]
    fn 裸路径既当子模块也当_crate_名() {
        use crate::graph::imports::ImportFact;
        // ① 子模块：`machine/mod.rs` 里的 `ram::smbios_memory`
        let mut a: SymbolTable = BTreeMap::new();
        a.insert(
            "smbios_memory".to_string(),
            vec![SymbolEntry {
                id: "symbol:sys/machine/ram.rs#smbios_memory".to_string(),
                class: "value".to_string(),
            }],
        );
        let r = resolve_file(
            "sys/machine/mod.rs",
            &[rf("smbios_memory", &["symbol:sys/machine/mod.rs#spec_report"])],
            &SymbolTable::new(),
            &[ImportFact {
                local: "smbios_memory".to_string(),
                path: "ram::smbios_memory".to_string(),
            }],
            &a,
        );
        assert_eq!(r.resolved.len(), 1, "子模块那条解释失效了");

        // ② crate 名：`examples/export_graph.rs` 里的 `jingwei_engine::graph::index::index_source`
        let mut b: SymbolTable = BTreeMap::new();
        b.insert(
            "index_source".to_string(),
            vec![SymbolEntry {
                id: "symbol:crates/engine/src/graph/index.rs#index_source".to_string(),
                class: "value".to_string(),
            }],
        );
        let r = resolve_file(
            "crates/engine/examples/export_graph.rs",
            &[rf("index_source", &["symbol:crates/engine/examples/export_graph.rs#main"])],
            &SymbolTable::new(),
            &[ImportFact {
                local: "index_source".to_string(),
                path: "jingwei_engine::graph::index::index_source".to_string(),
            }],
            &b,
        );
        assert_eq!(
            r.resolved.len(),
            1,
            "crate 名那条解释失效了：{:?}",
            r.unmapped_imports
        );
    }

    /// **crate 根的模块文件叫 `main.rs` / `lib.rs`，不叫 `mod.rs`**。
    ///
    /// 这条是图谱自己抓出来的：把 CLI 外壳按领域拆成 `cmd_*.rs` 之后，
    /// 那些文件里写的 `use super::{c, pad, ...}` 全部报"没接上"——
    /// 而 `super` 指的正是 crate 根，它的文件名叫 `main.rs`。
    /// 少了这两个候选，**同一个形状的 11 处会一直挂着**。
    ///
    /// 这条的价值不在"11 变成 0"，而在于：**缺口是工具自己报出来的**，
    /// 不是靠人逐个去读那些文件看出来的。
    #[test]
    fn crate_根的模块文件是_main_或_lib() {
        use crate::graph::imports::ImportFact;
        let mut all: SymbolTable = BTreeMap::new();
        all.insert(
            "c".to_string(),
            vec![SymbolEntry {
                id: "symbol:crates/cli/src/main.rs#c".to_string(),
                class: "value".to_string(),
            }],
        );
        let imports = vec![ImportFact {
            local: "c".to_string(),
            path: "super::c".to_string(),
        }];
        let refs = vec![rf("c", &["symbol:crates/cli/src/cmd_clean.rs#render_scan"])];
        let r = resolve_file(
            "crates/cli/src/cmd_clean.rs",
            &refs,
            &SymbolTable::new(),
            &imports,
            &all,
        );
        assert_eq!(
            r.resolved.len(),
            1,
            "`super::c` 要能落到 crate 根的 main.rs 上：{:?}",
            r.unmapped_imports
        );
    }

    /// **写了 `use` 却没接上，是我们漏了，不许混进"外部"。**
    ///
    /// 这条以前没有：`external` 涨了也看不出是"标准库"还是"我们自己没接上"，
    /// 而后者恰恰是最该被发现的那一类错——它看起来一切正常。
    #[test]
    fn use_没接上要单独报不许算外部() {
        use crate::graph::imports::ImportFact;
        let imports = vec![ImportFact {
            local: "mystery".to_string(),
            path: "super::nope::mystery".to_string(),
        }];
        let refs = vec![rf("mystery", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &SymbolTable::new(), &imports, &SymbolTable::new());
        assert_eq!(r.external, 0, "有 use 的就不是外部");
        assert_eq!(r.unmapped_imports.len(), 1);
        assert!(
            r.unmapped_imports[0].contains("mystery") && r.unmapped_imports[0].contains("a.rs:1"),
            "要报出名字和位置：{:?}",
            r.unmapped_imports
        );
    }

    /// **名字在、但不在值命名空间**：既不是"外部"，也不该被静默丢掉。
    #[test]
    fn 命名空间不符单独报() {
        let all = table(&[("Shape", &["symbol:a.rs#Shape"])]);
        let mut all = all;
        all.get_mut("Shape").unwrap()[0].class = "type".to_string();
        let refs = vec![rf("Shape", &["symbol:a.rs#caller"])];
        let r = resolve_file("a.rs", &refs, &SymbolTable::new(), &[], &all);
        assert_eq!(r.external, 0, "名字在图谱里，不该说成外部");
        assert!(r.resolved.is_empty(), "类型不能被当函数调");
        assert_eq!(r.namespace_mismatch.len(), 1);
    }
}



