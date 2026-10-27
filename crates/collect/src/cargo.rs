//! 采·构建：`Cargo.toml` + `Cargo.lock` → 依赖清单。
//!
//! 只认最常见的写法（行式 TOML）：`[dependencies] name = "1"`、`name = { version = "1" }`、
//! `[workspace] members = [...]`、`[[package]]`。复杂 TOML（多行表、target 三元组）留给后续。

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

impl DepKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DepKind::Normal => "normal",
            DepKind::Dev => "dev",
            DepKind::Build => "build",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    /// 清单里写的版本（可能是 "1" 这种范围）
    pub version: Option<String>,
    /// Cargo.lock 里解析到的确切版本
    pub resolved: Option<String>,
    pub kind: DepKind,
}

#[derive(Debug, Clone, Default)]
pub struct BuildScan {
    pub deps: Vec<Dep>,
    pub members: Vec<String>,
    /// 用于对账：声明了多少行、锁到多少个包
    pub lock_packages: usize,
    /// `[workspace.dependencies]` 里的声明数（是候选版本，不是真依赖，不计入 deps）
    pub workspace_decls: usize,
}

/// 扫一个仓库：**所有** Cargo.toml（根、工作区成员、以及不在 workspace 里的独立 crate，
/// 如 fuzz/、examples/）+ Cargo.lock。没有 Cargo.toml → Err("no_manifest")。
pub fn scan(root: &Path) -> Result<BuildScan, String> {
    let mut out = BuildScan::default();
    if !root.join("Cargo.toml").is_file() {
        return Err("no_manifest".into());
    }

    // 只扫 workspace 成员会**静默漏掉**独立 crate（rg 的 fuzz/ 就漏过）——改成全树扫
    let mut manifests = Vec::new();
    collect_manifests(root, root, &mut manifests);
    manifests.sort();
    for m in &manifests {
        let _ = parse_manifest(m, &mut out);
    }

    // 去重：同名同类只留一条
    let mut seen: Vec<(String, DepKind)> = Vec::new();
    out.deps.retain(|d| {
        let key = (d.name.clone(), d.kind);
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        true
    });

    let lock = parse_lock(&root.join("Cargo.lock"));
    out.lock_packages = lock.len();
    for d in out.deps.iter_mut() {
        d.resolved = lock.get(&d.name).cloned();
        if d.version.is_none() {
            d.version = d.resolved.clone();
        }
    }
    Ok(out)
}

/// 递归收集 Cargo.toml（跳过 target/.git/node_modules/.jingwei）。
fn collect_manifests(root: &Path, dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if matches!(name.as_str(), "target" | ".git" | "node_modules" | ".jingwei") {
                continue;
            }
            collect_manifests(root, &p, out);
            continue;
        }
        if name == "Cargo.toml" {
            out.push(p);
        }
    }
}

/// 解析一份清单，把依赖与成员追加进 out。
fn parse_manifest(path: &Path, out: &mut BuildScan) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|_| "no_manifest".to_string())?;
    let mut section = String::new();
    let mut in_members = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            in_members = false;
            continue;
        }
        // workspace.members 可能是多行数组
        if section == "workspace" && (line.starts_with("members") || in_members) {
            let body = line.split_once('=').map(|(_, v)| v).unwrap_or(line);
            for part in body.split(',') {
                let s = part.trim().trim_matches(|c| c == '"' || c == '[' || c == ']');
                if s.is_empty() || s == "]" {
                    continue;
                }
                if !out.members.iter().any(|m| m == s) {
                    out.members.push(s.to_string());
                }
            }
            in_members = !line.contains(']');
            continue;
        }
        // workspace.dependencies 只是候选版本声明，不是"项目依赖"——只计数，不入 deps
        if section == "workspace.dependencies" {
            if line.contains('=') && !line.trim_start().starts_with('#') {
                out.workspace_decls += 1;
            }
            continue;
        }
        let kind = match section.as_str() {
            "dependencies" => Some(DepKind::Normal),
            "dev-dependencies" => Some(DepKind::Dev),
            "build-dependencies" => Some(DepKind::Build),
            _ => None,
        };
        let Some(kind) = kind else { continue };
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"').to_string();
        if name.is_empty() {
            continue;
        }
        out.deps.push(Dep {
            name,
            version: version_of(value.trim()),
            resolved: None,
            kind,
        });
    }
    Ok(())
}

/// `"1.2"` 或 `{ version = "1.2", features = [...] }` → 版本号。
fn version_of(v: &str) -> Option<String> {
    let v = v.trim();
    if let Some(stripped) = v.strip_prefix('"') {
        return stripped.split('"').next().map(|s| s.to_string());
    }
    if v.starts_with('{') {
        let inner = v.trim_matches(|c| c == '{' || c == '}');
        for part in inner.split(',') {
            let (k, val) = part.split_once('=')?;
            if k.trim() == "version" {
                return Some(val.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Cargo.lock：`[[package]]` 块里的 name/version。
fn parse_lock(path: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return map;
    };
    let mut name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            name = None;
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "name" => name = Some(v),
                "version" => {
                    if let Some(n) = name.take() {
                        map.entry(n).or_insert(v);
                    }
                }
                _ => {}
            }
        }
    }
    map
}

/// 单个 Cargo.toml 的 `[dependencies]` 名字（模块卡用；无该节返回空）。
pub fn manifest_dep_names(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut in_deps = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            let name = name.trim().trim_matches('"');
            if !name.is_empty() && !name.contains(' ') {
                out.push(name.to_string());
            }
        }
    }
    out
}
