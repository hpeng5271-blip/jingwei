//! 工作记录，按笔记库来存：一篇笔记一个 Markdown 文件。
//!
//! 为什么不用一个大 jsonl：笔记是要**带走**的东西。Markdown + frontmatter
//! 意味着这些字可以直接扔进 Obsidian、Typora、或者只用一个记事本打开，
//! 正文里还能写 [[某某]] 互相指来指去。工具换掉，笔记还在。
//!
//! 机器的事实（环境快照）在 `.jingwei/snapshots/`，人的想法在
//! `.jingwei/notes/`。两者分开，排查问题时不会互相淹没。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 固定的几类。新手最缺的不是自由格式，而是「我该记什么」的提示。
pub const KINDS: &[(&str, &str)] = &[
    ("goal", "目的"),
    ("idea", "想法"),
    ("plan", "计划"),
    ("blocker", "卡住"),
    ("learned", "学到"),
    ("decision", "决定"),
    ("note", "随手记"),
];

pub fn kind_label(kind: &str) -> &'static str {
    KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, l)| *l)
        .unwrap_or("随手记")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Note {
    /// 文件名（不含 .md），也是它的稳定身份
    pub slug: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created: String,
    pub updated: String,
    /// 正文里出现的 [[链接]]
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NoteInput {
    /// 空 = 新建
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub fn dir(root: &Path) -> PathBuf {
    crate::data_dir(root).join("notes")
}

fn trash_dir(root: &Path) -> PathBuf {
    dir(root).join(".trash")
}

pub fn path_of(root: &Path, slug: &str) -> PathBuf {
    dir(root).join(format!("{}.md", slug))
}

/// 文件名要能在 Windows 上活下来：去掉那几个非法字符，中文留着
fn sanitize(s: &str) -> String {
    let out: String = s
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let squeezed = out.split_whitespace().collect::<Vec<_>>().join("-");
    let trimmed: String = squeezed.trim_matches('-').chars().take(48).collect();
    if trimmed.is_empty() {
        "未命名".to_string()
    } else {
        trimmed
    }
}

fn slug_for(root: &Path, title: &str, created: &str) -> String {
    let stamp: String = created
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect();
    let base = format!("{}-{}", stamp, sanitize(title));
    let mut candidate = base.clone();
    let mut n = 1;
    while path_of(root, &candidate).exists() {
        n += 1;
        candidate = format!("{}-{}", base, n);
    }
    candidate
}

fn extract_links(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("[[") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("]]") else { break };
        let name = after[..j].trim();
        if !name.is_empty() && !out.iter().any(|x: &String| x == name) {
            out.push(name.to_string());
        }
        rest = &after[j + 2..];
    }
    out
}

fn render(n: &Note) -> String {
    let tags = if n.tags.is_empty() {
        String::new()
    } else {
        format!("[{}]", n.tags.join(", "))
    };
    format!(
        "---\nkind: {}\ntitle: {}\ncreated: {}\nupdated: {}\ntags: {}\n---\n\n{}\n",
        n.kind,
        n.title.replace('\n', " "),
        n.created,
        n.updated,
        tags,
        n.body.trim()
    )
}

fn parse(slug: &str, text: &str) -> Note {
    let mut kind = String::new();
    let mut title = String::new();
    let mut created = String::new();
    let mut updated = String::new();
    let mut tags: Vec<String> = Vec::new();
    let body;

    if let Some(after) = text.strip_prefix("---") {
        if let Some(end) = after.find("\n---") {
            let head = &after[..end];
            body = after[end + 4..].trim_start_matches('\n').to_string();
            for line in head.lines() {
                let Some((k, v)) = line.split_once(':') else { continue };
                let v = v.trim();
                match k.trim() {
                    "kind" => kind = v.to_string(),
                    "title" => title = v.to_string(),
                    "created" => created = v.to_string(),
                    "updated" => updated = v.to_string(),
                    "tags" => {
                        tags = v
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    }
                    _ => {}
                }
            }
        } else {
            body = text.to_string();
        }
    } else {
        body = text.to_string();
    }

    if kind.is_empty() {
        kind = "note".into();
    }
    if title.is_empty() {
        title = body
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("未命名")
            .trim_start_matches('#')
            .trim()
            .to_string();
    }
    let links = extract_links(&body);
    Note {
        slug: slug.into(),
        kind,
        title,
        body,
        tags,
        created,
        updated,
        links,
    }
}

/// 一篇笔记坏了不该毁掉整本记录：跳过读不了的，别的照常。
pub fn list(root: &Path) -> Vec<Note> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir(root)) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(slug) = p.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.push(parse(&slug, &text));
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.slug.cmp(&a.slug)));
    out
}

pub fn get(root: &Path, slug: &str) -> Option<Note> {
    let p = path_of(root, slug);
    let text = std::fs::read_to_string(p).ok()?;
    Some(parse(slug, &text))
}

pub fn count(root: &Path) -> usize {
    list(root).len()
}

pub fn save(root: &Path, input: &NoteInput) -> Result<Note> {
    let now = crate::facts::now_iso();
    let existing = input.slug.as_deref().and_then(|s| get(root, s));
    let title = if input.title.trim().is_empty() {
        input
            .body
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("未命名")
            .trim()
            .to_string()
    } else {
        input.title.trim().to_string()
    };
    let mut note = Note {
        slug: existing
            .as_ref()
            .map(|n| n.slug.clone())
            .unwrap_or_else(|| slug_for(root, &title, &now)),
        kind: if input.kind.trim().is_empty() {
            "note".into()
        } else {
            input.kind.trim().to_string()
        },
        title,
        body: input.body.clone(),
        tags: input.tags.clone(),
        created: existing
            .as_ref()
            .map(|n| n.created.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| now.clone()),
        updated: now,
        links: Vec::new(),
    };
    note.links = extract_links(&note.body);
    std::fs::create_dir_all(dir(root))?;
    let p = path_of(root, &note.slug);
    std::fs::write(&p, render(&note)).with_context(|| format!("写 {} 失败", p.display()))?;
    Ok(note)
}

/// 删除只是搬进笔记库里的 `.trash/`——自己写的字不该被一次误点抹掉。
pub fn delete(root: &Path, slug: &str) -> Result<()> {
    let p = path_of(root, slug);
    if !p.is_file() {
        bail!("找不到这篇笔记");
    }
    std::fs::create_dir_all(trash_dir(root))?;
    let mut dst = trash_dir(root).join(format!("{}.md", slug));
    let mut n = 1;
    while dst.exists() {
        n += 1;
        dst = trash_dir(root).join(format!("{}-{}.md", slug, n));
    }
    std::fs::rename(&p, &dst).with_context(|| "移进 trash 失败")?;
    Ok(())
}

/// 老版本把记录存在一个 jsonl 里。第一次打开时搬过来，之后就不管了。
pub fn migrate_legacy(root: &Path) -> usize {
    let old = crate::data_dir(root).join("notes.jsonl");
    let Ok(text) = std::fs::read_to_string(&old) else {
        return 0;
    };
    /// 老格式里的时间戳。搬过来之后用不上了——created 由 save 生成新值，
    /// 但保留字段是为了让 serde 能顺利吞下老记录。
    #[derive(Deserialize)]
    struct Old {
        #[allow(dead_code)]
        at: String,
        kind: String,
        text: String,
    }
    let mut moved = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(o) = serde_json::from_str::<Old>(line) else { continue };
        let input = NoteInput {
            slug: None,
            kind: o.kind,
            title: o.text.lines().next().unwrap_or("旧记录").to_string(),
            body: o.text,
            tags: vec!["从旧格式导入".into()],
        };
        if save(root, &input).is_ok() {
            moved += 1;
        }
    }
    if moved > 0 {
        // 把旧格式文件挪开、免得下次又导一遍。挪不动最多是重复导入被下面去重挡住。
        if let Err(e) = std::fs::rename(&old, crate::data_dir(root).join("notes.jsonl.migrated"))
        {
            crate::dlog::w("9005", &format!("旧格式笔记未改名：{e}"));
        }
    }
    moved
}
