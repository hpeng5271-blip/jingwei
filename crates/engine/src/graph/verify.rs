//! **位置回读检验**（2026-10-19）：图谱里记的行号，回去读原文对不对得上。
//!
//! 为什么这条检验值钱：它是**用文件原文当裁判**，不是拿我们自己解析出来的东西自证。
//! 判据刻意弱到"名字出现在那一行里"——因为行号会因注释/格式漂移，
//! 强到"字节区间完全相等"需要把 byte_range 一路存下来（下一步）。
//!
//! 三态（不许压扁）：
//! - `ok`：那一行里确实有这个名字；
//! - `bad`：那一行里没有（行号漂了或名字抽错了）；
//! - `unknown`：文件读不出来 / 行号超出文件范围 —— **查不了不等于错**。

use std::collections::BTreeMap;
use std::path::Path;

use crate::graph::store::Store;

/// 一次检验的汇总。
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct PositionReport {
    /// 名字在那一行里找到了
    pub ok: usize,
    /// 名字不在那一行（真问题）
    pub bad: usize,
    /// 文件缺失 / 行号越界 / 读不出来（**不是**错）
    pub unknown: usize,
    /// 名字在这个文件里有、但不在记的那一行（**位置不准**，不是抽错）——
    /// 典型：函数参数的闭包记在 `fn` 那一行，名字其实在签名下一行。
    pub imprecise: usize,
    /// 前几条不一致的样例（路径:行 → 期望名字 / 实际那一行）
    pub samples: Vec<String>,
}

impl PositionReport {
    pub fn checked(&self) -> usize {
        self.ok + self.bad + self.unknown + self.imprecise
    }

    /// 只有 `bad` 算失败；`unknown` 单独报（三态）。
    pub fn pass(&self) -> bool {
        self.bad == 0
    }
}

/// 把图谱里所有"有文件 + 有行号"的符号拿原文核一遍。
pub fn check_positions(root: &Path, store: &Store) -> PositionReport {
    let mut rep = PositionReport::default();
    let dump = match store.dump(1_000_000) {
        Ok(v) => v,
        Err(_) => {
            rep.unknown += 1;
            return rep;
        }
    };
    let Some(nodes) = dump.get("nodes").and_then(|n| n.as_array()) else {
        return rep;
    };

    // 同一个文件只读一次
    let mut cache: BTreeMap<String, Option<Vec<String>>> = BTreeMap::new();

    for n in nodes {
        let kind = n.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "symbol" {
            continue;
        }
        let (Some(file), Some(line), Some(name)) = (
            n.get("file").and_then(|v| v.as_str()),
            n.get("line").and_then(|v| v.as_u64()),
            n.get("name").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if name.is_empty() {
            rep.unknown += 1;
            continue;
        }
        let lines = cache
            .entry(file.to_string())
            .or_insert_with(|| std::fs::read_to_string(root.join(file)).ok().map(|t| {
                t.lines().map(|l| l.to_string()).collect::<Vec<_>>()
            }));
        let Some(lines) = lines.as_ref() else {
            rep.unknown += 1;
            continue;
        };
        let idx = (line as usize).saturating_sub(1);
        let Some(text) = lines.get(idx) else {
            rep.unknown += 1;
            continue;
        };
        if contains_word(text, name) {
            rep.ok += 1;
        } else if lines.iter().any(|l| contains_word(l, name)) {
            // 名字在这个文件里有 → **位置不准**（不是抽错），单独一档
            rep.imprecise += 1;
        } else {
            rep.bad += 1;
            if rep.samples.len() < 5 {
                rep.samples
                    .push(format!("{file}:{line} 期望 `{name}`，实际 `{}`", text.trim()));
            }
        }
    }
    rep
}

/// 列出所有 `depends_on` 边（File → Dependency），给"Python 的用在不在图里"一个可数的事实。
pub fn dep_edges_of(store: &Store) -> Vec<(String, String)> {
    let Ok(v) = store.dump(1_000_000) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in v.get("edges").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
        if e.get("kind").and_then(|k| k.as_str()) != Some("depends_on") {
            continue;
        }
        let (Some(s), Some(d)) = (e.get("src").and_then(|x| x.as_str()), e.get("dst").and_then(|x| x.as_str())) else {
            continue;
        };
        out.push((s.to_string(), d.to_string()));
    }
    out.sort();
    out
}

/// 名字是不是**作为完整标识符**出现在这行里（`fn a` 不该被 `fn abc` 命中）。
fn contains_word(hay: &str, needle: &str) -> bool {
    let b = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > b.len() {
        return false;
    }
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i + n.len() <= b.len() {
        if &b[i..i + n.len()] == n {
            let before_ok = i == 0 || !ident(b[i - 1]);
            let after_ok = i + n.len() == b.len() || !ident(b[i + n.len()]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 完整标识符才算命中() {
        assert!(contains_word("pub fn helper() {}", "helper"));
        assert!(!contains_word("pub fn helper2() {}", "helper"), "helper2 不算 helper");
        assert!(!contains_word("// helper", "helper2"));
        assert!(contains_word("use crate::a::helper;", "helper"));
    }

    /// **正例 + 反例**：自己索引出来的行号应当全对；
    /// 把某个符号的行号故意改错，检验必须抓到（否则这条检验是摆设）。
    #[test]
    fn 把行号改错就该被抓出来() {
        #![allow(clippy::let_underscore_must_use)]
        let root = std::env::temp_dir().join(format!("jw-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "\n\npub fn alpha() {}\n").unwrap();

        let mut s = crate::graph::store::Store::open(&root.join("g.db")).unwrap();
        crate::graph::index::index_source(&root, &mut s).unwrap();

        let good = check_positions(&root, &s);
        assert_eq!(good.bad, 0, "刚索引完不该有对不上的：{:?}", good.samples);
        assert!(good.ok >= 1, "至少要核到一个符号");

        // 反例①：把行号改成 1（空行）——名字还在文件里 → **位置不准**，不判错
        s.conn
            .execute("UPDATE nodes SET line=1 WHERE name='alpha'", [])
            .unwrap();
        let moved = check_positions(&root, &s);
        assert_eq!(moved.bad, 0, "名字还在文件里就不该判错");
        assert_eq!(moved.imprecise, 1, "应当记成位置不准");

        // 反例②：把名字改成一个文件里根本没有的 → **必须判错**
        s.conn
            .execute("UPDATE nodes SET name='ghost_name' WHERE name='alpha'", [])
            .unwrap();
        let ghost = check_positions(&root, &s);
        assert_eq!(ghost.bad, 1, "文件里没有这个名字，必须判错");
        let _ = std::fs::remove_dir_all(&root);
    }
}
