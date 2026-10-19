//! **Rust 侧 import → 符号**的解析（从 `resolve.rs` 拆出，2026-10-20）。
//!
//! 为什么拆：`resolve.rs` 贴到 1000 行上限（门禁守）。这一族函数彼此独立：
//! `through_import`（走 use）→ `import_candidates`（路径 → 候选文件/名字）→ `split_rel` / `parent_dir`。

use super::parse::RefFact;
use super::resolve::{id_is_symbol_in, landing, SymbolTable};
pub(crate) fn through_import(
    r: &RefFact,
    imports: &[super::imports::ImportFact],
    all: &SymbolTable,
    rel: &str,
) -> Option<String> {
    // 两种引用形状，**朴素名那条一个字没动**：
    //
    //   ① `r.name == "which"`：本文件 `use ...::which;` 引进来的名字。
    //   ② `r.name == "hello_python::core::greet"`：**限定名**——Python 那边
    //      遇到"接收者是 import 引进来的模块"时会产出这种形状
    //      （`from hello_python import core` 之后的 `core.greet()`）。
    //
    // 第二条是 2026-10-16 加 Python 时补的。**它有一个硬前提**：
    // `r.name` 的末段才是符号名，前面那段必须**恰好等于**某条 import 的 path。
    // 用"前缀匹配"而不是"随便找个 import"——后者会给出错误的落点，
    // 而错的边比没有边糟得多（那是这个文件反复强调的一条）。
    let (imp, name): (&super::imports::ImportFact, &str) = match imports
        .iter()
        .find(|i| i.local == r.name)
    {
        Some(i) => (i, r.name.as_str()),
        None => {
            let (prefix, last) = r.name.rsplit_once("::")?;
            let i = imports.iter().find(|i| i.path == prefix)?;
            (i, last)
        }
    };
    let mut hits: Vec<String> = Vec::new();
    for (file, nm) in import_candidates(&imp.path, rel) {
        if let Some(entries) = all.get(&nm) {
            for e in entries {
                if id_is_symbol_in(&e.id, &file, &nm) {
                    hits.push(e.id.clone());
                }
            }
        }
    }
    // 限定名那一路：候选文件名要用**末段**（`greet`），不是整串路径。
    // 上面 `import_candidates(&imp.path, …)` 已经用 `imp.path` 推出了文件，
    // 所以这里只需要确认"那个文件里的 `name`"存在。
    if name != r.name {
        for (file, _) in import_candidates(&imp.path, rel) {
            if let Some(entries) = all.get(name) {
                for e in entries {
                    if id_is_symbol_in(&e.id, &file, name) {
                        hits.push(e.id.clone());
                    }
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

fn import_candidates(path: &str, rel: &str) -> Vec<(String, String)> {
    let segs: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return Vec::new();
    }

    // ① 名字：末段就是它；`use a::b::{self}` 那种末段是 self 时，名字取前一段
    //    （`push_import` 记 local 时用的是同一套规矩）。
    let (name, mids): (String, Vec<&str>) = if segs[segs.len() - 1] == "self" {
        if segs.len() < 2 {
            return Vec::new();
        }
        (
            segs[segs.len() - 2].to_string(),
            segs[..segs.len() - 2].to_vec(),
        )
    } else {
        (segs[segs.len() - 1].to_string(), segs[..segs.len() - 1].to_vec())
    };

    // ② 基准目录：把 crate / self / super 前缀吃掉，剩下的当"相对基准的模块路径"。
    let (base, rest, from_crate_root) = if mids.first() == Some(&"crate") {
        (String::new(), mids[1..].to_vec(), true)
    } else {
        let (own_dir, is_mod_file) = split_rel(rel);
        // "当前模块"在磁盘上的目录：`mod.rs` 就是它自己那层；
        // `machine/settings.rs` 是 `machine::settings`，它那层是 `machine/settings`。
        let mut cur = if is_mod_file {
            own_dir.to_string()
        } else {
            let stem = rel.rsplit('/').next().unwrap_or(rel);
            let stem = stem.strip_suffix(".rs").unwrap_or(stem);
            format!("{own_dir}/{stem}")
        };
        let mut i = 0;
        // `self` 与 `super` 可以连着写（`super::super::x`）
        while i < mids.len() && (mids[i] == "super" || mids[i] == "self") {
            if mids[i] == "super" {
                match parent_dir(&cur) {
                    Some(p) => cur = p,
                    None => return Vec::new(),
                }
            }
            i += 1;
        }
        (cur, mids[i..].to_vec(), false)
    };

    let mut out = Vec::new();
    let joined = rest.join("/");
    // (a) 基准 + 子路径
    out.extend(landing(&base, &joined, &name));
    // (b) 裸路径的第二种解释：第一段其实是 **crate 名**，剩下的才是 crate 内的路径
    //     （`use jingwei_engine::graph::index::index_source`——那个 crate 就是我们自己，
    //     只是 `use` 里写的是 crate 名而不是 `crate`）。
    //     **crate 名那一节要丢掉**：它是"哪个 crate"，不是"crate 里哪个目录"。
    if !from_crate_root && !rest.is_empty() {
        out.extend(landing("", &rest[1..].join("/"), &name));
    }
    // (c) **"名字之前那一段全是目录"**——Python 的模块路径就是这个形状。
    //
    //     这一条是加 Python 时补的，而**Rust 那边一直也缺它**：
    //     `from hello_python.core import greet` 里 `greet` 是符号名，
    //     落点 `hello_python/core.py` 要求"把 `hello_python` 当目录"——
    //     而 (a) 用的是 `base`（对本文件 `main.py` 来说是空）、(b) 又丢掉了首段。
    //     两条路都给不出 `hello_python/core.py`，于是**一条边都连不上**。
    //
    //     为什么现在才加：Rust 那边 `use crate::a::b` 有显式的 `crate` 前缀，
    //     (a) 走 `base="" + joined="a/b"`，正好等价于这一条；
    //     只有"裸路径"这个形状两边都漏。加它的代价是候选多几条——
    //     而候选多**不会造假边**（`through_import` 要求恰好一个命中）。
    let mut all_segs: Vec<&str> = rest.clone();
    all_segs.push(&name);
    if all_segs.len() >= 2 {
        let dir = all_segs[..all_segs.len() - 1].join("/");
        out.extend(landing(&format!("{base}/{dir}"), "", &name));
        // **`base` 为空那一种也要给**：Python 的 `from hello_python.core import greet`
        // 是**从项目根算起**的绝对导入，与"当前模块在哪"无关。
        //
        // 少这一条会出一个**很隐蔽**的错：`base` 对本文件 `main.py` 来说是
        // `main.py`（`split_rel` 之后 `is_mod=false`，于是把文件自己的名字当成了
        // 模块目录），候选就变成 `main.py/hello_python/core.py`——
        // **一个把文件当目录的路径**，永远不可能命中，而且不报错、只是静默不连边。
        // 实测：`hello_python` 演示项目里 `main.py` 调 `core.greet()` 一条边都没有。
        out.extend(landing(&dir, "", &name));
    }
    out.sort();
    out.dedup();
    out
}

fn split_rel(rel: &str) -> (&str, bool) {
    let (dir, file) = match rel.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", rel),
    };
    let is_mod = matches!(file, "mod.rs" | "lib.rs" | "main.rs" | "__init__.py");
    (dir, is_mod)
}

fn parent_dir(dir: &str) -> Option<String> {
    let d = dir.trim_matches('/');
    if d.is_empty() {
        return None;
    }
    match d.rsplit_once('/') {
        Some((p, _)) => Some(p.to_string()),
        None => Some(String::new()),
    }
}
