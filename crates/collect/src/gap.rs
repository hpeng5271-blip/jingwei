//! `?use` 未解析的原因码（gap 台账）。
//!
//! 原则：**报状态码而不是沉默**（xterm DECRPM / LSP diagnostics 同款做法）——
//! IDE 要能分清"缺文件 / 宏生成 / 跨 crate 同名模块遮蔽 / glob / crate 根别名"。
//! 原因现算不落库（不动 schema、不 bump rules）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 闭枚举：新增一个码必须同时给样例与规则（否则 other 会偷偷长大）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapCode {
    /// `use a::*;` / `use a::{b::*}`：glob 不展开
    Glob,
    /// `use crate as alias;`：crate 根别名
    CrateRootAlias,
    /// 目标由 `#[path = "..."]` 属性给出，本层不读属性
    PathAttrMissing,
    /// 首段命中本仓某个模块名，但目标文件不存在（跨 crate 同名遮蔽）
    ModuleShadow,
    /// `crate::` / `self::` / `super::` 开头但 item 找不到
    CrateItemMissing,
    /// 以 `::` 开头的绝对路径（extern prelude）：按语义是外部 crate，不该走内部模块表
    ExternAbsolute,
    /// 归不了类（不变量要求必须为 0）
    Other,
}

impl GapCode {
    pub fn as_str(self) -> &'static str {
        match self {
            GapCode::Glob => "glob",
            GapCode::CrateRootAlias => "crate_root_alias",
            GapCode::PathAttrMissing => "path_attr_missing",
            GapCode::ModuleShadow => "module_shadow",
            GapCode::CrateItemMissing => "crate_item_missing",
            GapCode::ExternAbsolute => "extern_absolute",
            GapCode::Other => "other",
        }
    }

    pub const ALL: [GapCode; 7] = [
        GapCode::Glob,
        GapCode::CrateRootAlias,
        GapCode::PathAttrMissing,
        GapCode::ModuleShadow,
        GapCode::CrateItemMissing,
        GapCode::ExternAbsolute,
        GapCode::Other,
    ];
}

/// 一次构建模块表，逐条判原因（避免每行都重扫整棵树）。
pub struct GapTable {
    root: PathBuf,
    modules: HashMap<String, String>,
}

impl GapTable {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            modules: crate::parse::module_map(root),
        }
    }

    /// 判一条 `?use` 的原因。`dst` 是 `?use <text>` 原样。
    pub fn reason(&self, src: &str, dst: &str, line: i64) -> GapCode {
        let text = dst.strip_prefix("?use ").unwrap_or(dst).trim();
        if text.contains('*') {
            return GapCode::Glob;
        }
        if text.starts_with("crate as ") || text.starts_with("crate:: as ") {
            return GapCode::CrateRootAlias;
        }
        if self.has_path_attr(src, line) {
            return GapCode::PathAttrMissing;
        }
        if text.starts_with("::") {
            return GapCode::ExternAbsolute;
        }
        let first = text.split("::").next().unwrap_or("").trim();
        if matches!(first, "crate" | "self" | "super") {
            return GapCode::CrateItemMissing;
        }
        if !first.is_empty()
            && self
                .modules
                .keys()
                .any(|k| k == first || k.starts_with(&format!("{first}::")))
        {
            return GapCode::ModuleShadow;
        }
        GapCode::Other
    }

    /// 该行**紧邻上方**的属性里是否有 `#[path = ...]`（遇空行/非属性行即停：
    /// 属性只作用于紧跟着的 item，隔空行就不再属于它）。
    fn has_path_attr(&self, src: &str, line: i64) -> bool {
        if line <= 1 {
            return false;
        }
        let p = self.root.join(src.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(body) = std::fs::read_to_string(&p) else {
            return false;
        };
        let lines: Vec<&str> = body.lines().collect();
        let mut i = (line as usize).saturating_sub(1); // 0-based: 上一行
        while i > 0 {
            i -= 1;
            let t = lines.get(i).map(|l| l.trim()).unwrap_or("");
            if t.is_empty() || !t.starts_with('#') {
                return false;
            }
            if t.starts_with("#[path") {
                return true;
            }
        }
        false
    }
}

/// 便利入口（一次性建表；测试与小数据量用）。
pub fn unresolved_reason(root: &Path, src: &str, dst: &str, line: i64) -> GapCode {
    GapTable::new(root).reason(src, dst, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("jw-gap-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(p.join("src")).expect("建目录");
        p
    }

    #[test]
    fn classifies_without_io() {
        let d = temp_dir("plain");
        let t = GapTable::new(&d);
        assert_eq!(t.reason("src/a.rs", "?use a::*", 1), GapCode::Glob);
        assert_eq!(t.reason("src/a.rs", "?use crate as np", 1), GapCode::CrateRootAlias);
        assert_eq!(t.reason("src/a.rs", "?use crate::x::Item", 3), GapCode::CrateItemMissing);
        assert_eq!(t.reason("src/a.rs", "?use self::x::Item", 3), GapCode::CrateItemMissing);
        // 本仓没有名为 othercrate 的模块 → 归不了类（不变量要求 0，这里暴露真未知）
        assert_eq!(t.reason("src/a.rs", "?use othercrate::Item", 3), GapCode::Other);
    }

    #[test]
    fn module_shadow_and_path_attr() {
        let d = temp_dir("shadow");
        std::fs::write(d.join("src/image.rs"), "pub struct X;\n").expect("写");
        std::fs::write(d.join("src/lib.rs"), "pub mod image;\n").expect("写");
        std::fs::write(
            d.join("src/a.rs"),
            "#[path = \"x/impl_tls.rs\"]\nmod impl_tls;\n\nuse image::X;\n",
        )
        .expect("写");
        let t = GapTable::new(&d);
        // image 是本仓模块名 → 跨 crate 同名遮蔽
        assert_eq!(t.reason("src/a.rs", "?use image::X", 4), GapCode::ModuleShadow);
        // 第 2 行的 mod 上方有 #[path] 属性
        assert_eq!(t.reason("src/a.rs", "?use x", 2), GapCode::PathAttrMissing);
    }
}
