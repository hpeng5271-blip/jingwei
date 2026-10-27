//! 对照组 B 的库形态：独立遍历（git 直读 / tree-sitter 直读 / TOML 自解析），不依赖 jw。
//!
//! 与 jw 共用 tree-sitter 语法（不可消除），代码路径独立。

pub mod expand;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn git_counts(root: &Path, limit: usize) -> (usize, usize, usize) {
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
        let Some((meta, path)) = line[1..].split_once('\t') else { continue };
        let _status = meta.split_whitespace().last().unwrap_or("").to_string();
        let path = path.to_string();
        if seen.insert((cur.clone(), path.clone())) {
            changes += 1;
            files.insert(path);
        }
    }
    (commits, changes, files.len())
}

pub fn walk(root: &Path) -> Vec<(String, PathBuf)> {
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

pub fn parse_counts(root: &Path) -> (BTreeSet<String>, BTreeMap<String, usize>) {
    use tree_sitter::Parser;
    let mut with_syms = BTreeSet::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
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
            with_syms.insert(rel);
        }
    }
    (with_syms, kinds)
}

/// 原始层计数：use / mod / impl 块。
pub fn edge_counts(root: &Path) -> (usize, usize, usize) {
    use tree_sitter::Parser;
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    let (mut m, mut u, mut i) = (0usize, 0usize, 0usize);
    for (rel, abs) in walk(root) {
        if !rel.ends_with(".rs") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "mod_item" => m += 1,
                "use_declaration" => u += 1,
                "impl_item" => i += 1,
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
    (u, m, i)
}

/// 树里没有符号的文件（相对路径）。
pub fn unparsed_files(root: &Path) -> Vec<String> {
    let (with_syms, _) = parse_counts(root);
    walk(root)
        .into_iter()
        .map(|(rel, _)| rel)
        .filter(|rel| !with_syms.contains(rel))
        .collect()
}

/// Python 原始层计数（独立遍历）：func/method/class/import。
pub fn py_counts(root: &Path) -> BTreeMap<String, usize> {
    use tree_sitter::Node;
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return BTreeMap::new();
    }
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    fn count(n: Node, src: &[u8], in_class: bool, out: &mut BTreeMap<String, usize>) {
        match n.kind() {
            "function_definition" => {
                let k = if in_class { "method" } else { "func" };
                *out.entry(k.into()).or_default() += 1;
            }
            "class_definition" => {
                *out.entry("class".into()).or_default() += 1;
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    count(ch, src, true, out);
                }
                return;
            }
            "import_statement" | "import_from_statement" => {
                let text = n.utf8_text(src).unwrap_or("");
                let names = count_import_names(text);
                *out.entry("import".into()).or_default() += names;
            }
            _ => {}
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            count(ch, src, in_class, out);
        }
    }
    fn py_files(dir: &Path, out: &mut Vec<PathBuf>) {
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
                py_files(&p, out);
            } else if name.ends_with(".py") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    py_files(root, &mut files);
    for abs in files {
        let rel = abs
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let _ = rel;
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut c = tree.root_node().walk();
        for ch in tree.root_node().children(&mut c) {
            count(ch, &src, false, &mut out);
        }
    }
    out
}

/// 一行 import 里的名字数（from x import a, b → 2；星号算 1）。
fn count_import_names(text: &str) -> usize {
    let t = text.trim();
    let list = if let Some(rest) = t.strip_prefix("from") {
        let after = rest.trim().trim_start_matches('.').trim();
        match after.strip_prefix("import") {
            Some(l) => l.trim().to_string(),
            None => after.splitn(2, " import").nth(1).unwrap_or("").trim().to_string(),
        }
    } else if let Some(rest) = t.strip_prefix("import") {
        rest.trim().to_string()
    } else {
        String::new()
    };
    list.trim_matches(|c: char| c == '(' || c == ')')
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .count()
}

/// 每个 .py 文件的导入名字数（与 jw 的 import 边数应逐文件相等）。
pub fn py_import_names_by_file(root: &Path) -> BTreeMap<String, usize> {
    use tree_sitter::Node;
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return BTreeMap::new();
    }
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    fn count(n: Node, src: &[u8], per: &mut usize) {
        match n.kind() {
            "import_statement" | "import_from_statement" => {
                *per += count_import_names(n.utf8_text(src).unwrap_or(""));
            }
            _ => {}
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            count(ch, src, per);
        }
    }
    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
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
                files(&p, out);
            } else if name.ends_with(".py") {
                out.push(p);
            }
        }
    }
    let mut list = Vec::new();
    files(root, &mut list);
    for abs in list {
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut per = 0usize;
        let mut c = tree.root_node().walk();
        for ch in tree.root_node().children(&mut c) {
            count(ch, &src, &mut per);
        }
        if per > 0 {
            let rel = abs
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            out.insert(rel, per);
        }
    }
    out
}

/// TS 原始层计数：func/class/interface/type/import（import 按名字数）。
pub fn ts_counts(root: &Path) -> BTreeMap<String, usize> {
    use tree_sitter::Node;
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    if parser.set_language(&lang).is_err() {
        return BTreeMap::new();
    }
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    fn count(n: Node, src: &[u8], out: &mut BTreeMap<String, usize>) {
        let k = match n.kind() {
            "function_declaration" | "method_definition" => Some("func"),
            "class_declaration" => Some("class"),
            "interface_declaration" => Some("interface"),
            "type_alias_declaration" => Some("type"),
            _ => None,
        };
        if let Some(k) = k {
            *out.entry(k.into()).or_default() += 1;
        }
        if n.kind() == "import_statement" {
            *out.entry("import".into()).or_default() += count_ts_names(n.utf8_text(src).unwrap_or(""));
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            count(ch, src, out);
        }
    }
    for abs in ts_files(root) {
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut c = tree.root_node().walk();
        for ch in tree.root_node().children(&mut c) {
            count(ch, &src, &mut out);
        }
    }
    out
}

/// 每文件 TS 导入名数。
pub fn ts_import_names_by_file(root: &Path) -> BTreeMap<String, usize> {
    use tree_sitter::Node;
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    if parser.set_language(&lang).is_err() {
        return BTreeMap::new();
    }
    let mut out = BTreeMap::new();
    fn count(n: Node, src: &[u8], per: &mut usize) {
        if n.kind() == "import_statement" {
            *per += count_ts_names(n.utf8_text(src).unwrap_or(""));
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            count(ch, src, per);
        }
    }
    for abs in ts_files(root) {
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut per = 0usize;
        let mut c = tree.root_node().walk();
        for ch in tree.root_node().children(&mut c) {
            count(ch, &src, &mut per);
        }
        if per > 0 {
            let rel = abs
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            out.insert(rel, per);
        }
    }
    out
}

fn ts_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if matches!(name.as_str(), "node_modules" | ".git" | ".jingwei" | "target" | "out" | "dist")
                    || name.starts_with('.')
                {
                    continue;
                }
                walk(&p, out);
            } else if (name.ends_with(".ts") && !name.ends_with(".d.ts")) || name.ends_with(".tsx") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

/// 一行 TS import 的名字数（import {a, b} / import x / import * as y / import "m"）。
fn count_ts_names(text: &str) -> usize {
    let t = text.trim();
    if let Some(open) = t.find('{') {
        if let Some(close) = t[open..].find('}') {
            return t[open + 1..open + close]
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .count()
                .max(1);
        }
    }
    1
}

/// 与 jw 相同的政策常量：超过 2MB 的文件跳过（不共享代码，只共享政策）。
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 被跳过的文件（相对路径）。
pub fn skipped_files(root: &Path) -> Vec<String> {
    fn walk_all(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if matches!(name.as_str(), "node_modules" | ".git" | ".jingwei" | "target")
                    || name.starts_with('.')
                {
                    continue;
                }
                walk_all(base, &p, out);
            } else {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext, "rs" | "go" | "py" | "ts" | "tsx") {
                    continue;
                }
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                if size > MAX_FILE_BYTES {
                    if let Ok(rel) = p.strip_prefix(base) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk_all(root, root, &mut out);
    out.sort();
    out
}
