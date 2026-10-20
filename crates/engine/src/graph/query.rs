//! 图谱的**读回来那一层**：把"跑过一次"这件事问出答案。
//!
//! ## 为什么单独一个文件
//!
//! `store.rs` 管的是"怎么存、怎么取一个节点、怎么走一跳"——它是**机制**。
//! 这里管的是"问一个问题"——它是**查询**。两者的读者不一样：
//! 前者是索引器，后者是外壳与界面。混在一起的话，store 会慢慢长成一个什么都能问的巨物。
//!
//! ## 它补的是 M5 缺的后半截
//!
//! M5 那天只做了"记得下"：`launch()` 收尾时把运行写进图谱（`Run` 节点、
//! `Run --produced--> Diagnostic`、`Diagnostic --diagnosed_at--> File`）。
//! **但一个只有写、没有读的图谱等于没写**——库里躺着"上次跑失败是哪些问题"，
//! 而没有任何一条路径能把它问出来。
//!
//! 这一层就是那条路径。它回答的问题很具体：
//!
//! ```text
//! 上次跑的是什么命令？退出码多少？什么时候跑的？
//! 那次跑出了几条问题？各是什么？
//! 每条问题挂在哪个文件上？（没有落点是正常情形——不是每条诊断都带位置）
//! ```
//!
//! ## 三态在这里也要守住
//!
//! 一条诊断**没有落点**（`files` 为空）与**落点文件不在图谱里**（`unknown`）
//! 是两件不同的事——前者是"这条问题本来就没说在哪个文件"，后者是"它说了，
//! 但那个文件没索引过"。混成一句"没有文件"就把"我们的失败"伪装成了"世界的空白"。

use anyhow::{Context, Result};
use jingwei_model::{EdgeKind, Node, NodeKind, Verdict};
use std::path::Path;

use super::store::{Dir, Store};

/// 一次运行，加上它产生的诊断——图谱对"上次跑失败是哪些问题"的完整回答。
///
/// 只实现 `PartialEq` 不实现 `Eq`：里面的 `Verdict` 本来就只有 `PartialEq`
/// （它是三态，不是可全序比较的枚举）。跟着它走，不硬凑。
#[derive(Debug, Clone, PartialEq)]
pub struct RunAnswer {
    pub run_id: String,
    /// 跑的那条命令
    pub command: String,
    pub exit_code: i64,
    /// 运行节点是什么时候建的（毫秒）
    pub at: i64,
    pub diagnostics: Vec<DiagnosticAnswer>,
}

/// 一条诊断，以及它挂在哪。
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticAnswer {
    pub title: String,
    /// 落点：`Yes(id)` = 挂上了；`No` = 这条诊断本来就没说在哪个文件；
    /// `Unknown` = 它说了，但那个文件不在图谱里（没索引过）。
    ///
    /// **`No` 与 `Unknown` 必须分开**：前者是"这条问题没有位置"，
    /// 后者是"我们还没索引那个文件"。混起来就成了"查不了"伪装"没有"。
    pub file: Verdict<String>,
    /// **它原本说自己在哪个文件**（诊断节点 `data.target` 里的那份）。
    ///
    /// 为什么要单独带出来：`Verdict::Unknown` 是**不带数据**的（三态类型本身如此），
    /// 所以"它说在 `src/never_seen.rs`"这句**是人能看懂的关键信息**，
    /// 只能另开一个字段送到外壳。没有它的话，界面只能说"没索引到"，
    /// 而用户最想知道的是**哪个文件**没索引到。
    ///
    /// `None` = 这条诊断本来就没说位置（与 `file == Verdict::No` 是同一件事的两种表达，
    /// 一个给程序判断、一个给人读）。
    pub stated_file: Option<String>,
    /// **`file == Unknown` 时，那是谁造成的**（2026-10-16 接上）。
    ///
    /// 这一处的归属**在产出点就已经确定了**，不需要任何推断：
    /// 走到 `Unknown` 的唯一一条路是「诊断说了在某个文件，而那个文件不在图谱里」
    /// （见 `answer_run` 里那个 `match read_target`）——**它说了、我们没索引，是我们的**。
    ///
    /// 而"它本来就没说位置"走的是 `Verdict::No`（**用户的空白**），
    /// 三态本身已经把两者分开了。所以这里只是**把三态已经表达的东西
    /// 用一个外壳读得懂的字段带出去**——不然每个外壳（CLI / Tauri / 界面）
    /// 都要自己猜"Unknown 是谁的"，而那正是 32 处没接上的原因。
    pub unknown_why: crate::Unknown,
}

/// 最近若干次运行。**默认不返回诊断明细**——那要走边，是另一份钱。
///
/// 分开的理由和 `graph_stats` / `graph_dump` 分开是同一条：
/// 列表是"切到这一页就要看"的东西，必须便宜；明细是"点开某一条才要"的东西。
///
/// 但**条数要带上**："哪次失败、失败了几条"正是选择点开哪一条的依据，
/// 少了它这张列表就只能按时间盲选。走一跳边很便宜（实测正向 1 跳 0.00ms）。
/// **库里的全部符号**（file, name, id, class）。
///
/// 为什么需要：全局解析要用"整个项目有哪些名字"。增量时没变文件不会被重新解析，
    /// 于是只靠"这次解析出来的符号"去解析 → **跨文件调用边会丢**。
/// 这个查询让解析阶段永远看到完整的名字表。
pub fn symbol_rows(store: &Store) -> Result<Vec<(String, String, String, String)>> {
    let mut st = store
    .conn
        .prepare("SELECT file_path, name, id, data FROM nodes WHERE kind='symbol' AND file_path IS NOT NULL")
        .context("准备读符号表失败")?;
    let mut rows = st.query([]).context("读符号表失败")?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().context("读符号行失败")? {
        let file: String = r.get(0).unwrap_or_default();
        let name: String = r.get(1).unwrap_or_default();
        let id: String = r.get(2).unwrap_or_default();
        let data: Option<String> = r.get(3).unwrap_or_default();
        // class 存在 nodes.data.sym 里（parse 那边写的）
        let class = data
            .and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok())
            .and_then(|v| v.get("sym").and_then(|s| s.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "other".to_string());
        out.push((file, name, id, class));
    }
    Ok(out)
    }


/// 这个文件上次的**解析结果指纹**（没有就是 None）。
///
/// 用途：内容变了（改注释/格式）但解析结果一模一样时，**整条写库路径可以跳过**。
pub fn result_sha(store: &Store, path: &str) -> Result<Option<String>> {
    let mut st = store
        .conn
        .prepare("SELECT result_sha FROM file_results WHERE path=?1")
        .context("准备读结果指纹失败")?;
    let mut rows = st.query(rusqlite::params![path]).context("读结果指纹失败")?;
    Ok(rows.next().context("读结果指纹行失败")?.map(|r| r.get(0).unwrap_or_default()))
}

/// 记下这次解析结果的指纹（与文件状态同一个事务里写更安全，这里先分开）。
pub fn set_result_sha(store: &Store, path: &str, sha: &str, at: i64) -> Result<()> {
    store
        .conn
        .execute(
            "INSERT INTO file_results(path,result_sha,at) VALUES(?1,?2,?3) \
             ON CONFLICT(path) DO UPDATE SET result_sha=excluded.result_sha, at=excluded.at",
            rusqlite::params![path, sha, at],
        )
        .context("写结果指纹失败")?;
    Ok(())
}


pub fn recent_runs(store: &Store, limit: usize) -> Result<Vec<RunSummary>> {
    let nodes = store.list_kind(NodeKind::Run, limit)?;
    let mut out = Vec::with_capacity(nodes.len());
    for n in &nodes {
        let mut s = summarize(n);
        s.diagnostics = diagnostics_count(store, &n.id)?;
        out.push(s);
    }
    Ok(out)
}

/// 列表里那一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub run_id: String,
    pub command: String,
    pub exit_code: i64,
    pub at: i64,
    /// 这一次跑出了几条问题。**列表上就该看得见**：
    /// "哪次失败、失败了几条"是选择看哪一条的依据。
    pub diagnostics: usize,
}

fn summarize(n: &Node) -> RunSummary {
    let (command, exit_code) = read_run_data(n);
    RunSummary {
        run_id: n.id.clone(),
        // 命令为空时退回节点名（name 也是命令，data 是结构化副本）
        command: if command.is_empty() { n.name.clone() } else { command },
        exit_code,
        at: n.created_at,
        diagnostics: 0,
    }
}

/// `Run` 节点 `data` 里那两个字段。**读不出来不报错**——
/// 图谱可能比这个版本更老，历史节点要能原样读回来（`store::node` 的规矩）。
fn read_run_data(n: &Node) -> (String, i64) {
    let d = n.data.as_ref();
    let command = d
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let exit_code = d
        .and_then(|v| v.get("exit_code"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    (command, exit_code)
}

/// 这条诊断**原本说**自己在哪个文件（写作时存进 `data.target` 的那份）。
///
/// 读不到就是 `None`——那表示"它本来就没说位置"，与"说了但我们没查到"是两回事。
/// **不许拿标题去猜**：标题里那对括号是给人看的，不是给程序解析的。
fn read_target(d: &Node) -> Option<String> {
    d.data
        .as_ref()
        .and_then(|v| v.get("target"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 某一次运行的完整回答：它跑出了哪些问题、各挂在哪。
pub fn answer_run(store: &Store, run_id: &str) -> Result<Option<RunAnswer>> {
    let Some(run) = store.node(run_id).context("读运行节点失败")? else {
        return Ok(None);
    };
    if run.kind != NodeKind::Run {
        return Ok(None);
    }
    let (command, exit_code) = read_run_data(&run);

    let mut diagnostics = Vec::new();
    for d_id in store
        .neighbors(run_id, EdgeKind::Produced, Dir::Out)
        .context("查 Run --produced--> Diagnostic 失败")?
    {
        let Some(d) = store.node(&d_id).context("读诊断节点失败")? else {
            continue; // 边在节点不在，理论上被外键挡着；真发生也不该崩
        };
        let files = store
            .neighbors(&d_id, EdgeKind::DiagnosedAt, Dir::Out)
            .context("查 Diagnostic --diagnosed_at--> File 失败")?;
        let file = match files.first() {
            // 挂上了：这是"查到的事实"
            Some(f) => Verdict::Yes(f.clone()),
            None => {
                // 一条都没挂。**这里必须分两种情形**，而它们之前被混成了一种：
                //
                //   ① 这条诊断**本来就没说**是哪个文件（"缺包 requests"）→ 那是用户的空白，回 `No`；
                //   ② 它**说了**，但那个文件不在图谱里 → 那是**我们没查到**，回 `Unknown`。
                //
                // 判据落在**数据**上：写入时把"它说自己在哪个文件"存进了诊断节点的
                // `data.target`（`graph::index::record_run`）。原来没存，于是这里只能一律
                // 回 `No`——界面上那句「（这条问题没有说是哪个文件）」对第 ② 种就是在撒谎，
                // 而 CLI / Tauri / 界面里**为第 ② 种写的那三支渲染全是死代码**。
                //
                // 这正是 `AGENTS.md` §2 那条招牌不变量在查询层**没有生产者**：
                // "`unknown` 永远不许显示成'没有'"。2026-10-16 外部对照复核抓出来的。
                //
                // `Verdict::Unknown` 是**不带数据**的（三态类型本身如此），所以"原本说的是哪个
                // 文件"另开一个字段带给外壳——见 `DiagnosticAnswer::stated_file`。
                match read_target(&d) {
                    Some(_) => Verdict::Unknown,
                    // 真的没说位置——如实回"没有"，不猜。
                    None => Verdict::No,
                }
            }
        };
        // 三态已经分好了：`No` = 它本来就没说位置（用户的空白）；
        // `Unknown` = 它说了、我们没索引（我们的失败）；`Yes` = 挂上了。
        // **先判断再 move**（`file` 会被移进结构体）。
        let unknown_why = if matches!(file, Verdict::Unknown) {
            crate::Unknown::OurGap
        } else {
            crate::Unknown::Unclear
        };
        diagnostics.push(DiagnosticAnswer {
            title: d.name.clone(),
            file,
            stated_file: read_target(&d),
            unknown_why,
        });
    }

    Ok(Some(RunAnswer {
        run_id: run.id.clone(),
        command: if command.is_empty() { run.name.clone() } else { command },
        exit_code,
        at: run.created_at,
        diagnostics,
    }))
}

/// 最近一次运行。界面最常问的就是这一句。
pub fn last_run(store: &Store) -> Result<Option<RunSummary>> {
    Ok(recent_runs(store, 1)?.into_iter().next())
}

/// 一次运行的诊断条数——列表那一栏要用，所以单独一条（不用把明细全取回来）。
pub fn diagnostics_count(store: &Store, run_id: &str) -> Result<usize> {
    Ok(store
        .neighbors(run_id, EdgeKind::Produced, Dir::Out)
        .context("数诊断条数失败")?
        .len())
}

/// 打开某个项目的图谱库**只读**。查询一律走这条，写走 `Store::open`。
///
/// 查询不需要建库：项目还没索引过时这里会失败，那是**正常情形**，
/// 由调用方转成"还没建过图谱"（`no`），而不是报成错误。
pub fn open_for_query(root: &Path) -> Result<Store> {
    Store::open_readonly(&crate::graph_db_path(root))
}


/// **一个符号到底是什么**（2026-10-20）：定义在哪、被谁调用、被谁引用。
///
/// 为什么放引擎侧：CLI 拿不到 `Store.conn`（故意的），而这条查询只读图谱，
/// 属于"理解层"的读路径。
pub struct Explain {
    pub id: String,
    pub name: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub callers: Vec<(String, i64)>,   // (文件, 行)
    pub references: Vec<(String, i64)>,
    pub calls: i64,
    pub refs: i64,
}

pub fn explain_symbol(store: &Store, id: &str) -> Result<Option<Explain>> {
    let row = store
        .conn
        .query_row(
            "SELECT name, file_path, line FROM nodes WHERE id=?1 AND kind='symbol'",
            rusqlite::params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .ok();
    let Some((name, file, line)) = row else {
        return Ok(None);
    };
    let callers = sites_for(store, id, "calls")?;
    let references = sites_for(store, id, "references")?;
    let calls = count_edges(store, id, "calls")?;
    let refs = count_edges(store, id, "references")?;
    Ok(Some(Explain { id: id.to_string(), name, file, line, callers, references, calls, refs }))
}

fn sites_for(store: &Store, id: &str, kind: &str) -> Result<Vec<(String, i64)>> {
    let mut st = store.conn.prepare(
        "SELECT s.file, s.line FROM edge_sites s                          JOIN edges e ON e.src=s.src AND e.dst=s.dst AND e.kind=s.kind                          WHERE s.dst=?1 AND s.kind=?2 AND e.expires_at IS NULL                          ORDER BY s.file, s.line LIMIT 50",
    )?;
    let rows = st.query_map(rusqlite::params![id, kind], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    Ok(rows.flatten().collect())
}

fn count_edges(store: &Store, id: &str, kind: &str) -> Result<i64> {
    Ok(store.conn.query_row(
        "SELECT count(*) FROM edges WHERE dst=?1 AND kind=?2 AND expires_at IS NULL",
        rusqlite::params![id, kind],
        |r| r.get(0),
    )?)
}
