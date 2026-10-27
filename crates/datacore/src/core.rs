//! 多态核心：Subject / Endpoint / EdgeKind / Edge / Judgment / View。
//!
//! 目标：系统内一切"东西"只认 [`Subject`]；语义只由注册表里的 kind 决定，
//! 禁止在调用处按 `symbol:` / `file:` 前缀猜行为。这里是 **类型与不变量**，
//! 不含 I/O（store 负责落库，cli 负责渲染）。

use std::fmt;

/// 系统中可被引用/判断的东西。所有命令入参与所有边端点都归一到它。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subject {
    File(String),
    Symbol { path: String, name: String },
    Test { path: String, name: String },
    Range(String),
    Doc(String),
    Dep(String),
    /// `impl` 的左侧：trait/类型名本身（库里是 `type:X`），不是文件也不是带路径的 symbol。
    Type(String),
}

/// 从 subject 串取"文件路径"（symbol/test/file/range/doc 都带 path）；取不到返回 None。
pub fn typed_path(s: &str) -> Option<String> {
    match Subject::parse(s).ok()? {
        Subject::Symbol { path, .. }
        | Subject::Test { path, .. }
        | Subject::File(path)
        | Subject::Range(path)
        | Subject::Doc(path) => Some(path),
        _ => None,
    }
}


/// 只表示"形状"，用于注册表的 domain 校验与视图选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectShape {
    File,
    Symbol,
    Test,
    Range,
    Doc,
    Dep,
    Type,
}

impl Subject {
    pub fn shape(&self) -> SubjectShape {
        match self {
            Subject::File(_) => SubjectShape::File,
            Subject::Symbol { .. } => SubjectShape::Symbol,
            Subject::Test { .. } => SubjectShape::Test,
            Subject::Range(_) => SubjectShape::Range,
            Subject::Doc(_) => SubjectShape::Doc,
            Subject::Dep(_) => SubjectShape::Dep,
            Subject::Type(_) => SubjectShape::Type,
        }
    }

    /// 规范串（与库里的 subject 文本一致）。
    pub fn canonical(&self) -> String {
        match self {
            Subject::File(p) => format!("file:{p}"),
            Subject::Symbol { path, name } => format!("symbol:{path}#{name}"),
            Subject::Test { path, name } => format!("test:{path}#{name}"),
            Subject::Range(p) => format!("range:{p}"),
            Subject::Doc(p) => format!("doc:{p}"),
            Subject::Dep(n) => format!("dep:{n}"),
            Subject::Type(n) => format!("type:{n}"),
        }
    }

    /// 解析规范串；也接受裸路径（当作 File）。
    /// `#` 只按**第一个** `#` 切分（`r#async` 这类名字必须完整保留）。
    pub fn parse(input: &str) -> Result<Self, SubjectError> {
        let s = input.trim();
        if s.is_empty() {
            return Err(SubjectError::Empty);
        }
        if let Some(rest) = s.strip_prefix("symbol:") {
            let (path, name) = split_hash(rest)?;
            return Ok(Subject::Symbol {
                path: path.to_string(),
                name: name.to_string(),
            });
        }
        if let Some(rest) = s.strip_prefix("test:") {
            let (path, name) = split_hash(rest)?;
            return Ok(Subject::Test {
                path: path.to_string(),
                name: name.to_string(),
            });
        }
        if let Some(rest) = s.strip_prefix("file:") {
            if rest.is_empty() {
                return Err(SubjectError::MissingPath);
            }
            return Ok(Subject::File(rest.to_string()));
        }
        if let Some(rest) = s.strip_prefix("range:") {
            if rest.is_empty() {
                return Err(SubjectError::MissingPath);
            }
            return Ok(Subject::Range(rest.to_string()));
        }
        if let Some(rest) = s.strip_prefix("doc:") {
            if rest.is_empty() {
                return Err(SubjectError::MissingPath);
            }
            return Ok(Subject::Doc(rest.to_string()));
        }
        if let Some(rest) = s.strip_prefix("dep:") {
            if rest.is_empty() {
                return Err(SubjectError::MissingName);
            }
            return Ok(Subject::Dep(rest.to_string()));
        }
        if let Some(rest) = s.strip_prefix("type:") {
            if rest.is_empty() {
                return Err(SubjectError::MissingName);
            }
            return Ok(Subject::Type(rest.to_string()));
        }
        // 裸路径：给 card/impact 这类"人直接写路径"的入口兜底
        Ok(Subject::File(s.to_string()))
    }
}

fn split_hash(rest: &str) -> Result<(&str, &str), SubjectError> {
    let (path, name) = rest.split_once('#').ok_or(SubjectError::MissingName)?;
    if path.is_empty() {
        return Err(SubjectError::MissingPath);
    }
    if name.is_empty() {
        return Err(SubjectError::MissingName);
    }
    if name.contains('#') {
        // name 里允许出现 `r#`，但不允许出现第二个分隔符造成的歧义：
        // `symbol:a.rs#r#async` 的 name 是 `r#async`，这里 split_once 已正确。
    }
    Ok((path, name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectError {
    Empty,
    MissingPath,
    MissingName,
}

impl fmt::Display for SubjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SubjectError::Empty => "empty subject",
            SubjectError::MissingPath => "subject missing path",
            SubjectError::MissingName => "subject missing name (need '#')",
        };
        f.write_str(s)
    }
}

impl std::error::Error for SubjectError {}

/// 边的右端：要么指向另一个 Subject，要么明确是外部/未知（带原因码，不许空着）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Subject(Subject),
    Ext(String),
    Unknown(String),
}

impl Endpoint {
    pub fn canonical(&self) -> String {
        match self {
            Endpoint::Subject(s) => s.canonical(),
            Endpoint::Ext(c) => format!("ext:{c}"),
            Endpoint::Unknown(r) => format!("?{r}"),
        }
    }
    pub fn is_unknown(&self) -> bool {
        matches!(self, Endpoint::Unknown(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Syntax,
    Import,
    Semantic,
    Test,
    Call,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Edge,
    Judgment,
}

/// 一个 kind 的声明。语义只由它决定。
#[derive(Debug, Clone, Copy)]
pub struct KindSpec {
    pub id: &'static str,
    pub layer: Layer,
    pub domain: SubjectShape,
    pub role: Role,
    pub parent: Option<&'static str>,
}

/// 边 kind 注册表（独立于 judgment，避免两套语义被一张表绑死）。
pub const EDGE_KINDS: &[KindSpec] = &[
    KindSpec { id: "use", layer: Layer::Import, domain: SubjectShape::File, role: Role::Edge, parent: None },
    KindSpec { id: "mod", layer: Layer::Import, domain: SubjectShape::File, role: Role::Edge, parent: Some("use") },
    KindSpec { id: "impl", layer: Layer::Semantic, domain: SubjectShape::Type, role: Role::Edge, parent: Some("use") },
    KindSpec { id: "import", layer: Layer::Import, domain: SubjectShape::File, role: Role::Edge, parent: Some("use") },
    KindSpec { id: "test", layer: Layer::Test, domain: SubjectShape::Test, role: Role::Edge, parent: None },
    KindSpec { id: "test_macro", layer: Layer::Test, domain: SubjectShape::Test, role: Role::Edge, parent: Some("test") },
    KindSpec { id: "call", layer: Layer::Call, domain: SubjectShape::Symbol, role: Role::Edge, parent: None },
];

/// judgment kind 注册表。
pub const JUDGMENT_KINDS: &[KindSpec] = &[
    KindSpec { id: "symbol", layer: Layer::Semantic, domain: SubjectShape::Symbol, role: Role::Judgment, parent: None },
    KindSpec { id: "doc", layer: Layer::Semantic, domain: SubjectShape::Doc, role: Role::Judgment, parent: None },
    KindSpec { id: "dep", layer: Layer::Semantic, domain: SubjectShape::Dep, role: Role::Judgment, parent: None },
    KindSpec { id: "file", layer: Layer::Syntax, domain: SubjectShape::File, role: Role::Judgment, parent: None },
    KindSpec { id: "range", layer: Layer::Syntax, domain: SubjectShape::Range, role: Role::Judgment, parent: None },
];

pub fn edge_kind_spec(id: &str) -> Option<&'static KindSpec> {
    EDGE_KINDS.iter().find(|k| k.id == id)
}

pub fn judgment_kind_spec(id: &str) -> Option<&'static KindSpec> {
    JUDGMENT_KINDS.iter().find(|k| k.id == id)
}

/// 注册表不变量（启动/测试时调用一次；两表分别校验）。
pub fn validate_registry() -> Result<(), String> {
    for (label, kinds, role) in [
        ("edge", EDGE_KINDS, Role::Edge),
        ("judgment", JUDGMENT_KINDS, Role::Judgment),
    ] {
        let mut seen = std::collections::BTreeSet::new();
        for k in kinds {
            if k.role != role {
                return Err(format!("{label} registry contains non-{label} kind {}", k.id));
            }
            if !seen.insert(k.id) {
                return Err(format!("duplicate {label} kind id: {}", k.id));
            }
            if let Some(p) = k.parent {
                if p == k.id {
                    return Err(format!("kind {} is its own parent", k.id));
                }
                let Some(_ps) = kinds.iter().find(|x| x.id == p) else {
                    return Err(format!("kind {} has missing parent {}", k.id, p));
                };
            }
            if role == Role::Edge && matches!(k.domain, SubjectShape::Dep | SubjectShape::Doc) {
                return Err(format!("edge kind {} cannot have domain {:?}", k.id, k.domain));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loc {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub src: Subject,
    pub kind: &'static str,
    pub dst: Endpoint,
    pub leaf: Option<String>,
    pub loc: Loc,
}

impl Edge {
    /// 构造时校验：kind 必须已注册、role 必须是 Edge、src 的 shape 必须匹配 domain。
    pub fn new(
        src: Subject,
        kind: &'static str,
        dst: Endpoint,
        leaf: Option<String>,
        loc: Loc,
    ) -> Result<Self, String> {
        let spec = edge_kind_spec(kind).ok_or_else(|| format!("unregistered edge kind: {kind}"))?;
        if spec.role != Role::Edge {
            return Err(format!("kind {kind} is not an edge"));
        }
        if src.shape() != spec.domain {
            return Err(format!(
                "kind {kind} expects domain {:?}, got {:?}",
                spec.domain,
                src.shape()
            ));
        }
        Ok(Self { src, kind: spec.id, dst, leaf, loc })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Held,
    Refuted,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgment {
    pub subject: Subject,
    pub kind: &'static str,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub at: i64,
}

impl Judgment {
    /// Uncertain 必须带 reason（否则前端无法解释"为什么不确定"）。
    pub fn new(
        subject: Subject,
        kind: &'static str,
        verdict: Verdict,
        reason: Option<String>,
        at: i64,
    ) -> Result<Self, String> {
        let spec = judgment_kind_spec(kind).ok_or_else(|| format!("unregistered judgment kind: {kind}"))?;
        if spec.role != Role::Judgment {
            return Err(format!("kind {kind} is not a judgment"));
        }
        if subject.shape() != spec.domain {
            return Err(format!(
                "judgment kind {kind} expects domain {:?}, got {:?}",
                spec.domain,
                subject.shape()
            ));
        }
        if verdict == Verdict::Uncertain && reason.as_deref().unwrap_or("").is_empty() {
            return Err("uncertain judgment requires a reason".to_string());
        }
        Ok(Self { subject, kind: spec.id, verdict, reason, at })
    }
}

/// 视图：纯函数 `(Subject) -> ViewId`，不落库。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewId {
    File,
    Symbol,
    Test,
    Range,
    Doc,
    Dep,
    Type,
}

pub fn view_for(subject: &Subject) -> ViewId {
    match subject.shape() {
        SubjectShape::File => ViewId::File,
        SubjectShape::Symbol => ViewId::Symbol,
        SubjectShape::Test => ViewId::Test,
        SubjectShape::Range => ViewId::Range,
        SubjectShape::Doc => ViewId::Doc,
        SubjectShape::Dep => ViewId::Dep,
        SubjectShape::Type => ViewId::Type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_valid() {
        validate_registry().expect("registry invariants");
        // 每个 id 都能被查到，且 parent 链可终止
        for k in EDGE_KINDS.iter().chain(JUDGMENT_KINDS.iter()) {
            let lookup = if k.role == Role::Edge { edge_kind_spec(k.id) } else { judgment_kind_spec(k.id) };
            assert!(lookup.is_some());
            let mut cur = k.parent;
            let mut depth = 0;
            while let Some(p) = cur {
                depth += 1;
                assert!(depth < 16, "parent chain too deep for {}", k.id);
                cur = (if k.role == Role::Edge { edge_kind_spec(p) } else { judgment_kind_spec(p) }).and_then(|s| s.parent);
            }
        }
    }

    #[test]
    fn subject_roundtrip_all_shapes() {
        let cases = [
            "file:src/lib.rs",
            "symbol:src/lib.rs#Foo",
            "symbol:src/lib.rs#r#async",
            "test:tests/a.rs#works",
            "range:crates/core",
            "doc:docs/x.md",
            "dep:serde",
        ];
        for c in cases {
            let s = Subject::parse(c).expect("parse");
            assert_eq!(s.canonical(), c, "roundtrip {c}");
        }
        // r# 名字必须整体保留
        match Subject::parse("symbol:a.rs#r#async").unwrap() {
            Subject::Symbol { name, .. } => assert_eq!(name, "r#async"),
            other => panic!("unexpected {other:?}"),
        }
        // 裸路径 → File
        assert_eq!(Subject::parse("crates/core").unwrap(), Subject::File("crates/core".into()));
    }

    #[test]
    fn subject_rejects_bad_forms() {
        for bad in ["", "symbol:a.rs", "symbol:#x", "symbol:a.rs#", "file:"] {
            assert!(Subject::parse(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn edge_constructor_enforces_domain_and_kind() {
        let ok = Edge::new(
            Subject::Symbol { path: "a.rs".into(), name: "f".into() },
            "call",
            Endpoint::Subject(Subject::Symbol { path: "b.rs".into(), name: "g".into() }),
            Some("g".into()),
            Loc { file: "a.rs".into(), line: 7 },
        );
        assert!(ok.is_ok());
        // 用错 domain
        let bad = Edge::new(
            Subject::File("a.rs".into()),
            "call",
            Endpoint::Unknown("unresolved".into()),
            None,
            Loc { file: "a.rs".into(), line: 1 },
        );
        assert!(bad.is_err());
        // 未注册 kind
        assert!(Edge::new(
            Subject::File("a.rs".into()),
            "nope",
            Endpoint::Unknown("x".into()),
            None,
            Loc { file: "a.rs".into(), line: 1 },
        )
        .is_err());
        // judgment kind 不能当 edge
        assert!(Edge::new(
            Subject::Symbol { path: "a.rs".into(), name: "f".into() },
            "symbol",
            Endpoint::Unknown("x".into()),
            None,
            Loc { file: "a.rs".into(), line: 1 },
        )
        .is_err());
    }

    #[test]
    fn judgment_requires_reason_when_uncertain() {
        let s = Subject::File("a.rs".into());
        assert!(Judgment::new(s.clone(), "file", Verdict::Held, None, 1).is_ok());
        assert!(Judgment::new(s.clone(), "file", Verdict::Uncertain, None, 1).is_err());
        assert!(Judgment::new(s, "file", Verdict::Uncertain, Some("no_data".into()), 1).is_ok());
    }

    #[test]
    fn impl_src_is_type_not_file_or_symbol() {
        // impl 的 src 是 type:Trait（不是文件、不是带路径 symbol）
        let src = Subject::parse("type:Display").expect("parse type");
        assert_eq!(src.shape(), SubjectShape::Type);
        let e = Edge::new(
            src,
            "impl",
            Endpoint::Subject(Subject::File("a.rs".into())),
            Some("a.rs".into()),
            Loc { file: "a.rs".into(), line: 3 },
        );
        assert!(e.is_ok(), "impl 应以 Type 为 src");
        // 用 File 当 impl 的 src 必须被注册表拒绝（防止静默错分类）
        assert!(Edge::new(
            Subject::File("a.rs".into()),
            "impl",
            Endpoint::Unknown("x".into()),
            None,
            Loc { file: "a.rs".into(), line: 1 },
        )
        .is_err());
    }

    #[test]
    fn view_mapping_is_total() {
        let cases = [
            (Subject::File("a".into()), ViewId::File),
            (Subject::Symbol { path: "a".into(), name: "f".into() }, ViewId::Symbol),
            (Subject::Test { path: "a".into(), name: "t".into() }, ViewId::Test),
            (Subject::Range("crates".into()), ViewId::Range),
            (Subject::Doc("d.md".into()), ViewId::Doc),
            (Subject::Dep("serde".into()), ViewId::Dep),
            (Subject::Type("Trait".into()), ViewId::Type),
        ];
        for (s, v) in cases {
            assert_eq!(view_for(&s), v);
        }
    }
}
