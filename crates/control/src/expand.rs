//! 展开 import/use 文本：返回 (原名, 本地绑定名, 行号)。行为与原逻辑一致，作为后续改口径的单一入口。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportName {
    pub name: String,
    pub local: String,
    pub line: usize,
}


/// 找到与首个 `{` 配对的 `}`（返回相对下标）；嵌套组必须按深度配对。
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
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

/// 拆顶层逗号（忽略花括号/圆括号内部）。
fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '{' | '(' => depth += 1,
            '}' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// 剥掉 `pub` / `pub(crate)` / `pub(super)` / `pub(in path)` 前缀（B 自己的实现，不共享 jw 代码）。
fn strip_visibility(t: &str) -> &str {
    let Some(rest) = t.strip_prefix("pub") else {
        return t;
    };
    let mut rest = rest.trim_start();
    if rest.starts_with('(') {
        if let Some(i) = rest.find(')') {
            rest = rest[i + 1..].trim_start();
        }
    }
    rest
}

fn push_name(out: &mut Vec<ImportName>, raw: &str, line: usize) {
    let t = raw.trim();
    if t.is_empty() || t == "*" {
        return;
    }
    let (name, local) = match t.split_once(" as ") {
        Some((n, l)) => (n.trim().to_string(), l.trim().to_string()),
        None => (t.to_string(), t.to_string()),
    };
    out.push(ImportName { name, local, line });
}

/// Rust `use` / TS `import` / Python import 三种形态的展开（只做词法，不解析路径）。
pub fn expand_imports(text: &str, line: usize) -> Vec<ImportName> {
    let mut out = Vec::new();
    let t = text.trim().trim_end_matches(';').trim();

    // TS: import ... from "spec"
    if t.starts_with("import") && t.contains("from") {
        if let Some(open) = t.find('{') {
            if let Some(close) = t[open..].find('}') {
                for item in split_top(&t[open + 1..open + close]) {
                    push_name(&mut out, &item, line);
                }
                return out;
            }
        }
        if let Some(rest) = t.strip_prefix("import") {
            let head = rest.split("from").next().unwrap_or("").trim();
            if !head.is_empty() {
                push_name(&mut out, head, line);
            }
        }
        return out;
    }

    // Python: from x import a, b / import a, b
    if let Some(rest) = t.strip_prefix("from") {
        let after = rest.trim().trim_start_matches('.').trim();
        let list = match after.strip_prefix("import") {
            Some(l) => l.trim(),
            None => after.splitn(2, " import").nth(1).unwrap_or("").trim(),
        };
        for item in split_top(list.trim_matches(|c: char| c == '(' || c == ')')) {
            push_name(&mut out, &item, line);
        }
        return out;
    }
    if let Some(rest) = t.strip_prefix("import") {
        for item in split_top(rest.trim()) {
            push_name(&mut out, &item, line);
        }
        return out;
    }

    // Rust: use a::{b::C, D as E}, F;
    // 可见性（pub / pub(crate) / pub(super) / pub(in path)）不是路径的一部分，
    // 不剥会得到 local="pub use tao" 这种不是标识符的东西。
    let t = strip_visibility(t);
    let body = t.strip_prefix("use").unwrap_or(t).trim();
    fn walk(prefix: &str, items: &[String], line: usize, out: &mut Vec<ImportName>) {
        for it in items {
            if let Some(open) = it.find('{') {
                let pre = it[..open].trim().trim_end_matches("::");
                if let Some(close) = matching_brace(&it[open..]) {
                    let base = if prefix.is_empty() {
                        pre.to_string()
                    } else {
                        format!("{prefix}::{pre}")
                    };
                    let inner = split_top(&it[open + 1..open + close]);
                    walk(&base, &inner, line, out);
                    continue;
                }
            }
            let it_trim = it.trim();
            if it_trim == "self" {
                if let Some(p) = prefix.rsplit("::").next() {
                    if !p.is_empty() {
                        push_name(out, p, line);
                        continue;
                    }
                }
            }
            let full = if prefix.is_empty() {
                it.clone()
            } else {
                format!("{prefix}::{it}")
            };
            push_name(out, &full, line);
        }
    }
    let items = split_top(body);
    walk("", &items, line, &mut out);
    out
}

#[cfg(test)]
mod expand_tests {
    use super::expand_imports;

    #[test]
    fn rust_visibility_is_not_part_of_path() {
        let v = expand_imports("pub use tao;", 9);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].local, "tao", "pub use 的 local 必须是名字，不是整行");
        let v = expand_imports("pub(crate) use a::b as c;", 3);
        assert_eq!(v[0].local, "c");
        let v = expand_imports("pub(in crate::x) use d::e;", 4);
        assert_eq!(v[0].local, "d::e", "无别名时 local 是全路径（main 再取末段）");
    }

    #[test]
    fn rust_plain_and_alias() {
        let v = expand_imports("use a::B as C;", 7);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "a::B");
        assert_eq!(v[0].local, "C");
        assert_eq!(v[0].line, 7);
    }

    #[test]
    fn rust_braces_nested() {
        let v = expand_imports("use a::{b::C, D as E, self};", 1);
        let locals: Vec<&str> = v.iter().map(|x| x.local.as_str()).collect();
        assert!(locals.contains(&"a::b::C"));
        assert!(locals.contains(&"E"));
        assert!(locals.contains(&"a"));
    }

    #[test]
    fn ts_named_and_alias() {
        let v = expand_imports("import { A, A as B } from './x';", 3);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].local, "A");
        assert_eq!(v[1].local, "B");
        assert_eq!(v[1].name, "A");
    }

    #[test]
    fn python_from_star() {
        let v = expand_imports("from os.path import a, b as c", 2);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].local, "c");
    }

    #[test]
    fn python_plain_imports() {
        let v = expand_imports("import os, sys as s", 5);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].local, "os");
        assert_eq!(v[1].local, "s");
    }
}
