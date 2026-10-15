//! 把「一个目录」变成图谱里的一批节点与边。M3 接图谱的那一步。
//!
//! 这是**第一次让图谱真的有自己的东西**——在这之前，"这个项目里有什么"每次都靠重扫；
//! 从这一步起，它落在库里，下次可以直接查。
//!
//! ## 三条边界（都是为了避免"一口气吃下整个硬盘"）
//!
//! 1. **只收 `.rs`**（M3 只做 Rust，第二门语言要等抽象被验证过——设计 §1.2）；
//! 2. **文件数有上限**：超了就停，并把 `truncated` 说出来（**不许静默**）；
//! 3. **单个文件太大就跳过**：生成物、压缩进去的数据文件不该进解析器。

use anyhow::{anyhow, Result};
use jingwei_model::{Confidence, Edge, EdgeKind, Node, NodeKind, Source};
use std::path::{Path, PathBuf};

/// 一次索引干了什么。**数字要能报到界面上**——"索引完了"不算信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexReport {
    pub files: usize,
    pub symbols: usize,
    /// 解析不完整（语法树里带错误）的文件数。**它们仍然会进图谱**（符号是真的），
    /// 只是调用方据此知道"这些文件的调用关系不可信"。
    pub incomplete: usize,
    pub skipped_big: usize,
    pub unreadable: usize,
    /// 撞到文件数上限了。
    pub truncated: bool,
    /// **一级检测**说"没变"（大小与修改时间都一样），连文件都没读。
    pub skipped_unchanged: usize,
    /// **二级检测**说"没变"（改了修改时间，但内容哈希一样——比如 `touch` 了一下）。
    pub skipped_same_content: usize,
    /// 磁盘上已经没有了、从图谱里撤掉的文件。
    pub removed: usize,
    pub nodes_written: usize,
    pub edges_written: usize,
    /// 这次更新里被标成失效的旧关系条数。
    pub edges_expired: usize,
}

const MAX_FILES: usize = 2000;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// 把一个 `IllegalPair` 变成可读的错误。
///
/// 走到这里说明**代码里写错了关系类型**（比如拿 `calls` 去连 File→File）——
/// 那是编译期该发现的问题，没能拦住就得说清楚是哪一条，而不是 `.unwrap()` 崩掉。
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
        // 用 `IllegalPair` 自己的字段，而不是把三个值再抄一遍——
        // 抄一遍就是同一事实的第二个副本，迟早对不上。
        anyhow!(
            "关系类型用错了：{} 不允许 {} → {}（两端合法矩阵拒绝了它）",
            bad.kind.as_str(),
            bad.src.as_str(),
            bad.dst.as_str()
        )
    })
}

fn collect_rust_files(root: &Path, rep: &mut IndexReport) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 12 || out.len() >= MAX_FILES {
            if out.len() >= MAX_FILES {
                rep.truncated = true;
            }
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                // 网络盘、联接点、超长路径——同一道闸，别处用过就也在这儿用
                if crate::scan::should_skip(&p, root).is_some() {
                    continue;
                }
                if matches!(
                    name.as_str(),
                    "target" | "node_modules" | ".git" | "dist" | "build" | ".venv" | "venv"
                ) {
                    continue;
                }
                stack.push((p, depth + 1));
            } else if name.ends_with(".rs") {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push((
                        rel.to_string_lossy().replace('\\', "/"),
                        p,
                    ));
                }
            }
        }
    }
    out.sort();
    out
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 一次运行记下来的东西。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunRecorded {
    pub run_id: String,
    pub diagnostics: usize,
    /// 有诊断指向、但**图谱里还没有**的文件数（没索引过就挂不上去）。
    /// 它不是错误，是"这次没记全"——要能说出来，不能假装记全了。
    pub unknown_targets: usize,
}

/// 记一次运行。M5 的落点：**让"跑过一次"这件事在图谱里留下痕迹**。
///
/// 三条边各回答一个问题（矩阵里对应的组合都验过）：
///
/// - `Run --follows--> Contract`：这次跑的是哪份契约
/// - `Run --produced--> Diagnostic`：这次跑出了哪些问题（**Q11 补的那条边**）
/// - `Diagnostic --diagnosed_at--> File`：每一条判在哪个文件上
///
/// **文件不在库里就挂不上去**（外键会拒）——这不是缺陷，是保护：
/// 没索引过的文件不该凭空出现在图谱里。这种情形记进 `unknown_targets`，
/// 由调用方决定要不要先补一次索引。
pub fn record_run(
    store: &mut crate::store::Store,
    root: &Path,
    command: &str,
    exit_code: i32,
    diagnostics: &[(String, Option<String>)],
    now: i64,
) -> Result<RunRecorded> {
    let mut out = RunRecorded::default();
    let project_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    let project = Node::new(
        NodeKind::Project,
        ".",
        project_name,
        Source::Meta,
        Confidence::Certain,
        now,
    );
    // 单独跑一次（没有先索引过）也要能记下来，所以项目节点先落一次。
    store.upsert(std::slice::from_ref(&project), &[])?;

    let key = format!("{now}-{}", crate::facts::stable_hash([command]));
    let mut run = Node::new(
        NodeKind::Run,
        &key,
        command,
        Source::SelfTool,
        Confidence::Certain,
        now,
    );
    // 进程自己产生的记录：命令与退出码放 data（该类实体独有的字段）
    run.data = Some(serde_json::json!({ "command": command, "exit_code": exit_code }));

    let mut nodes = vec![run.clone()];
    let mut edges = Vec::new();
    for (i, (title, target)) in diagnostics.iter().enumerate() {
        let d = Node::new(
            NodeKind::Diagnostic,
            &format!("{key}-{i}"),
            title,
            Source::SelfTool,
            Confidence::Certain,
            now,
        );
        edges.push(edge(
            &run.id,
            &d.id,
            EdgeKind::Produced,
            NodeKind::Run,
            NodeKind::Diagnostic,
            Source::SelfTool,
            Confidence::Certain,
            now,
        )?);
        if let Some(rel) = target {
            let fid = jingwei_model::node_id(NodeKind::File, rel);
            if store.node(&fid)?.is_some() {
                edges.push(edge(
                    &d.id,
                    &fid,
                    EdgeKind::DiagnosedAt,
                    NodeKind::Diagnostic,
                    NodeKind::File,
                    Source::SelfTool,
                    Confidence::Certain,
                    now,
                )?);
            } else {
                out.unknown_targets += 1;
            }
        }
        nodes.push(d);
        out.diagnostics += 1;
    }
    store.upsert(&nodes, &edges)?;
    out.run_id = run.id;
    Ok(out)
}

/// 文件修改时间的纳秒数。
///
/// 用纳秒而不是秒：**秒级精度会把"同一秒里改两次"看成没变**，
/// 而"保存一下又改一下"正是最常见的编辑节奏。
fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// **增量索引**：只重解析变了的文件，并把消失的文件撤掉。
///
/// 三级检测（数据层设计 §6.2）在这里落地：
///
/// | 级 | 判据 | 什么时候用 |
/// |---|---|---|
/// | 一级 | 大小 + 修改时间都一样 | 每次都先用它筛——**不读文件** |
/// | 二级 | 内容哈希 | 一级说"变了"时才花这个钱（`touch` 一下不该触发重解析）|
///
/// 三级（变更日志 / watcher）不在这里：那是外壳的事，它把改动事件喂进来，
/// 这里只管"给我一个目录，我把图谱对齐"。
pub fn index_rust(
    root: &Path,
    store: &mut crate::store::Store,
) -> Result<IndexReport> {
    let now = now_ms();
    let mut rep = IndexReport::default();

    let project_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    let project = Node::new(
        NodeKind::Project,
        ".",
        project_name,
        Source::Meta,
        Confidence::Certain,
        now,
    );
    // 项目节点先落一次（幂等）。它没有"文件"，所以不走下面按文件的更新流程。
    store.upsert(std::slice::from_ref(&project), &[])?;

    let files = collect_rust_files(root, &mut rep);
    let seen: std::collections::BTreeSet<&str> = files.iter().map(|(r, _)| r.as_str()).collect();

    // 磁盘上已经没有的：撤掉（节点 + 状态行，边由级联带走）
    for known in store.indexed_files()? {
        if !seen.contains(known.as_str()) {
            let n = store.drop_file(&known)?;
            if n > 0 {
                rep.removed += 1;
            }
        }
    }

    for (rel, abs) in files {
        let Ok(meta) = std::fs::metadata(&abs) else {
            rep.unreadable += 1;
            continue;
        };
        let size = meta.len() as i64;
        let mt = mtime_ns(&meta);
        let known = store.file_state(&rel)?;

        // 一级：大小与时间都没动 → 连读都不读
        if let Some(k) = &known {
            if k.size == size && k.mtime_ns == mt {
                rep.skipped_unchanged += 1;
                continue;
            }
        }
        if size as u64 > MAX_FILE_BYTES {
            rep.skipped_big += 1;
            continue;
        }
        let Ok(bytes) = std::fs::read(&abs) else {
            rep.unreadable += 1;
            continue;
        };
        let file = crate::textfile::decode(&bytes);
        if !file.readable {
            rep.unreadable += 1;
            continue;
        }
        // 二级：内容哈希。`touch` 一下会让一级判"变了"，这里把它挡回去。
        let sha = crate::facts::stable_hash([&file.text]);
        if let Some(k) = &known {
            if k.content_sha.as_deref() == Some(sha.as_str()) {
                rep.skipped_same_content += 1;
                continue;
            }
        }

        let base = abs
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| rel.clone());
        let file_node = Node::new(
            NodeKind::File,
            &rel,
            base,
            Source::Meta,
            Confidence::Certain,
            now,
        );
        rep.files += 1;

        let parsed = crate::parse::parse_rust(&rel, &file.text, now);
        if !parsed.complete {
            rep.incomplete += 1;
        }
        let symbols = parsed.symbols.len();
        rep.symbols += symbols;
        let keep: Vec<String> = parsed.symbols.iter().map(|s| s.id.clone()).collect();

        let mut nodes: Vec<Node> = Vec::with_capacity(symbols + 1);
        let mut edges: Vec<Edge> = Vec::with_capacity(symbols + 1);
        edges.push(edge(
            &project.id,
            &file_node.id,
            EdgeKind::Contains,
            NodeKind::Project,
            NodeKind::File,
            Source::Meta,
            Confidence::Certain,
            now,
        )?);
        for s in parsed.symbols {
            edges.push(edge(
                &file_node.id,
                &s.id,
                EdgeKind::Defines,
                NodeKind::File,
                NodeKind::Symbol,
                Source::TreeSitter,
                Confidence::High,
                now,
            )?);
            nodes.push(s);
        }
        nodes.push(file_node);

        let state = crate::store::FileState {
            size,
            mtime_ns: mt,
            content_sha: Some(sha),
            nodes_count: Some(symbols as i64),
        };
        let w = store.apply_file_update(
            &rel,
            &state,
            &nodes,
            &edges,
            &keep,
            now,
            !parsed.complete,
        )?;
        rep.nodes_written += w.nodes;
        rep.edges_written += w.edges;
        rep.edges_expired += w.expired;
    }

    Ok(rep)
}
