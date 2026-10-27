//! 归一化层（工序 ⑤）：把用户输入/磁盘事实整理成模型能吃的形状。
//!
//! 只做三件事：输入 → subject、subject → 路径、构造 scope。
//! **`ask` 不走这里**——ask 只答给定 subject，不替用户猜（猜会配错对象）。

use datacore::model::Scope;
use datacore::store::Store;

/// 用户输入 → 内部 subject（路径工具 `why` / `explain` / `card` 用）。
///
/// 顺序：库内已有 → 已知 kind 前缀 → 含 `#` 当符号 → file → doc。
pub fn subject(store: &Store, input: &str) -> String {
    // ① 库里已有这个 subject 就原样用（前缀是任意标签，不要求等于 kind 名）
    if store.find_judgment(input).ok().flatten().is_some() {
        return input.to_string();
    }
    // ② 已知 kind 前缀（kind_registry 决定，不是硬编码）
    if let Some((prefix, _)) = input.split_once(':') {
        if store.kind_layer(prefix).ok().flatten().is_some() {
            return input.to_string();
        }
    }
    if input.contains('#') {
        return format!("symbol:{input}");
    }
    for s in [format!("file:{input}"), format!("doc:{input}")] {
        if store.find_judgment(&s).ok().flatten().is_some() {
            return s;
        }
    }
    format!("file:{input}")
}

/// subject → 仓库相对路径（explain 的明细查询统一用它，别拿原始输入去查）。
pub fn path_of(subject: &str) -> &str {
    if let Some(rest) = subject.strip_prefix("symbol:") {
        return rest.split_once('#').map(|(p, _)| p).unwrap_or(rest);
    }
    for p in ["file:", "doc:"] {
        if let Some(rest) = subject.strip_prefix(p) {
            return rest;
        }
    }
    subject
}

/// 构造 scope：objects + 粒度 + repo（env 里其它键由调用方补）。
pub fn scope(objects: Vec<String>, granularity: &str, repo: &str) -> Scope {
    let mut env = std::collections::BTreeMap::new();
    env.insert("repo".to_string(), repo.to_string());
    Scope {
        objects,
        time_window: None,
        env,
        granularity: granularity.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("jw-norm-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir).unwrap();
        (dir, s)
    }

    #[test]
    fn subject_maps_input_kinds() {
        let (dir, store) = temp_store("subject");
        // 含 # → 符号
        assert_eq!(subject(&store, "src/x.rs#foo"), "symbol:src/x.rs#foo");
        // 已注册的 kind 前缀 → 原样（"dependency" 是 seed 里的 kind）
        assert_eq!(subject(&store, "dependency:anyhow"), "dependency:anyhow");
        // 前缀不是 kind 名且库里查不到 → 当路径兜底（"dep:anyhow" 要靠库里已有才原样）
        assert_eq!(subject(&store, "dep:anyhow"), "file:dep:anyhow");
        // 裸路径、库里没这个 subject → file 兜底
        assert_eq!(subject(&store, "nope.rs"), "file:nope.rs");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_of_strips_prefixes() {
        assert_eq!(path_of("symbol:crates/x.rs#f"), "crates/x.rs");
        assert_eq!(path_of("file:README.md"), "README.md");
        assert_eq!(path_of("doc:docs/a.md"), "docs/a.md");
        assert_eq!(path_of("dep:anyhow"), "dep:anyhow");
    }
}
