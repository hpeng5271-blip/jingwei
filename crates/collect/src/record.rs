//! 人工记录（decision 文档）解析：只认 `D-*.md`。
//!
//! 口径（A2 裁决 / `.pro\B2-开.md` / E1 v3）：
//! - **只读显式章节**：标题以「为什么不是 / 为什么不做 / 为什么不 / 三个选项 / 必须放弃 / 不做」
//!   开头的章节；正文段落不是替代项（`D-20261022-01` 的「**为什么不做…**」就是段落 → 0 条）。
//! - 章节里的 **bullet / 编号 / 表格数据行** 都算；**任何表的表头行都跳过**
//!   （v2 漏跳表头，把 `D-20261020-04` 数成 8，实际 7）。
//! - 路径口径冻结在 E1 v3：只认 `crates|tools|docs|src-tauri|ui|bench|skills|scripts`
//!   开头的**前斜杠**路径；裸文件名、反斜杠 Windows 路径不收；扩展名白名单。

use std::path::{Path, PathBuf};

/// 一条显式替代项（记录里被考虑过、被否或选中的选项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alt {
    /// 从 1 开始的序号，用于 subject `hypothesis:<repo>#<stem>/<n>`。
    pub index: usize,
    pub text: String,
}

/// 一条人工记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 文件名去 `.md`，如 `D-20261021-05-主路径八动词`。
    pub id: String,
    /// 标题去掉编号后的部分。
    pub title: String,
    /// 状态行原文（去掉 markdown 装饰）；没有状态行则为空。
    pub status_raw: String,
    pub alts: Vec<Alt>,
    /// 路径 token（去重排序）；口径见文件头。
    pub paths: Vec<String>,
    /// 原始出现次数（去重前），对账用。
    pub path_mentions: usize,
}

const EXTENSIONS: &[&str] = &["rs", "md", "mjs", "js", "json", "ps1", "py", "toml", "cmd"];

/// E1 v3 冻结的路径前缀：只有这些开头的才算路径（裸文件名不算）。
const PATH_PREFIXES: &[&str] = &[
    "crates", "tools", "docs", "src-tauri", "ui", "bench", "skills", "scripts",
];

const ALT_HEADINGS: &[&str] = &[
    "为什么不是",
    "为什么不做",
    "为什么不",
    "三个选项",
    "必须放弃",
    "不做",
];

/// 扫一个目录里的 `D-*.md`（按文件名排序；不递归）。
pub fn scan(dir: &Path) -> Vec<Record> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            name.starts_with("D-") && name.ends_with(".md")
        })
        .collect();
    files.sort();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let stem = f
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .trim_end_matches(".md")
            .to_string();
        out.push(parse_markdown(&text, &stem));
    }
    out
}

/// 解析一篇记录。`stem` 是文件名（不含 `.md`），用于标题去编号。
pub fn parse_markdown(text: &str, stem: &str) -> Record {
    let mut title = String::new();
    let mut status_raw = String::new();
    let mut alts: Vec<Alt> = Vec::new();

    let mut in_alt_section = false;
    let mut table_header_seen = false;

    for line in text.lines() {
        let trimmed = line.trim_end();

        if let Some(rest) = trimmed.strip_prefix("## ") {
            let head = rest.trim();
            in_alt_section = ALT_HEADINGS.iter().any(|m| head.starts_with(m));
            table_header_seen = false;
            continue;
        }
        if trimmed.starts_with("### ") {
            // 三级标题不重置：章节判定沿用二级标题
            continue;
        }

        if title.is_empty() {
            if let Some(rest) = trimmed.strip_prefix("# ") {
                title = clean_title(rest.trim(), stem);
                continue;
            }
        }
        if status_raw.is_empty() {
            if let Some(v) = status_of(trimmed) {
                status_raw = v;
            }
        }
        if !in_alt_section {
            continue;
        }
        if trimmed.trim().is_empty() {
            continue;
        }
        if !trimmed.starts_with('|') {
            // 非表格行：下一张表的首行要重新当表头（同章第二张表的表头不能算替代项）
            table_header_seen = false;
        }

        if trimmed.starts_with('|') {
            let cells: Vec<&str> = trimmed
                .trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim())
                .collect();
            if cells.iter().all(|c| c.is_empty()) {
                continue;
            }
            let is_separator = cells
                .iter()
                .all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '));
            if is_separator {
                continue;
            }
            if !table_header_seen {
                table_header_seen = true;
                continue;
            }
            let joined = cells
                .iter()
                .filter(|c| !c.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" / ");
            push_alt(&mut alts, joined);
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("- ") {
            if !rest.starts_with('-') {
                push_alt(&mut alts, rest.trim().to_string());
            }
            continue;
        }

        if let Some(rest) = strip_ordered(trimmed) {
            push_alt(&mut alts, rest);
        }
    }

    let (paths, path_mentions) = extract_paths(text);

    Record {
        id: stem.to_string(),
        title,
        status_raw,
        alts,
        paths,
        path_mentions,
    }
}

fn push_alt(alts: &mut Vec<Alt>, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    alts.push(Alt {
        index: alts.len() + 1,
        text,
    });
}

fn clean_title(raw: &str, stem: &str) -> String {
    let mut t = raw.trim();
    if let Some(rest) = t.strip_prefix(stem) {
        t = rest;
    }
    t.trim_start_matches(|c: char| c.is_whitespace() || c == '·' || c == '-' || c == ':' || c == '：')
        .trim()
        .to_string()
}

fn status_of(line: &str) -> Option<String> {
    // 状态行有三种写法：`- 状态：…`、`> **状态**：…`（blockquote）、裸 `状态：…`
    let l = line.trim().trim_start_matches('>').trim();
    let l = l.trim_start_matches("- ").trim();
    for p in ["**状态**：", "**状态**:", "状态：", "状态:"] {
        if let Some(rest) = l.strip_prefix(p) {
            let v = rest
                .chars()
                .filter(|c| *c != '*' && *c != '`')
                .collect::<String>()
                .trim()
                .to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// `1. xxx` / `12) xxx` → `xxx`。
fn strip_ordered(line: &str) -> Option<String> {
    let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &line[digits.len()..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    let text = rest.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn extract_paths(text: &str) -> (Vec<String>, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut hits: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        // (?<![\w/])：前面是字母数字/_// 就不算起点
        let prev_ok = i == 0 || {
            let p = chars[i - 1];
            !(p.is_alphanumeric() || p == '_' || p == '/')
        };
        if prev_ok && starts_with_prefix(&chars, i) {
            let start = i;
            while i < chars.len() && is_path_char(chars[i]) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if has_known_ext(&token) {
                hits.push(token);
            }
            continue;
        }
        i += 1;
    }
    let mentions = hits.len();
    hits.sort();
    hits.dedup();
    (hits, mentions)
}

/// 只认 8 个目录前缀 + 前斜杠（裸文件名、反斜杠路径都不收）。
fn starts_with_prefix(chars: &[char], i: usize) -> bool {
    PATH_PREFIXES.iter().any(|p| {
        let pc: Vec<char> = p.chars().collect();
        chars.len() > i + pc.len()
            && &chars[i..i + pc.len()] == &pc[..]
            && chars[i + pc.len()] == '/'
    })
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-') || is_cjk(c)
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn has_known_ext(token: &str) -> bool {
    match token.rsplit_once('.') {
        Some((head, ext)) => !head.is_empty() && EXTENSIONS.contains(&ext),
        None => false,
    }
}
