//! 语言能力表：哪种语言我们能解析什么（capability negotiation 的数据面）。
//!
//! 目的：**不许把"我们没有这门语言的能力"说成"这个文件没有符号"**。
//! card/ask 由此派生 `symbols_status ∈ {held, no_symbols, no_capability}`。

/// 能力维度以 datacore 的接口为准（judgment 侧要用同一套枚举，避免两份定义漂移）。
pub use datacore::capability::Feature;

/// collect 的语言表实现：给 datacore 的读时能力闸用（`Caps`）。
pub struct LangCaps;

impl datacore::capability::Caps for LangCaps {
    fn supports(&self, path: &str, f: Feature) -> bool {
        supports(ext_of_path(path), f)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Capability {
    pub lang: &'static str,
    pub symbol: bool,
    pub use_edge: bool,
    pub import_edge: bool,
}

const UNSUPPORTED: Capability = Capability {
    lang: "",
    symbol: false,
    use_edge: false,
    import_edge: false,
};

/// 已登记语言：扩展名 → 能力。未登记返回 None（不是"没能力"，是"这门语言我们没定义"）。
pub fn capability_for_ext(ext: &str) -> Option<Capability> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let cap = match ext.as_str() {
        "rs" => Capability {
            lang: "rust",
            symbol: true,
            use_edge: true,
            import_edge: false,
        },
        "go" => Capability {
            lang: "go",
            symbol: true,
            use_edge: false,
            import_edge: true,
        },
        "py" | "pyi" => Capability {
            lang: "python",
            symbol: true,
            use_edge: false,
            import_edge: true,
        },
        "ts" | "tsx" => Capability {
            lang: "typescript",
            symbol: true,
            use_edge: false,
            import_edge: true,
        },
        // 登记了语言、但还没写解析器：能力为 false，不是"文件没符号"
        "js" | "jsx" | "mjs" | "cjs" => Capability {
            lang: "javascript",
            ..UNSUPPORTED
        },
        "vue" => Capability {
            lang: "vue",
            ..UNSUPPORTED
        },
        "c" | "h" | "cc" | "cpp" | "hpp" => Capability {
            lang: "c/c++",
            ..UNSUPPORTED
        },
        _ => return None,
    };
    Some(cap)
}

/// 已登记扩展名（顺序稳定，`jw capability --json` 的外部契约）。
pub const KNOWN_EXTS: &[&str] = &[
    "rs", "go", "py", "pyi", "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "c", "h", "cc", "cpp",
    "hpp",
];

/// 能力表（外部读取用；不落库）。
pub fn table() -> Vec<(&'static str, Capability)> {
    KNOWN_EXTS
        .iter()
        .filter_map(|e| capability_for_ext(e).map(|c| (*e, c)))
        .collect()
}

pub fn supports(ext: &str, f: Feature) -> bool {
    match capability_for_ext(ext) {
        Some(c) => match f {
            Feature::Symbol => c.symbol,
            Feature::Use => c.use_edge,
            Feature::Import => c.import_edge,
        },
        None => false,
    }
}

/// 边 kind → 能力维度。mod/use/impl 属 Rust 的 use 层，import 属 py/ts/go。
pub fn feature_of_kind(kind: &str) -> Option<Feature> {
    match kind {
        "use" | "mod" | "impl" | "test" | "test_macro" | "call" => Some(Feature::Use),
        "import" => Some(Feature::Import),
        _ => None,
    }
}

/// 文件扩展名（`foo.tar.rs` → `rs`；无扩展名 → ""）。
pub fn ext_of_path(path: &str) -> &str {
    path.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

/// 符号状态三值：能力缺失 / 有能力但没符号 / 有符号。
pub fn symbols_status(path: &str, n_syms: i64) -> &'static str {
    if !supports(ext_of_path(path), Feature::Symbol) {
        return "no_capability";
    }
    if n_syms > 0 {
        "held"
    } else {
        "no_symbols"
    }
}

/// 状态的中文说明（给文本卡用；JSON 用 [`symbols_status`] 的英文枚举）。
pub fn symbols_note(path: &str) -> String {
    let ext = ext_of_path(path);
    match capability_for_ext(ext) {
        Some(c) if c.symbol => format!("{} 已支持符号解析", c.lang),
        Some(c) => format!("无能力（{} 未支持符号解析）", c.lang),
        None => {
            if ext.is_empty() {
                "无能力（没有扩展名，未登记语言）".to_string()
            } else {
                format!("无能力（{ext} 未登记语言）")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_languages_have_symbols() {
        for ext in ["rs", "go", "py", "ts", "tsx", "pyi"] {
            assert!(supports(ext, Feature::Symbol), "{ext} 应支持符号");
            assert_eq!(symbols_status(&format!("a.{ext}"), 0), "no_symbols");
            assert_eq!(symbols_status(&format!("a.{ext}"), 3), "held");
        }
    }

    #[test]
    fn unsupported_languages_are_no_capability() {
        for ext in ["js", "jsx", "vue", "c", "cpp", "mjs"] {
            assert_eq!(symbols_status(&format!("a.{ext}"), 0), "no_capability");
            assert_eq!(symbols_status(&format!("a.{ext}"), 5), "no_capability", "谎报有符号更糟");
            assert!(!supports(ext, Feature::Symbol), "{ext}");
        }
        // 未登记语言也是 no_capability（"没定义"不等于"没符号"）
        assert_eq!(symbols_status("README.md", 0), "no_capability");
        assert_eq!(symbols_status("Makefile", 0), "no_capability");
    }

    #[test]
    fn kind_maps_to_feature() {
        assert_eq!(feature_of_kind("use"), Some(Feature::Use));
        assert_eq!(feature_of_kind("impl"), Some(Feature::Use));
        assert_eq!(feature_of_kind("import"), Some(Feature::Import));
        assert_eq!(feature_of_kind("wat"), None);
        assert!(supports("rs", Feature::Use));
        assert!(!supports("py", Feature::Use));
        assert!(supports("py", Feature::Import));
    }
}
