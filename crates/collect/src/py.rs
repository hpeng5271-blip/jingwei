//! Python 依赖最小版：只读 `requirements.txt`（`name==ver` / `name>=ver` 取声明串）。
//! `pyproject.toml` / `poetry.lock` 不解析——调用方必须显式说“未采集”。

use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PyDep {
    pub name: String,
    /// 声明串（原样，如 "==2.31.0" 或 ">=4.2"）；空则 None
    pub version: Option<String>,
}

const SKIP: &[&str] = &["node_modules", ".git", ".jingwei", "target", "venv", ".venv", "site-packages"];

pub fn scan(root: &Path) -> Vec<PyDep> {
    let mut files = Vec::new();
    collect(root, root, &mut files);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('-') {
                continue;
            }
            let (name, ver) = match line.find(|c: char| c == '=' || c == '>' || c == '<' || c == '~' || c == '!') {
                Some(i) => (line[..i].trim(), Some(line[i..].trim().to_string())),
                None => (line, None),
            };
            let name = name.split('[').next().unwrap_or(name).trim().to_string();
            if name.is_empty() {
                continue;
            }
            if seen.insert(name.clone()) {
                out.push(PyDep { name, version: ver.filter(|v| !v.is_empty()) });
            }
        }
    }
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if SKIP.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            collect(root, &p, out);
            continue;
        }
        if name.starts_with("requirements") && name.ends_with(".txt") {
            out.push(p);
        }
    }
}
