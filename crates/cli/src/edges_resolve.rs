//! 边解析后置 pass：把 `?use ...` 解析到文件 / ext。
//!
//! 输入两张表：符号表 `name -> file`（DB 里的 symbol 判断）、模块表 `module::path -> file`（文件树）。
//! 花括号组先展开成多条目标，再逐个解析；`(src,dst,kind)` 由主键去重。

use collect::parse::leaf_of;
use datacore::store::Store;
use std::collections::HashMap;
use std::path::Path;

/// `crate::{A, b::{C, D}}` → `["crate::A", "crate::b::C", "crate::b::D"]`
pub fn expand_use(text: &str) -> Vec<String> {
    // 不在这里剥 `as 别名`：leaf 必须取别名（本地绑定名），dst 由 resolve_target 自己剥。
    // 先剥会把 `a::{B as C, D}` 截断成 `a::{B`，并且让 leaf 退回原名。
    let text = text.trim();
    let Some(open) = text.find('{') else {
        return vec![text.to_string()];
    };
    let mut depth = 0i32;
    let mut close = None;
    for (i, c) in text.char_indices().skip(0) {
        if i < open {
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return vec![text.to_string()];
    };
    let prefix = text[..open].trim().trim_end_matches("::").to_string();
    let inside = &text[open + 1..close];
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in inside.chars() {
        match c {
            '{' => {
                depth += 1;
                cur.push(c);
            }
            '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                let item = cur.trim().to_string();
                if !item.is_empty() {
                    out.extend(expand_use(&join(&prefix, &item)));
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let item = cur.trim().to_string();
    if !item.is_empty() {
        out.extend(expand_use(&join(&prefix, &item)));
    }
    out
}

fn join(prefix: &str, item: &str) -> String {
    if prefix.is_empty() {
        item.to_string()
    } else {
        format!("{prefix}::{item}")
    }
}

/// 解析一个 use 目标：文件路径 / `ext:<crate>` / 保留 `?`。
fn resolve_target(
    target: &str,
    from: &str,
    symbols: &HashMap<String, String>,
    modules: &HashMap<String, String>,
    root: &Path,
) -> String {
    // alias only affects leaf；resolve 失败的 `?use` 行要原样保留别名，
    // 否则下一轮 parse 再展开时 leaf 会从别名退回原名（非幂等）。
    let raw = target.trim();
    let target = target.split(" as ").next().unwrap_or(target).trim();
    // `::` 开头 = extern prelude 绝对路径，不是内部模块
    if let Some(rest) = target.strip_prefix("::") {
        let rest = rest.trim_start_matches(':');
        if !rest.is_empty() {
            return format!("ext:{rest}");
        }
    }
    // `use crate as X`：目标是 crate 根文件
    if target == "crate" {
        return match collect::parse::crate_root_file(root, from) {
            Some(f) => f,
            None => format!("?use {raw}"),
        };
    }
    let segs: Vec<&str> = target.split("::").filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return format!("?use {raw}");
    }
    let mut base: Vec<String> = Vec::new();
    let mut i = 0usize;
    match segs[0] {
        "crate" => i = 1,
        "self" => {
            i = 1;
            base = file_modules(from);
        }
        "super" => {
            i = 1;
            let mut b = file_modules(from);
            b.pop();
            base = b;
        }
        "std" | "core" | "alloc" => return format!("ext:{target}"),
        _ => {}
    }
    let mut full: Vec<String> = base;
    for s in &segs[i..] {
        full.push(s.to_string());
    }
    // `crate::utils::self` 是"这个模块本身"，去掉末段 self 再按模块匹配
    if full.last().map(|s| s == "self").unwrap_or(false) {
        full.pop();
    }
    // 1) 模块后缀匹配（最长优先）
    for cut in 0..full.len() {
        let cand = full[cut..].join("::");
        if let Some(f) = modules.get(&cand) {
            return f.clone();
        }
    }
    // 2) 末段按符号名找文件（item 级目标，如 crate::Attributes）
    if let Some(name) = full.last() {
        if let Some(f) = symbols.get(name) {
            return f.clone();
        }
    }
    // 3) 首段是本地顶层模块？→ 仍是内部，但解析不到
    if let Some(first) = segs.get(i) {
        if modules.keys().any(|k| k == first || k.starts_with(&format!("{first}::"))) {
            return format!("?use {raw}");
        }
        return format!("ext:{target}");
    }
    format!("?use {raw}")
}

fn file_modules(rel: &str) -> Vec<String> {
    let no_ext = rel.strip_suffix(".rs").unwrap_or(rel);
    let mut parts: Vec<String> = no_ext
        .split('/')
        .filter(|s| *s != "src")
        .map(|s| s.to_string())
        .collect();
    if parts
        .last()
        .map(|s| s == "mod" || s == "lib" || s == "main")
        .unwrap_or(false)
    {
        parts.pop();
    }
    parts
}


/// 收集 .py：相对路径 → 模块名候选（去掉 src/ 前缀）。
fn py_module_index(root: &Path) -> HashMap<String, Vec<String>> {
    fn walk(base: &Path, dir: &Path, out: &mut HashMap<String, Vec<String>>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if matches!(name.as_str(), "node_modules" | ".git" | ".jingwei" | "target" | "venv" | ".venv")
                    || name.starts_with('.')
                {
                    continue;
                }
                walk(base, &p, out);
                continue;
            }
            if !name.ends_with(".py") {
                continue;
            }
            let rel = match p.strip_prefix(base) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let mut parts: Vec<String> = rel.split('/').map(|s| s.to_string()).collect();
            if parts.len() == 1 {
                continue; // 顶层脚本，模块名不可靠
            }
            let file = parts.pop().unwrap();
            let stem = file.trim_end_matches(".py");
            if stem != "__init__" {
                parts.push(stem.to_string());
            }
            let module = parts.join(".");
            out.entry(module.clone()).or_default().push(rel.clone());
            if parts.first().map(|s| s == "src").unwrap_or(false) {
                let m2 = parts[1..].join(".");
                out.entry(m2).or_default().push(rel);
            }
        }
    }
    let mut out = HashMap::new();
    walk(root, root, &mut out);
    out
}

/// 把 `?py:<stmt>` 的 import 边解析到本仓文件；找不到标 ext:，多义/动态留 ?。

/// 把 `?ts:<spec>|<name>` 解析到文件：相对路径按目录拼、补常见扩展名；裸包名标 ext。
pub fn resolve_ts_imports(store: &Store, root: &Path) -> anyhow::Result<(usize, usize, usize)> {
    let rows = store.unresolved_ts_edges()?;
    let (mut internal, mut ext, mut unknown) = (0usize, 0usize, 0usize);
    for (src, dst, line, leaf) in rows {
        let spec_part = dst.strip_prefix("?ts:").unwrap_or(&dst);
        let (spec, name) = match spec_part.rsplit_once('|') {
            Some((s, n)) => (s.to_string(), n.to_string()),
            None => (spec_part.to_string(), "*".to_string()),
        };
        let new_dst = if spec.starts_with("./") || spec.starts_with("../") {
            let mut dir: Vec<&str> = src.split('/').collect();
            dir.pop();
            let mut base = dir.join("/");
            for seg in spec.split('/') {
                match seg {
                    "." | "" => {}
                    ".." => {
                        base = base.rsplit_once('/').map(|(a, _)| a.to_string()).unwrap_or_default();
                    }
                    other => {
                        if !base.is_empty() {
                            base.push('/');
                        }
                        base.push_str(other);
                    }
                }
            }
            let mut hit: Option<String> = None;
            for cand in [
                format!("{base}.ts"),
                format!("{base}.tsx"),
                format!("{base}.d.ts"),
                format!("{base}/index.ts"),
                format!("{base}/index.tsx"),
                base.clone(),
            ] {
                if root.join(&cand).is_file() {
                    hit = Some(cand);
                    break;
                }
            }
            match hit {
                Some(p) => {
                    internal += 1;
                    format!("ts:{p}")
                }
                None => {
                    unknown += 1;
                    format!("?ts:{spec}")
                }
            }
        } else {
            ext += 1;
            let pkg = if spec.starts_with('@') {
                spec.split('/').take(2).collect::<Vec<_>>().join("/")
            } else {
                spec.split('/').next().unwrap_or(&spec).to_string()
            };
            format!("ext:{pkg}")
        };
        store.delete_edge(&src, &dst, "import")?;
        let leaf = if leaf.is_some() {
            leaf
        } else if name.is_empty() {
            None
        } else {
            Some(name)
        };
        store.put_edge(&src, &new_dst, "import", 0, "", leaf.as_deref(), Some(line))?;
    }
    Ok((internal, ext, unknown))
}

pub fn resolve_py_imports(store: &Store, root: &Path) -> anyhow::Result<(usize, usize, usize, usize)> {
    let index = py_module_index(root);
    let rows = store.unresolved_py_edges()?;
    let (mut internal, mut ext, mut dynamic, mut star) = (0usize, 0usize, 0usize, 0usize);
    for (src, dst, line) in rows {
        let spec = dst.strip_prefix("?py:").unwrap_or(&dst);
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() != 3 {
            continue;
        }
        let level: usize = parts[0].parse().unwrap_or(0);
        let module = parts[1].to_string();
        let name = parts[2].to_string();

        if name == "*" {
            star += 1;
            store.delete_edge(&src, &dst, "import")?;
            store.put_edge(&src, &format!("?py:star:{module}"), "import", 0, "", Some("*"), Some(line))?;
            continue;
        }

        let mut candidates: Vec<String> = Vec::new();
        if level > 0 {
            let mut dir: Vec<String> = src.split('/').map(|s| s.to_string()).collect();
            dir.pop();
            if dir.first().map(|s| s == "src").unwrap_or(false) {
                dir.remove(0);
            }
            for _ in 1..level {
                dir.pop();
            }
            if !module.is_empty() {
                dir.extend(module.split('.').map(|s| s.to_string()));
            }
            dir.push(name.clone());
            candidates.push(dir.join("."));
            if !module.is_empty() {
                let mut d2: Vec<String> = src.split('/').map(|s| s.to_string()).collect();
                d2.pop();
                if d2.first().map(|s| s == "src").unwrap_or(false) {
                    d2.remove(0);
                }
                for _ in 1..level {
                    d2.pop();
                }
                d2.extend(module.split('.').map(|s| s.to_string()));
                candidates.push(d2.join("."));
            }
        } else if module.is_empty() {
            candidates.push(name.clone());
        } else {
            candidates.push(format!("{module}.{name}"));
            candidates.push(module.clone());
        }

        let mut hit: Option<(String, usize)> = None;
        let mut ambiguous = false;
        for key in &candidates {
            if let Some(paths) = index.get(key) {
                if paths.len() > 1 {
                    ambiguous = true;
                    break;
                }
                hit = Some((paths[0].clone(), 1));
                break;
            }
        }
        let new_dst = match hit {
            _ if ambiguous => {
                dynamic += 1;
                format!("?py:{spec}")
            }
            Some((p, _)) => {
                internal += 1;
                format!("py:{p}")
            }
            None => {
                ext += 1;
                let modname = if module.is_empty() { name.clone() } else { module.clone() };
                format!("ext:{modname}")
            }
        };
        store.delete_edge(&src, &dst, "import")?;
        // leaf = 导入名（不是文件：同一行 `from a import b, c` 必须是两行）
        let leaf = name.rsplit('.').next().unwrap_or(&name).to_string();
        let leaf = if leaf.is_empty() { None } else { Some(leaf) };
        store.put_edge(&src, &new_dst, "import", 0, "", leaf.as_deref(), Some(line))?;
    }
    Ok((internal, ext, dynamic, star))
}
/// 跑一遍：把所有 `?use` 行重写（多目标展开），返回 (处理行数, 解析后 ? 数)。
pub fn resolve_all(store: &Store, root: &Path) -> anyhow::Result<(usize, usize)> {
    let modules = collect::parse::module_map(root);
    // Go 内部 import：go.mod module 前缀 → dir:<相对目录>（不重解析，只重写边）
    if let Some(m) = collect::parse::go_module(root) {
        let prefix = format!("ext:{m}/");
        let exact = format!("ext:{m}");
        for (src, dst) in store.import_edges()? {
            let new_dst = if let Some(rest) = dst.strip_prefix(&prefix) {
                format!("dir:{rest}")
            } else if dst == exact {
                "dir:.".to_string()
            } else {
                continue;
            };
            store.delete_edge(&src, &dst, "import")?;
            store.put_edge(&src, &new_dst, "import", 0, "", None, None)?;
        }
    }
    let mut symbols: HashMap<String, String> = HashMap::new();
    for subject in store.symbol_subjects()? {
        if let Some(rest) = subject.strip_prefix("symbol:") {
            if let Some((path, name)) = rest.split_once('#') {
                symbols.entry(name.to_string()).or_insert_with(|| path.to_string());
            }
        }
    }
    let rows = store.unresolved_use_edges()?;
    let mut handled = 0usize;
    let mut left = 0usize;
    for (src, dst, line) in rows {
        let text = dst.strip_prefix("?use ").unwrap_or(&dst).to_string();
        store.delete_use_edge(&src, &dst, line)?;
        for target in expand_use(&text) {
            let r = resolve_target(&target, &src, &symbols, &modules, root);
            if r.starts_with('?') {
                left += 1;
            }
            let leaf = leaf_of(&target);
            store.put_edge(&src, &r, "use", 0, "", leaf.as_deref(), Some(line))?;
        }
        handled += 1;
    }
    // 跨文件 call v1.5：**作用域优先**。
    // ① 本文件有 `use …X` 绑定且目标是内部文件、且该文件里确有 fn X → 指到绑定目标；
    // ② 绑定是 std/core/alloc 的外部项 → 保持 `?call`（同名本地 fn 不是它）；
    // ③ 其余才走"全仓 fn 名唯一"；④ 都不中 → `?call`。
    let mut fn_files: HashMap<String, std::collections::BTreeSet<String>> = HashMap::new();
    let mut file_fns: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    for (name, path, kind) in store.symbol_def_index()? {
        if kind == "fn" {
            fn_files.entry(name.clone()).or_default().insert(path.clone());
            file_fns.entry(path).or_default().insert(name);
        }
    }
    let mut bindings: HashMap<(String, String), String> = HashMap::new();
    for (src, leaf, dst) in store.use_bindings()? {
        bindings.entry((src, leaf)).or_insert(dst);
    }
    for (src, dst, leaf, line) in store.call_edges_all()? {
        let caller_file = src
            .strip_prefix("symbol:")
            .and_then(|r| r.split_once('#'))
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        let unresolved_name = dst.strip_prefix("?call ").map(|s| s.to_string());
        let name = match &unresolved_name {
            Some(n) => n.clone(),
            None => dst
                .strip_prefix("symbol:")
                .and_then(|r| r.split_once('#'))
                .map(|(_, n)| n.to_string())
                .unwrap_or_default(),
        };
        if name.is_empty() {
            continue;
        }
        let bound = bindings.get(&(caller_file, name.clone())).cloned();
        let mut new_dst: Option<String> = None;
        if let Some(b) = &bound {
            if b.starts_with("ext:std") || b.starts_with("ext:core") || b.starts_with("ext:alloc") {
                // 显式 std 绑定：同名本地 fn 不是它 → 保持 ?call（不误导）
                new_dst = Some(format!("?call {name}"));
            } else if !b.starts_with("ext:") && !b.starts_with('?') {
                if file_fns.get(b).map(|s| s.contains(&name)).unwrap_or(false) {
                    new_dst = Some(format!("symbol:{b}#{name}"));
                } else {
                    // 绑定说内部但目标文件没有该 fn（re-export 等）：不猜
                    new_dst = Some(format!("?call {name}"));
                }
            }
            // 非 std 的 ext 绑定：保留现状（workspace crate 被标 ext 的情况多为真阳性）
        }
        // 无绑定的未解析行 → 全仓唯一 fn 名
        if new_dst.is_none() && unresolved_name.is_some() && bound.is_none() {
            if let Some(files) = fn_files.get(&name) {
                if files.len() == 1 {
                    let f = files.iter().next().unwrap();
                    new_dst = Some(format!("symbol:{f}#{name}"));
                }
            }
        }
        if let Some(nd) = new_dst {
            if nd != dst {
                store.delete_edge(&src, &dst, "call")?;
                store.put_edge(&src, &nd, "call", 0, "", leaf.as_deref(), line)?;
            }
        }
    }
    // impl 边：trait:Name → 内部 trait 定义文件；找不到的标 ext:；多义保留 ?
    let traits = store.trait_paths()?;
    for (src, dst, line, leaf) in store.impl_edges()? {
        let name = dst.strip_prefix("trait:").unwrap_or(&dst).to_string();
        let paths = traits.get(&name).cloned().unwrap_or_default();
        let new_dst = match paths.len() {
            1 => paths.into_iter().next().unwrap(),
            0 => format!("ext:{name}"),
            _ => format!("?trait:{name}"),
        };
        if new_dst != dst {
            store.delete_edge(&src, &dst, "impl")?;
            store.put_edge(&src, &new_dst, "impl", 0, "", leaf.as_deref(), line)?;
        }
    }
    let (py_in, py_ext, py_dyn, py_star) = resolve_py_imports(store, root)?;
    let (ts_in, ts_ext, ts_unknown) = resolve_ts_imports(store, root)?;
    if ts_in + ts_ext + ts_unknown > 0 {
        println!("ts import：内部 {ts_in} · ext {ts_ext} · 未定 {ts_unknown}");
    }
    if py_in + py_ext + py_dyn > 0 {
        println!(
            "py import：内部 {py_in} · ext {py_ext} · dynamic {py_dyn} · star {py_star}"
        );
    }
    Ok((handled, left))
}
