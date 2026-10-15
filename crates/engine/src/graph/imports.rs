//! `use` 语句：**索引期就能确定的"名字 → 路径"映射**。
//!
//! ## 为什么单独一个文件
//!
//! 它和 `parse.rs` 里的语法树遍历**不是一回事**：
//!
//! - 那边是"走树、认节点类型、记位置"；
//! - 这边是"把一句文本拆成名字和路径"，**全是字符串操作**，不建语法树。
//!
//! 所以能确定的那几种形状用字符串就分得清，而通配和复杂嵌套本来就要丢。
//! 混在一个文件里的时候，`parse.rs` 撞到了门禁的 1000 行上限——
//! 那条门禁逼出来的这次拆分正好落在这条真实的接缝上。
//!
//! ## 为什么要留跨文件的接口
//!
//! `use crate::util::winutil::which;` 这句话本身就把"本文件里的 `which`
//! 指那条路径"写死了，**不需要看到别的文件**就能记下来——所以它属于索引期。
//! 跨文件那一步（路径 → 符号 id）才需要全图，那是 `resolve::through_import` 的事。

use tree_sitter::Node as TsNode;

/// 一条 `use` 语句——**索引期能确定的"名字 → 路径"映射**。
///
/// 为什么它是"事实"而不是"解析结果"：`use crate::util::winutil::which;`
/// 这句话本身就把"本文件里的 `which` 指那条路径"写死了，
/// **不需要看到别的文件**就能记下来。所以它属于索引期。
///
/// 跨文件那一步（路径 → 符号 id）才需要全图——
/// 因为 `crate::util::winutil` 到底对应哪个文件，要看整棵模块树。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportFact {
    /// 引入后在本文件里用的名字（`use ...::which` → `which`；带 `as` 时用别名）。
    pub local: String,
    /// 原来的路径，原样留着（`crate::util::winutil::which`）。
    /// **不在这里拆**：拆成模块段与末段是解析期的事，
    /// 而且要处理 `self` / `super` / `crate` / 通配 这些形状。
    pub path: String,
}

/// 走一遍语法树，收本文件的全部 `use`（含 `mod x { use ...; }` 里的）。
pub fn collect_imports(root: TsNode<'_>, src: &str) -> Vec<ImportFact> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "use_declaration" {
            if let Ok(text) = child.utf8_text(src.as_bytes()) {
                expand_use(text.trim(), &mut out);
            }
        }
        // 模块里的 use 也要收（`mod x { use ...; }`）
        if matches!(child.kind(), "mod_item" | "declaration_list") {
            out.extend(collect_imports(child, src));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 把一句 `use` 展开成若干条"本文件的名字 → 路径"。
///
/// 这是**字符串层面**的展开，不建语法树——能确定的那几种形状用字符串就分得清，
/// 而通配和复杂嵌套本来就要丢。
fn expand_use(text: &str, out: &mut Vec<ImportFact>) {
    // **先剥可见性修饰**：`pub use …` / `pub(crate) use …` / `pub(super) use …`
    // 都是 use 语句。不剥的话，`strip_prefix("use")` 会失败，
    // 整句原样当成路径 —— 于是 `pub use crate::x` 变成了一个
    // 以 "pub use crate" 开头的路径，谁也匹配不上。
    //
    // 这一条是实测逼出来的：`probe.rs` 里那句 `pub use …::{run_cmd, which};`
    // 之后调用 `which(...)` 一直报"拿不准"，而它明明有 use。
    let mut head = text.trim();
    if let Some(rest) = head.strip_prefix("pub") {
        let rest = rest.trim_start();
        head = if rest.starts_with('(') {
            match rest.find(')') {
                Some(i) => rest[i + 1..].trim_start(),
                None => rest,
            }
        } else {
            rest
        };
    }
    let body = head
        .strip_prefix("use")
        .unwrap_or(head)
        .trim()
        .trim_end_matches(';')
        .trim();
    if body.is_empty() || body.contains('*') {
        return; // 通配不收
    }
    // 花括号：`a::b::{c, d as e}` → 前缀 `a::b` + 每一项
    if let Some(open) = body.find('{') {
        if let Some(close) = body.rfind('}') {
            let prefix = body[..open].trim().trim_end_matches("::").to_string();
            let inner = &body[open + 1..close];
            for item in inner.split(',') {
                let item = item.trim();
                if item.is_empty() || item.contains('*') {
                    continue;
                }
                let full = format!("{prefix}::{item}");
                push_import(&full, out);
            }
            return;
        }
    }
    push_import(body, out);
}

/// `a::b::c` 或 `a::b::c as d` → 一条 import。
fn push_import(full: &str, out: &mut Vec<ImportFact>) {
    let (path, alias) = match full.split_once(" as ") {
        Some((p, a)) => (p.trim(), Some(a.trim())),
        None => (full, None),
    };
    let mut segs: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return;
    }
    // 名字取路径末段（`crate::a::b::c` → `c`）；
    // 末段是 `self` 的（`use a::b::{self}`）指向的是 `b` 本身。
    let mut local = segs.pop().unwrap_or("").to_string();
    if local == "self" {
        local = segs.pop().unwrap_or("").to_string();
    }
    if let Some(a) = alias {
        local = a.to_string();
    }
    if local.is_empty() {
        return;
    }
    out.push(ImportFact {
        local,
        path: path.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use crate::graph::parse::parse_rust;

    /// **`pub use` 也只是一句 use**——它和自己的 use 一样，索引期就能确定
    /// "本文件的 `which` 指哪条路径"。重导出（re-export）的影响在**别的文件**
    /// 从这条路径再走一次的时候，与这一句本身无关。
    ///
    /// 这条是实测逼出来的：`probe.rs` 里 `pub use crate::util::winutil::{run_cmd, which};`
    /// 之后那句 `which("py.exe")` 一直报"拿不准"，而它明明有 use。
    #[test]
    fn pub_use_也要抽到() {
        let src = "pub use crate::util::winutil::{run_cmd, which};\n";
        let r = parse_rust("a.rs", src, 1);
        let got: Vec<(String, String)> = r
            .imports
            .iter()
            .map(|i| (i.local.clone(), i.path.clone()))
            .collect();
        assert!(
            got.contains(&("which".into(), "crate::util::winutil::which".into())),
            "pub use 也是 use：{got:?}"
        );
    }

    /// **`use` 抽取要真的抽到**——这是 `through_import` 能工作的前提。
    ///
    /// 三种能确定的形状都收，通配一律不收（`use a::*` 记下来也没法用，
    /// 反而会让人以为"这里有个确定的名字"）。
    #[test]
    fn use_抽取只收能确定的形状() {
        let src = r#"
use crate::util::winutil::{run_cmd, which};
use crate::graph::store::Store as S;
use std::collections::BTreeMap;
use crate::*;
"#;
        let r = parse_rust("a.rs", src, 1);
        let got: Vec<(String, String)> = r
            .imports
            .iter()
            .map(|i| (i.local.clone(), i.path.clone()))
            .collect();
        assert!(
            got.contains(&("which".into(), "crate::util::winutil::which".into())),
            "{got:?}"
        );
        assert!(
            got.contains(&("run_cmd".into(), "crate::util::winutil::run_cmd".into())),
            "{got:?}"
        );
        assert!(
            got.contains(&("S".into(), "crate::graph::store::Store".into())),
            "别名要记：{got:?}"
        );
        assert!(
            got.contains(&("BTreeMap".into(), "std::collections::BTreeMap".into())),
            "{got:?}"
        );
        assert!(!got.iter().any(|(l, _)| l == "*"), "通配不收：{got:?}");
    }
}
