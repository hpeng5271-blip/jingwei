//! Node 的锁文件：从里面读出传递依赖图。
//!
//! ## 为什么单独一个文件
//!
//! 它跟 `deps.rs` 里别的部分不是一回事：那边是「读清单、比声明与实际」，
//! 这边是「解析一种外部格式」。而且**只做核过官方文档的那一种**（npm），
//! 别的包管理器要如实说「还没做」——这一层最怕的就是「看起来对」的图。

use super::deps::{read, TreeEdge};
use std::path::Path;

/// Node 那边的传递依赖图，**以及"没做的那部分"的说明**。
///
/// `source` 有值 = 这张图是从那份文件里读出来的；`note` 有值 = 为什么没有图。
/// **两者都摆出来**，因为界面上"没有"和"没查"必须分得清。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebTree {
    pub edges: Vec<TreeEdge>,
    pub total: usize,
    pub source: Option<String>,
    pub note: Option<String>,
}

/// 从 npm 的锁文件读出 Node 的依赖图。
///
/// ## 事实从哪来
///
/// `package-lock.json`（npm v7 起，`lockfileVersion` 2/3）里的 `packages` 是一个对象，
/// **键就是"这个包被装到哪儿"的路径**（引用自 npm 官方文档）：
///
/// ```text
/// ""                                  ← 根项目自己
/// "node_modules/vue"                  ← 装在根上
/// "node_modules/vue/node_modules/foo" ← 装在 vue 里面（npm 的嵌套安装）
/// "packages/web"                      ← workspace 成员
/// ```
///
/// 所以父子关系**不是推断出来的，是键里写着的**：把最后一段 `node_modules/<名字>` 去掉，
/// 剩下的前缀就是它的父。这就是"谁把谁捎带进来"。
///
/// ## 为什么只做 npm
///
/// pnpm 的 `pnpm-lock.yaml` 与 yarn 的 `yarn.lock` 是另一套格式（pnpm 新版本还把
/// 依赖列表放在 `snapshots` 段），**我没有对着官方文档核过它们的每个字段**。
/// 按这个项目的老规矩：**没核过就不写解析器**——写一个半懂的解析器，
/// 产出的会是一张"看起来对"的图，而错的图比没有图糟得多。
/// 所以那两种锁文件如实返回 `note`，让人知道"这里还没做"，而不是给一张空图。
pub fn web_tree(root: &Path) -> WebTree {
    // npm 官方写明：`npm-shrinkwrap.json` 优先于 `package-lock.json`
    let found = ["npm-shrinkwrap.json", "package-lock.json"]
        .iter()
        .find_map(|f| {
            let p = root.join(f);
            if p.is_file() {
                Some((*f, p))
            } else {
                None
            }
        });
    let Some((file, path)) = found else {
        return known_gap(root);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return WebTree {
            note: Some(format!("{file} 读不出来（权限或编码）")),
            ..Default::default()
        };
    };
    let Ok(j) = serde_json::from_str::<serde_json::Value>(crate::util::bom::strip_bom(&text)) else {
        return WebTree {
            note: Some(format!("{file} 不是合法的 JSON")),
            ..Default::default()
        };
    };
    let ver = j
        .get("lockfileVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let Some(pkgs) = j.get("packages").and_then(|v| v.as_object()) else {
        // lockfileVersion 1（npm v5/v6）没有 packages 段，只有老的 dependencies 段
        return WebTree {
            note: Some(format!(
                "{file} 是 lockfileVersion {ver}：那一代的格式没有 `packages` 段，读不出嵌套关系"
            )),
            ..Default::default()
        };
    };

    // 根项目的名字（边的一端要用它）
    let root_name = read(root, "package.json")
        .and_then(|t: String| serde_json::from_str::<serde_json::Value>(crate::util::bom::strip_bom(t.as_str())).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
        .unwrap_or_else(|| "（根项目）".to_string());

    let mut edges = Vec::new();
    let mut truncated = false;
    const MAX_EDGES: usize = 4000;
    for (key, _) in pkgs.iter() {
        if key.is_empty() {
            continue; // 根项目自己
        }
        // `.bin` 是命令入口，不是依赖
        if key.contains("node_modules/.bin/") {
            continue;
        }
        let (parent_key, name) = split_node_path(key);
        let Some(name) = name else { continue };
        if edges.len() >= MAX_EDGES {
            truncated = true;
            break;
        }
        let from = parent_key
            .and_then(|p| split_node_path(&p).1)
            .unwrap_or_else(|| root_name.clone());
        if from == name {
            continue; // 自己指自己不是依赖关系
        }
        edges.push(TreeEdge { from, to: name });
    }
    // 先按"谁"再按"捎带了谁"排，为的是**输出稳定**（同一份锁文件每次读出同样的顺序），
    // 不是为了好看——顺序会变的话，界面上的树每次刷新都在动。
    edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));
    edges.dedup();
    let total = edges.len();
    let mut note = None;
    if truncated {
        note = Some(format!(
            "只画了前 {MAX_EDGES} 条（这份锁文件里的包更多），没有截断的部分在文件里"
        ));
    }
    WebTree {
        edges,
        total,
        source: Some(format!("{file}（lockfileVersion {ver}）的 packages 段")),
        note,
    }
}

/// 这个项目用的是别的包管理器 / 还没装依赖时的说法。
/// **一条都不许省**：界面上"没有图"和"没有查"是两回事。
fn known_gap(root: &Path) -> WebTree {
    for (file, who) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ] {
        if root.join(file).is_file() {
            return WebTree {
                note: Some(format!(
                    "{who} 的锁文件（{file}）格式还没做——这一层只做核过官方文档的那一种（npm）"
                )),
                ..Default::default()
            };
        }
    }
    WebTree {
        note: Some("没有锁文件，读不出传递依赖".to_string()),
        ..Default::default()
    }
}

/// `node_modules/a/node_modules/b` → (`Some("node_modules/a")`, `Some("b")`)
///
/// 规则就是 npm 文档里那句话：键是"相对根项目的路径"，最后一段是包名，
/// 去掉它就得到"这个包被装在谁里面"。
fn split_node_path(key: &str) -> (Option<String>, Option<String>) {
    let Some(idx) = key.rfind("node_modules/") else {
        return (None, None);
    };
    let name = key[idx + "node_modules/".len()..].trim_end_matches('/');
    if name.is_empty() {
        return (None, None);
    }
    let parent = key[..idx].trim_end_matches('/');
    let parent = if parent.is_empty() {
        None
    } else {
        Some(parent.to_string())
    };
    (parent, Some(name.to_string()))
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声（跟 deps.rs 的测试同一套规矩）。
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jingwei-nodelock-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn web_tree_从_npm_锁文件的键读出嵌套关系() {
        let d = tmp("webtree");
        std::fs::write(d.join("package.json"), r#"{"name":"demo"}"#).unwrap();
        std::fs::write(
            d.join("package-lock.json"),
            r#"{
              "name": "demo",
              "lockfileVersion": 3,
              "packages": {
                "": { "name": "demo" },
                "node_modules/vue": { "version": "3.4.21" },
                "node_modules/vue/node_modules/foo": { "version": "1.0.0" },
                "node_modules/@scope/bar": { "version": "2.0.0" },
                "node_modules/.bin/vite": { "version": "5.0.0" }
              }
            }"#,
        )
        .unwrap();
        let t = web_tree(&d);
        assert!(t.source.is_some(), "读出来了就要说清出处：{:?}", t.note);
        let pairs: Vec<(String, String)> = t
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        assert!(pairs.contains(&("demo".into(), "vue".into())), "{pairs:?}");
        assert!(
            pairs.contains(&("vue".into(), "foo".into())),
            "嵌套那一层要读出来：{pairs:?}"
        );
        assert!(
            pairs.contains(&("demo".into(), "@scope/bar".into())),
            "带 scope 的包名要原样保留：{pairs:?}"
        );
        assert!(
            !pairs.iter().any(|(_, to)| to.contains(".bin")),
            "`.bin` 是命令入口，不是依赖：{pairs:?}"
        );
        assert_eq!(t.total, 3);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn shrinkwrap_优先于_package_lock() {
        let d = tmp("shrink");
        std::fs::write(d.join("package.json"), r#"{"name":"demo"}"#).unwrap();
        std::fs::write(
            d.join("package-lock.json"),
            r#"{"lockfileVersion":3,"packages":{"":"","node_modules/来自lock":{}}}"#,
        )
        .unwrap();
        std::fs::write(
            d.join("npm-shrinkwrap.json"),
            r#"{"lockfileVersion":3,"packages":{"":"","node_modules/来自shrinkwrap":{}}}"#,
        )
        .unwrap();
        let t = web_tree(&d);
        let names: Vec<String> = t.edges.iter().map(|e| e.to.clone()).collect();
        assert_eq!(names, vec!["来自shrinkwrap".to_string()], "{names:?}");
        assert!(t.source.unwrap().contains("npm-shrinkwrap.json"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 别的包管理器如实说没做() {
        let d = tmp("pnpmgap");
        std::fs::write(d.join("package.json"), r#"{"name":"demo"}"#).unwrap();
        std::fs::write(d.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        let t = web_tree(&d);
        assert!(t.edges.is_empty());
        assert!(t.source.is_none(), "没读出来就不许说来源");
        let note = t.note.unwrap();
        assert!(note.contains("pnpm"), "{note}");
        assert!(note.contains("还没做"), "要明说没做：{note}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 老版锁文件说清是哪一代() {
        let d = tmp("oldlock");
        std::fs::write(d.join("package.json"), r#"{"name":"demo"}"#).unwrap();
        std::fs::write(
            d.join("package-lock.json"),
            r#"{"lockfileVersion":1,"dependencies":{"vue":{"version":"3.0.0"}}}"#,
        )
        .unwrap();
        let t = web_tree(&d);
        let note = t.note.unwrap();
        assert!(note.contains("lockfileVersion 1"), "{note}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
