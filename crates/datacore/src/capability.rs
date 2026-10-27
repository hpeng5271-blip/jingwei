//! 能力协商的接口面（datacore 不依赖任何语言实现）。
//!
//! 规则：judgment 的 kind 决定它"需要哪种语言能力"；读取时（ask/compose）用外部传入的
//! [`Caps`] 判断该 subject 的路径是否支持这个能力。**只有 `symbol` 依赖符号能力**，
//! `doc`/`file`/`decision`/`dependency` 与语言能力正交（`doc:` 是 .md，绝不能映射到 symbol，
//! 否则会误杀全部文档判断）。

/// 能力维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Symbol,
    Use,
    Import,
}

/// 由外部（collect 的语言表）实现：某个路径是否支持某能力。
pub trait Caps {
    fn supports(&self, path: &str, f: Feature) -> bool;
}

/// 旧语义：什么能力都当作有。`query::ask` 默认用它，行为与加能力维度之前完全一致。
pub struct AllowAll;

impl Caps for AllowAll {
    fn supports(&self, _path: &str, _f: Feature) -> bool {
        true
    }
}

/// judgment.kind → 需要的语言能力。只有 `symbol` 需要符号解析能力。
pub fn feature_required(kind: &str) -> Option<Feature> {
    match kind {
        "symbol" => Some(Feature::Symbol),
        _ => None,
    }
}

/// 从 subject 取路径：`symbol:<path>#<name>` → `<path>`；`file:<path>` / `doc:<path>` → `<path>`。
/// 取不到（`dep:`/`decision:`/裸词）返回 None。
pub fn path_of_subject(subject: &str) -> Option<&str> {
    if let Some(rest) = subject.strip_prefix("symbol:") {
        return Some(rest.split_once('#').map(|(p, _)| p).unwrap_or(rest));
    }
    for p in ["file:", "doc:"] {
        if let Some(rest) = subject.strip_prefix(p) {
            return Some(rest);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_symbol_requires_capability() {
        assert_eq!(feature_required("symbol"), Some(Feature::Symbol));
        for k in ["file", "change", "doc", "dependency", "decision", "hypothesis", "mastery"] {
            assert_eq!(feature_required(k), None, "{k} 不该依赖语言能力");
        }
    }

    #[test]
    fn subject_path_extraction() {
        assert_eq!(path_of_subject("symbol:src/a.rs#f"), Some("src/a.rs"));
        assert_eq!(path_of_subject("file:docs/x.md"), Some("docs/x.md"));
        assert_eq!(path_of_subject("doc:README.md"), Some("README.md"));
        assert_eq!(path_of_subject("dep:npm:react"), None);
        assert_eq!(path_of_subject("decision:docs/decisions/D-1.md#D-1"), None);
    }
}
