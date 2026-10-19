//! **重命名**（2026-10-19，债表 0.25）：把"这个符号在哪些位置出现"算准，再交给编辑器。
//!
//! 为什么不直接给 LSP 一个"猜"的编辑集：**重命名是写用户代码**。
//! 一行里出现两次同名标识符、或者字符串/注释里恰好有同名，改错一次就是数据损失。
//! 所以这里的判据是：**精确到列号**；判不准的位置**不收**，单独报出来。
//!
//! 位置来源（与 `lsp::references` 同一套事实）：
//! - 定义：`nodes.file_path/line`；
//! - 调用点：`edge_sites(file,line)`；
//! - 退回：被引用符号的定义行（只在没有 sites 时用，标 `precise=false`）。

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Result, bail};
use rusqlite::Connection;

/// 一处可替换的位置。`col` 是 **1 起始的字节列**（Rust 源码按字节扫，够用且不猜）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Edit {
    pub file: String,
    pub line: u32,
    pub col: u32,
    /// 精确字节偏移（0 = 不知道，退回按列替换）。
    pub byte: u32,
    pub old: String,
    pub new: String,
}

/// 判不准、因此**不替换**的位置。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Ambiguous {
    pub file: String,
    pub line: u32,
    pub reason: String,
}

/// 一次计划的结果：能改的 + 不敢改的。
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct RenamePlan {
    pub edits: Vec<Edit>,
    pub ambiguous: Vec<Ambiguous>,
}

impl RenamePlan {
    pub fn files(&self) -> Vec<String> {
        let mut s: BTreeSet<String> = BTreeSet::new();
        for e in &self.edits {
            s.insert(e.file.clone());
        }
        s.into_iter().collect()
    }
}

/// 按符号 id 规划重命名。`new_name` 必须是合法标识符（不做语言特定的关键字检查）。
pub fn plan_rename(root: &Path, conn: &Connection, symbol_id: &str, new_name: &str) -> Result<RenamePlan> {
    if new_name.trim().is_empty() {
        bail!("新名字不能为空");
    }
    if !new_name
        .chars()
        .next()
        .map(|c| c.is_alphabetic() || c == '_')
        .unwrap_or(false)
    {
        bail!("新名字要以字母或下划线开头：{new_name}");
    }
    if !new_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        bail!("新名字只能是字母/数字/下划线：{new_name}");
    }

    let (name, def_file, def_line): (String, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT name, file_path, line FROM nodes WHERE id=?1 AND kind='symbol'",
            rusqlite::params![symbol_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| anyhow::anyhow!("找不到符号 {symbol_id}：{e}"))?;

    // 出现位置：定义行 + 调用点表（与 lsp 的引用查询同一套事实）
    let mut sites: BTreeSet<(String, u32, u32)> = BTreeSet::new();
    if let (Some(f), Some(l)) = (&def_file, def_line) {
        sites.insert((f.clone(), l as u32, 0));
    }
    if let Ok(mut st) = conn.prepare(
        "SELECT s.file, s.line, COALESCE(s.byte,0) FROM edge_sites s \
         JOIN edges e ON e.src=s.src AND e.dst=s.dst AND e.kind=s.kind \
         WHERE s.dst=?1 AND s.kind IN ('calls','references') AND e.expires_at IS NULL",
    ) {
        if let Ok(mut rows) = st.query(rusqlite::params![symbol_id]) {
            while let Ok(Some(r)) = rows.next() {
                sites.insert((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u32,
                    r.get::<_, i64>(2).unwrap_or(0) as u32,
                ));
            }
        }
    }

    // **守卫**：同文件内的直接调用边没有 `edge_sites` 记录（那是 resolve 阶段才写的），
    // 于是"能改的位置"会少于"实际引用数"——这种情况下改名会留下半截旧名。
    // 判据：调用/引用边数 > 已记录位置数 → 整次拒绝（宁可不动）。
    let edge_cnt: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges WHERE dst=?1 AND kind IN ('calls','references') AND expires_at IS NULL",
            rusqlite::params![symbol_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let site_cnt = sites.len() as i64;
    if edge_cnt > site_cnt {
        plan_guard(&mut sites);
        return Ok(RenamePlan {
            edits: Vec::new(),
            ambiguous: vec![Ambiguous {
                file: String::new(),
                line: 0,
                reason: format!(
                    "有 {edge_cnt} 条引用边，但只记录了 {site_cnt} 个位置（同文件调用点没记）——拒绝改，避免半截重命名"
                ),
            }],
        });
    }

    let mut plan = RenamePlan::default();
    // 同一个文件只读一次
    let mut cache: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for (file, line, byte) in sites {
        let lines = cache.entry(file.clone()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&file))
                .map(|t| t.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        });
        let Some(text) = lines.get((line.saturating_sub(1)) as usize) else {
            plan.ambiguous.push(Ambiguous {
                file,
                line,
                reason: "行号越界（文件变短了？）".into(),
            });
            continue;
        };
        // **优先用字节偏移**（tree-sitter 给的）：同一行多处同名也能唯一命中。
        if byte > 0 {
            let b = byte as usize;
            if text
                .as_bytes()
                .get(b..b + name.len())
                .map(|s| s == name.as_bytes())
                .unwrap_or(false)
            {
                plan.edits.push(Edit {
                    file,
                    line,
                    col: byte + 1,
                    byte,
                    old: name.clone(),
                    new: new_name.to_string(),
                });
                continue;
            }
        }
        let hits = word_hits(text, &name);
        match hits.len() {
            1 => plan.edits.push(Edit {
                file,
                line,
                col: hits[0] as u32 + 1,
                byte: 0,
                old: name.clone(),
                new: new_name.to_string(),
            }),
            0 => plan.ambiguous.push(Ambiguous {
                file,
                line,
                reason: format!("这一行里没有 `{name}`（位置可能漂了）"),
            }),
            _ => plan.ambiguous.push(Ambiguous {
                file,
                line,
                reason: format!("这一行有 {} 处 `{name}`，分不清改哪一处", hits.len()),
            }),
        }
    }
    Ok(plan)
}

/// 找出 `needle` 作为**完整标识符**出现在 `hay` 里的字节偏移（0 起始）。
fn plan_guard(_s: &mut BTreeSet<(String, u32, u32)>) {}

fn word_hits(hay: &str, needle: &str) -> Vec<usize> {
    let b = hay.as_bytes();
    let n = needle.as_bytes();
    let mut out = Vec::new();
    if n.is_empty() || n.len() > b.len() {
        return out;
    }
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i + n.len() <= b.len() {
        if &b[i..i + n.len()] == n {
            let before_ok = i == 0 || !is_word(b[i - 1]);
            let after_ok = i + n.len() == b.len() || !is_word(b[i + n.len()]);
            if before_ok && after_ok {
                out.push(i);
                i += n.len();
                continue;
            }
        }
        i += 1;
    }
    out
}


impl crate::graph::store::Store {
    /// 按名字找符号 id（重命名用）。**同名多义由调用方处理**，这里只如实给全部。
    pub fn symbol_ids_by_name(&self, name: &str) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT id FROM nodes WHERE kind='symbol' AND name=?1 ORDER BY id")?;
        let rows = st.query_map(rusqlite::params![name], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// 规划一次重命名（债表 0.25）：位置来自图谱，判不准的不收。
    pub fn plan_rename(&self, root: &Path, symbol_id: &str, new_name: &str) -> Result<RenamePlan> {
        plan_rename(root, &self.conn, symbol_id, new_name)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 只认完整标识符() {
        assert_eq!(word_hits("f(a, f)", "a"), vec![2]);
        assert_eq!(word_hits("ab a ba a", "a"), vec![3, 8]);
        assert!(word_hits("abc", "a").is_empty());
    }

    #[test]
    fn 一行两处分不清就不收() {
        let dir = std::env::temp_dir().join(format!("jw-ren-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "fn f() {}\n\nfn g() { f(); let x = f; }\n").unwrap();

        let db = dir.join("g.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(crate::graph::store::SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO nodes(id,kind,name,file_path,line,source,confidence,created_at,updated_at) VALUES('symbol:a.rs#f','symbol','f','a.rs',1,'ts',1.0,0,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO nodes(id,kind,name,file_path,line,source,confidence,created_at,updated_at) VALUES('symbol:a.rs#g','symbol','g','a.rs',3,'ts',1.0,0,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO edges(src,dst,kind,source,confidence,created_at) VALUES('symbol:a.rs#g','symbol:a.rs#f','calls','ts',1.0,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO edge_sites(src,dst,kind,file,line) VALUES('symbol:a.rs#g','symbol:a.rs#f','calls','a.rs',3)",
            [],
        )
        .unwrap();

        let plan = plan_rename(&dir, &conn, "symbol:a.rs#f", "h").unwrap();
        // 定义行能改；第 3 行有两处 f，分不清 → 进 ambiguous，不许猜
        assert_eq!(plan.edits.len(), 1, "{:?}", plan);
        assert_eq!(plan.edits[0].line, 1);
        assert_eq!(plan.ambiguous.len(), 1);
        assert!(plan.ambiguous[0].reason.contains("2 处"), "{:?}", plan);
        let _ = std::fs::remove_dir_all(&dir).ok();
    }
}
