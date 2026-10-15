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
    /// 抽出来的**文件内调用关系**条数。
    /// 跨文件的那部分还没做——它要等"解析期"（设计 §6.2.1）。
    pub calls: usize,
    /// **解析不了**的引用条数。这个数字要显示出来：
    /// 界面上该能回答"这张图漏了多少"，而不是让人以为"就这些调用"。
    pub unresolved_refs: usize,
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
    /// **解析期连上的跨文件关系**（含就近遮蔽命中的）。
    pub resolved: usize,
    /// 命中多个候选、**不猜**的引用数。
    pub ambiguous: usize,
    /// **外部符号**（标准库 / 别的 crate / 宏生成）——不是错误，是不归我们管。
    pub external: usize,
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

    // ── 解析期的两份料 ──────────────────────────────────────────────
    //
    // 索引期每个文件独立处理（那是增量的前提），所以那时说不出"这个名字指谁"。
    // 这一遍把料攒起来，等所有文件都进了库再做全局解析。
    //
    // **两份表，用途不同**：
    // - `file_tables`：这个文件里有哪些名字 → 就近解析（遮蔽规则）
    // - `all_names`：全图谱有哪些名字 → 跨文件解析（只在唯一命中时连）
    let mut file_tables: std::collections::BTreeMap<String, crate::graph::resolve::SymbolTable> =
        std::collections::BTreeMap::new();
    let mut all_names: crate::graph::resolve::SymbolTable =
        std::collections::BTreeMap::new();
    let mut pending_refs: Vec<(String, Vec<crate::graph::parse::RefFact>)> = Vec::new();

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
        rep.unresolved_refs += parsed.references.len();
        // 攒料：这个文件的名字表 + 全图谱的名字表 + 它的引用事实
        {
            let ft = file_tables.entry(rel.clone()).or_default();
            for s in &parsed.symbols {
                ft.entry(s.name.clone()).or_default().push(s.id.clone());
                all_names
                    .entry(s.name.clone())
                    .or_default()
                    .push(s.id.clone());
            }
        }
        if !parsed.references.is_empty() {
            pending_refs.push((rel.clone(), parsed.references.clone()));
        }
        // 调用关系单独取出来——`symbols` 下面会被 move 走。
        let calls = parsed.calls.clone();

        let mut nodes: Vec<Node> = Vec::with_capacity(symbols + 1);
        let mut edges: Vec<Edge> = Vec::with_capacity(symbols + 1 + calls.len());
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
        // **调用关系**：只收文件内能确定的那部分（裸标识符且本文件有定义）。
        // 跨文件的要等"解析期"——见设计 §6.2.1。
        for (caller, callee) in calls {
            edges.push(edge(
                &caller,
                &callee,
                EdgeKind::Calls,
                NodeKind::Symbol,
                NodeKind::Symbol,
                Source::TreeSitter,
                Confidence::High,
                now,
            )?);
            rep.calls += 1;
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

    // ── 阶段 2：全局解析 ────────────────────────────────────────────
    //
    // 到这里所有节点都进库了，才有资格回答"这个名字指谁"。
    // **自下而上**：从每条引用出发去找定义（rust-analyzer 讲的 IDE 做法），
    // 而不是从头把每个文件再遍历一遍（那是编译器的做法）。
    if !pending_refs.is_empty() {
        let empty = crate::graph::resolve::SymbolTable::new();
        let mut extra: Vec<Edge> = Vec::new();
        for (rel, refs) in &pending_refs {
            let ft = file_tables.get(rel).unwrap_or(&empty);
            let res = crate::graph::resolve::resolve_file(rel, refs, ft, &all_names);
            rep.resolved += res.resolved.len();
            rep.ambiguous += res.ambiguous;
            rep.external += res.external;
            extra.extend(crate::graph::resolve::to_edges(&res, now));
        }
        // 只写边不写节点——节点在上面的循环里已经落库了。
        // 外键会在这一句把关：指向不存在节点的边进不去。
        if !extra.is_empty() {
            let w = store.upsert(&[], &extra)?;
            rep.edges_written += w.edges;
        }
    }

    Ok(rep)
}
