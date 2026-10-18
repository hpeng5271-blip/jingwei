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
    /// **这次真正处理（解析 + 写库）的文件数。**
    ///
    /// ⚠️ **它不等于"项目里有几个文件"**——没变的那些被三级检测跳过了
    /// （见 `skipped_unchanged` / `skipped_same_content`）。
    /// 想知道项目规模要看 `scanned` 或 `store.indexed_files()`。
    pub files: usize,
    /// **这次扫到的文件总数**（收集阶段的结果，不管变没变）。
    ///
    /// 2026-10-16 加。原来只有一个 `files`，而外壳把它当"项目有几个文件"报出去——
    /// 于是**增量的一次普通运行会显示「1 个文件 · 10 个符号」**，
    /// 看起来像"这个项目只有一个文件"。实测就是这个症状：
    /// `jingwei index` 报 1 个而 `export_graph`（临时空库、全部处理）报 91 个，
    /// 差别**不是我漏了输入，是增量检测在正常工作**。
    pub scanned: usize,
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
    /// 命中多个候选、**不猜**的引用（记名字，不记个数——个数解释不了任何事）。
    pub ambiguous: Vec<String>,
    /// **外部符号**（标准库 / 别的 crate / 宏生成）——不是错误，是不归我们管。
    pub external: usize,
    /// **写了 `use` 却没接上**的名字 + 位置（`super::` 相对路径、重导出链、
    /// 多 crate 同名文件……）。这一类是**我们自己的漏接**，不许伪装成"外部"。
    pub unmapped_imports: Vec<String>,
    /// 名字在图谱里、但不在值命名空间——调用点写错了，或者我们的命名空间判断没跟上。
    pub namespace_mismatch: Vec<String>,
}

impl IndexReport {
    /// **一份 JSON，两个出口**（CLI 与桌面界面）（2026-10-16）。
    ///
    /// ## 为什么要把它放在这儿，而不是各写各的
    ///
    /// 原来 CLI（`cmd_graph.rs`）与 Tauri（`src-tauri/src/graph.rs`）**各拼一份**，
    /// 而它们**已经在漂了**——实测差异：
    ///   - CLI 有 `unmappedImports` / `namespaceMismatch`，**界面那份没有**；
    ///   - `skippedUnchanged` 的口径不同：CLI 只算一级（`skipped_unchanged`），
    ///     界面把二级也算上（`+ skipped_same_content`）——**两个数说的不是一件事**。
    ///
    /// 这与 `--help` 漏列 4 个命令、说明书承诺一个不存在的 `inspect` 是**同一个形态**：
    /// **两份清单各写各的，必然漂移。** 而这一处比那个更隐蔽——
    /// 两边都是"能跑通的 JSON"，只是字段不一样，**没有任何东西会红**。
    ///
    /// ## 一条口径
    ///
    /// `skippedUnchanged` = **一级 + 二级**（"没变，所以我们没读它"）。
    /// 二级（内容哈希一样、只是 `touch` 过）在用户眼里同样是"没变"，
    /// 分开报只会让人以为漏了一批文件。
    ///
    /// `unmappedImports` / `namespaceMismatch` 报**长度**：名字列表是给
    /// `jingwei index` 的文字输出用的，JSON 这边只需要知道"有几处"。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            // ⚠️ **`files` / `symbols` 是"这次处理了几个"，不是"项目有几个"。**
            // 增量检测会跳过没变的文件，所以**第二次索引时它们就是 0**——
            // 只说这两个数，用户会以为图谱被清空了（而它其实好好的）。
            // `scanned` 才是"扫到多少"。
            "files": self.files,
            "scanned": self.scanned,
            "skippedUnchanged": self.skipped_unchanged + self.skipped_same_content,
            "symbols": self.symbols,
            "calls": self.calls,
            "incomplete": self.incomplete,
            "truncated": self.truncated,
            "unreadable": self.unreadable,
            // 解析期的三个数**分开报**——它们的说法完全不同：
            // "连上"是我们接住了，"拿不准"是命中了多个候选所以不猜，
            // "外部"是不归我们管。合成一个数等于把三种情况压扁。
            "resolved": self.resolved,
            "ambiguous": self.ambiguous.len(),
            "external": self.external,
            // **我们自己漏的那两类**：不许省——它们与"外部"是两件事，
            // 少了它们，"外部 N 个"里就混着我们自己的漏接。
            "unmappedImports": self.unmapped_imports.len(),
            "namespaceMismatch": self.namespace_mismatch.len(),
        })
    }
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
                if crate::util::scan::should_skip(&p, root).is_some() {
                    continue;
                }
                // 判法只有一处（`util::scan::is_not_source_dir`，内部会转小写）——
                // 原来这里直接比 `name.as_str()`，于是 Windows 上 `Target/` 漏过去了。
                if crate::util::scan::is_not_source_dir(&name) {
                    continue;
                }
                stack.push((p, depth + 1));
            } else if name.ends_with(".rs") || name.ends_with(".py") {
                match p.strip_prefix(root) {
                    Ok(rel) => out.push((
                        rel.to_string_lossy().replace('\\', "/"),
                        p,
                    )),
                    // **不静默丢掉**：`strip_prefix` 失败时至少把它记进 `unreadable`，
                    // 否则"少收了文件"这件事一声不响（2026-10-16 查收集器时加）。
                    Err(_) => rep.unreadable = rep.unreadable.saturating_add(1),
                }
            }
        }
    }
    out.sort();
    out
}

pub(crate) fn now_ms() -> i64 {
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
    store: &mut crate::graph::store::Store,
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
        // **把"它说自己在哪个文件"存下来。**
        //
        // 2026-10-16 外部对照复核抓出的缺陷：查询层分不开这两种情形——
        //   ① 这条诊断**本来就没说**是哪个文件（比如"缺包 requests"）；
        //   ② 它**说了**，但那个文件不在图谱里（指向未索引的文件）。
        // 两者都没有 `diagnosed_at` 边，而诊断节点又没存原本的路径，
        // 于是查询层**只能**一律回 `Verdict::No`——界面上那句「（这条问题没有说是哪个文件）」
        // 对第 ② 种就是在撒谎，而 CLI / Tauri / 界面里**为第 ② 种写的那三支渲染成了死代码**
        // （`ui/graph.js` 的「没索引到」、`cmd_graph.rs` 与 `src-tauri/graph.rs` 的同名分支）。
        //
        // 这正是这个项目的招牌不变量（是 / 不是 / **查不了**）在查询层**没有生产者**：
        // `AGENTS.md` §2 写着"`unknown` 永远不许显示成'没有'"，而唯一能产出它的地方
        // 从来没产出过。修法不是改文案，是**把这个事实存下来**——
        // 写入侧手里本来就有那个相对路径，只是丢掉了。
        let d = match target {
            Some(rel) => d.with_data(serde_json::json!({ "target": rel })),
            None => d,
        };
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
            // **文件在磁盘上真的存在时，就地为它落一个 File 节点。**
            //
            // 这一条是 M5 接线时才暴露出来的真缺陷：原来只在"图谱里已经有这个文件节点"
            // 时才挂 `diagnosed_at`，而"先跑一次、后索引"是最常见的顺序——
            // 那时节点还不存在，于是**每条诊断都挂不上去**，`unknown_targets` 涨一片，
            // 图谱里留下一堆没有落点的诊断，"改了这里会影响谁"根本答不出来。
            //
            // 为什么这是对的而不是造假：项目节点在上面就是"先落一次"（单独跑一次也要记得下），
            // 文件节点同理——我们**确知**这个文件就是这次运行报出来的那个（相对路径就在眼前），
            // 落一个真节点比留一条悬空诊断诚实得多。
            // 为什么还要判 is_file：路径可能是输出文本里的一段，磁盘上没有它；
            // 那种情况**不凭空造节点**，仍然如实记进 `unknown_targets`。
            let known = store.node(&fid)?.is_some();
            if !known && root.join(rel).is_file() {
                let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
                let f = Node::new(NodeKind::File, rel, name, Source::SelfTool, Confidence::Certain, now);
                nodes.push(f.with_file(rel, 1));
            }
            if known || nodes.iter().any(|n| n.id == fid) {
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
pub fn index_source(
    root: &Path,
    store: &mut crate::graph::store::Store,
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
    // **扫到多少要单独记下来**（`files` 是"处理了几个"，两者不是一回事）。
    rep.scanned = files.len();
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
    let mut file_imports: std::collections::BTreeMap<String, Vec<crate::graph::imports::ImportFact>> =
        std::collections::BTreeMap::new();

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
        let file = crate::util::textfile::decode(&bytes);
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

        // **按扩展名分派**（2026-10-16 加的 Python）。
        //
        // 这两门语言的"符号"定义不一样，所以是两个解析器而不是一个带开关的解析器：
        // 一个函数里塞两套规则，改一门必然碰到另一门。
        let parsed = if rel.ends_with(".py") {
            crate::graph::parse_python::parse_python(&rel, &file.text, now)
        } else {
            crate::parse::parse_rust(&rel, &file.text, now)
        };
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
                // **带上命名空间**：解析调用时只该在值命名空间里找
                // （《Rust Reference》：名字被分进不同命名空间，可以同名而不冲突）。
                let class = s
                    .data
                    .as_ref()
                    .and_then(|d| d.get("sym"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("other")
                    .to_string();
                let entry = crate::graph::resolve::SymbolEntry {
                    id: s.id.clone(),
                    class,
                };
                ft.entry(s.name.clone()).or_default().push(entry.clone());
                all_names.entry(s.name.clone()).or_default().push(entry);
            }
        }
        if !parsed.imports.is_empty() {
            file_imports.insert(rel.clone(), parsed.imports.clone());
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

        let state = crate::graph::store::FileState {
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
            let no_imports: Vec<crate::graph::imports::ImportFact> = Vec::new();
            let imps = file_imports.get(rel).unwrap_or(&no_imports);
            let res = crate::graph::resolve::resolve_file(rel, refs, ft, imps, &all_names);
            rep.resolved += res.resolved.len();
            rep.ambiguous.extend(res.ambiguous.iter().cloned());
            rep.unmapped_imports.extend(res.unmapped_imports.iter().cloned());
            rep.namespace_mismatch
                .extend(res.namespace_mismatch.iter().cloned());
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

    // **把"理解覆盖度"存进库**（2026-10-18）：界面上的角色图例要能说
    // "这些角色有多可信"。数据跟着库走——谁打开、什么时候打开，都是同一个事实。
    //
    // 为什么不在界面里现算：算它要重跑一次解析聚合，而这是**索引时已经知道的事**。
    // 写失败只影响"界面那句提醒说不说"，不影响这次索引的结果——但**不许静默**，
    // 所以走 `.ok()` 而不是 `let _ =`（后者会被 clippy 拦，见 `let_underscore_must_use`）。
    store.set_meta("index_linked", &rep.resolved.to_string()).ok();
    store.set_meta("index_unresolved", &rep.ambiguous.len().to_string()).ok();
    store.set_meta("index_external", &rep.external.to_string()).ok();
    store.set_meta("index_at_ms", &now_ms().to_string()).ok();

    Ok(rep)
}

/// **这个目录里有多少"我们能解析的之外"的源码**（按扩展名数）。
///
/// 为什么需要它（2026-10-18 真输入打出来的）：拿 `vite`（TypeScript）跑 `index`，
/// 扫到 0 个文件，界面说「这个目录里没有可索引的源文件」——
/// **那是把"我们还不会解析这门语言"说成了"你没有源码"**（三态压扁的典型）。
/// 有了这个计数，那句话才能说准：不是你没有，是我们还没支持。
///
/// 只数**常见源码扩展名**，并且跳过 `node_modules` / `target` / `.git` 之类的目录——
/// 它只用于"把话说准"，不是精确统计，所以不值得为它做全量精确遍历。
pub fn foreign_sources(root: &Path) -> Vec<(String, usize)> {
    const WANTED: &[&str] = &[
        "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "java", "kt", "kts", "go", "c",
        "h", "cc", "cpp", "hpp", "cs", "rb", "php", "swift",
    ];
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        // **上限**：真项目可能有几万个目录，而这里只要"有没有、大概多少"。
        if visited > 20_000 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let ft = match e.file_type() { Ok(f) => f, Err(_) => continue };
            let name = e.file_name().to_string_lossy().to_string();
            if ft.is_dir() {
                if matches!(name.as_str(), "node_modules" | "target" | ".git" | "dist" | "build" | "vendor" | ".venv") {
                    continue;
                }
                stack.push(e.path());
            } else if let Some(ext) = name.rsplit('.').next() {
                let ext = ext.to_lowercase();
                if WANTED.contains(&ext.as_str()) {
                    *counts.entry(ext).or_insert(0) += 1;
                }
            }
        }
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    v
}

#[cfg(test)]
mod to_json_tests {
    use super::IndexReport;

    /// **`files` 与 `scanned` 是两个数，JSON 里必须都在**（2026-10-16）。
    ///
    /// 这条守的是一个**两次被同一个误解咬过**的地方：外壳把"这次处理了几个"
    /// 当成"项目有几个"报出去，于是**一个正常项目跑第二遍时会显示「0 个文件」**，
    /// 看起来像图谱被清空了。
    ///   - 第一次在 CLI（`jingwei index`），修法：加 `scanned`；
    ///   - 第二次在桌面界面（`graph_index`），**同一个形态又出现了一次**——
    ///     因为两处各拼各的 JSON。
    ///
    /// 所以现在**只有一份 `to_json()`**，这条测试守住它：
    /// 少了 `scanned`，两处外壳就都瞎了。
    #[test]
    fn files_与_scanned_是两个数() {
        // 这次什么都没变，但项目里扫到 91 个
        let v = IndexReport {
            files: 0,
            scanned: 91,
            ..Default::default()
        }
        .to_json();
        assert_eq!(v["files"], 0);
        assert_eq!(v["scanned"], 91, "少了它，界面会把'没变'读成'项目是空的'");
    }

    /// **`skippedUnchanged` 要合并两级**。
    ///
    /// 一级（大小与时间都没动）和二级（`touch` 过、内容哈希一样）在用户眼里
    /// **是同一件事："没变"**。分开报只会让人以为漏了一批文件。
    ///
    /// 而且这条测试还钉住了一个**真实存在过的口径不一致**：
    /// CLI 那份只算了 `skipped_unchanged`，界面那份把二级也算上了——
    /// **两个数说的不是一件事，而两边都"能跑通"，没有任何东西会红。**
    #[test]
    fn 跳过的数要把两级合并() {
        let v = IndexReport {
            skipped_unchanged: 3,
            skipped_same_content: 4,
            ..Default::default()
        }
        .to_json();
        assert_eq!(v["skippedUnchanged"], 7, "两级都是'没变'，该合并");
    }

    /// **"我们自己漏的"那两类不许省。**
    ///
    /// `unmappedImports`（写了 `use` 却没接上）与 `namespaceMismatch`
    /// （名字在、命名空间不符）是**我们自己的失败**——少了它们，
    /// 「外部 N 个」这个数里就混着我们的漏接，"不归我们管"就成了挡箭牌。
    #[test]
    fn 我们自己漏的两类不许省() {
        let v = IndexReport {
            unmapped_imports: vec!["a".into(), "b".into()],
            namespace_mismatch: vec!["c".into()],
            ..Default::default()
        }
        .to_json();
        assert_eq!(v["unmappedImports"], 2, "写了 use 却没接上的处数");
        assert_eq!(v["namespaceMismatch"], 1, "命名空间不符的处数");
    }

    /// **解析期那三个数分开报**——它们说的是三件完全不同的事：
    /// "连上"是我们接住了，"拿不准"是命中多个候选所以**不猜**，
    /// "外部"是不归我们管。合成一个数就是把三种情况压扁。
    #[test]
    fn 解析期的三个数不许合成一个() {
        let v = IndexReport {
            resolved: 292,
            ambiguous: vec!["x".into(), "y".into()],
            external: 4613,
            ..Default::default()
        }
        .to_json();
        assert_eq!(v["resolved"], 292);
        assert_eq!(v["ambiguous"], 2);
        assert_eq!(v["external"], 4613);
        assert_ne!(v["resolved"], v["external"], "这两个数不是一件事");
    }
}
