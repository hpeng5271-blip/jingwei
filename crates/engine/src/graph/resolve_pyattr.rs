//! **Python 模块属性的解析**（债 0.28 / D-20261020-04）。
//!
//! 从 `resolve.rs` 拆出来：那个文件已经贴到 1000 行上限（门禁守）。
//! 这里只放"前缀是模块"的解析——与 Rust 的 `import` 解析分开，口径不同。

use super::parse::RefFact;
use super::resolve::SymbolTable;

pub(crate) fn through_python_module_attr(
    r: &RefFact,
    imports: &[super::imports::ImportFact],
    all: &SymbolTable,
) -> Option<String> {
    let (prefix, last) = r.name.rsplit_once("::")?;
    // 前缀要么是 import 的**路径**，要么是它的**别名**（`from pkg import cfg` → 别名 cfg）。
    let imp = imports
        .iter()
        .find(|i| i.path == prefix || i.local == prefix)?;
    // **不把末段拼进模块路径**：路径是模块，末段是模块里的名字（拼错过一次）。
    let mod_path = imp
        .path
        .trim_start_matches('.')
        .replace("self::", "")
        .replace("::", ".");
    let base = mod_path.replace('.', "/");
    for rel in [format!("{base}.py"), format!("{base}/__init__.py")] {
        let want = format!("symbol:{rel}#{last}");
        if let Some(entries) = all.get(last) {
            if entries.iter().any(|e| e.id == want) {
                return Some(want);
            }
        }
    }
    None
}

pub fn resolve_attr_refs(
    rel: &str,
    refs: &[RefFact],
    imports: &[super::imports::ImportFact],
    all: &SymbolTable,
) -> Vec<(String, String, u32)> {
    if !rel.ends_with(".py") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for r in refs {
        let Some(hit) = through_python_module_attr(r, imports, all) else {
            continue;
        };
        let Some(caller) = r.scope.last() else {
            continue;
        };
        if &hit != caller {
            out.push((caller.clone(), hit, r.line));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 把各文件的待解析属性引用一次算完：`(调用者, 目标, 行)`。
///
/// 拆出这个函数的直接原因：`index.rs` 贴到 1000 行上限（门禁守），
/// 而这段逻辑本来就属于"解析"，不该长在索引里。
pub fn attr_pairs(
    pending: &[(String, Vec<RefFact>)],
    file_imports: &std::collections::BTreeMap<String, Vec<super::imports::ImportFact>>,
    all: &SymbolTable,
) -> Vec<(String, String, u32)> {
    let mut out = Vec::new();
    for (rel, refs) in pending {
        if let Some(imps) = file_imports.get(rel) {
            out.extend(resolve_attr_refs(rel, refs, imps, all));
        }
    }
    out
}
