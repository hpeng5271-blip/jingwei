//! 图谱那几条命令：内核里那套东西的对外接口。
//!
//! 四条命令，对应图谱链的四段：
//!
//! | 命令 | 干什么 | 贵不贵 |
//! |---|---|---|
//! | `graph_index` | 索引（扫 → 解析 → 写库 → 全局解析） | 几百毫秒 |
//! | `graph_stats` | 规模（多少节点、多少边） | 毫秒级，界面可以频繁问 |
//! | `graph_dump` | 数据（给可视化用，**天生带上限**，超了如实说截断） | 看 limit |
//! | `graph_runs` | 运行历史（M5 那条链的出口）：列表 + 某一次的明细 | 一跳边，很便宜 |
//!
//! **为什么"规模"要单独一条**：它是"看状态"，`graph_index` 是"干活"。
//! 混成一条的话，状态栏每刷新一次就重扫一遍全项目。
//!
//! **为什么单独一个文件**（2026-10-16 拆）：`main.rs` 加了 `graph_runs` 之后撞了
//! 门禁的 1000 行上限。这次拆分落在一条真实接缝上——这四条命令是**同一件事的四个深度**，
//! 而 `main.rs` 剩下的部分是"窗口、对话框、编辑器、笔记、清理"那一摊。
//! 门禁的行数上限这已经是第四次逼出正确的拆分。

use jingwei_engine::envelope::Envelope;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 图谱库的位置（跟项目走，可删）。
///
/// **注意**：真正的唯一入口在引擎那边（`jingwei_engine::graph_db_path`），
/// 这里只是把它包一层方便本模块用。凡是能直接调引擎的（比如查询），
/// 一律用引擎那条（`graph::query::open_for_query`），别在这里各拼一遍。
pub(crate) fn graph_db(root: &Path) -> PathBuf {
    jingwei_engine::graph_db_path(root)
}

/// 索引一次：扫 → 解析 → 写库 → 全局解析。
#[tauri::command]
pub(crate) fn graph_index(root: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{root}"));
    }
    let t0 = std::time::Instant::now();
    let mut store = jingwei_engine::graph::store::Store::open(&graph_db(&p))
        .map_err(|e| format!("打开图谱库失败：{e}"))?;
    let rep = jingwei_engine::graph::index::index_source(&p, &mut store)
        .map_err(|e| format!("索引失败：{e}"))?;
    // **形状由 `IndexReport::to_json()` 给**（2026-10-16）——与 `jingwei index --json` 同源。
    // 原来这里与 `cmd_graph.rs` 各拼一份，而它们已经漂了（字段缺一个、
    // `skippedUnchanged` 的口径还不一样）。**两份各写各的，必然漂移。**
    let mut v = rep.to_json();
    if let Some(o) = v.as_object_mut() {
        o.insert("ms".into(), json!(t0.elapsed().as_millis() as u64));
    }
    Ok(Envelope::yes(v).to_value())
}

/// 看规模。**只读打开**——状态栏问得频繁，不该跟写者抢锁。
#[tauri::command]
pub(crate) fn graph_stats(root: String) -> Value {
    let p = PathBuf::from(&root);
    let db = graph_db(&p);
    if !db.is_file() {
        // 还没索引过——**这不是错误**，是"你还没让它建过图"。
        // 走 `no` 而不是 `unknown`：我们确知它不存在。
        return Envelope::<Value>::no("这个项目还没建过图谱（点一下索引就行）").to_value();
    }
    match jingwei_engine::graph::store::Store::open_readonly(&db) {
        Ok(s) => match s.counts() {
            Ok((nodes, edges)) => Envelope::yes(json!({ "nodes": nodes, "edges": edges })).to_value(),
            Err(e) => Envelope::<Value>::unknown(format!("读图谱规模失败：{e}")).to_value(),
        },
        Err(e) => Envelope::<Value>::unknown(format!("打开图谱库失败：{e}")).to_value(),
    }
}

/// 给可视化用的数据。**天生带上限**——超了截断并如实说。
#[tauri::command]
pub(crate) fn graph_dump(root: String, limit: Option<usize>) -> Value {
    let p = PathBuf::from(&root);
    let db = graph_db(&p);
    if !db.is_file() {
        return Envelope::<Value>::no("这个项目还没建过图谱").to_value();
    }
    match jingwei_engine::graph::store::Store::open_readonly(&db) {
        Ok(s) => match s.dump(limit.unwrap_or(2000)) {
            Ok(v) => Envelope::yes(v).to_value(),
            Err(e) => Envelope::<Value>::unknown(format!("导出图谱失败：{e}")).to_value(),
        },
        Err(e) => Envelope::<Value>::unknown(format!("打开图谱库失败：{e}")).to_value(),
    }
}

/// M5 的后半截：**把"跑过一次"问出来**。
///
/// 不传 `runId` → 最近若干次运行的列表（命令、退出码、出了几条问题）。
/// 传 `runId`   → 那一次的明细（每条问题是什么、挂在哪）。
///
/// 为什么合成一条命令而不是两条：界面上它们是同一件事的两个深度
/// （列表 → 点开某一条），分两条会让前端多一次 invoke 的往返，也容易忘记传 root。
#[tauri::command]
pub(crate) fn graph_runs(root: String, run_id: Option<String>, limit: Option<usize>) -> Value {
    let p = PathBuf::from(&root);
    // 查询一律走引擎那条只读入口：路径怎么拼只有一处实现。
    let store = match jingwei_engine::graph::query::open_for_query(&p) {
        Ok(s) => s,
        Err(_) => {
            // 打不开多半是"还没建过图谱"——**这不是错误**，走 `no`。
            return Envelope::<Value>::no("这个项目还没建过图谱（点一下「索引一次」）").to_value();
        }
    };
    match run_id {
        Some(id) => match jingwei_engine::graph::query::answer_run(&store, &id) {
            // 查不到就如实说没有这次运行，**不返回空答案**：
            // "没有这次运行"和"这次运行没有问题"是两件事。
            Ok(None) => Envelope::<Value>::no("图谱里没有这次运行").to_value(),
            Ok(Some(a)) => Envelope::yes(json!({
                "runId": a.run_id,
                "command": a.command,
                "exitCode": a.exit_code,
                "at": a.at,
                "diagnostics": a.diagnostics.iter().map(|d| json!({
                    "title": d.title,
                    // 三态照原样给前端：`yes` 带落点、`no` 是"这条问题本来就没位置"、
                    // `unknown` 是"它说了但我们没索引那个文件"。混成一句就是撒谎。
                    "file": match &d.file {
                        jingwei_engine::Verdict::Yes(id) => json!({ "state": "yes", "data": id }),
                        // `No` = 它本来就没说位置 → **用户的空白**（这条问题就是没落点）
                        jingwei_engine::Verdict::No => json!({
                            "state": "no",
                            "reason": "这条问题没有说是哪个文件",
                            "why": "their_gap",
                        }),
                        // `unknown` 这一支在 2026-10-16 之前是**死代码**：查询层从来没产出过它。
                        // 现在带上了 `stated`——**只说"没索引到"而不说哪个文件，用户没法行动。**
                        // 归属（`why`）由引擎带出来（`d.unknown_why`），**不在这里猜**。
                        jingwei_engine::Verdict::Unknown => json!({
                            "state": "unknown",
                            "reason": "它说了是哪个文件，但那个文件不在图谱里",
                            "stated": d.stated_file,
                            "why": "our_gap",
                            "whatToDo": d.unknown_why.what_to_do(),
                        }),
                    },
                })).collect::<Vec<_>>(),
            }))
            .to_value(),
            Err(e) => Envelope::<Value>::unknown(format!("查这次运行失败：{e}")).to_value(),
        },
        None => match jingwei_engine::graph::query::recent_runs(&store, limit.unwrap_or(20)) {
            Ok(list) => Envelope::yes(json!({
                "runs": list.iter().map(|r| json!({
                    "runId": r.run_id,
                    "command": r.command,
                    "exitCode": r.exit_code,
                    "at": r.at,
                    "diagnostics": r.diagnostics,
                })).collect::<Vec<_>>(),
            }))
            .to_value(),
            Err(e) => Envelope::<Value>::unknown(format!("列运行记录失败：{e}")).to_value(),
        },
    }
}
