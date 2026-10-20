//! 读 `docs/decisions/` 里的决策记录（2026-10-20）。
//!
//! 为什么放引擎侧：`handover` 要回答"这个系统为什么长这样"——那就得读**决策记录**，
//! 而决策记录不属于图谱（它是人写的 know-why）。解析放这里，CLI 只负责排版，
//! 以后界面要用同一份解析，不各写一遍。
//!
//! **三态**：目录不存在 / 没有记录 → 返回空；调用方要显示"查不了"，**不许当"没有决策"**。

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// 文件名去 `.md`，如 `D-20261020-04`
    pub id: String,
    /// `# D-... · 标题` 里 `·` 之后的部分
    pub title: String,
    /// `## 决定` 下的第一段（压缩空白、截断 200 字）；没有就是 `None`
    pub why: Option<String>,
}

/// 取最近 `n` 条决策（按文件名倒序；`README.md` / `PRO-GOAL.md` 不算）。
pub fn recent(root: &Path, n: usize) -> Vec<Decision> {
    let dir = root.join("docs/decisions");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()) == Some("md")
                && p.file_name().and_then(|x| x.to_str()).is_some_and(|s| s.starts_with('D'))
        })
        .collect();
    files.sort();
    files.reverse();
    files.into_iter().take(n).filter_map(|p| parse(&p)).collect()
}

fn parse(path: &Path) -> Option<Decision> {
    let text = std::fs::read_to_string(path).ok()?;
    let id = path.file_stem()?.to_str()?.to_string();
    let title = text
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches("# ").to_string())
        .map(|l| l.split('·').nth(1).unwrap_or(&l).trim().to_string())
        .unwrap_or_default();
    let why = section(&text, "## 决定").or_else(|| section(&text, "## 问题"));
    Some(Decision { id, title, why })
}

/// 取某个 `## 标题` 下的第一段（到空行或下一个标题为止）。
fn section(text: &str, heading: &str) -> Option<String> {
    let start = text.find(heading)? + heading.len();
    let body = &text[start..];
    let mut para = String::new();
    for line in body.lines() {
        let s = line.trim();
        if s.starts_with('#') || s.starts_with("```") {
            break;
        }
        if s.is_empty() {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(s);
        if para.chars().count() > 240 {
            break;
        }
    }
    if para.is_empty() {
        return None;
    }
    Some(para.chars().take(200).collect())
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-dec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("docs/decisions")).unwrap();
        d
    }

    #[test]
    fn 读得出标题与理由() {
        let d = tmp("ok");
        std::fs::write(
            d.join("docs/decisions/D-20261020-01.md"),
            "# D-20261020-01 · 用 A 不用 B\n\n- 决策者：pro\n\n## 决定\n\n**A** 更好，因为 B 会绑死。\n\n## 别的一节\n\nxxx\n",
        )
        .unwrap();
        let v = recent(&d, 5);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "D-20261020-01");
        assert_eq!(v[0].title, "用 A 不用 B");
        assert_eq!(v[0].why.as_deref(), Some("**A** 更好，因为 B 会绑死。"));
    }

    #[test]
    fn 没有目录就是空_不是编一个() {
        let d = std::env::temp_dir().join(format!("jw-dec-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(recent(&d, 5).is_empty());
    }
}
