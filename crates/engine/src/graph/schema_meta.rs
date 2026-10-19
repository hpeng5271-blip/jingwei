//! **schema 版本与整库重建**（2026-10-19，加 `registers` 时踩出来的）。
//!
//! 为什么单独一个文件：`store.rs` 已经贴到 1000 行上限，而这段逻辑与"怎么存节点/边"
//! 是两件事——它只回答"这个库的 DDL 还认不认新值"。

use anyhow::Result;
use rusqlite::Connection;
/// 读库里的 schema 版本；读不到（新库 / 没有 meta 表）返回 None。
pub(crate) fn read_schema_version(conn: &Connection) -> Option<i64> {
    conn.query_row("SELECT v FROM meta WHERE k='schema_version'", [], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .and_then(|v| v.parse().ok())
}

/// 删掉库里的所有表与视图（`sqlite_` 内部表不动）。
pub(crate) fn drop_all(conn: &Connection) -> Result<()> {
    let mut st = conn.prepare(
        "SELECT name, type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%'",
    )?;
    let rows: Vec<(String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|x| x.ok())
        .collect();
    for (name, kind) in rows {
        let sql = format!("DROP {kind} IF EXISTS \"{name}\"", kind = if kind == "view" { "VIEW" } else { "TABLE" });
        conn.execute_batch(&sql)?;
    }
    Ok(())
}

/// 打开库时调用：版本不符就重建，然后执行 DDL。
///
/// `edges.kind` 是 CHECK 白名单，SQLite 改不了已有表的 CHECK——新值写不进去，
/// 症状是 `写关系失败：file:xxx`（看不出真正原因）。图谱是派生数据，删了能重算，
/// 所以这里选择**整库重建**而不是逐表迁移。
pub(crate) fn prepare(conn: &Connection) -> Result<()> {
    if let Some(v) = read_schema_version(conn) {
        if v < crate::graph::store::SCHEMA_VERSION {
            drop_all(conn)?;
        }
    }
    conn.execute_batch(crate::graph::store::SCHEMA_SQL)
        .map_err(|e| anyhow::anyhow!("建库/迁移失败——DDL 与现有库不兼容？{e}"))?;
    Ok(())
}
