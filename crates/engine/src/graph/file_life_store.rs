//! 文件生命周期相关的 Store 方法（2026-10-20，从 store.rs 拆出来）。
//!
//! 为什么拆：store.rs 撞到 1000 行上限。这里只放"文件生命周期"这一件事——
//! `file_life` / `note_scanned_same` / `drop_file`（墓碑），
//! 转移规则在 graph::file_life，存储细节在本文件。

use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};

use super::store::Store;

impl Store {
    pub fn schema_version(&self) -> Result<i64> {
        let v: String = self
            .conn
            .query_row("SELECT v FROM meta WHERE k='schema_version'", [], |r| r.get(0))
            .context("读 schema_version 失败")?;
        v.parse().context("schema_version 不是整数")
    }
    /// **扫描说"没变"时记一笔**（2026-10-20）：文件可能因为一级/二级检测被整批跳过，
    /// 根本没走到 `apply_file_update`——但生命周期必须从 `modified` 落回 `unchanged`。
    /// 走同一个 `transition`；非法（比如墓碑文件又"没变"）就报错，不兜底。
    pub fn note_scanned_same(&mut self, rel: &str) -> Result<()> {
        use crate::graph::file_life::{transition, FileEvent, FileLife};
        let cur: Option<String> = self
            .conn
            .query_row("SELECT file_life FROM files WHERE path=?1", params![rel], |r| r.get(0))
            .optional()
            .context("读文件生命周期失败")?;
        let from = cur
            .and_then(|x| FileLife::parse(&x))
            .unwrap_or(FileLife::Unseen);
        let to = transition(from, FileEvent::ScanSame)
            .map_err(|e| anyhow::anyhow!("{e}（文件 {rel}）"))?;
        self.conn
            .execute(
                "UPDATE files SET file_life=?2 WHERE path=?1",
                params![rel, to.as_str()],
            )
            .context("记文件生命周期失败")?;
        Ok(())
    }

    /// 读一个文件的生命周期状态（2026-10-20）。`None` = 库里没有这行。
    pub fn file_life(&self, rel: &str) -> Result<Option<crate::graph::file_life::FileLife>> {
        let s: Option<String> = self
            .conn
            .query_row("SELECT file_life FROM files WHERE path=?1", params![rel], |r| r.get(0))
            .optional()
            .context("读文件生命周期失败")?;
        Ok(s.and_then(|x| crate::graph::file_life::FileLife::parse(&x)))
    }
    /// 丢掉一个已经不存在的文件：**它的节点连同边一起删**。
    ///
    /// 与"关系失效只标记不删"不矛盾——那条说的是**关系**（曾经存在过是信息），
    /// 而这里删的是**节点**：一个磁盘上已经没有的符号，留在图谱里就是假的。
    pub fn drop_file(&mut self, rel: &str) -> Result<usize> {
        let tx = self.conn.transaction().context("开事务失败")?;
        // 先删节点（边由外键级联），再删状态行。
        let n = tx
            .execute(
                "DELETE FROM nodes WHERE file_path=?1 OR id=?2",
                params![rel, format!("file:{rel}")],
            )
            .context("删文件节点失败")?;
        // **墓碑**（2026-10-20）：行不删，标成 deleted。
        // 删掉行就分不清"从没见过"和"删掉了"——而这两者对下一次扫描的含义完全不同。
        tx.execute(
            "INSERT INTO files(path,size,mtime_ns,content_sha,parsed_at,incomplete,nodes_count,file_life)
             VALUES(?1,0,0,NULL,0,0,0,'deleted')
             ON CONFLICT(path) DO UPDATE SET
               size=0, mtime_ns=0, content_sha=NULL, incomplete=0, nodes_count=0, file_life='deleted'",
            params![rel],
        )
        .context("记删除墓碑失败")?;
        tx.commit().context("提交失败")?;
        Ok(n)
    }
}
