use std::path::Path;
use tree_sitter::{Node, Parser};

#[derive(Debug, Clone)]
pub struct Symbol {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub line: usize,
    /// 文件 mtime（unix 秒）；0 = 取不到。判断的 at 用它，重跑才幂等。
    pub mtime: i64,
    /// 纯内容 hash（查同内容实体用）。
    pub content_hash: String,
    /// 内容身份：改名/移动后不变（FNV-1a，按整个文件）。
    pub entity: String,
}

const SKIP_DIRS: &[&str] = &["target", ".git", ".jingwei", "node_modules"];

/// `mod name;` 的目标文件（相对 root）：先按 `foo.rs → foo/` 找，再退回同目录。
/// Rust 的 crate 根不一定是 lib.rs/main.rs（集成测试、build.rs 也是根），所以两条布局都要试。
fn mod_target(root: &Path, abs: &Path, name: &str) -> String {
    // `mod r#type;` 的原始标识符前缀不是文件名的一部分（文件是 type.rs）
    let name = name.strip_prefix("r#").unwrap_or(name);
    let parent = abs.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let stem_dir = match abs.file_stem().and_then(|s| s.to_str()).unwrap_or("") {
        "" | "mod" | "lib" | "main" => parent.clone(),
        stem => parent.join(stem),
    };
    let mut cands = vec![
        stem_dir.join(format!("{name}.rs")),
        stem_dir.join(name).join("mod.rs"),
    ];
    if stem_dir != parent {
        cands.push(parent.join(format!("{name}.rs")));
        cands.push(parent.join(name).join("mod.rs"));
    }
    for f in cands {
        if f.is_file() {
            if let Ok(r) = f.strip_prefix(root) {
                return r.to_string_lossy().replace('\\', "/");
            }
        }
    }
    format!("?{name}")
}

/// 一条关系（不是判断）：src → dst，kind ∈ {mod, use, impl}。
#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub src: String,
    pub dst: String,
    pub kind: String,
    /// use 边导入的叶子名（mod/impl 为 None）
    pub leaf: Option<String>,
    pub line: Option<i64>,
}

/// 扫 .rs 文件的三种最小关系。只做能机械判定的，不做调用图/类型推导。
pub fn edges(root: &Path, limit: usize) -> Vec<EdgeRow> {
    let mut out = Vec::new();
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return out;
    }
    walk_edges(root, root, limit, &mut parser, &mut out);
    out
}

fn walk_edges(root: &Path, dir: &Path, limit: usize, parser: &mut Parser, out: &mut Vec<EdgeRow>) {
    if out.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            walk_edges(root, &p, limit, parser, out);
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if lang_for_ext(ext).is_none() {
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or(name);
        let Ok(src) = std::fs::read(&p) else { continue };
        let Some(tree) = parser.parse(&src, None) else {
            continue;
        };
        let mut cur = tree.root_node().walk();
        for child in tree.root_node().named_children(&mut cur) {
            match child.kind() {
                "mod_item" => {
                    // inline `mod x { ... }` 不是文件边（没有目标文件）；但体内的 use 仍要下去看
                    if child.child_by_field_name("body").is_none() {
                        if let Some(n) = child.child_by_field_name("name") {
                            if let Ok(t) = n.utf8_text(&src) {
                                out.push(EdgeRow {
                                    src: rel.clone(),
                                    dst: mod_target(root, &p, t),
                                    kind: "mod".into(),
                                    leaf: None,
                                    line: Some(child.start_position().row as i64 + 1),
                                });
                            }
                        }
                    }
                }
                "use_declaration" => {
                    if let Ok(t) = child.utf8_text(&src) {
                        // 多行 use 折成一行；同一条 use 出现多次只记一条边（关系不是判断）
                        let t = t.trim().trim_end_matches(';').trim().trim_start_matches("use ").trim();
                        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                        out.push(EdgeRow {
                            src: rel.clone(),
                            dst: format!("?{t}"),
                            kind: "use".into(),
                            leaf: leaf_of(&t),
                            line: None,
                        });
                    }
                }
                "impl_item" => {
                    let tr = child
                        .child_by_field_name("trait")
                        .and_then(|n| n.utf8_text(&src).ok().map(|s| s.to_string()));
                    let ty = child
                        .child_by_field_name("type")
                        .and_then(|n| n.utf8_text(&src).ok().map(|s| s.to_string()));
                    if let (Some(trait_name), Some(type_name)) = (tr, ty) {
                        let trait_name = trait_name.split_whitespace().last().unwrap_or(&trait_name).to_string();
                        out.push(EdgeRow {
                            src: format!("type:{type_name}"),
                            dst: format!("trait:{trait_name}"),
                            kind: "impl".into(),
                            leaf: None,
                            line: Some(child.start_position().row as i64 + 1),
                        });
                    }
                }
                _ => {}
            }
            if out.len() >= limit {
                return;
            }
        }
    }
}

/// 单文件的三种边（与 walk_edges 同一套规则；parse 分批时按文件调用）。

/// 模块路径表：`src/foo/bar.rs` -> `foo::bar`，`src/foo/mod.rs` -> `foo`。
pub fn module_map(root: &Path) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    for (rel, _) in files {
        let no_ext = rel.strip_suffix(".rs").unwrap_or(&rel);
        let parts: Vec<&str> = no_ext.split('/').collect();
        let start = parts.iter().position(|p| *p == "src").map(|i| i + 1).unwrap_or(0);
        let mut mods: Vec<String> = parts[start..].iter().map(|s| s.to_string()).collect();
        if mods.last().map(|s| s == "mod").unwrap_or(false) {
            mods.pop();
        }
        if mods.last().map(|s| s == "lib" || s == "main").unwrap_or(false) {
            mods.pop();
        }
        if !mods.is_empty() {
            m.entry(mods.join("::")).or_insert_with(|| rel.clone());
        }
    }
    m
}

/// 把一个 use 目标解析成文件路径 / ext:<crate> / 原样 `?use ...`。
fn resolve_use(
    text: &str,
    rel: &str,
    map: &std::collections::HashMap<String, String>,
    root: &Path,
) -> String {
    let path = text
        .trim_start_matches("use ")
        .split(['{', ';'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches("::")
        .to_string();
    // 去掉 as 别名
    let path = path.split(" as ").next().unwrap_or(&path).trim().to_string();
    // 以 `::` 开头 = extern prelude 的绝对路径（Rust 2018）：不是内部模块，直接标 ext
    if let Some(rest) = path.strip_prefix("::") {
        let rest = rest.trim_start_matches(':');
        if !rest.is_empty() {
            return format!("ext:{rest}");
        }
    }
    let segs: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return format!("?use {text}");
    }
    // `use crate as X`：目标就是 crate 根（最近 Cargo.toml 的 src/lib.rs|main.rs）
    if segs.len() == 1 && segs[0] == "crate" {
        return match crate_root_file(root, rel) {
            Some(f) => f,
            None => format!("?use {text}"),
        };
    }
    let mut i = 0;
    // crate / self / super 前缀：crate 从根，self 从当前文件模块，super 退一级
    let mut base: Vec<String> = Vec::new();
    match segs[0] {
        "crate" => i = 1,
        "self" => {
            i = 1;
            base = rel
                .strip_suffix(".rs")
                .unwrap_or(rel)
                .split('/')
                .filter(|s| *s != "src")
                .map(|s| s.to_string())
                .collect();
            if base.last().map(|s| s == "mod" || s == "lib" || s == "main").unwrap_or(false) {
                base.pop();
            }
        }
        "super" => {
            i = 1;
            let mut b: Vec<String> = rel
                .strip_suffix(".rs")
                .unwrap_or(rel)
                .split('/')
                .filter(|s| *s != "src")
                .map(|s| s.to_string())
                .collect();
            if b.last().map(|s| s == "mod" || s == "lib" || s == "main").unwrap_or(false) {
                b.pop();
            }
            b.pop();
            base = b;
        }
        "std" | "core" | "alloc" => return format!("ext:{}", path),
        _ => {
            // 本地顶层模块？
            if map.keys().any(|k| k == segs[0] || k.starts_with(&format!("{}::", segs[0]))) {
                i = 0;
            } else {
                return format!("ext:{path}");
            }
        }
    }
    let mut target = base;
    for s in &segs[i..] {
        target.push(s.to_string());
    }
    // 最长后缀匹配
    for cut in 0..target.len() {
        let cand = target[cut..].join("::");
        if let Some(f) = map.get(&cand) {
            return f.clone();
        }
    }
    format!("?use {text}")
}

/// `use crate as X` 的目标：从该文件往上找最近的 Cargo.toml 目录，取 `src/lib.rs` 或 `src/main.rs`。
pub fn crate_root_file(root: &Path, rel: &str) -> Option<String> {
    let mut dir = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    dir.pop();
    loop {
        if dir.join("Cargo.toml").is_file() {
            for cand in ["src/lib.rs", "src/main.rs"] {
                let p = dir.join(cand);
                if p.is_file() {
                    if let Ok(r) = p.strip_prefix(root) {
                        return Some(r.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
            return None;
        }
        if !dir.pop() || !dir.starts_with(root) {
            return None;
        }
    }
}

/// 函数上方紧邻的连续属性里的测试属性：
/// - `test` / `tokio::test` → `Some("standard")`（标准 harness）
/// - `rstest` / `quickcheck` / 其它以 `::test` 结尾的自定义属性宏 → `Some("macro")`
///   （`#[proptest]` 与 `proptest!` 宏体不纳入：token 树里没有可遍历的 fn）
fn has_test_attr(node: Node, src: &[u8]) -> Option<&'static str> {
    let mut cur = node.prev_sibling();
    let mut macro_hit = false;
    while let Some(s) = cur {
        // 注释是 trivia：`#[test]` 与 fn 之间夹注释行时仍算同一个属性链
        if s.kind() == "line_comment" || s.kind() == "block_comment" {
            cur = s.prev_sibling();
            continue;
        }
        if s.kind() != "attribute_item" {
            break;
        }
        if let Ok(t) = s.utf8_text(src) {
            let t = t.trim();
            let inner = t.trim_start_matches("#[").trim_end_matches(']').trim();
            let head = inner.split('(').next().unwrap_or(inner).trim();
            if head == "test" || head == "tokio::test" {
                return Some("standard");
            }
            // 末段判定：`rstest` / `rstest::rstest` / `quickcheck` / `nu_test_support::test`
            let last = head.rsplit("::").next().unwrap_or(head);
            if matches!(last, "test" | "rstest" | "quickcheck") {
                macro_hit = true;
            }
        }
        cur = s.prev_sibling();
    }
    if macro_hit {
        Some("macro")
    } else {
        None
    }
}

/// `#[path = "..."] mod x;` 里的路径（取紧邻的连续 attribute_item 里最后一个 path）。
fn path_attr_of(node: Node, src: &[u8]) -> Option<String> {
    let mut cur = node.prev_sibling();
    let mut found: Option<String> = None;
    while let Some(s) = cur {
        if s.kind() != "attribute_item" {
            break;
        }
        if let Ok(t) = s.utf8_text(src) {
            let t = t.trim();
            if t.starts_with("#[path") {
                if let Some(q) = t.find('"') {
                    if let Some(end) = t[q + 1..].find('"') {
                        found = Some(t[q + 1..q + 1 + end].to_string());
                    }
                }
            }
        }
        cur = s.prev_sibling();
    }
    found
}

/// 把 `#[path]` 的字符串解析成相对 root 的文件路径（相对声明处文件所在目录）。
fn resolve_attr_path(root: &Path, abs: &Path, attr: &str, name: &str) -> String {
    let base = abs.parent().unwrap_or(root);
    let cand = base.join(attr.replace('/', std::path::MAIN_SEPARATOR_STR));
    if cand.is_file() {
        if let Ok(r) = cand.strip_prefix(root) {
            return r.to_string_lossy().replace('\\', "/");
        }
    }
    format!("?{name}")
}

/// v0 调用图：只认同文件、裸标识符调用 `foo(...)`。
fn callee_name<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    let f = node.child_by_field_name("function")?;
    if f.kind() != "identifier" {
        return None;
    }
    f.utf8_text(src).ok()
}

fn collect_fn_names(node: Node, src: &[u8], out: &mut std::collections::HashSet<String>) {
    if node.kind() == "function_item" {
        if let Some(n) = node.child_by_field_name("name") {
            if let Ok(t) = n.utf8_text(src) {
                out.insert(t.to_string());
            }
        }
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            if c.is_named() {
                collect_fn_names(c, src, out);
            }
        }
    }
}

fn collect_call_edges(
    node: Node,
    src: &[u8],
    rel: &str,
    fns: &std::collections::HashSet<String>,
    cur: Option<&str>,
    out: &mut Vec<EdgeRow>,
) {
    let cur = if node.kind() == "function_item" {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(src).ok())
            .or(cur)
    } else {
        cur
    };
    if node.kind() == "call_expression" {
        if let (Some(caller), Some(name)) = (cur, callee_name(node, src)) {
            let dst = if fns.contains(name) {
                format!("symbol:{rel}#{name}")
            } else {
                format!("?call {name}")
            };
            out.push(EdgeRow {
                src: format!("symbol:{rel}#{caller}"),
                dst,
                kind: "call".into(),
                leaf: Some(name.to_string()),
                line: Some(node.start_position().row as i64 + 1),
            });
        }
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            if c.is_named() {
                collect_call_edges(c, src, rel, fns, cur, out);
            }
        }
    }
}

pub fn file_edges(root: &Path, abs: &Path, rel: &str, map: &std::collections::HashMap<String, String>) -> Vec<EdgeRow> {
    let mut out = Vec::new();
    let Ok(src) = std::fs::read(abs) else {
        return out;
    };
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return out;
    }
    let Some(tree) = parser.parse(&src, None) else {
        return out;
    };
    let mut stack = vec![(tree.root_node(), 0usize)];
    while let Some((child, depth)) = stack.pop() {
        // 不按深度过滤：inline `mod tests { ... }` 里的 impl/use 也是本文件里的关系
        match child.kind() {
            "mod_item" => {
                // inline `mod x { ... }` 不是文件边（没有目标文件）；但体内的 use 仍要下去看
                if child.child_by_field_name("body").is_none() {
                    if let Some(n) = child.child_by_field_name("name") {
                        if let Ok(t) = n.utf8_text(&src) {
                            // `#[path = "..."] mod x;`：目标由属性给出（相对当前文件所在目录）
                            let dst = match path_attr_of(child, &src) {
                                Some(p) => resolve_attr_path(root, abs, &p, t),
                                None => mod_target(root, abs, t),
                            };
                            out.push(EdgeRow {
                                src: rel.to_string(),
                                dst,
                                kind: "mod".into(),
                                leaf: None,
                                line: Some(child.start_position().row as i64 + 1),
                            });
                        }
                    }
                }
            }
            "use_declaration" => {
                if let Ok(t) = child.utf8_text(&src) {
                    let t = t.trim().trim_end_matches(';').trim().trim_start_matches("use ").trim();
                    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                    // 花括号组展开成多条（每条一个 leaf），与原始层逐条对应
                    for target in expand_use_text(&t) {
                        out.push(EdgeRow {
                            src: rel.to_string(),
                            dst: resolve_use(&target, rel, map, root),
                            kind: "use".into(),
                            leaf: leaf_of(&target),
                            line: Some(child.start_position().row as i64 + 1),
                        });
                    }
                }
            }
            "function_item" => {
                // 测试属性：标准 harness（test / tokio::test）与自定义宏（*::test）分开记
                if let Some(class) = has_test_attr(child, &src) {
                    if let Some(n) = child.child_by_field_name("name") {
                        if let Ok(name) = n.utf8_text(&src) {
                            let kind = if class == "standard" { "test" } else { "test_macro" };
                            out.push(EdgeRow {
                                src: format!("test:{rel}#{name}"),
                                dst: rel.to_string(),
                                kind: kind.to_string(),
                                leaf: Some(name.to_string()),
                                line: Some(child.start_position().row as i64 + 1),
                            });
                        }
                    }
                }
            }
            "impl_item" => {
                let tr = child
                    .child_by_field_name("trait")
                    .and_then(|n| n.utf8_text(&src).ok().map(|s| s.to_string()));
                let ty = child
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(&src).ok().map(|s| s.to_string()));
                if let (Some(trait_name), Some(type_name)) = (tr, ty) {
                    let trait_name = trait_name
                        .split_whitespace()
                        .last()
                        .unwrap_or(&trait_name)
                        .to_string();
                    out.push(EdgeRow {
                        src: format!("type:{type_name}"),
                        dst: format!("trait:{trait_name}"),
                        kind: "impl".into(),
                        leaf: Some(rel.to_string()),
                        line: Some(child.start_position().row as i64 + 1),
                    });
                }
            }
            _ => {}
        }
        for k in 0..child.child_count() {
            if let Some(c) = child.child(k) {
                if c.is_named() {
                    stack.push((c, depth + 1));
                }
            }
        }
    }
    // v0 调用图：同文件裸标识符调用（caller/callee 都是本文件里的 fn）
    let mut fns: std::collections::HashSet<String> = Default::default();
    collect_fn_names(tree.root_node(), &src, &mut fns);
    collect_call_edges(tree.root_node(), &src, rel, &fns, None, &mut out);
    out
}

/// 分批解析：逐文件回调（symbols, edges），跳过已解析的文件（断点续跑）。
/// 返回处理的文件数。
pub fn scan_batched<F>(
    root: &Path,
    limit: usize,
    skip: &std::collections::HashSet<String>,
    mut f: F,
) -> usize
where
    F: FnMut(&str, &Path, Vec<Symbol>, Vec<EdgeRow>) -> bool,
{
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    files.sort();
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return 0;
    }
    let mut done = 0usize;
    let mut total_syms = 0usize;
    let map = module_map(root);
    let total_files = files.len();
    let mut idx = 0usize;
    for (rel, abs) in files {
        idx += 1;
        let files_left = total_files.saturating_sub(idx);
        if skip.contains(&rel) {
            continue;
        }
        let mut syms = Vec::new();
        parse_file(&abs, &rel, &mut parser, &mut syms, usize::MAX);
        let edges = file_edges(root, &abs, &rel, &map);
        total_syms += syms.len();
        done += 1;
        if !f(&rel, &abs, syms, edges) {
            return done;
        }
        if total_syms >= limit {
            return done;
        }
    }
    done
}



/// 一条 use 文本的叶子名：`a::b::C` → C；`a::b::{C, D}` 展开由调用方；`::self` 取模块名。
pub fn leaf_of(text: &str) -> Option<String> {
    let t = text.trim().trim_end_matches(';').trim();
    let t = t.split(" as ").last().unwrap_or(t).trim();
    let t = t.trim_start_matches("use ").trim();
    let segs: Vec<&str> = t.split("::").filter(|s| !s.is_empty()).collect();
    let mut last = segs.last().copied().unwrap_or("");
    if last == "self" && segs.len() >= 2 {
        last = segs[segs.len() - 2];
    }
    let last = last.trim_matches(|c| c == '{' || c == '}' || c == ',').trim();
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

/// 语言注册表（扩展名 → 名字）；新增语言只加一行，不写 if lang==...
pub fn languages() -> &'static [(&'static str, &'static str)] {
    &[("rs", "rust"), ("go", "go"), ("py", "python"), ("ts", "typescript"), ("tsx", "typescript")]
}

fn file_size_for_skip(abs: &std::path::Path) -> Option<u64> {
    std::fs::metadata(abs).ok().map(|m| m.len())
}

pub fn lang_for_ext(ext: &str) -> Option<&'static str> {
    languages().iter().find(|(e, _)| *e == ext).map(|(_, n)| *n)
}



/// 拆 Python import 语句：返回 (level, module, 名字列表)。
fn split_py_import(text: &str) -> (usize, String, Vec<String>) {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("from") {
        let rest = rest.trim();
        let level = rest.chars().take_while(|c| *c == '.').count();
        let after = rest.trim_start_matches('.').trim();
        let (module, names_part) = if let Some(list) = after.strip_prefix("import") {
            (String::new(), list.trim().to_string())
        } else {
            let mut it = after.splitn(2, " import");
            let m = it.next().unwrap_or("").trim().to_string();
            let n = it.next().unwrap_or("").trim().to_string();
            (m, n)
        };
        let names: Vec<String> = names_part
            .trim_matches(|c: char| c == '(' || c == ')')
            .split(',')
            .map(|s| s.trim().trim_end_matches(" as").split(" as ").next().unwrap_or("").trim().to_string())
            .map(|s| s.split(" as ").next().unwrap_or(&s).to_string())
            .filter(|s| !s.is_empty())
            .collect();
        (level, module, names)
    } else if let Some(rest) = t.strip_prefix("import") {
        let names: Vec<String> = rest
            .trim_matches(|c: char| c == '(' || c == ')')
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        (0, String::new(), names)
    } else {
        (0, String::new(), Vec::new())
    }
}


/// 拆 TS import：返回 (模块说明符, 名字列表)。一行多个名字 → 多条边。
fn split_ts_import(text: &str) -> (String, Vec<String>) {
    let t = text.trim();
    let spec = t
        .rsplit("from")
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(';')
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .to_string();
    let spec = if spec.is_empty() {
        // import "./x";
        t.trim_start_matches("import")
            .trim()
            .trim_matches(|c: char| c == '"' || c == '\'' || c == ';')
            .to_string()
    } else {
        spec
    };
    let mut names = Vec::new();
    if let Some(open) = t.find('{') {
        if let Some(close) = t[open..].find('}') {
            for n in t[open + 1..open + close].split(',') {
                let n = n.trim();
                if n.is_empty() {
                    continue;
                }
                // 本地绑定名：`A as B` → B
                let local = n.split(" as ").nth(1).unwrap_or(n).trim();
                names.push(local.to_string());
            }
        }
    }
    if names.is_empty() {
        if let Some(rest) = t.strip_prefix("import") {
            let head = rest.split("from").next().unwrap_or("").trim();
            if head.starts_with('*') {
                names.push("*".into());
            } else if !head.is_empty() && !head.starts_with('"') && !head.starts_with('\'') {
                names.push(head.trim_end_matches(',').trim().to_string());
            } else {
                names.push("*".into()); // 副作用导入
            }
        }
    }
    (spec, names)
}

/// TS/TSX 最小提取：函数/类/接口/类型 + import 边。
fn parse_ts(parser: &mut Parser, path: &Path, rel: &str) -> (Vec<Symbol>, Vec<EdgeRow>) {
    use tree_sitter::Node;
    let mut syms = Vec::new();
    let mut edges = Vec::new();
    let Ok(src) = std::fs::read(path) else {
        return (syms, edges);
    };
    let Some(tree) = parser.parse(&src, None) else {
        return (syms, edges);
    };
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let ch = crate::hash::content_id(&src);
    let ent = crate::hash::entity_id(&src, rel);
    fn name_of(n: Node, src: &[u8]) -> Option<String> {
        n.child_by_field_name("name")
            .and_then(|c| c.utf8_text(src).ok())
            .map(|s| s.to_string())
    }
    fn walk(n: Node, src: &[u8], rel: &str, mtime: i64, ch: &str, ent: &str,
            syms: &mut Vec<Symbol>, edges: &mut Vec<EdgeRow>) {
        let kind = match n.kind() {
            "function_declaration" => Some("ts.func"),
            "method_definition" => Some("ts.func"),
            "class_declaration" => Some("ts.class"),
            "interface_declaration" => Some("ts.interface"),
            "type_alias_declaration" => Some("ts.type"),
            _ => None,
        };
        if let Some(k) = kind {
            if let Some(name) = name_of(n, src) {
                syms.push(Symbol {
                    path: rel.into(),
                    name,
                    kind: k.into(),
                    line: n.start_position().row + 1,
                    mtime,
                    content_hash: ch.into(),
                    entity: ent.into(),
                });
            }
        }
        if n.kind() == "import_statement" {
            let text = n.utf8_text(src).unwrap_or("");
            let (spec, names) = split_ts_import(text);
            if !spec.is_empty() {
                for name in names {
                    edges.push(EdgeRow {
                        src: rel.to_string(),
                        dst: format!("?ts:{spec}|{name}"),
                        kind: "import".to_string(),
                        leaf: Some(name),
                        line: Some(n.start_position().row as i64 + 1),
                    });
                }
            }
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            walk(child, src, rel, mtime, ch, ent, syms, edges);
        }
    }
    let mut c = tree.root_node().walk();
    for child in tree.root_node().children(&mut c) {
        walk(child, &src, rel, mtime, &ch, &ent, &mut syms, &mut edges);
    }
    (syms, edges)
}

/// Python 的最小提取：函数/方法/类 + import 边。内部包解析本轮不做（import 一律 ext:，未识别的留 ?）。
fn parse_python(parser: &mut Parser, path: &Path, rel: &str) -> (Vec<Symbol>, Vec<EdgeRow>) {
    use tree_sitter::Node;
    let mut syms = Vec::new();
    let mut edges = Vec::new();
    let Ok(src) = std::fs::read(path) else {
        return (syms, edges);
    };
    let Some(tree) = parser.parse(&src, None) else {
        return (syms, edges);
    };
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let ch = crate::hash::content_id(&src);
    let ent = crate::hash::entity_id(&src, rel);
    fn name_of(n: Node, src: &[u8]) -> Option<(String, usize)> {
        let c = n.child_by_field_name("name")?;
        Some((c.utf8_text(src).ok()?.to_string(), n.start_position().row + 1))
    }
    fn push(syms: &mut Vec<Symbol>, rel: &str, name: String, kind: &str, line: usize, mtime: i64, ch: &str, ent: &str) {
        syms.push(Symbol {
            path: rel.into(),
            name,
            kind: kind.into(),
            line,
            mtime,
            content_hash: ch.into(),
            entity: ent.into(),
        });
    }
    fn walk(n: Node, src: &[u8], rel: &str, in_class: bool, mtime: i64, ch: &str, ent: &str,
            syms: &mut Vec<Symbol>, edges: &mut Vec<EdgeRow>) {
        match n.kind() {
            "function_definition" => {
                if let Some((name, line)) = name_of(n, src) {
                    let kind = if in_class { "py.method" } else { "py.func" };
                    push(syms, rel, name, kind, line, mtime, ch, ent);
                }
            }
            "class_definition" => {
                if let Some((name, line)) = name_of(n, src) {
                    push(syms, rel, name, "py.class", line, mtime, ch, ent);
                }
                let mut c = n.walk();
                for child in n.children(&mut c) {
                    walk(child, src, rel, true, mtime, ch, ent, syms, edges);
                }
                return;
            }
            "import_statement" | "import_from_statement" => {
                let text = n.utf8_text(src).unwrap_or("").trim().to_string();
                let line = n.start_position().row as i64 + 1;
                // 一行可以有多个导入名：from . import a, b → 2 条边（与 raw 口径一致）
                let (level, module, names) = split_py_import(&text);
                for name in names {
                    edges.push(EdgeRow {
                        src: rel.to_string(),
                        dst: format!("?py:{level}|{module}|{name}"),
                        kind: "import".to_string(),
                        leaf: None,
                        line: Some(line),
                    });
                }
            }
            _ => {}
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            walk(child, src, rel, in_class, mtime, ch, ent, syms, edges);
        }
    }
    let mut c = tree.root_node().walk();
    for child in tree.root_node().children(&mut c) {
        walk(child, &src, rel, false, mtime, &ch, &ent, &mut syms, &mut edges);
    }
    (syms, edges)
}



/// TS 原始层计数：func/class/interface/type/import（import 按名字数）。
pub fn raw_ts_counts(root: &Path) -> std::collections::BTreeMap<String, usize> {
    use tree_sitter::Node;
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    let mut parser = Parser::new();
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    if parser.set_language(&lang).is_err() {
        return Default::default();
    }
    let mut out: std::collections::BTreeMap<String, usize> = Default::default();
    fn count(n: Node, src: &[u8], out: &mut std::collections::BTreeMap<String, usize>) {
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
            let text = n.utf8_text(src).unwrap_or("");
            let (_, names) = split_ts_import(text);
            *out.entry("import".into()).or_default() += names.len();
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            count(ch, src, out);
        }
    }
    for (rel, abs) in files {
        if !(rel.ends_with(".ts") || rel.ends_with(".tsx")) {
            continue;
        }
        if rel.ends_with(".d.ts") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut c = tree.root_node().walk();
        for ch in tree.root_node().children(&mut c) {
            count(ch, &src, &mut out);
        }
    }
    out
}

/// Python 原始层计数：只看声明节点（不落库、不解析含义）。
pub fn raw_py_counts(root: &Path) -> std::collections::BTreeMap<String, usize> {
    use tree_sitter::Node;
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    let mut parser = Parser::new();
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return Default::default();
    }
    let mut out: std::collections::BTreeMap<String, usize> = Default::default();
    fn count(n: Node, src: &[u8], in_class: bool, out: &mut std::collections::BTreeMap<String, usize>) {
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
                let (_, _, names) = split_py_import(text);
                *out.entry("import".into()).or_default() += names.len();
            }
            _ => {}
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            count(ch, src, in_class, out);
        }
    }
    for (rel, abs) in files {
        if !rel.ends_with(".py") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut c = tree.root_node().walk();
        for ch in tree.root_node().children(&mut c) {
            count(ch, &src, false, &mut out);
        }
    }
    out
}

/// Go 的最小提取：func / method / type / const / var + import 边。
fn parse_go(parser: &mut Parser, path: &Path, rel: &str, module: Option<&str>) -> (Vec<Symbol>, Vec<EdgeRow>) {
    let mut syms = Vec::new();
    let mut edges = Vec::new();
    let Ok(src) = std::fs::read(path) else {
        return (syms, edges);
    };
    let Some(tree) = parser.parse(&src, None) else {
        return (syms, edges);
    };
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let content_hash = crate::hash::content_id(&src);
    let entity = crate::hash::entity_id(&src, rel);

    fn name_of(node: Node, src: &[u8]) -> Option<(String, usize)> {
        let n = node.child_by_field_name("name")?;
        Some((n.utf8_text(src).ok()?.to_string(), n.start_position().row + 1))
    }

    fn walk_go(node: Node, src: &[u8], rel: &str, mtime: i64, ch: &str, ent: &str,
               module: Option<&str>, syms: &mut Vec<Symbol>, edges: &mut Vec<EdgeRow>) {
        match node.kind() {
            "function_declaration" => {
                if let Some((name, line)) = name_of(node, src) {
                    syms.push(Symbol { path: rel.into(), name, kind: "go.func".into(), line, mtime,
                        content_hash: ch.into(), entity: ent.into() });
                }
            }
            "method_declaration" => {
                if let Some((name, line)) = name_of(node, src) {
                    syms.push(Symbol { path: rel.into(), name, kind: "go.method".into(), line, mtime,
                        content_hash: ch.into(), entity: ent.into() });
                }
            }
            "type_alias" => {
                if let Some((name, line)) = name_of(node, src) {
                    syms.push(Symbol { path: rel.into(), name, kind: "go.type".into(), line, mtime,
                        content_hash: ch.into(), entity: ent.into() });
                }
            }
            "type_spec" => {
                if let Some((name, line)) = name_of(node, src) {
                    syms.push(Symbol { path: rel.into(), name, kind: "go.type".into(), line, mtime,
                        content_hash: ch.into(), entity: ent.into() });
                }
            }
            "import_spec" => {
                if let Some(p) = node.child_by_field_name("path") {
                    if let Ok(t) = p.utf8_text(src) {
                        let t = t.trim_matches('"');
                        // 本模块内的包 → dir:<相对目录>；其余 → ext:<path>
                        let dst = match module {
                            Some(m) if t == m => "dir:.".to_string(),
                            Some(m) if t.starts_with(&format!("{m}/")) => {
                                format!("dir:{}", &t[m.len() + 1..])
                            }
                            _ => format!("ext:{t}"),
                        };
                        edges.push(EdgeRow { src: rel.into(), dst, kind: "import".into(), leaf: None, line: None });
                    }
                }
            }
            _ => {}
        }
        let mut cur = node.walk();
        for c in node.children(&mut cur) {
            walk_go(c, src, rel, mtime, ch, ent, module, syms, edges);
        }
    }

    walk_go(tree.root_node(), &src, rel, mtime, &content_hash, &entity, module, &mut syms, &mut edges);
    (syms, edges)
}


/// 读 go.mod 的 module 行（只读根目录那份）。
pub fn go_module(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("go.mod")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// 未解析 Go 文件的原因（判据：各类之和 = 未解析数，不许有"其他"）。
pub fn go_reasons(root: &Path, unparsed: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return unparsed.iter().map(|p| (p.clone(), "parse_failed".into())).collect();
    }
    for rel in unparsed {
        let abs = root.join(rel);
        let Ok(src) = std::fs::read(&abs) else {
            out.push((rel.clone(), "gone".into()));
            continue;
        };
        let head = String::from_utf8_lossy(&src[..src.len().min(2048)]).to_string();
        let reason = if head.contains("Code generated") && head.contains("DO NOT EDIT") {
            "generated"
        } else if src.iter().all(|b| b.is_ascii_whitespace()) {
            "empty"
        } else if head.lines().any(|l| l.trim_start().starts_with("//go:build") || l.trim_start().starts_with("// +build")) {
            "build_tag_excluded"
        } else {
            match parser.parse(&src, None) {
                Some(tree) => {
                    let root_node = tree.root_node();
                    let mut has_item = false;
                    let mut has_mapped = false;
                    let mut cur = root_node.walk();
                    for c in root_node.children(&mut cur) {
                        match c.kind() {
                            "function_declaration" | "method_declaration" | "type_declaration" => {
                                has_item = true;
                                has_mapped = true;
                            }
                            "const_declaration" | "var_declaration" | "import_declaration"
                            | "package_clause" => {
                                has_item = true;
                            }
                            _ => {}
                        }
                    }
                    if has_mapped {
                        "no_mapped_item"
                    } else if has_item {
                        "only_decls"
                    } else {
                        "empty"
                    }
                }
                None => "parse_failed",
            }
        };
        out.push((rel.clone(), reason.to_string()));
    }
    out
}


/// 树里所有登记语言的文件（相对路径）。
pub fn source_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    files.into_iter().map(|(rel, _)| rel).collect()
}



/// 把 `crate::{A, b::{C, D}}` 展开成多条目标文本。
fn matching_close(t: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in t.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in t.chars() {
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
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    out.push(cur.trim().to_string());
    out.retain(|x| !x.is_empty());
    out
}

fn join_path(prefix: &str, item: &str) -> String {
    let item = item.trim();
    if prefix.is_empty() {
        item.to_string()
    } else if item.starts_with("::") {
        item.to_string()
    } else {
        format!("{prefix}::{item}")
    }
}

fn expand_items(s: &str, prefix: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    // 整体是 `{...}`：剥掉外层，沿用 prefix
    if s.starts_with('{') {
        if let Some(close) = matching_close(s) {
            if s[close + 1..].trim().is_empty() {
                return expand_items(&s[1..close], prefix);
            }
        }
    }
    let items = split_top_level(s);
    if items.len() > 1 {
        let mut out = Vec::new();
        for it in items {
            out.extend(expand_items(&it, prefix));
        }
        return out;
    }
    let item = &items[0];
    if let Some(open) = item.find('{') {
        let pre = item[..open].trim().trim_end_matches("::");
        if let Some(close) = matching_close(&item[open..]) {
            let inside = &item[open + 1..open + close];
            let base = join_path(prefix, pre);
            return expand_items(inside, &base);
        }
    }
    let (item_path, item_alias) = match item.split_once(" as ") {
        Some((p, a)) => (p.trim(), Some(a.trim())),
        None => (item.trim(), None),
    };
    let leaf = item_path;
    if leaf == "self" || leaf.is_empty() {
        let parent = prefix.trim_end_matches("::").to_string();
        if parent.is_empty() {
            Vec::new()
        } else if let Some(a) = item_alias.filter(|a| !a.is_empty()) {
            // `use a::{self as x}`：整条指模块 a 本身，但本地绑定名是 x（leaf 取别名）
            vec![format!("{parent} as {a}")]
        } else {
            vec![parent]
        }
    } else {
        let full = join_path(prefix, leaf);
        match item_alias {
            Some(a) if !a.is_empty() => vec![format!("{full} as {a}")],
            _ => vec![full],
        }
    }
}

/// 剥掉可见性前缀：`pub use` / `pub(crate) use` / `pub(super) use` / `pub(in path) use`。
/// 可见性不是导入路径的一部分；不剥会把 dst 写成 `ext:pub use ...`。
fn strip_vis_prefix(t: &str) -> &str {
    let Some(rest) = t.strip_prefix("pub") else {
        return t;
    };
    let rest = rest.trim_start();
    let rest = if rest.starts_with('(') {
        match rest.find(')') {
            Some(i) => rest[i + 1..].trim_start(),
            None => rest,
        }
    } else {
        rest
    };
    rest
}

pub fn expand_use_text(text: &str) -> Vec<String> {
    let mut t = text.trim().trim_end_matches(';').trim().to_string();
    t = strip_vis_prefix(&t).to_string();
    if let Some(rest) = t.strip_prefix("use") {
        t = rest.trim_start().to_string();
    }
    let mut out = expand_items(&t, "");
    out.retain(|s| !s.is_empty());
    out
}

/// 原始层叶子名：按文件给出 use 的叶子名多重集（用于"边 leaf 多重集 == 原始多重集"不变量）。
pub fn raw_use_leaves(root: &Path) -> std::collections::BTreeMap<String, Vec<(String, i64)>> {
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let mut out: std::collections::BTreeMap<String, Vec<(String, i64)>> = Default::default();
    if parser.set_language(&language).is_err() {
        return out;
    }
    for (rel, abs) in files {
        if abs.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut stack = vec![tree.root_node()];
        let mut leaves = Vec::new();
        while let Some(node) = stack.pop() {
            if node.kind() == "use_declaration" {
                if let Ok(t) = node.utf8_text(&src) {
                    let line = node.start_position().row as i64 + 1;
                    for target in expand_use_text(t) {
                        if let Some(l) = leaf_of(&target) {
                            leaves.push((l, line));
                        }
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
        if !leaves.is_empty() {
            leaves.sort();
            out.insert(rel, leaves);
        }
    }
    out
}


/// walk 到的源文件（相对路径，排序）；与实现同一条 SKIP 规则。
pub fn walk_paths(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    let mut out: Vec<String> = files.into_iter().map(|(rel, _)| rel).collect();
    out.sort();
    out
}

/// 原始层计数：use 声明数 / mod 声明数 / impl 块数（不判 trait、不解析）。
pub fn raw_totals(root: &Path) -> (usize, usize, usize) {
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return (0, 0, 0);
    }
    let (mut use_n, mut mod_n, mut impl_n) = (0usize, 0usize, 0usize);
    for (_rel, abs) in files {
        if abs.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(src) = std::fs::read(&abs) else { continue };
        let Some(tree) = parser.parse(&src, None) else { continue };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "use_declaration" => use_n += 1,
                "mod_item" => mod_n += 1,
                "impl_item" => impl_n += 1,
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
    (use_n, mod_n, impl_n)
}

/// 分批解析：按扩展名分派到对应语言（rust/go），逐文件回调。
pub struct ScanReport {
    pub files: usize,
    pub skipped: Vec<(String, u64)>,
    pub timings: Vec<(String, u128)>,
    /// 树里各扩展名的候选文件数（覆盖的分母）
    pub seen: std::collections::HashMap<String, usize>,
    /// 因 --limit 提前结束、没扫到的文件数（0 = 全量）
    pub remaining_after_limit: usize,
}

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

pub fn scan_batched_any<F>(
    root: &Path,
    limit: usize,
    skip: &std::collections::HashSet<String>,
    mut f: F,
) -> ScanReport
where
    F: FnMut(&str, &Path, Vec<Symbol>, Vec<EdgeRow>) -> bool,
{
    let mut files = Vec::new();
    collect_rust_files(root, root, &mut files);
    files.sort();
    let mut parser = Parser::new();
    let rust_lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&rust_lang).is_err() {
        return ScanReport { files: 0, skipped: vec![], timings: vec![], seen: Default::default(), remaining_after_limit: 0 };
    }
    let mut go_parser = Parser::new();
    let go_lang: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    let _ = go_parser.set_language(&go_lang);
    let mut py_parser = Parser::new();
    let py_lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    let _ = py_parser.set_language(&py_lang);
    let mut ts_parser = Parser::new();
    let ts_lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let _ = ts_parser.set_language(&ts_lang);
    let map = module_map(root);
    let go_mod = go_module(root);
    let mut done = 0usize;
    let mut total_syms = 0usize;
    let mut skipped: Vec<(String, u64)> = Vec::new();
    let mut timings: Vec<(String, u128)> = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let total_files = files.len();
    let mut idx = 0usize;
    for (rel, abs) in files {
        idx += 1;
        let files_left = total_files.saturating_sub(idx);
        let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("");
        if lang_for_ext(ext).is_none() {
            continue;
        }
        // 分母是"树里的文件"，必须算在 skip 之前
        *seen.entry(ext.to_string()).or_insert(0) += 1;
        if skip.contains(&rel) {
            continue;
        }
        // 超大生成文件：显式跳过并记原因（不许静默跳过）
        // 读不到元数据 = 按"超大"处理（绝不按 0＝小文件去解析）
        let size = match file_size_for_skip(&abs) {
            Some(s) => s,
            None => {
                skipped.push((rel.clone(), u64::MAX));
                continue;
            }
        };
        if size > MAX_FILE_BYTES {
            skipped.push((rel.clone(), size));
            continue;
        }
        let t0 = std::time::Instant::now();
        let (syms, edges) = match lang_for_ext(ext) {
            Some("rust") => {
                let mut syms = Vec::new();
                parse_file(&abs, &rel, &mut parser, &mut syms, usize::MAX);
                (syms, file_edges(root, &abs, &rel, &map))
            }
            Some("go") => parse_go(&mut go_parser, &abs, &rel, go_mod.as_deref()),
            Some("python") => parse_python(&mut py_parser, &abs, &rel),
            Some("typescript") => parse_ts(&mut ts_parser, &abs, &rel),
            _ => continue,
        };
        timings.push((rel.clone(), t0.elapsed().as_millis()));
        total_syms += syms.len();
        done += 1;
        if !f(&rel, &abs, syms, edges) {
            return ScanReport { files: done, skipped, timings, seen, remaining_after_limit: 0 };
        }
        if total_syms >= limit {
            let remaining = files_left;
            return ScanReport { files: done, skipped, timings, seen, remaining_after_limit: remaining };
        }
    }
    ScanReport { files: done, skipped, timings, seen, remaining_after_limit: 0 }
}

fn collect_rust_files(root: &Path, dir: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            collect_rust_files(root, &p, out);
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if lang_for_ext(ext).is_none() {
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or(name);
        out.push((rel, p));
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileFacts {
    /// AST 里可映射项的数量（>0 却没进库 = 真不一致）
    pub mapped: usize,
    /// 顶层是否有 use/mod/impl
    pub decl_only: bool,
    /// 顶层宏调用数量（只说"有宏"，不推断符号在里面）
    pub macros: usize,
    /// 文件没有顶层项
    pub empty: bool,
}

/// 只解析一个文件，返回它的"为什么没符号"事实。
pub fn facts(path: &Path) -> FileFacts {
    let mut f = FileFacts::default();
    let Ok(src) = std::fs::read(path) else {
        return f;
    };
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return f;
    }
    let Some(tree) = parser.parse(&src, None) else {
        return f;
    };
    let root = tree.root_node();
    let mut cur = root.walk();
    let mut named = 0usize;
    for child in root.named_children(&mut cur) {
        named += 1;
        match child.kind() {
            "use_declaration" | "mod_item" | "impl_item" | "extern_crate_declaration" => {
                f.decl_only = true
            }
            "macro_invocation" => f.macros += 1,
            _ => {}
        }
    }
    f.empty = named == 0;
    // 宏调用可能嵌在 mod/impl 里，全树数（只数事实，不做推断）
    fn count_kind(node: Node, kind: &str) -> usize {
        let mut n = if node.kind() == kind { 1 } else { 0 };
        let mut cur = node.walk();
        for child in node.children(&mut cur) {
            n += count_kind(child, kind);
        }
        n
    }
    f.macros = count_kind(root, "macro_invocation");
    let mut syms = Vec::new();
    collect_nodes(root, &src, "x", &mut syms, usize::MAX);
    f.mapped = syms.len();
    f
}

/// 扫描仓库里的 .rs 文件，抽取函数/结构体/枚举/trait。
pub fn scan(root: &Path, limit: usize) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return out;
    }
    walk(root, root, limit, &mut parser, &mut out);
    out
}

/// 只解析一个文件（verify 用）。
pub fn file_symbols(path: &Path, rel: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return out;
    }
    parse_file(path, rel, &mut parser, &mut out, usize::MAX);
    out
}

fn walk(root: &Path, dir: &Path, limit: usize, parser: &mut Parser, out: &mut Vec<Symbol>) {
    if out.len() >= limit {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            walk(root, &p, limit, parser, out);
            if out.len() >= limit {
                return;
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or(name);
        parse_file(&p, &rel, parser, out, limit);
        if out.len() >= limit {
            return;
        }
    }
}

fn parse_file(path: &Path, rel: &str, parser: &mut Parser, out: &mut Vec<Symbol>, limit: usize) {
    let src = match std::fs::read(path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let tree = match parser.parse(&src, None) {
        Some(t) => t,
        None => return,
    };
    let start = out.len();
    collect_nodes(tree.root_node(), &src, rel, out, limit);
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let content_hash = crate::hash::content_id(&src);
    let entity = crate::hash::entity_id(&src, rel);
    for s in out[start..].iter_mut() {
        s.mtime = mtime;
        s.entity = entity.clone();
        s.content_hash = content_hash.clone();
    }
}

fn collect_nodes(node: Node, src: &[u8], rel: &str, out: &mut Vec<Symbol>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let kind = node.kind();
    let mapped = match kind {
        "function_item" => Some("fn"),
        "struct_item" => Some("struct"),
        "enum_item" => Some("enum"),
        "trait_item" => Some("trait"),
        "const_item" => Some("const"),
        "static_item" => Some("static"),
        "type_item" => Some("type"),
        // 宏定义也是"这个名字存在"的事实（pro：别让 0 符号文件靠"本来没符号"糊过去）
        "macro_definition" => Some("macro"),
        "union_item" => Some("union"),
        "mod_item" => Some("mod"),
        _ => None,
    };
    if let Some(k) = mapped {
        if let Some(n) = node.child_by_field_name("name") {
            if let Ok(text) = n.utf8_text(src) {
                out.push(Symbol {
                    path: rel.to_string(),
                    name: text.to_string(),
                    kind: k.to_string(),
                    line: n.start_position().row + 1,
                    mtime: 0,
                    content_hash: String::new(),
                    entity: String::new(),
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_nodes(child, src, rel, out, limit);
        if out.len() >= limit {
            return;
        }
    }
}

#[cfg(test)]
mod size_guard_tests {
    use super::file_size_for_skip;

    #[test]
    fn missing_file_is_none_not_zero() {
        let p = std::path::Path::new("Z:\\definitely-not-there\\x.rs");
        assert_eq!(file_size_for_skip(p), None);
    }
}
