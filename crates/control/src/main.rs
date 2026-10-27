//! 对照组 B：独立遍历（tree-sitter 直读 + git 直读 + SQL 聚合），不调用 jw。
//!
//! 与 jw 共用 tree-sitter 语法（不可消除），但代码路径完全独立；输出 JSON 供 exp-control.ps1 比对。

use control::{py_counts, py_import_names_by_file, skipped_files, ts_counts, ts_import_names_by_file};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `--out <file>`：写 JSON 到文件，绕开 PowerShell 5.1 的 stdout 编码坑（窗口也能直接读）
    let mut out_file: Option<String> = None;
    let mut pos: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--out" {
            out_file = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        pos.push(args[i].clone());
        i += 1;
    }
    if pos.is_empty() {
        eprintln!("用法：control <repo> [limit] [--out <file>]");
        std::process::exit(2);
    }
    let root = PathBuf::from(&pos[0]);
    let limit: usize = pos.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);

    let (commits, changes, files) = git_counts(&root, limit);
    let (b_raw, b_distinct, b_by_file) = b_use_items(&root);
    let (with_syms, kinds, per_file) = parse_counts(&root);
    let deps = dep_names(&root);
    let (mod_n, use_n, impl_n, mod_targets, use_by_src, mod_by_src, raw) = edge_counts(&root);
    let unparsed = unparsed_files(&root, &with_syms);
    let py = py_counts(&root);
    let py_names = py_import_names_by_file(&root);
    let ts = ts_counts(&root);
    let ts_names = ts_import_names_by_file(&root);
    let skipped = skipped_files(&root);
    let locs = b_locs(&root);

    let out = serde_json::json!({
        "git": {"commits": commits, "changes": changes, "files": files},
        "parse": {"files_with_symbols": with_syms.len(), "kinds": kinds,
                  "per_file": per_file.iter().map(|(k, v)| (k.clone(), *v)).collect::<BTreeMap<_,_>>()},
        "use_items": {"raw": b_raw, "distinct": b_distinct, "dup": b_raw - b_distinct,
                      "by_file": b_by_file},
        "locs": {"by_file": locs},
        "deps": deps,
        "edges": {"mod": mod_n, "use": use_n, "impl": impl_n, "mod_targets": mod_targets,
                  "use_by_src": use_by_src, "mod_by_src": mod_by_src,
                  "raw": {"use": raw.0, "mod": raw.1, "impl": raw.2}},
        "coverage": {"unparsed": unparsed},
        "py": py,
        "py_import_names_by_file": py_names,
        "ts": ts,
        "ts_import_names_by_file": ts_names,
        "skipped": skipped,
    });
    let text = serde_json::to_string_pretty(&out).unwrap();
    match out_file {
        Some(p) => {
            if let Err(e) = std::fs::write(&p, format!("{text}\n")) {
                eprintln!("写 {p} 失败：{e}");
                std::process::exit(1);
            }
        }
        None => println!("{text}"),
    }
}


/// B 自己的 use 展开（独立实现）：返回 (leaf, line) 列表。
fn b_expand(text: &str) -> Vec<String> {
    let mut t = text.trim().trim_end_matches(';').trim().to_string();
    if let Some(r) = t.strip_prefix("use") {
        t = r.trim_start().to_string();
    }
    b_items(&t, "")
}

fn b_items(s: &str, prefix: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return vec![];
    }
    if s.starts_with('{') {
        if let Some(close) = b_close(s) {
            if s[close + 1..].trim().is_empty() {
                return b_items(&s[1..close], prefix);
            }
        }
    }
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '{' => {
                depth += 1;
                cur.push(ch);
            }
            '}' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    parts.push(cur.trim().to_string());
    parts.retain(|x| !x.is_empty());
    if parts.len() > 1 {
        return parts.iter().flat_map(|p| b_items(p, prefix)).collect();
    }
    let item = &parts[0];
    if let Some(open) = item.find('{') {
        if let Some(close) = b_close(&item[open..]) {
            let pre = item[..open].trim().trim_end_matches("::");
            let base = if prefix.is_empty() {
                pre.to_string()
            } else {
                format!("{prefix}::{pre}")
            };
            return b_items(&item[open + 1..open + close], &base);
        }
    }
    let leaf = item.split(" as ").next().unwrap_or(item).trim();
    if leaf == "self" {
        vec![prefix.trim_end_matches("::").to_string()]
    } else if prefix.is_empty() {
        vec![leaf.to_string()]
    } else {
        vec![format!("{prefix}::{leaf}")]
    }
}

/// B 自己的位置表（AST 遍历，自带行号）：file → kind → 行号列表（含重数）。
/// use 的重数 = 该声明的导入名个数（与 jw 的"按导入名计边"对齐）；
/// mod 只算文件模块（`mod x;`，inline `mod x { }` 没有目标文件）；impl 只算带 trait 的（`impl T for X`）。
fn b_locs(root: &Path) -> BTreeMap<String, BTreeMap<String, Vec<i64>>> {
    use tree_sitter::Parser;
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_rust::LANGUAGE.into()).is_err() {
        return BTreeMap::new();
    }
    let mut out: BTreeMap<String, BTreeMap<String, Vec<i64>>> = BTreeMap::new();
    for (rel, abs) in walk(root) {
        if !rel.ends_with(".rs") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut entry: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let line = node.start_position().row as i64 + 1;
            match node.kind() {
                "use_declaration" => {
                    if let Ok(t) = node.utf8_text(&src) {
                        let n = control::expand::expand_imports(t, line as usize).len();
                        let v = entry.entry("use".to_string()).or_default();
                        for _ in 0..n {
                            v.push(line);
                        }
                    }
                }
                "mod_item" => {
                    if node.child_by_field_name("body").is_none() {
                        entry.entry("mod".to_string()).or_default().push(line);
                    }
                }
                "impl_item" => {
                    if node.child_by_field_name("trait").is_some() {
                        entry.entry("impl".to_string()).or_default().push(line);
                    }
                }
                _ => {}
            }
            for k in 0..node.child_count() {
                if let Some(c) = node.child(k) {
                    if c.is_named() {
                        stack.push(c);
                    }
                }
            }
        }
        for v in entry.values_mut() {
            v.sort();
        }
        if !entry.is_empty() {
            out.insert(rel, entry);
        }
    }
    out
}

fn b_close(t: &str) -> Option<usize> {
    let mut d = 0i32;
    for (i, ch) in t.char_indices() {
        match ch {
            '{' => d += 1,
            '}' => {
                d -= 1;
                if d == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// B 的原始 use 项：(leaf, line) 全量列表 + 去重数。
fn b_use_items(root: &Path) -> (usize, usize, BTreeMap<String, Vec<(String, i64)>>) {
    use tree_sitter::Parser;
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_rust::LANGUAGE.into()).is_err() {
        return (0, 0, BTreeMap::new());
    }
    let mut raw = 0usize;
    let mut distinct = 0usize;
    let mut by_file: BTreeMap<String, Vec<(String, i64)>> = BTreeMap::new();
    for (rel, abs) in walk(root) {
        if !rel.ends_with(".rs") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut stack = vec![tree.root_node()];
        let mut items: Vec<(String, i64)> = Vec::new();
        while let Some(node) = stack.pop() {
            if node.kind() == "use_declaration" {
                if let Ok(t) = node.utf8_text(&src) {
                    let line = node.start_position().row as i64 + 1;
                    for it in control::expand::expand_imports(t, line as usize) {
                        let local = it.local.rsplit("::").next().unwrap_or(&it.local).to_string();
                        items.push((local, line));
                    }
                }
            }
            for k in 0..node.child_count() {
                if let Some(c) = node.child(k) {
                    if c.is_named() {
                        stack.push(c);
                    }
                }
            }
        }
        raw += items.len();
        let mut uniq: std::collections::BTreeSet<(String, i64)> = Default::default();
        for it in &items {
            uniq.insert(it.clone());
        }
        distinct += uniq.len();
        if !items.is_empty() {
            by_file.insert(rel.clone(), items.clone());
        }
    }
    (raw, distinct, by_file)
}

fn git_counts(root: &Path, limit: usize) -> (usize, usize, usize) {
    let out = std::process::Command::new("git")
        .args(["-c", "core.quotepath=false", "log", "-n", &limit.to_string(), "-M", "-m", "--raw", "--pretty=format:%x00%H"])
        .current_dir(root)
        .output()
        .expect("git log");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut commits = 0usize;
    let mut changes = 0usize;
    let mut files: BTreeSet<String> = BTreeSet::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut cur = String::new();
    for line in text.lines() {
        if let Some(h) = line.strip_prefix('\u{0}') {
            commits += 1;
            cur = h.to_string();
            continue;
        }
        if !line.starts_with(':') {
            continue;
        }
        // :mode mode sha sha status\tpath
        let Some((meta, path)) = line[1..].split_once('\t') else { continue };
        let status = meta.split_whitespace().last().unwrap_or("").to_string();
        let path = path.to_string();
        if seen.insert((cur.clone(), path.clone())) {
            changes += 1;
            files.insert(path);
        }
    }
    (commits, changes, files.len())
}

fn walk(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    fn rec(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if matches!(name.as_str(), "target" | ".git" | ".jingwei" | "node_modules") || name.starts_with('.') {
                    continue;
                }
                rec(root, &p, out);
                continue;
            }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "go") {
                continue;
            }
            let rel = p.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or(name);
            out.push((rel, p));
        }
    }
    rec(root, root, &mut out);
    out
}

fn parse_counts(root: &Path) -> (BTreeSet<String>, BTreeMap<String, usize>, BTreeMap<String, usize>) {
    use tree_sitter::Parser;
    let mut with_syms = BTreeSet::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut rust = Parser::new();
    rust.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    let mut go = Parser::new();
    go.set_language(&tree_sitter_go::LANGUAGE.into()).unwrap();

    for (rel, abs) in walk(root) {
        let Ok(src) = std::fs::read(&abs) else { continue };
        let is_go = rel.ends_with(".go");
        let parser = if is_go { &mut go } else { &mut rust };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut n = 0usize;
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let kind = node.kind();
            let mapped = if is_go {
                match kind {
                    "function_declaration" => Some("go.func"),
                    "method_declaration" => Some("go.method"),
                    "type_spec" | "type_alias" => Some("go.type"),
                    _ => None,
                }
            } else {
                match kind {
                    "function_item" => Some("fn"),
                    "struct_item" => Some("struct"),
                    "enum_item" => Some("enum"),
                    "trait_item" => Some("trait"),
                    "impl_item" => Some("impl"),
                    "mod_item" => Some("mod"),
                    "macro_definition" => Some("macro"),
                    "union_item" => Some("union"),
                    "const_item" => Some("const"),
                    "static_item" => Some("static"),
                    "type_item" => Some("type"),
                    "foreign_mod_item" => Some("foreign_mod"),
                    _ => None,
                }
            };
            if let Some(k) = mapped {
                if node.child_by_field_name("name").is_some() {
                    n += 1;
                    *kinds.entry(k.to_string()).or_insert(0) += 1;
                }
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.is_named() {
                        stack.push(c);
                    }
                }
            }
        }
        if n > 0 {
            with_syms.insert(rel.clone());
            per_file.insert(rel, n);
        }
    }
    (with_syms, kinds, per_file)
}

fn dep_names(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    fn rec(dir: &Path, out: &mut BTreeSet<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if matches!(name.as_str(), "target" | ".git" | ".jingwei" | "node_modules") || name.starts_with('.') {
                    continue;
                }
                rec(&p, out);
                continue;
            }
            if name == "Cargo.toml" {
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                let mut in_deps = false;
                for line in text.lines() {
                    let l = line.trim();
                    if l.starts_with('[') {
                        in_deps = l == "[dependencies]";
                        continue;
                    }
                    if in_deps {
                        if let Some((k, _)) = l.split_once('=') {
                            let k = k.trim().trim_matches('"');
                            if !k.is_empty() && !k.contains(' ') {
                                out.insert(k.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    rec(root, &mut out);
    out
}

fn edge_counts(root: &Path) -> (usize, usize, usize, Vec<String>, BTreeMap<String, usize>, BTreeMap<String, usize>, (usize, usize, usize)) {
    // 与 jw 同口径：同一 (src,dst) 只算一条（jw 的 edge 主键是 src,dst,kind）
    use tree_sitter::Parser;
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    let mut mods: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    let mut uses: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    let mut impl_blocks = 0usize;
    let (mut raw_use, mut raw_mod) = (0usize, 0usize);
    let mut mod_by_src: BTreeMap<String, usize> = BTreeMap::new();
    let mut targets: BTreeSet<String> = BTreeSet::new();
    for (rel, abs) in walk(root) {
        if !rel.ends_with(".rs") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "mod_item" => {
                    raw_mod += 1;
                    if let Some(n) = node.child_by_field_name("name") {
                        if let Ok(t) = n.utf8_text(&src) {
                            let dst = {
                                let base = abs.parent().unwrap_or(root);
                                let mut found = format!("?{t}");
                                for c in [base.join(format!("{t}.rs")), base.join(&t).join("mod.rs")] {
                                    if c.is_file() {
                                        if let Ok(r) = c.strip_prefix(root) {
                                            found = r.to_string_lossy().replace('\\', "/");
                                        }
                                    }
                                }
                                found
                            };
                            *mod_by_src.entry(rel.clone()).or_insert(0) += 1;
                            mods.insert((rel.clone(), dst));
                            let base = abs.parent().unwrap_or(root);
                            for c in [base.join(format!("{t}.rs")), base.join(&t).join("mod.rs")] {
                                if c.is_file() {
                                    if let Ok(r) = c.strip_prefix(root) {
                                        targets.insert(r.to_string_lossy().replace('\\', "/"));
                                    }
                                }
                            }
                        }
                    }
                }
                "use_declaration" => {
                    raw_use += 1;
                    if let Ok(t) = node.utf8_text(&src) {
                        let t = t.trim().trim_end_matches(';').trim().trim_start_matches("use ").trim();
                        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                        uses.insert((rel.clone(), t));
                    }
                }
                "impl_item" => impl_blocks += 1,
                _ => {}
            }
            for k in 0..node.child_count() {
                if let Some(c) = node.child(k) {
                    if c.is_named() {
                        stack.push(c);
                    }
                }
            }
        }
    }
    let mut use_by_src: BTreeMap<String, usize> = BTreeMap::new();
    for (src, _) in &uses {
        *use_by_src.entry(src.clone()).or_insert(0) += 1;
    }
    (mods.len(), uses.len(), impl_blocks, targets.into_iter().collect(), use_by_src, mod_by_src, (raw_use, raw_mod, impl_blocks))
}

fn unparsed_files(root: &Path, with_syms: &BTreeSet<String>) -> Vec<String> {
    walk(root)
        .into_iter()
        .map(|(rel, _)| rel)
        .filter(|rel| !with_syms.contains(rel))
        .collect()
}
