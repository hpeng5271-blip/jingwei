//! **读用户文件时，先去掉 UTF-8 BOM**（2026-10-18）。
//!
//! ## 为什么值得一个模块
//!
//! Windows 上记事本、PowerShell 5.1 的 `Set-Content -Encoding utf8` **默认写 BOM**。
//! 于是用户手里合规的 `package.json` / `env.contract.json` 前面多三个字节
//! （`EF BB BF`），而 `serde_json` / `toml` 见到它们报的是
//! 「expected value at line 1 column 1」——**这句话跟"文件头多了三个字节"完全对不上**。
//!
//! 实测（2026-10-18）：同一个 Node 项目，
//!
//! ```text
//! package.json 带 BOM → 「还查不出来 / their_gap」
//! package.json 无 BOM → 「1 个依赖没装：left-pad」     ← 这才是真的
//! ```
//!
//! **一个 BOM 就能让工具从"说准"退化成"查不了"。**
//!
//! ## 边界
//!
//! - 只**去掉开头**的 BOM，中间的不动（JSON/TOML 里 BOM 出现在中间本来就是非法字符，
//!   那种情况该报错，不该被我们悄悄吃掉）；
//! - 只处理 UTF-8 的 BOM（`\u{feff}`）。UTF-16/32 的 BOM 说明编码根本不是 UTF-8，
//!   那种情况应该报"读不了"，而不是硬转。
//!
//! 这个仓库为 BOM 付过四次账（`pack.ps1` 的 `.ps1`、`gates.json`、契约夹具、契约加载器），
//! 所以这一处**写成公共函数**，而不是在每个解析点各写一遍。

/// 去掉开头的 UTF-8 BOM。**不改变其余任何字节**。
pub fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 去掉开头的bom() {
        assert_eq!(strip_bom("\u{feff}{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn 没有bom就原样返回() {
        assert_eq!(strip_bom("{\"a\":1}"), "{\"a\":1}");
        assert_eq!(strip_bom(""), "");
    }

    /// **中间的 BOM 不许吃**：那说明文件内容本身有问题，报错比悄悄修好更有用。
    #[test]
    fn 中间的bom不动() {
        assert_eq!(strip_bom("a\u{feff}b"), "a\u{feff}b");
    }

    /// **只去一次**：连续两个 BOM 也只去开头那一个（第二个是内容，不是签名）。
    #[test]
    fn 连续bom只去开头一个() {
        assert_eq!(strip_bom("\u{feff}\u{feff}x"), "\u{feff}x");
    }
}
