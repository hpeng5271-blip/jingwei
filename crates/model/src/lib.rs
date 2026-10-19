//! 经纬的领域模型。
//!
//! 这一层要守住设计里最贵的四条线，而且是**用类型守住**，不是靠自觉：
//!
//! * **关系必有来源**：`Edge::new` 的 `source` 是必填参数——没来源的关系造不出来。
//! * **三态而非布尔**：`Verdict` 区分"没有"与"无法确认"。后者永远不算缺失。
//! * **估的不许报 Error**：`Finding` 过 `rules::enforce_evidence` 时会被降级（见那个函数）
//!   推测来源（`Confidence::Guessed`）或无自查命令的结论。
//! * **稳定标识**：`node_id` 做规范化（分隔符、大小写），保证同一个东西永远同一个 id。
//!
//! 这里没有任何 IO、没有数据库、没有网络。它只描述"世界上有什么、它们怎么连"。

// 领域层不许出现 unsafe。这条用 crate 属性写在这里、而不是写进 Cargo.toml：
// Cargo 的 `[lints] workspace = true` 是全量继承，没法只给某一个 crate 加一条
// ——而 engine 因为调 Win32 必须允许 unsafe。写在这里，"谁想在 model 里用 unsafe
// 就得先删掉这一行"这件事是看得见的。
#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};

/// 十三种实体（清单以《数据层设计》§15.3 为唯一真源）。
/// **新增要过两端合法矩阵**：`EdgeKind::allows` 在构造期就会拒绝不合法的组合，
/// 所以"随手加一个"是加不进来的——加之前得先想清楚它能连谁、谁连它。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Project,
    File,
    /// 逻辑模块（≠目录）：多层级下钻的中间一层，笔记也挂得上
    Module,
    Symbol,
    Dependency,
    /// 机器上的工具链（M/D/U 里的 M）：七窍玲珑要做"环境 × 语义"交叉，它是前提
    Toolchain,
    Diagnostic,
    /// 判定规则：诊断要能追溯到"哪条规则判的"，规则表还要能自己改
    Rule,
    /// 契约：一条龙执行的依据；人改过的字段要保住
    Contract,
    Note,
    Commit,
    Snapshot,
    Run,
}

impl NodeKind {
    /// 全部取值。**DDL 的 CHECK 子句由它生成**，
    /// 这样"枚举与建表语句各写一遍然后慢慢漂掉"这类事故不会发生。
    pub const ALL: [NodeKind; 13] = [
        NodeKind::Project,
        NodeKind::File,
        NodeKind::Module,
        NodeKind::Symbol,
        NodeKind::Dependency,
        NodeKind::Toolchain,
        NodeKind::Diagnostic,
        NodeKind::Rule,
        NodeKind::Contract,
        NodeKind::Note,
        NodeKind::Commit,
        NodeKind::Snapshot,
        NodeKind::Run,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            NodeKind::Project => "project",
            NodeKind::File => "file",
            NodeKind::Module => "module",
            NodeKind::Symbol => "symbol",
            NodeKind::Dependency => "dependency",
            NodeKind::Toolchain => "toolchain",
            NodeKind::Diagnostic => "diagnostic",
            NodeKind::Rule => "rule",
            NodeKind::Contract => "contract",
            NodeKind::Note => "note",
            NodeKind::Commit => "commit",
            NodeKind::Snapshot => "snapshot",
            NodeKind::Run => "run",
        }
    }
}

/// 十种关系。关系类型是设计层（人定），关系实例是运行层（从项目读）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    DependsOn,
    Calls,
    References,
    Defines,
    Contains,
    /// 实现了哪个 trait（Rust 的结构性关系，calls/references 表达不了）
    Implements,
    /// 这个依赖需要哪个工具链（环境与语义的接缝）
    Needs,
    DiagnosedAt,
    /// 这条诊断是哪条规则判的（规则可追溯）
    ReportedBy,
    Causes,
    Affects,
    NotedAt,
    ChangedAt,
    /// 快照记下了哪些对象（对比两次差异要靠它）
    SnapshottedAt,
    /// 这次运行产生了哪条诊断。
    ///
    /// **这条是 Q11 补的**：设计 §14 的 M5 写"运行结果变成 `Run` 节点与 `diagnosed_at` 关系"，
    /// 但矩阵里 `diagnosed_at` 的两端是 `Diagnostic → File/Symbol/Module`，
    /// 而 `Run` 当时只能 `follows` 契约——**诊断挂不到运行上**，
    /// 于是图谱回答不了"上次跑失败是哪些问题"。缺的正是这条边。
    Produced,
    /// 这次运行遵循哪份契约
    Follows,
    /// **注册**：文件把某个函数注册给了外部框架（Python 装饰器 `@app.route` / `@click.command`）。
    ///
    /// 为什么单独一条（D-20261019-02）：它既不是 `defines`（装饰器不是定义），
    /// 也不是 `calls`（不是函数体在执行），但它是 **flask 这类项目的入口/路由核心信号**。
    /// 混进 `calls` 会污染角色的入/出度。
    Registers,
}

impl EdgeKind {
    /// 全部取值；理由同 `NodeKind::ALL`。
    pub const ALL: [EdgeKind; 17] = [
        EdgeKind::Contains,
        EdgeKind::Defines,
        EdgeKind::Implements,
        EdgeKind::Calls,
        EdgeKind::References,
        EdgeKind::DependsOn,
        EdgeKind::Needs,
        EdgeKind::DiagnosedAt,
        EdgeKind::ReportedBy,
        EdgeKind::Causes,
        EdgeKind::Affects,
        EdgeKind::NotedAt,
        EdgeKind::ChangedAt,
        EdgeKind::SnapshottedAt,
        EdgeKind::Produced,
        EdgeKind::Follows,
        EdgeKind::Registers,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::DependsOn => "depends_on",
            EdgeKind::Calls => "calls",
            EdgeKind::References => "references",
            EdgeKind::Defines => "defines",
            EdgeKind::Contains => "contains",
            EdgeKind::Implements => "implements",
            EdgeKind::Needs => "needs",
            EdgeKind::DiagnosedAt => "diagnosed_at",
            EdgeKind::ReportedBy => "reported_by",
            EdgeKind::Causes => "causes",
            EdgeKind::Affects => "affects",
            EdgeKind::NotedAt => "noted_at",
            EdgeKind::ChangedAt => "changed_at",
            EdgeKind::SnapshottedAt => "snapshotted_at",
            EdgeKind::Produced => "produced",
            EdgeKind::Follows => "follows",
            EdgeKind::Registers => "registers",
        }
    }

    /// **两端合法矩阵。** 这是防"模型糊掉"的那道闸：
    /// 只允许这里出现的组合，越界的边在构造时就失败。
    /// 没有它，一个拼错的组合（比如 `Note -calls-> File`）会安静地进库，
    /// 然后在某次查询里变成一条读不懂的结果。
    pub fn allows(&self, src: NodeKind, dst: NodeKind) -> bool {
        use EdgeKind as E;
        use NodeKind as N;
        match self {
            E::Contains => matches!(
                (src, dst),
                (N::Project, N::File)
                    | (N::Module, N::Module)
                    | (N::Module, N::File)
                    | (N::Module, N::Symbol)
                    | (N::File, N::Symbol)
                    | (N::File, N::Dependency)
                    | (N::File, N::Diagnostic)
            ),
            E::Defines => matches!((src, dst), (N::File, N::Symbol) | (N::Module, N::Symbol)),
            E::Implements => matches!((src, dst), (N::Symbol, N::Symbol)),
            // **`(File, Symbol)` 只为"模块级调用"开**（2026-10-19，D-20261019-02）：
            // Python 的 `if __name__ == "__main__":` 块里，调用者在语法上不是符号，
            // 而是这个文件——脚本被执行时顶层语句确实会跑，这是事实不是猜。
            // 只给 Calls 开；References 仍要求 Symbol→Symbol。
            E::Calls => matches!((src, dst), (N::Symbol, N::Symbol) | (N::File, N::Symbol)),
            E::References => matches!((src, dst), (N::Symbol, N::Symbol)),
            E::DependsOn => matches!(
                (src, dst),
                (N::Dependency, N::Dependency) | (N::File, N::Dependency)
            ),
            E::Needs => matches!((src, dst), (N::Dependency, N::Toolchain)),
            E::DiagnosedAt => matches!(
                (src, dst),
                (N::Diagnostic, N::File) | (N::Diagnostic, N::Symbol) | (N::Diagnostic, N::Module)
            ),
            E::ReportedBy => matches!((src, dst), (N::Diagnostic, N::Rule)),
            E::Causes => matches!(
                (src, dst),
                (N::Diagnostic, N::Diagnostic) | (N::Diagnostic, N::Symbol)
            ),
            E::Affects => matches!((src, dst), (N::Symbol, N::Symbol) | (N::Diagnostic, N::Symbol)),
            E::NotedAt => matches!(
                (src, dst),
                (N::Note, N::File)
                    | (N::Note, N::Symbol)
                    | (N::Note, N::Module)
                    | (N::Note, N::Dependency)
                    | (N::Note, N::Diagnostic)
                    | (N::Note, N::Run)
            ),
            E::ChangedAt => matches!((src, dst), (N::Commit, N::File) | (N::Commit, N::Symbol)),
            // 快照可以指向任何实体
            E::SnapshottedAt => src == N::Snapshot,
            E::Follows => matches!((src, dst), (N::Run, N::Contract)),
            // 注册：**文件 → 函数**。装饰器写在哪个文件里，注册就属于哪个文件。
            E::Registers => matches!((src, dst), (N::File, N::Symbol)),
            E::Produced => matches!((src, dst), (N::Run, N::Diagnostic)),
        }
    }
}

/// 关系与结论的来源。**没有来源的东西不许进图谱。**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// tree-sitter 语法层
    TreeSitter,
    /// 语言服务器语义层
    Lsp,
    /// 官方工具的权威输出（cargo metadata / npm ls / pom）
    Meta,
    /// 用户写的
    User,
    /// 工具自己产生的（运行记录、快照）
    SelfTool,
}

impl Source {
    /// 全部取值。`tools/schema.sql` 里 `source` 的 CHECK 子句必须与它一字不差——
    /// 这条由 `jingwei-engine` 的 `store` 测试守着（那边才允许读文件，本 crate 没有 IO）。
    pub const ALL: [Source; 5] = [
        Source::TreeSitter,
        Source::Lsp,
        Source::Meta,
        Source::User,
        Source::SelfTool,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Source::TreeSitter => "ts",
            Source::Lsp => "lsp",
            Source::Meta => "meta",
            Source::User => "user",
            Source::SelfTool => "self",
        }
    }

    /// 权威来源：只有这些才允许支撑"错误"级别的结论。
    pub fn is_authoritative(&self) -> bool {
        matches!(self, Source::Meta | Source::SelfTool | Source::Lsp)
    }
}

/// 置信度只有三档，且**只有推测才标低**——确定的不用标。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// 1.0：权威工具直接给出的
    Certain,
    /// 0.85：语法层推断，通常可靠
    High,
    /// 0.5：推测（例如"没找到锁文件，按清单估的"）
    Guessed,
}

impl Confidence {
    pub fn value(&self) -> f32 {
        match self {
            Confidence::Certain => 1.0,
            Confidence::High => 0.85,
            Confidence::Guessed => 0.5,
        }
    }

    pub fn from_value(v: f32) -> Confidence {
        if v >= 1.0 {
            Confidence::Certain
        } else if v >= 0.7 {
            Confidence::High
        } else {
            Confidence::Guessed
        }
    }
}

/// 三态。**这是"不误报"的类型化表达**：
/// 查不到就必须是 `Unknown`，绝不能退化成 `No`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict<T> {
    Yes(T),
    No,
    /// 无法确认（例如 C++ 的包可能来自系统 / vcpkg / Conan / 源码树）
    Unknown,
}

/// **"查不了"是谁造成的**——`Verdict::Unknown` 缺的那一半。
///
/// ## 为什么它必须存在（2026-10-16）
///
/// `AGENTS.md` §2 一直写着一条判据：
/// **「这是"用户的空白"，还是"我们的失败"？」**
/// 而它**只活在文档里**——代码里 32 处 `Verdict::Unknown` **一处都没区分**。
/// 判据写在文档里就是没人执行（这个项目为这条付过 5 次账）。
///
/// ## 为什么不做成 `Verdict::Unknown(Unknown)` 的载荷
///
/// 因为 `Verdict::Unknown` 是**无载荷**的，32 处 `matches!` /
/// `Verdict::Unknown =>` 都依赖这一点。给它加字段 = 一次性改 32 处，
/// 而其中大多数（例如 `query.rs` 的"没索引到"）**本身就知道原因**、
/// 只是没有地方表达。所以做成**独立类型、钉在产出点**：
/// 只有产出 `Unknown` 的那一处才知道为什么，下游猜不出来。
///
/// ## 为什么"分不清"要单独一档
///
/// 有些地方真的分不出（拿到的只是一个字符串）。**硬归到两头就是编**——
/// 而"编"正是这个系统最反对的事。所以给一档 `Unclear`，让它如实站着。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unknown {
    /// **用户的空白**：项目里本来就没有，或用户没提供判据的入口。
    /// 例：没有声明文件、契约里没写这一项。→ 用户能做点什么。
    TheirGap,
    /// **我们的失败**：东西可能存在，是我们查不到 / 没实现。
    /// 例：问不到 pip、这个语言的依赖检测我们还没做。→ 我们能做点什么。
    OurGap,
    /// 分不清是哪一头。**不许硬归**（编出来的归类比不归更糟）。
    Unclear,
    /// **不适用**：问错了对象，不是缺东西，也不是我们查不到（2026-10-17）。
    ///
    /// ## 这个变体是拿外部数据换来的
    ///
    /// 拿 8 个**别人写的真 crate**（`anyhow` `itoa` `log` `memchr` `regex` `serde`
    /// `clap` `rand`）跑 `guide`，**8 个全是同一句**：
    /// 「这一项你这边没有；**补上就能判断**」+ 退出码 2。
    ///
    /// 而它们都是**库**——**库根本没有"跑"这回事**，它要被别人用。
    /// 让用户"补上"，补什么？补一个 `main`？那是把库改成可执行文件。
    ///
    /// 同一个洞在**空目录**和 `site-packages` 里的库上各碰过一次
    /// （第 25 轮记成"没修的"），当时用 `what_to_do` 覆盖绕过去了——
    /// **绕过去不算修好**：`why` 还是 `their_gap`，退出码还是 2，
    /// 别的消费者（脚本、界面）照样会说"你缺东西"。
    ///
    /// ## 它与前三者的区别
    ///
    ///   `TheirGap` —— 你能做点什么（补文件、写契约）
    ///   `OurGap`   —— 我们能做点什么（那是我们的短板）
    ///   `Unclear`  —— 两边都有点，分不清
    ///   **`NotApplicable` —— 谁都不用做什么**，因为这个问题对它不成立
    NotApplicable,
}

impl Unknown {
    /// 给用户看的一句话。**两种情况的说法必须不同**——
    /// 因为用户能做的事完全不同：一种是"你补上"，一种是"等我们/换个办法"。
    pub fn what_to_do(self) -> &'static str {
        match self {
            Unknown::TheirGap => "这一项你这边没有；补上就能判断",
            Unknown::OurGap => "可能是有的，是我们查不到（不是你的问题）",
            Unknown::Unclear => "查不了，而且现在分不清是哪一头",
            Unknown::NotApplicable => {
                "这个问题对它不适用——不用做什么（它是个库，要被别人用，不是拿来跑的）"
            }
        }
    }
}

impl<T> Verdict<T> {
    pub fn is_yes(&self) -> bool {
        matches!(self, Verdict::Yes(_))
    }
    /// 只有明确为否才算"缺失"。**Unknown 不算缺失。**
    pub fn is_missing(&self) -> bool {
        matches!(self, Verdict::No)
    }
    pub fn ok(&self) -> Option<&T> {
        match self {
            Verdict::Yes(v) => Some(v),
            _ => None,
        }
    }
}

/// 一个实体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// 该类实体独有的字段（JSON）。通用字段一律不许塞这里面。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    pub source: Source,
    pub confidence: Confidence,
    /// 这个节点**自身内容**的指纹（种类 + 名称 + 签名）。
    ///
    /// **不是**"这个文件的节点集合"的哈希——那是文件级的事，落在 `files.nodes_count`。
    /// 这两个东西曾经在《数据层设计》里被混成一个字段，结果是抽样校验没法实现：
    /// 一行里存"整个集合的哈希"说不通。改清楚之后各归各位。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: NodeKind,
        key: &str,
        name: impl Into<String>,
        source: Source,
        confidence: Confidence,
        now: i64,
    ) -> Node {
        Node {
            id: node_id(kind, key),
            kind,
            name: name.into(),
            file_path: None,
            line: None,
            language: None,
            data: None,
            source,
            confidence,
            checksum: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_file(mut self, path: &str, line: u32) -> Node {
        self.file_path = Some(normalize_path(path));
        self.line = Some(line);
        self
    }

    /// 挂一段结构化数据在节点上（持久化在 `nodes.data`，读回来还是这份 JSON）。
    ///
    /// 用途是**记住"构造时知道、但边不足以表达"的事实**。第一个用它是 `Diagnostic`：
    /// 它要记住"我原本说自己在哪个文件"——不然查询层分不开
    /// "本来就没说位置"和"说了但那个文件没索引到"，只会一律回"没有位置"，
    /// 而界面上为后者写的那一支就永远是死代码。
    pub fn with_data(mut self, data: serde_json::Value) -> Node {
        self.data = Some(data);
        self
    }
}

/// 边两端的组合不合法。**这不是错误处理，这是类型系统的出口**：
/// 越界的组合在构造时就失败，不许进库。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IllegalPair {
    pub kind: EdgeKind,
    pub src: NodeKind,
    pub dst: NodeKind,
}

impl std::fmt::Display for IllegalPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} 不允许 {} -> {}",
            self.kind.as_str(),
            self.src.as_str(),
            self.dst.as_str()
        )
    }
}

/// 一条关系。**构造它就必须要来源**——这是设计 §4.2 那条"无来源不许入库"的落点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub src: String,
    pub dst: String,
    pub kind: EdgeKind,
    pub source: Source,
    pub confidence: Confidence,
    pub created_at: i64,
    /// 失效只标记，不物理删除（`None` = 有效）
    pub expires_at: Option<i64>,
}

impl Edge {
    /// 受检构造：来源必填（编译期），两端组合合法（构造期）。
    /// 从数据库读回来时**不走这里**——老数据可能是在规则变更之前写进去的，
    /// 读取路径必须能原样还原（见 `from_row`）。
    ///
    /// 参数确实多，但每一个都对应一条设计约束（两端 id + 两端类型 + 来源 + 置信度 + 时间），
    /// 包成参数对象只会让"哪一端的类型"变模糊。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        src: impl Into<String>,
        dst: impl Into<String>,
        kind: EdgeKind,
        src_kind: NodeKind,
        dst_kind: NodeKind,
        source: Source,
        confidence: Confidence,
        now: i64,
    ) -> Result<Edge, IllegalPair> {
        if !kind.allows(src_kind, dst_kind) {
            return Err(IllegalPair {
                kind,
                src: src_kind,
                dst: dst_kind,
            });
        }
        Ok(Edge::from_row(src, dst, kind, source, confidence, now, None))
    }

    /// 从存储还原。**不做矩阵校验**：历史数据要能读回来。
    /// 参数多是因为它如实对应表里的列——这类"镜像函数"不适合作参数对象，
    /// 多一层包装只会让对应关系更难对上。
    #[allow(clippy::too_many_arguments)]
    pub fn from_row(
        src: impl Into<String>,
        dst: impl Into<String>,
        kind: EdgeKind,
        source: Source,
        confidence: Confidence,
        created_at: i64,
        expires_at: Option<i64>,
    ) -> Edge {
        Edge {
            src: src.into(),
            dst: dst.into(),
            kind,
            source,
            confidence,
            created_at,
            expires_at,
        }
    }

    pub fn is_live(&self) -> bool {
        self.expires_at.is_none()
    }
}

/// 结论级别。颜色只是它的表现，语义在这里。
///
/// **这里原来有两个"级别"类型，2026-10-16 合成了一个。**
/// 一个是这个 `Severity`；另一个叫 `Level`（`Silent` / `Progress` / `Warn` / `Error`），
/// 文档说它们分工是"`Level` 管给不给用户看、`Severity` 管这条本身多严重"。
/// 实情是 **`Level` 在生产里零构造点**——它只活在另一个也没人用的类型 `Conclusion` 上，
/// 两者一起删了。留着一个并行概念比删掉它危险：读代码的人会以为"给不给用户看"
/// 这件事有人在管，其实一行都没执行。
///
/// 连带删掉的还有那条挂在上面的设计：`Level::Silent` 背后的"抑制下位"机制
/// **从来没实现**（详见 `AGENTS.md` §5），所以"隐藏必须可查"那条要求也就无从谈起。
/// 顺带的好处：`复审记录.md` 当初列的 B 档风险"抑制下位可能是永久沉默"
/// **因为不存在而不可能发生**。
///
/// **如果将来真要做"给不给用户看"这一层**：把 `Level` 加回来时，
/// **同一次改动里连"隐藏必须可查"的入口一起做**，否则就会重演这件事。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
    Info,
}

/// 项目体检与排查链上的一条发现。
///
/// 它同时管两件事：**"这一条凭什么成立"**（`source` / `confidence` / `evidence` / `verify`）
/// 与**"它在界面上挂在哪一层、哪一组"**（`layer` / `group`）。
/// 2026-10-16 之前这两件事分在两个类型上（另一个叫 `Conclusion`），而那个类型生产零调用——
/// 所以合成了一个，见下面那段删除说明。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    /// 属于排查链的哪一层。让人顺着层走，比面对一堆并列的报错清楚得多。
    #[serde(default)]
    pub layer: String,
    /// 这一层在问什么问题。解释一句，人才知道该往哪儿看。
    #[serde(default)]
    pub layer_note: String,
    /// 项目体检里的分组（忽略与提交 / 依赖清单 / …）。
    /// 排查链上的结论没有这个——它们按层分组，体检按这个分组。
    #[serde(default)]
    pub group: String,
    pub evidence: Vec<String>,
    pub verify: Vec<String>,
    pub boundary: String,
    pub next: String,
    pub excluded: Vec<String>,
    /// 这条结论是**凭什么来的**。`Error` 级必须权威来源（见 `rules::enforce_evidence`）。
    ///
    /// 2026-10-16 补：这两个字段原来**只在一个生产零调用的类型上有**，
    /// 于是"估的不许报 Error"这条线，机器上只守住了三分之一（只有"有自查命令"那条）。
    /// 现在补在生产真正构造的类型上，三道闸齐全（见 `rules::enforce_evidence`）。
    #[serde(default = "default_source")]
    pub source: Source,
    /// 置信度。`Guessed` 不许报 `Error`。
    #[serde(default = "default_confidence")]
    pub confidence: Confidence,
}

fn default_source() -> Source {
    // 老数据 / 反序列化缺少这个字段时的兜底：**最保守的那个**。
    // 用 `User`（非权威）而不是 `Meta`，这样"缺字段"不会意外通过权威闸。
    Source::User
}

fn default_confidence() -> Confidence {
    // 同理，兜底用最保守的：算"推测"，不许撑 `Error`。
    Confidence::Guessed
}

// **这里原来有一整套「四要素结论」类型：`Conclusion` / `Level` / `SuppressReason`
// 以及 `error_checked`（三道闸：权威来源 / 非推测置信度 / 有自查命令）。**
// 2026-10-16 删掉了，理由值得留着：
//
//   - 它们在生产代码里**零调用**——真正出卡的是 `Finding`（这个文件里还在），
//     而"不误报"的闸长在 `crates/engine/src/understand/rules.rs` 的 `enforce_evidence` 上。
//   - 而那道闸原来**只有一条**（"有自查命令"），因为 `Finding` 上根本没有
//     source / confidence 两个字段——**设计里那条"估的不许报 Error"只守住了三分之一**。
//     现在补齐了：`Finding` 带这两个字段（默认最保守：非权威 + 推测），
//     `enforce_evidence` 三条闸齐全，想报 Error 的规则必须显式用 `f_meta` 声明权威。
//   - 两处实现迟早分家，而这个仓库已经因为"同一个判断两处各写一遍"吃过苦头。
//     **所以留下生产真正走的那一份，删掉没人走的那一份。**
//
// 如果将来真需要"结论"这一层（例如把 `Finding` 拆成"凭什么成立"与"挂在哪"两半），
// 再把它们加回来——**加的时候连闸一起加**，不要只加类型。


/// 稳定标识：`<kind>:<规范化 key>`。
/// 规范化做的事：反斜杠转正斜杠、去掉 `./`、统一小写（Windows 路径大小写不敏感）。
pub fn node_id(kind: NodeKind, key: &str) -> String {
    // **只对"路径类"的实体做路径规范化。**
    //
    // 这一条是测试当场抓出来的：原来对任何 key 都跑 `normalize_path`，
    // 而它最后一步是全小写——于是 `Ledger::post` 变成了 `ledger::post`。
    // 但 **Rust 是大小写敏感的**：`Ledger` 与 `ledger` 是两个不同的类型。
    // 把语言自己的标识符也小写，会让它们**静默地撞成同一个节点**
    // （后者覆盖前者，不报错）——正是本设计最不许出现的那类错。
    //
    // 判据：**文件系统大小写不敏感，所以路径小写；语言的标识符大小写敏感，所以原样保留。**
    // 符号 key 里那半截路径由调用方负责先规范化（扫描出来的路径本来就是规范化的）。
    let k = match kind {
        // 位置类实体：路径规范化（小写、斜杠统一）。
        //
        // `Project` 的 key 是"相对根的路径"，也就是 `"."`——规范化之后是空串，
        // 所以项目的 id 长这样：`project:`。它看着有点秃，但这是对的：
        // **项目 id 不该随目录名或位置变**，否则项目一改名，库里指向它的边全要重写。
        NodeKind::File | NodeKind::Module | NodeKind::Project => normalize_path(key),
        // 其余是语言自己的标识符：**大小写敏感，原样保留**。
        _ => key.trim().to_string(),
    };
    format!("{}:{}", kind.as_str(), k)
}

/// 路径规范化：幂等（跑两遍结果一样），这是属性测试要守的不变量。
pub fn normalize_path(p: &str) -> String {
    let s = p.replace('\\', "/");
    let mut out = String::with_capacity(s.len());
    for part in s.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    out.to_lowercase()
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    /// **"无法确认"绝不是"没有"**。C++ 的包查不到时，说缺失就是误报。
    #[test]
    fn unknown_is_not_missing() {
        let v: Verdict<String> = Verdict::Unknown;
        assert!(!v.is_missing(), "无法确认不算缺失");
        assert!(!v.is_yes());
        let no: Verdict<String> = Verdict::No;
        assert!(no.is_missing());
    }

    /// 路径规范化必须幂等；否则同一个文件会产生两个节点，图谱会分裂。
    #[test]
    fn path_normalization_is_idempotent() {
        for raw in [
            r"src\engine\lib.rs",
            "./src//engine/./lib.rs",
            r"Src\Engine\Lib.rs",
            "src/engine/lib.rs",
        ] {
            let once = normalize_path(raw);
            let twice = normalize_path(&once);
            assert_eq!(once, twice, "对 {raw} 不幂等");
            assert_eq!(once, "src/engine/lib.rs");
        }
    }

    /// 同一个东西必须永远得到同一个 id（否则增量和查询全乱）。
    #[test]
    fn node_ids_are_stable_across_path_spellings() {
        let a = node_id(NodeKind::File, r"Src\Engine\Lib.rs");
        let b = node_id(NodeKind::File, "./src/engine/lib.rs");
        assert_eq!(a, b);
        assert_eq!(a, "file:src/engine/lib.rs");
    }

    /// 关系可以被标记失效，但**不能被"删除"**——历史要留得住。
    #[test]
    fn edges_expire_but_survive() {
        let mut e = Edge::new(
            "symbol:a",
            "symbol:b",
            EdgeKind::Calls,
            NodeKind::Symbol,
            NodeKind::Symbol,
            Source::TreeSitter,
            Confidence::High,
            1,
        )
        .expect("Symbol -calls-> Symbol 是合法组合");
        assert!(e.is_live());
        e.expires_at = Some(2);
        assert!(!e.is_live(), "失效后不算有效");
        assert_eq!(e.created_at, 1, "但建立时间还在，历史可查");
    }

    /// **越界的组合在构造时就失败**。这条守着"模型不糊"：
    /// 没有它，一个拼错的组合会安静地进库，然后在某次查询里变成读不懂的结果。
    #[test]
    fn illegal_node_pairs_cannot_be_constructed() {
        let bad = Edge::new(
            "note:x",
            "file:src/lib.rs",
            EdgeKind::Calls,
            NodeKind::Note,
            NodeKind::File,
            Source::User,
            Confidence::Certain,
            1,
        );
        assert!(bad.is_err(), "Note -calls-> File 不该构造得出来");
        assert_eq!(bad.unwrap_err().to_string(), "calls 不允许 note -> file");
    }

    /// 补上的四条关系各自要能成立（缺口 15.2 的落地验证）。
    #[test]
    fn the_newly_added_relations_are_legal() {
        let cases = [
            (EdgeKind::Implements, NodeKind::Symbol, NodeKind::Symbol),
            (EdgeKind::Needs, NodeKind::Dependency, NodeKind::Toolchain),
            (EdgeKind::ReportedBy, NodeKind::Diagnostic, NodeKind::Rule),
            (EdgeKind::Follows, NodeKind::Run, NodeKind::Contract),
            (EdgeKind::SnapshottedAt, NodeKind::Snapshot, NodeKind::File),
        ];
        for (k, s, d) in cases {
            assert!(k.allows(s, d), "{} 应当允许 {} -> {}", k.as_str(), s.as_str(), d.as_str());
        }
    }

    /// 每一种关系**至少有一个合法组合**。否则它就是"写着好看但用不上"的僵尸类型。
    #[test]
    fn every_edge_kind_has_at_least_one_legal_pair() {
        for k in EdgeKind::ALL {
            let ok = NodeKind::ALL
                .iter()
                .any(|s| NodeKind::ALL.iter().any(|d| k.allows(*s, *d)));
            assert!(ok, "{} 没有任何合法组合", k.as_str());
        }
    }

    /// 枚举清单自己是完整的、不重复的。
    ///
    /// （这里原来写着"DDL 的 CHECK 子句由 `ALL` 生成"——**那个机制并不存在**：
    /// DDL 是 `tools/schema.sql` 里手写的。真正守"两边不许漂"的是
    /// `jingwei-engine::store` 里那条读 DDL 的测试。注释描述了一个不存在的机制，
    /// 比没有注释更糟：读的人会以为自己改 `ALL` 就够了。）
    /// **路径小写，标识符不许小写。**
    ///
    /// 这条是测试当场抓出来的：原来 `node_id` 对任何 key 都跑 `normalize_path`，
    /// 而它最后一步是全小写——于是 `Ledger::post` 变成 `ledger::post`。
    /// 但 Rust 是大小写敏感的，`Ledger` 与 `ledger` 是两个不同的类型；
    /// 小写化会让它们**静默地撞成同一个节点**（后者覆盖前者，不报错）。
    #[test]
    fn paths_are_lowercased_but_identifiers_are_not() {
        assert_eq!(node_id(NodeKind::File, "Src/Lib.RS"), "file:src/lib.rs");
        let a = node_id(NodeKind::Symbol, "src/lib.rs#Ledger::post");
        let b = node_id(NodeKind::Symbol, "src/lib.rs#ledger::post");
        assert_ne!(a, b, "Ledger 与 ledger 是两个符号，不许撞成同一个 id");
        assert!(a.contains("Ledger::post"), "标识符大小写要原样保留：{a}");
    }

    #[test]
    fn kind_tables_are_unique_and_complete() {
        assert_eq!(NodeKind::ALL.len(), 13);
        assert_eq!(EdgeKind::ALL.len(), 17);
        let mut names: Vec<&str> = NodeKind::ALL.iter().map(|k| k.as_str()).collect();
        names.sort_unstable();
        let uniq = {
            let mut v = names.clone();
            v.dedup();
            v
        };
        assert_eq!(names, uniq, "实体名有重复");
        let mut enames: Vec<&str> = EdgeKind::ALL.iter().map(|k| k.as_str()).collect();
        enames.sort_unstable();
        let euniq = {
            let mut v = enames.clone();
            v.dedup();
            v
        };
        assert_eq!(enames, euniq, "关系名有重复");
        assert!(names.iter().all(|n| n.chars().all(|c| c.is_ascii_lowercase() || c == '_')));
        assert!(enames.iter().all(|n| n.chars().all(|c| c.is_ascii_lowercase() || c == '_')));
    }

    /// 序列化往返：图谱要能存进 SQLite 再读回来。
    #[test]
    fn round_trips_through_json() {
        let n = Node::new(
            NodeKind::Symbol,
            "src/lib.rs#main",
            "main",
            Source::TreeSitter,
            Confidence::High,
            1,
        )
        .with_file(r"src\lib.rs", 12);
        let s = serde_json::to_string(&n).unwrap();
        let back: Node = serde_json::from_str(&s).unwrap();
        assert_eq!(n, back);
        assert_eq!(back.file_path.as_deref(), Some("src/lib.rs"));
    }

    /// 置信度只有三档，且从数值回来时不会漏掉"估的"。
    #[test]
    fn confidence_only_has_three_bands() {
        assert_eq!(Confidence::from_value(1.0), Confidence::Certain);
        assert_eq!(Confidence::from_value(0.85), Confidence::High);
        assert_eq!(Confidence::from_value(0.5), Confidence::Guessed);
        assert_eq!(Confidence::Guessed.value(), 0.5);
    }
}

