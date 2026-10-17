//! 图谱库：建库、迁移、读写、查询。M2 的落点。
//!
//! **DDL 只有一份**：`tools/schema.sql`，在编译期嵌进来。
//!
//! 为什么搬到文件、再嵌进二进制：贴在文档里的 SQL **既不能被执行也不能被校验**。
//! 实测把三处设计改掉之后，文档里那份副本没跟着改，于是"表结构到底是什么"
//! 有了两个互相矛盾的答案（《复审记录》Q1 记过这笔账）。
//! 嵌进来之后，"设计"和"实际建的表"不可能再分家——改错一边，测试立刻红。

/// 建库与迁移用的 DDL（编译期嵌入，与 `tools/schema.sql` 永远是同一份）。
// 路径比原来多一层：store.rs 现在在 `src/graph/` 里（六层重排）。
pub const SCHEMA_SQL: &str = include_str!("../../../../tools/schema.sql");

/// 当前 schema 版本。与 `schema.sql` 末尾写进 `meta` 的那个值必须一致，有测试守着。
pub const SCHEMA_VERSION: i64 = 1;

use anyhow::{Context, Result};
use jingwei_model::{Edge, EdgeKind, Node, NodeKind, Source};
use rusqlite::{params, Connection};
use std::path::Path;

/// 一次写入的结果。**数字是给界面看的**，不是给自己看的：
/// "写了 17 个节点、失效了 3 条边"要能说出来，而不是"操作成功"。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteReport {
    pub nodes: usize,
    pub edges: usize,
    pub expired: usize,
}

/// 遍历方向。反查是这套系统里同样常用的路径（"谁引用了这个符号"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Out,
    In,
}

/// 一个文件"上次被索引时"的状态。增量检测就靠比它。
///
/// **`content_sha` 与 `mtime_ns` 的作用不一样，别混**（数据层设计 §6.2 的三级检测）：
/// `mtime_ns` 便宜但会骗人（工具、检出、复制都可能改它），
/// `content_sha` 贵但可靠。先用便宜的筛，**只在便宜的说是"变了"时才花贵的钱确认**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    pub size: i64,
    pub mtime_ns: i64,
    pub content_sha: Option<String>,
    pub nodes_count: Option<i64>,
}

/// 项目里的图谱库。
///
/// **一个项目一个库**，跟着项目走（放在 `.jingwei/graph.db`），删掉即可重建——
/// 它是可重算的派生数据。真正不可丢的是笔记正文，那条路不在这个文件里。
pub struct Store {
    conn: Connection,
}

impl Store {
    /// 打开（必要时新建）某个项目的图谱库。
    ///
    /// 这条路径是**写者**：DDL 里所有 `IF NOT EXISTS` 让重复打开是幂等的，
    /// 所以"建库"和"迁移"共用同一段 SQL——不存在"第一次要跑另一个脚本"这种事。
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("建图谱库目录失败：{}", dir.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("打开图谱库失败：{}", path.display()))?;
        conn.execute_batch(SCHEMA_SQL)
            .context("建库/迁移失败——DDL 与现有库不兼容？")?;
        Ok(Store { conn })
    }

    /// 只读打开。查询用这条连接，写用 `open`——**读写分离**是为了
    /// "索引在写的时候，界面查询不被长事务卡住"。
    pub fn open_readonly(path: &Path) -> Result<Store> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("只读打开图谱库失败：{}", path.display()))?;
        Ok(Store { conn })
    }

    pub fn schema_version(&self) -> Result<i64> {
        let v: String = self
            .conn
            .query_row("SELECT v FROM meta WHERE k='schema_version'", [], |r| r.get(0))
            .context("读 schema_version 失败")?;
        v.parse().context("schema_version 不是整数")
    }

    /// 一个事务里写一批节点与边。**幂等**：同一批写两遍，库里还是那一份。
    ///
    /// 三条规矩落在这里：
    /// 1. 全在一个事务里——半批写进去比全没写更糟（图谱会自相矛盾）。
    /// 2. 走 UPSERT 而不是 INSERT——增量重跑是常态，裸 INSERT 第二次就炸。
    /// 3. **`created_at` 不覆盖**：第一次看到这个节点的时间是有信息量的，
    ///    每次重扫都刷新它，等于把"历史"擦掉。
    pub fn upsert(&mut self, nodes: &[Node], edges: &[Edge]) -> Result<WriteReport> {
        let tx = self.conn.transaction().context("开事务失败")?;
        let mut report = WriteReport::default();

        {
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO nodes(id,kind,name,file_path,line,language,data,source,confidence,checksum,created_at,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                     ON CONFLICT(id) DO UPDATE SET
                       name=excluded.name, file_path=excluded.file_path, line=excluded.line,
                       language=excluded.language, data=excluded.data, source=excluded.source,
                       confidence=excluded.confidence, checksum=excluded.checksum,
                       updated_at=excluded.updated_at",
                )
                .context("准备节点写入失败")?;
            for n in nodes {
                let data = match &n.data {
                    Some(v) => Some(serde_json::to_string(v).context("节点 data 不是合法 JSON")?),
                    None => None,
                };
                st.execute(params![
                    n.id,
                    n.kind.as_str(),
                    n.name,
                    n.file_path,
                    n.line,
                    n.language,
                    data,
                    n.source.as_str(),
                    n.confidence.value() as f64,
                    n.checksum,
                    n.created_at,
                    n.updated_at,
                ])
                .with_context(|| format!("写节点失败：{}", n.id))?;
                report.nodes += 1;
            }
        }

        {
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO edges(src,dst,kind,source,confidence,created_at)
                     VALUES(?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(src,dst,kind) DO UPDATE SET
                       source=excluded.source, confidence=excluded.confidence, expires_at=NULL",
                )
                .context("准备关系写入失败")?;
            for e in edges {
                st.execute(params![
                    e.src,
                    e.dst,
                    e.kind.as_str(),
                    e.source.as_str(),
                    e.confidence.value() as f64,
                    e.created_at,
                ])
                .with_context(|| format!("写关系失败：{} -{}-> {}", e.src, e.kind.as_str(), e.dst))?;
                report.edges += 1;
            }
        }

        tx.commit().context("提交失败")?;
        Ok(report)
    }

    /// 失效一条边：**标记而不删**（`expires_at` 置为时间戳）。
    ///
    /// 为什么留历史：一条关系"曾经存在过"本身是信息——
    /// 排查"为什么上个月还好的东西现在断了"时，它是有用的证据。
    ///
    /// 返回是否真的改了（`false` = 本来就是失效的，或者根本没有这条边）。
    pub fn expire_edge(&mut self, src: &str, dst: &str, kind: EdgeKind, at: i64) -> Result<bool> {
        let n = self
            .conn
            .execute(
                "UPDATE edges SET expires_at=?1
                 WHERE src=?2 AND dst=?3 AND kind=?4 AND expires_at IS NULL",
                params![at, src, dst, kind.as_str()],
            )
            .context("失效关系失败")?;
        Ok(n > 0)
    }

    /// 1 跳邻居。**只走 `live_edges` 视图**——失效边在语法上就查不到，
    /// 不需要每个调用点记得写 `WHERE expires_at IS NULL`。
    pub fn neighbors(&self, id: &str, kind: EdgeKind, dir: Dir) -> Result<Vec<String>> {
        let (sql, col) = match dir {
            Dir::Out => ("SELECT dst FROM live_edges WHERE src=?1 AND kind=?2", "dst"),
            Dir::In => ("SELECT src FROM live_edges WHERE dst=?1 AND kind=?2", "src"),
        };
        let mut st = self.conn.prepare_cached(sql).context("准备邻居查询失败")?;
        let rows = st
            .query_map(params![id, kind.as_str()], |r| r.get::<_, String>(0))
            .with_context(|| format!("查 {col} 失败"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("读邻居失败")?);
        }
        Ok(out)
    }

    /// 把整张图导出来（给可视化用，也是子图查询的基础）。
    ///
    /// **为什么天生带 `limit`**：几千个节点全导出去，序列化 + 传输本身就是瓶颈
    /// （这正是汇报过的"IPC 传大 JSON"那条）。所以这个接口自带闸门，
    /// 超了就截断——并且**如实说截断了**（`truncated: true`）：
    /// 界面上画不全时，必须能区分"只有这么多"和"被截了"。
    pub fn dump(&self, limit: usize) -> Result<serde_json::Value> {
        let mut nodes = Vec::new();
        {
            let mut st = self
                .conn
                .prepare_cached("SELECT id, kind, name, file_path, line FROM nodes LIMIT ?1")
                .context("准备节点导出失败")?;
            let mut rows = st.query(params![limit as i64]).context("导出节点失败")?;
            while let Some(r) = rows.next().context("读节点行失败")? {
                nodes.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "kind": r.get::<_, String>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "file": r.get::<_, Option<String>>(3)?,
                    "line": r.get::<_, Option<i64>>(4)?,
                }));
            }
        }
        let total_nodes: i64 = self
            .conn
            .query_row("SELECT count(*) FROM nodes", [], |r| r.get(0))
            .context("数节点失败")?;

        let mut edges = Vec::new();
        {
            let mut st = self
                .conn
                .prepare_cached("SELECT src, dst, kind FROM live_edges LIMIT ?1")
                .context("准备关系导出失败")?;
            let mut rows = st
                .query(params![(limit.saturating_mul(4)) as i64])
                .context("导出关系失败")?;
            while let Some(r) = rows.next().context("读关系行失败")? {
                edges.push(serde_json::json!({
                    "src": r.get::<_, String>(0)?,
                    "dst": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                }));
            }
        }
        let total_edges: i64 = self
            .conn
            .query_row("SELECT count(*) FROM live_edges", [], |r| r.get(0))
            .context("数关系失败")?;

        // 三种角色：判据与实现都在 `graph/roles.rs`（那里注释写清了为什么
        // 只算 `calls`、为什么要聚到文件、为什么测试文件不参与）。
        let roles = super::roles::annotate(&mut nodes, &edges);
        // **目录级**：文件级之后再看一层，才叫"模块结构"（`框架.md` §4.8）。
        let dirs = super::roles::dir_roles(&nodes, &edges);
        Ok(serde_json::json!({
            "nodes": nodes,
            "edges": edges,
            "roles": roles,
            "dirs": dirs,
            "total_nodes": total_nodes,
            "total_edges": total_edges,
            "truncated": (nodes.len() as i64) < total_nodes || (edges.len() as i64) < total_edges,
        }))
    }

    /// 库里有多少节点、多少条**有效**边。界面上"这个项目索引了多少东西"就靠它。
    pub fn counts(&self) -> Result<(i64, i64)> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM nodes", [], |r| r.get(0))
            .context("数节点失败")?;
        let e: i64 = self
            .conn
            .query_row("SELECT count(*) FROM live_edges", [], |r| r.get(0))
            .context("数关系失败")?;
        Ok((n, e))
    }

    /// 清掉过期太久的关系——**库不会自己瘦下来，得有人动手**。
    ///
    /// 两个例外**永不清理**：`noted_at`（笔记附着）与 `changed_at`（提交）。
    /// 它们是历史，不是可重算的派生数据。所以这里那条 `NOT IN` 不是可选参数：
    /// 写成"清掉所有过期的"就等于把笔记和提交记录一起删了，**而且不报错**。
    ///
    /// 返回删了几行（界面要说出来，不能默默删）。
    pub fn purge_expired(&mut self, older_than: i64) -> Result<usize> {
        let n = self
            .conn
            .execute(
                "DELETE FROM edges
                  WHERE expires_at IS NOT NULL
                    AND expires_at < ?1
                    AND kind NOT IN ('noted_at','changed_at')",
                params![older_than],
            )
            .context("清理过期关系失败")?;
        Ok(n)
    }

    /// FTS5 到底能不能用。
    ///
    /// `bundled` 版本自带 FTS5（Q2 实测），正常路径上永远是 true。
    /// **留着它是因为**：一旦将来换成动态链接的系统 SQLite（`winsqlite3.dll`），
    /// 缺 FTS5 的表现是**搜索悄悄坏掉**——不报错，只是搜不到东西。
    /// 那种错没人能自己查出来，所以启动时问一次，缺了就禁用搜索并在界面上说明。
    ///
    /// 用 `temp.` 前缀：探测表建在临时库里，**不落盘、不留痕**。
    pub fn fts5_available(&self) -> bool {
        self.conn
            .execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS temp._fts_probe USING fts5(x);
                 DROP TABLE IF EXISTS temp._fts_probe;",
            )
            .is_ok()
    }

    /// 从存储还原一个节点。读路径**不做任何校验**——历史数据要能原样读回来，
    /// 哪怕它是规则变更之前写进去的（对应 `Edge::from_row` 的那条说明）。
    pub fn node(&self, id: &str) -> Result<Option<Node>> {
        let mut st = self
            .conn
            .prepare_cached(
                "SELECT id,kind,name,file_path,line,language,data,source,confidence,checksum,created_at,updated_at
                 FROM nodes WHERE id=?1",
            )
            .context("准备节点查询失败")?;
        let mut rows = st.query(params![id]).context("查节点失败")?;
        let Some(row) = rows.next().context("读节点行失败")? else {
            return Ok(None);
        };
        Ok(Some(row_to_node(row)?))
    }
    /// 库里记着哪些文件（用来找"索引过但现在已经没有了"的）。
    pub fn indexed_files(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT path FROM files")
            .context("准备文件清单查询失败")?;
        let rows = st
            .query_map([], |r| r.get::<_, String>(0))
            .context("查文件清单失败")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("读文件路径失败")?);
        }
        Ok(out)
    }

    /// 按类型列节点。**新到旧的顺序**（`created_at` 降序）。
    ///
    /// 为什么需要它：在这之前，读图谱只有三条路——按 id 点查（`node`）、
    /// 按文件查（`file_state`）、整张导出（`dump`）。所以"上次跑失败了哪些问题"
    /// 这类问题**根本问不出来**：知道要看 `run` 节点，却没有"给我所有 run"这条入口。
    /// M5 的验收（`tests/m5_run_into_graph.rs`）就是卡在这一点上才把它补出来的。
    ///
    /// `limit` 是必需的，不给默认值：图谱是几千个节点的东西，
    /// "全给我"这种调用在界面上一定会变成卡顿，而卡顿的锅最后会算到图谱头上。
    pub fn list_kind(&self, kind: NodeKind, limit: usize) -> Result<Vec<Node>> {
        let mut st = self
            .conn
            .prepare_cached(
                "SELECT id,kind,name,file_path,line,language,data,source,confidence,checksum,created_at,updated_at
                 FROM nodes WHERE kind=?1 ORDER BY created_at DESC LIMIT ?2",
            )
            .context("准备按类型列节点失败")?;
        let mut rows = st
            .query(params![kind.as_str(), limit as i64])
            .context("按类型查节点失败")?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("读节点行失败")? {
            out.push(row_to_node(row)?);
        }
        Ok(out)
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
        tx.execute("DELETE FROM files WHERE path=?1", params![rel])
            .context("删文件状态失败")?;
        tx.commit().context("提交失败")?;
        Ok(n)
    }

    /// 读一个文件上次被索引时的状态。`None` = 从来没索引过。
    pub fn file_state(&self, rel: &str) -> Result<Option<FileState>> {
        let mut st = self
            .conn
            .prepare_cached(
                "SELECT size, mtime_ns, content_sha, nodes_count FROM files WHERE path=?1",
            )
            .context("准备文件状态查询失败")?;
        let mut rows = st.query(params![rel]).context("查文件状态失败")?;
        let Some(row) = rows.next().context("读文件状态行失败")? else {
            return Ok(None);
        };
        Ok(Some(FileState {
            size: row.get(0)?,
            mtime_ns: row.get(1)?,
            content_sha: row.get(2)?,
            nodes_count: row.get(3)?,
        }))
    }

    /// **一个文件的一次更新，全在一个事务里。**
    ///
    /// 顺序不是随便定的，四步各解决一件事：
    ///
    /// 1. **先把这个文件产生的旧关系标失效**——不然"上个月删掉的那次调用"
    ///    会永远留在图谱里冒充当前状态（UPSERT 只复活命中的，不清理没命中的）。
    ///    例外：`noted_at` / `changed_at` 是历史，不失效。
    /// 2. **删掉不再存在的符号**（`keep_symbol_ids` 是新解析出来的那批）。
    ///    它们带着自己的边一起被外键级联清掉——这正是要的：
    ///    一个已经不存在的符号，它的关系也该跟着消失。
    /// 3. 写入新的节点与边（UPSERT；命中的旧关系在这一步复活）。
    /// 4. 记下这次的文件状态（大小、mtime、内容哈希、节点数）。
    ///
    /// **为什么必须在一个事务里**：半途失败会让图谱停在
    /// "旧关系已失效、新关系还没写"的状态——看起来像"这个文件什么都没引用了"，
    /// 而那是个假象，而且**不报错**。
    #[allow(clippy::too_many_arguments)]
    pub fn apply_file_update(
        &mut self,
        rel: &str,
        state: &FileState,
        nodes: &[Node],
        edges: &[Edge],
        keep_symbol_ids: &[String],
        now: i64,
        incomplete: bool,
    ) -> Result<WriteReport> {
        let tx = self.conn.transaction().context("开事务失败")?;

        // 1. 旧关系失效（历史类除外）
        let expired = tx
            .execute(
                "UPDATE edges SET expires_at=?1
                  WHERE expires_at IS NULL
                    AND kind NOT IN ('noted_at','changed_at')
                    AND src IN (SELECT id FROM nodes WHERE file_path=?2)",
                params![now, rel],
            )
            .context("失效旧关系失败")?;
        let mut report = WriteReport {
            expired,
            ..Default::default()
        };

        // 2. 删掉不再存在的符号（边由外键级联）
        let mut gone: Vec<String> = Vec::new();
        {
            let mut st = tx
                .prepare_cached("SELECT id FROM nodes WHERE kind='symbol' AND file_path=?1")
                .context("准备符号查询失败")?;
            let rows = st
                .query_map(params![rel], |r| r.get::<_, String>(0))
                .context("查符号失败")?;
            for r in rows {
                let id = r.context("读符号 id 失败")?;
                if !keep_symbol_ids.contains(&id) {
                    gone.push(id);
                }
            }
        }
        for id in &gone {
            tx.execute("DELETE FROM nodes WHERE id=?1", params![id])
                .with_context(|| format!("删旧符号失败：{id}"))?;
        }

        // 3. 写节点与边
        {
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO nodes(id,kind,name,file_path,line,language,data,source,confidence,checksum,created_at,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                     ON CONFLICT(id) DO UPDATE SET
                       name=excluded.name, file_path=excluded.file_path, line=excluded.line,
                       language=excluded.language, data=excluded.data, source=excluded.source,
                       confidence=excluded.confidence, checksum=excluded.checksum,
                       updated_at=excluded.updated_at",
                )
                .context("准备节点写入失败")?;
            for n in nodes {
                let data = match &n.data {
                    Some(v) => Some(serde_json::to_string(v).context("节点 data 不是合法 JSON")?),
                    None => None,
                };
                st.execute(params![
                    n.id,
                    n.kind.as_str(),
                    n.name,
                    n.file_path,
                    n.line,
                    n.language,
                    data,
                    n.source.as_str(),
                    n.confidence.value() as f64,
                    n.checksum,
                    n.created_at,
                    n.updated_at,
                ])
                .with_context(|| format!("写节点失败：{}", n.id))?;
                report.nodes += 1;
            }
        }
        {
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO edges(src,dst,kind,source,confidence,created_at)
                     VALUES(?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(src,dst,kind) DO UPDATE SET
                       source=excluded.source, confidence=excluded.confidence, expires_at=NULL",
                )
                .context("准备关系写入失败")?;
            for e in edges {
                st.execute(params![
                    e.src,
                    e.dst,
                    e.kind.as_str(),
                    e.source.as_str(),
                    e.confidence.value() as f64,
                    e.created_at,
                ])
                .with_context(|| format!("写关系失败：{}", e.src))?;
                report.edges += 1;
            }
        }

        // 4. 记状态
        tx.execute(
            "INSERT INTO files(path,size,mtime_ns,content_sha,parsed_at,incomplete,nodes_count)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(path) DO UPDATE SET
               size=excluded.size, mtime_ns=excluded.mtime_ns, content_sha=excluded.content_sha,
               parsed_at=excluded.parsed_at, incomplete=excluded.incomplete,
               nodes_count=excluded.nodes_count",
            params![
                rel,
                state.size,
                state.mtime_ns,
                state.content_sha,
                now,
                i64::from(incomplete),
                state.nodes_count
            ],
        )
        .context("记文件状态失败")?;

        tx.commit().context("提交失败")?;
        Ok(report)
    }
}

/// 一行 `nodes` → 领域模型里的 `Node`。
///
/// **只有这一处映射**：它原来内联在 `node()` 里，而 `list_kind` 又要用同一套列顺序，
/// 分开写就迟早歪（列顺序或类型改一处、漏一处，症状是"读出来的节点字段串了"，不报错）。
/// 两条读路径（点查 / 按类型列）共用它。
fn row_to_node(row: &rusqlite::Row<'_>) -> Result<Node> {
    let id: String = row.get(0)?;
    let kind_s: String = row.get(1)?;
    let data: Option<String> = row.get(6)?;
    let source_s: String = row.get(7)?;
    let conf: f64 = row.get(8)?;
    Ok(Node {
        id,
        kind: kind_from_str(&kind_s).with_context(|| format!("库里存了未知的 kind：{kind_s}"))?,
        name: row.get(2)?,
        file_path: row.get(3)?,
        line: row.get(4)?,
        language: row.get(5)?,
        data: match data {
            Some(t) => Some(serde_json::from_str(&t).context("节点 data 不是合法 JSON")?),
            None => None,
        },
        source: source_from_str(&source_s)
            .with_context(|| format!("库里存了未知的 source：{source_s}"))?,
        confidence: jingwei_model::Confidence::from_value(conf as f32),
        checksum: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

/// 字符串 → 枚举。**认不出来是错误，不是默认值**：
/// 遇到不认识的 kind 就当成 File 之类，会让图谱安静地错下去。
fn kind_from_str(s: &str) -> Result<NodeKind> {
    NodeKind::ALL
        .iter()
        .copied()
        .find(|k| k.as_str() == s)
        .with_context(|| format!("未知的节点类型：{s}"))
}

fn source_from_str(s: &str) -> Result<Source> {
    Source::ALL
        .iter()
        .copied()
        .find(|k| k.as_str() == s)
        .with_context(|| format!("未知的来源：{s}"))
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]

    use super::*;
    use jingwei_model::{EdgeKind, NodeKind, Source};

    /// 从 DDL 里抠出 `xxx IN ('a','b',…)` 的取值列表。
    /// 手写扫描而不是引 regex：这一层不值得为一条测试多一个依赖。
    fn in_lists(sql: &str, needle: &str) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        for (i, _) in sql.match_indices(needle) {
            let rest = &sql[i + needle.len()..];
            let end = rest.find(')').expect("CHECK 的取值列表没闭合");
            out.push(
                rest[..end]
                    .split('\'')
                    .skip(1)
                    .step_by(2)
                    .map(|s| s.trim().to_string())
                    .collect(),
            );
        }
        out
    }

    /// **枚举在 Rust 里、取值在 SQL 里——这是"同一事实两个副本"最容易炸的地方。**
    ///
    /// 炸的方式还特别难受：不是编译错，而是**第一次往库里写东西时** `CHECK` 拒绝，
    /// 而且多半炸在别人机器上（本机试过的那条旧数据恰好合法）。
    /// 有了它，改一边不改另一边，`cargo test` 立刻红。
    #[test]
    fn ddl_kind_lists_match_the_domain_enums() {
        let lists = in_lists(SCHEMA_SQL, "kind IN (");
        assert_eq!(
            lists.len(),
            2,
            "DDL 里应有 nodes 与 edges 两处 kind 的 CHECK；实际 {} 处",
            lists.len()
        );
        let want_nodes: Vec<String> = NodeKind::ALL.iter().map(|k| k.as_str().to_string()).collect();
        let want_edges: Vec<String> = EdgeKind::ALL.iter().map(|k| k.as_str().to_string()).collect();
        // 顺序即 DDL 里的出现顺序：先 nodes 后 edges。
        assert_eq!(lists[0], want_nodes, "nodes.kind 的 CHECK 与 NodeKind::ALL 不一致");
        assert_eq!(lists[1], want_edges, "edges.kind 的 CHECK 与 EdgeKind::ALL 不一致");
    }

    #[test]
    fn ddl_source_lists_match_the_source_enum() {
        let lists = in_lists(SCHEMA_SQL, "source IN (");
        assert!(!lists.is_empty(), "DDL 里找不到 source 的 CHECK");
        let want: Vec<String> = Source::ALL.iter().map(|s| s.as_str().to_string()).collect();
        for list in &lists {
            assert_eq!(list, &want, "source 的 CHECK 与 Source::ALL 不一致");
        }
    }

    /// 版本号也是两个副本：Rust 常量与 DDL 末尾写进 `meta` 的那个值。
    #[test]
    fn ddl_version_stamp_matches_the_constant() {
        let needle = "VALUES ('schema_version', '";
        let i = SCHEMA_SQL
            .find(needle)
            .expect("DDL 末尾应有 schema_version 的初始写入");
        let rest = &SCHEMA_SQL[i + needle.len()..];
        let end = rest.find('\'').expect("版本号没有闭合引号");
        assert_eq!(
            rest[..end].parse::<i64>().expect("版本号不是整数"),
            SCHEMA_VERSION,
            "schema.sql 的版本戳与 store::SCHEMA_VERSION 不一致"
        );
    }

    /// DDL 必须能真的建出一个库来——建不了还叫什么施工图。
    /// 用 `rusqlite` 的内存库跑一遍；这一步同时把"我写错了 SQL"挡在提交之前。
    #[test]
    fn ddl_executes_and_live_edges_filters_expired() {
        let c = rusqlite::Connection::open_in_memory().expect("开内存库失败");
        c.execute_batch(SCHEMA_SQL).expect("DDL 执行失败");

        c.execute(
            "INSERT INTO nodes(id,kind,name,source,created_at,updated_at)
             VALUES('file:a.rs','file','a.rs','ts',1,1)",
            [],
        )
        .expect("插节点失败");
        c.execute(
            "INSERT INTO nodes(id,kind,name,source,created_at,updated_at)
             VALUES('run:1','run','r','self',1,1)",
            [],
        )
        .expect("插节点失败");
        c.execute(
            "INSERT INTO edges(src,dst,kind,source,confidence,created_at)
             VALUES('run:1','file:a.rs','diagnosed_at','self',1.0,1)",
            [],
        )
        .expect("插有效边失败");
        c.execute(
            "INSERT INTO edges(src,dst,kind,source,confidence,created_at,expires_at)
             VALUES('run:1','file:a.rs','causes','self',1.0,1,999)",
            [],
        )
        .expect("插失效边失败");

        let all: i64 = c
            .query_row("SELECT count(*) FROM edges", [], |r| r.get(0))
            .unwrap();
        let live: i64 = c
            .query_row("SELECT count(*) FROM live_edges", [], |r| r.get(0))
            .unwrap();
        assert_eq!((all, live), (2, 1), "失效边必须被 live_edges 挡掉");

        // 非法 kind 必须写不进去——这是"幽灵关系"的唯一一道闸。
        let bad = c.execute(
            "INSERT INTO nodes(id,kind,name,source,created_at,updated_at)
             VALUES('x','bogus','x','ts',1,1)",
            [],
        );
        assert!(bad.is_err(), "CHECK 没拦住非法 kind");
    }

    // ── 下面是 store 的读写行为 ─────────────────────────────────────

    use jingwei_model::{Confidence, Edge};
    use std::path::PathBuf;

    /// 每个用例一个临时目录：库里带着 WAL 文件，互相串起来会很难查。
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-store-{}-{}", tag, std::process::id()));
        if d.exists() {
            let _ = std::fs::remove_dir_all(&d);
        }
        std::fs::create_dir_all(&d).expect("建临时目录失败");
        d
    }

    fn file_node(id: &str) -> Node {
        Node::new(
            NodeKind::File,
            id,
            format!("{id}.rs"),
            Source::TreeSitter,
            Confidence::Certain,
            1000,
        )
    }

    /// 用 `from_row` 而不是 `new`：这层测的是**存储行为**，
    /// 两端合法性是 `model` 的事（那边有自己的测试）。
    fn edge(src: &str, dst: &str, kind: EdgeKind) -> Edge {
        Edge::from_row(src, dst, kind, Source::TreeSitter, Confidence::High, 1000, None)
    }

    #[test]
    fn upsert_is_idempotent_and_keeps_created_at() {
        let p = tmp("idem").join("g.db");
        let mut s = Store::open(&p).expect("开库失败");

        let n = file_node("a");
        s.upsert(std::slice::from_ref(&n), &[]).expect("第一次写失败");
        // 第二次带更新的时间戳：名字可以变，**created_at 不许被覆盖**。
        let mut later = n.clone();
        later.updated_at = 2000;
        later.name = "改过名".into();
        s.upsert(&[later], &[]).expect("第二次写失败");

        let (nodes, _) = s.counts().expect("数不过来");
        assert_eq!(nodes, 1, "UPSERT 应该只留一份");
        let got = s.node(&n.id).expect("读失败").expect("节点不见了");
        assert_eq!(got.name, "改过名", "更新该生效");
        assert_eq!(got.created_at, 1000, "created_at 是历史，不许被覆盖");
        assert_eq!(got.updated_at, 2000, "updated_at 该刷新");
    }

    #[test]
    fn expired_edge_leaves_neighbors_but_stays_in_the_table() {
        let p = tmp("expire").join("g.db");
        let mut s = Store::open(&p).expect("开库失败");
        s.upsert(
            &[file_node("a"), file_node("b")],
            &[edge("file:a", "file:b", EdgeKind::References)],
        )
        .expect("写失败");

        assert_eq!(
            s.neighbors("file:a", EdgeKind::References, Dir::Out)
                .expect("查邻居失败"),
            vec!["file:b".to_string()]
        );

        assert!(
            s.expire_edge("file:a", "file:b", EdgeKind::References, 9000)
                .expect("失效失败"),
            "第一次失效应该真的改到东西"
        );
        assert!(
            !s.expire_edge("file:a", "file:b", EdgeKind::References, 9001)
                .expect("重复失效不该报错"),
            "已经失效的边再失效一次应该返回 false"
        );

        assert!(
            s.neighbors("file:a", EdgeKind::References, Dir::Out)
                .expect("查邻居失败")
                .is_empty(),
            "失效边不该出现在 live_edges 里"
        );
        // 但表里还在——历史是证据。
        let all: i64 = s
            .conn
            .query_row("SELECT count(*) FROM edges", [], |r| r.get(0))
            .expect("数边失败");
        assert_eq!(all, 1, "失效只标记，不删");
    }

    #[test]
    fn rediscovering_an_edge_revives_it() {
        let p = tmp("revive").join("g.db");
        let mut s = Store::open(&p).expect("开库失败");
        let e = edge("file:a", "file:b", EdgeKind::References);
        s.upsert(&[file_node("a"), file_node("b")], std::slice::from_ref(&e))
            .expect("写失败");
        s.expire_edge("file:a", "file:b", EdgeKind::References, 9000)
            .expect("失效失败");

        // 再扫一遍又看到它了——**必须复活**，否则"改回去就好了"这种事永远不会被反映出来。
        s.upsert(&[], &[e]).expect("重写失败");
        assert_eq!(
            s.neighbors("file:a", EdgeKind::References, Dir::Out)
                .expect("查邻居失败")
                .len(),
            1,
            "重新发现的关系应该复活"
        );
    }

    #[test]
    fn unknown_kind_in_db_is_an_error_not_a_default() {
        // CHECK 已经把非法 kind 挡在库外，所以这条只验函数级行为：
        // 从库里读出一个认不出的字符串时，**必须报错**——
        // 当成默认值（比如落到 File）会让图谱安静地错下去。
        assert!(kind_from_str("file").is_ok());
        assert!(
            kind_from_str("bogus").is_err(),
            "认不出的类型必须报错——当成默认值会让图谱安静地错下去"
        );
    }

    /// 清理只清"派生数据"。**笔记附着与提交记录是历史，永远不许被清掉。**
    ///
    /// 这条测试守的是一个**不报错的错误**：如果清理 SQL 里少写那个 `NOT IN`，
    /// 用户会安静地丢掉笔记和提交记录，而且没有任何提示——等到某天想翻旧账才发现。
    #[test]
    fn purge_never_touches_notes_or_commits() {
        let p = tmp("purge").join("g.db");
        let mut s = Store::open(&p).expect("开库失败");
        let three = [
            EdgeKind::References, // 派生：可清
            EdgeKind::NotedAt,    // 历史：永不清理
            EdgeKind::ChangedAt,  // 历史：永不清理
        ];
        let edges: Vec<Edge> = three
            .iter()
            .map(|k| edge("file:a", "file:b", *k))
            .collect();
        s.upsert(&[file_node("a"), file_node("b")], &edges)
            .expect("写失败");
        for k in three {
            assert!(
                s.expire_edge("file:a", "file:b", k, 1000)
                    .expect("失效失败"),
                "三条都该被标成失效"
            );
        }

        let removed = s.purge_expired(2000).expect("清理失败");
        assert_eq!(removed, 1, "只该清掉派生关系那一条");

        let mut left: Vec<String> = s
            .conn
            .prepare("SELECT kind FROM edges ORDER BY kind")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec!["changed_at".to_string(), "noted_at".to_string()],
            "笔记与提交记录必须原样留着"
        );
    }

    /// **一批里有一条坏的，整批都不许进去。**
    ///
    /// 半批写进去比全没写更糟：图谱会自相矛盾（有边没节点）。而"半批"这种事
    /// 只在库变大、出错概率变高之后才会真的发生——那时再补测试就晚了。
    #[test]
    fn a_bad_row_rolls_back_the_whole_batch() {
        let p = tmp("rollback").join("g.db");
        let mut s = Store::open(&p).expect("开库失败");
        let good = edge("file:a", "file:a", EdgeKind::References);
        // 指向一个不存在的节点：外键会拦下它。
        let bad = edge("file:nonexistent", "file:a", EdgeKind::References);

        let r = s.upsert(&[file_node("a")], &[good, bad]);
        assert!(r.is_err(), "指向不存在节点的边必须被外键拦下");
        let (nodes, edges) = s.counts().expect("数不过来");
        assert_eq!((nodes, edges), (0, 0), "一旦有错，整批都不许进去");
    }

    /// 写事务没提交时，**只读连接不该看见它**。
    ///
    /// 这条守的是读写分离的语义（数据层设计 §5）：写连接在忙的时候，
    /// 界面查询既不该被它卡住，也不该读到一半的东西。
    #[test]
    fn a_reader_does_not_see_an_uncommitted_write() {
        let p = tmp("isolation").join("g.db");
        let mut s = Store::open(&p).expect("开库失败");
        s.upsert(&[file_node("a")], &[]).expect("写失败");

        let tx = s.conn.transaction().expect("开事务失败");
        tx.execute(
            "INSERT INTO nodes(id,kind,name,source,created_at,updated_at)
             VALUES('file:b','file','b.rs','ts',1,1)",
            [],
        )
        .expect("事务里写失败");

        let reader = Store::open_readonly(&p).expect("只读打开失败");
        assert_eq!(
            reader.counts().expect("读失败").0,
            1,
            "未提交的写对读者不可见——读到一半的东西比读不到更糟"
        );

        tx.commit().expect("提交失败");
        assert_eq!(
            reader.counts().expect("读失败").0,
            2,
            "提交之后读者该看到它"
        );
    }

    #[test]
    fn fts5_is_available_in_the_bundled_build() {
        let p = tmp("fts5").join("g.db");
        let s = Store::open(&p).expect("开库失败");
        assert!(
            s.fts5_available(),
            "bundled 版必须带 FTS5——缺了的话搜索会悄悄坏掉，谁都查不出来"
        );
    }
}
