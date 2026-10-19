//! **写边**（2026-10-19，债表 0.24）：把"从解析结果建哪几种关系"从 `index.rs` 搬出来。
//!
//! 为什么单独一个文件：`index.rs` 已经贴到 1000 行上限（登记在 `gate.mjs` 的
//! `KNOWN_LONG` 里）。它混着"怎么走目录/怎么截断/怎么写库"三件事，
//! 而"每种关系一条边"是**可以按种类拆开**的第四件——拆出来两边都更好读。
//!
//! 纪律不变：**关系必有来源**。这里每条边都带 `Source`；两端类型由
//! `Edge::new` 的合法矩阵当场把关，写错就是编译期/运行期报错，不会留幽灵边。

use anyhow::Result;
use jingwei_model::{Confidence, Edge, EdgeKind, NodeKind, Source};

// 参数多是因为它如实镜像 `Edge::new` 的签名——多一层参数对象只会让对应关系更难对上
// （与 `Edge::new` 自己的 `#[allow]` 同一个理由）。
#[allow(clippy::too_many_arguments)]
fn edge(
    src: &str,
    dst: &str,
    kind: EdgeKind,
    src_kind: NodeKind,
    dst_kind: NodeKind,
    source: Source,
    confidence: Confidence,
    now: i64,
) -> Result<Edge> {
    Edge::new(src, dst, kind, src_kind, dst_kind, source, confidence, now).map_err(|bad| {
        anyhow::anyhow!(
            "关系类型用错了：{} 不允许 {} → {}（两端合法矩阵拒绝了它）",
            bad.kind.as_str(),
            bad.src.as_str(),
            bad.dst.as_str()
        )
    })
}

/// **调用**：文件内能确定的那部分（裸标识符 + 本文件有定义）。
///
/// `caller` 可能是**文件本身**（Python 的 `if __name__ == "__main__":` 块）——
/// 那种边是 File→Symbol，矩阵为它开了一格（D-20261019-02）。
pub fn calls_into(edges: &mut Vec<Edge>, calls: &[(String, String)], now: i64) -> Result<usize> {
    let mut n = 0;
    for (caller, callee) in calls {
        let src_kind = if caller.starts_with("file:") {
            NodeKind::File
        } else {
            NodeKind::Symbol
        };
        edges.push(edge(
            caller,
            callee,
            EdgeKind::Calls,
            src_kind,
            NodeKind::Symbol,
            Source::TreeSitter,
            Confidence::High,
            now,
        )?);
        n += 1;
    }
    Ok(n)
}

/// **属性访问**（债表 0）：`Cfg.DEBUG` 这种"只用不调" → references 边。
pub fn references_into(
    edges: &mut Vec<Edge>,
    refs: &[(String, String)],
    now: i64,
) -> Result<usize> {
    let mut n = 0;
    for (caller, target) in refs {
        edges.push(edge(
            caller,
            target,
            EdgeKind::References,
            NodeKind::Symbol,
            NodeKind::Symbol,
            Source::TreeSitter,
            Confidence::High,
            now,
        )?);
        n += 1;
    }
    Ok(n)
}

/// **装饰器注册**：`@app.route("/")` 这类把函数注册给外部框架。
/// 不混进 `calls`——否则角色的入/出度会被路由函数带偏。
pub fn registers_into(
    edges: &mut Vec<Edge>,
    regs: &[(String, String)],
    now: i64,
) -> Result<usize> {
    let mut n = 0;
    for (file_id, sym_id) in regs {
        edges.push(edge(
            file_id,
            sym_id,
            EdgeKind::Registers,
            NodeKind::File,
            NodeKind::Symbol,
            Source::TreeSitter,
            Confidence::High,
            now,
        )?);
        n += 1;
    }
    Ok(n)
}
