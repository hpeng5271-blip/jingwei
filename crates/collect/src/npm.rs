//! npm 依赖：`package.json`（dependencies / devDependencies）+ `package-lock.json`（resolved）。
//!
//! `workspace:*` / `file:` / `git:` 一律视为"版本未知"（None），不猜。

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct NpmDep {
    pub name: String,
    /// 声明版本（package.json 里的原文）；unknown 形式为 None
    pub version: Option<String>,
    /// lock 里的实际版本（package-lock.json）
    pub resolved: Option<String>,
    pub kind: String, // normal / dev
}

const SKIP: &[&str] = &["node_modules", ".git", ".jingwei", "target"];

pub fn scan(root: &Path) -> Result<Vec<NpmDep>, String> {
    let mut manifests = Vec::new();
    collect(root, root, &mut manifests);
    let mut lock: BTreeMap<String, String> = BTreeMap::new();
    let lock_path = root.join("package-lock.json");
    if lock_path.is_file() {
        if let Ok(text) = std::fs::read_to_string(&lock_path) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(pkgs) = v.get("packages").and_then(|p| p.as_object()) {
                    for (path, info) in pkgs {
                        if let Some(name) = path.strip_prefix("node_modules/") {
                            if let Some(ver) = info.get("version").and_then(|x| x.as_str()) {
                                lock.insert(name.to_string(), ver.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    let mut out: BTreeMap<String, NpmDep> = BTreeMap::new();
    for m in manifests {
        let Ok(text) = std::fs::read_to_string(&m) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        for (section, kind) in [("dependencies", "normal"), ("devDependencies", "dev")] {
            let Some(deps) = v.get(section).and_then(|d| d.as_object()) else { continue };
            for (name, ver) in deps {
                let raw = ver.as_str().unwrap_or("");
                let unknown = raw.starts_with("workspace:")
                    || raw.starts_with("file:")
                    || raw.starts_with("git:")
                    || raw.starts_with("link:")
                    || raw.is_empty();
                let version = if unknown { None } else { Some(raw.to_string()) };
                let resolved = lock.get(name).cloned();
                out.entry(name.clone()).or_insert(NpmDep {
                    name: name.clone(),
                    version,
                    resolved,
                    kind: kind.to_string(),
                });
            }
        }
    }
    Ok(out.into_values().collect())
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
        if name == "package.json" {
            out.push(p);
        }
    }
}
