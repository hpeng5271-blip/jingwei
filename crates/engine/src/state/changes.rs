//! 「这个项目刚才变了什么」。
//!
//! 新手最难受的一刻不是报错，是「我刚才到底做了什么」——命令跑完、
// AI 改完，屏幕上多了几个文件，然后一切照旧。这里把「改了什么」变成
// 一句人话，顺带把隐患的增删也一起对上：改完之后是更危险了还是更干净了。
//!
//! 只在项目内看，跳过产物目录（node_modules 每天都在动，报它没有意义）。
use crate::facts::now_iso;
use crate::project;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

const MAX_FILES: usize = 4000;
const MAX_LINES: usize = 6;
/// 变化卡片最多列几个文件名，再多就变成刷屏了
const NAME_SHOW: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Risk {
    pub id: String,
    pub title: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Digest {
    /// 第一次记录：没有可比的上一次
    pub first_run: bool,
    pub from: String,
    pub at: String,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    /// 一句话结论
    pub headline: String,
    /// 4-6 条人话，第一条最重要
    pub lines: Vec<String>,
    pub new_risks: Vec<Risk>,
    pub fixed_risks: Vec<Risk>,
}

impl Digest {
    pub fn is_empty(&self) -> bool {
        !self.first_run
            && self.added.is_empty()
            && self.changed.is_empty()
            && self.removed.is_empty()
            && self.new_risks.is_empty()
            && self.fixed_risks.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Watch {
    #[serde(default)]
    schema: u32,
    at: String,
    /// 路径 -> 一个文件的三个指纹
    files: BTreeMap<String, Stamp>,
    risks: Vec<Risk>,
}

/// 一个文件的指纹。
/// `hash` 只在"深比对"时才填：它解决的是 NTFS 上的一个真实盲区——
/// 连续两次写入可能落在同一个时间戳、且长度相同，只看 mtime+size 会漏报。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Stamp {
    mtime: u64,
    size: u64,
    #[serde(default)]
    hash: u64,
}

const SCHEMA: u32 = 2;
/// 深比对时最多读这么多字节（大文件只看头部，够判断"是不是同一份"）
const HASH_CAP: usize = 256 * 1024;

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn file_hash(p: &Path) -> u64 {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(p) else { return 0 };
    let mut buf = vec![0u8; HASH_CAP];
    let n = f.read(&mut buf).unwrap_or(0);
    fnv(&buf[..n])
}

fn watch_path(root: &Path) -> std::path::PathBuf {
    crate::data_dir(root).join("watch.json")
}

fn skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".jingwei"
            | ".drydock"
            | ".pylaunch"
            | ".idea"
            | ".vscode"
            | "node_modules"
            | "target"
    )
}

fn stamps(root: &Path, deep: bool) -> BTreeMap<String, Stamp> {
    let mut out = BTreeMap::new();
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 6 || out.len() >= MAX_FILES {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let p = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                let n = name.to_lowercase();
                if skip_dir(&n) {
                    continue;
                }
                // 产物目录也要跳：它们的变动是「构建跑过了」，不是「你改了代码」
                if project::artifact_kind(&p, &dir, &name).is_some() {
                    continue;
                }
                stack.push((p, depth + 1));
                continue;
            }
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(md) = e.metadata() else { continue };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                // 用纳秒而不是毫秒：连续两次保存可能落在同一毫秒里，
                // 而「同一毫秒 + 同样长度」就会被当成没变（这是真漏报）。
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let size = md.len();
            // 深比对才读内容：普通轮询只看 mtime+size（快，且够用）
            let hash = if deep && size <= HASH_CAP as u64 {
                file_hash(&p)
            } else {
                0
            };
            out.insert(rel, Stamp { mtime, size, hash });
            if out.len() >= MAX_FILES {
                break;
            }
        }
    }
    out
}

/// 项目里当前所有体检条目（只取会变的那几个字段）。
fn risks_now(root: &Path) -> Vec<Risk> {
    project::scan(root)
        .items
        .into_iter()
        .map(|f| Risk {
            id: f.id,
            title: f.title,
            severity: match f.severity {
                jingwei_model::Severity::Error => "error".into(),
                jingwei_model::Severity::Warn => "warn".into(),
                jingwei_model::Severity::Info => "info".into(),
            },
        })
        .collect()
}

/// 文件在「哪一类」里。分类只为了说人话，不做判断。
fn kind_of(path: &str) -> &'static str {
    let base = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let in_deps = matches!(
        base.as_str(),
        "package.json"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "requirements.txt"
            | "pyproject.toml"
            | "cargo.toml"
            | "go.mod"
            | "composer.json"
            | "gemfile"
            | "pipfile"
            | "settings.gradle"
    ) || base.ends_with(".csproj");
    if in_deps {
        return "依赖清单";
    }
    if base.starts_with("readme") || base.ends_with(".md") || base.ends_with(".txt") {
        return "说明文档";
    }
    if base.starts_with('.')
        || matches!(
            base.rsplit('.').next().unwrap_or(""),
            "json" | "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "properties"
        )
    {
        return "配置";
    }
    if matches!(
        base.rsplit('.').next().unwrap_or(""),
        "py" | "js" | "ts" | "jsx" | "tsx" | "java" | "kt" | "c" | "h" | "cpp" | "hpp" | "cc"
            | "rs" | "go" | "cs" | "php" | "rb" | "vue" | "sql" | "sh" | "ps1"
    ) {
        return "源码";
    }
    "其他"
}

fn show(names: &[String]) -> String {
    let mut s = names
        .iter()
        .take(NAME_SHOW)
        .map(|n| n.rsplit('/').next().unwrap_or(n).to_string())
        .collect::<Vec<_>>()
        .join("、");
    if names.len() > NAME_SHOW {
        s.push_str(&format!(" 等 {} 个", names.len()));
    }
    s
}

/// 读一遍现在的状态，和上次记下的比，然后把新的状态记回去。
///
/// 故意是「取走」语义：报过一次就不再重复报，否则每轮轮询都会把同一批改动
/// 再念一遍，人很快就不看了。
pub fn take(root: &Path) -> Result<Digest> {
    take_inner(root, false)
}

/// 深比对：连内容一起对。用于「重新扫描」按钮与定时校验——
/// 普通轮询只比 mtime+size，遇到"同一时间戳同样长度"的写入会漏。
pub fn take_deep(root: &Path) -> Result<Digest> {
    take_inner(root, true)
}

fn take_inner(root: &Path, deep: bool) -> Result<Digest> {
    let now = stamps(root, deep);
    let risks = risks_now(root);
    let prev: Watch = std::fs::read_to_string(watch_path(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(|w: &Watch| w.schema == 0 || w.schema == SCHEMA)
        .unwrap_or_default();
    let first_run = prev.at.is_empty();

    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    for (p, st) in &now {
        match prev.files.get(p) {
            None => added.push(p.clone()),
            Some(old) => {
                // 深比对认内容；浅比对认时间戳+大小
                let differs = if deep && st.hash != 0 && old.hash != 0 {
                    st.hash != old.hash
                } else {
                    st.mtime != old.mtime || st.size != old.size
                };
                if differs {
                    changed.push(p.clone());
                }
            }
        }
    }
    for p in prev.files.keys() {
        if !now.contains_key(p) {
            removed.push(p.clone());
        }
    }

    let mut new_risks = Vec::new();
    let mut fixed_risks = Vec::new();
    for r in &risks {
        if !prev.risks.iter().any(|x| x.id == r.id) {
            new_risks.push(r.clone());
        }
    }
    for r in &prev.risks {
        if !risks.iter().any(|x| x.id == r.id) {
            fixed_risks.push(r.clone());
        }
    }

    let at = now_iso();
    let w = Watch {
        schema: SCHEMA,
        at: at.clone(),
        files: now,
        risks,
    };
    if let Ok(t) = serde_json::to_string_pretty(&w) {
        let dir = crate::data_dir(root);
        // 写不进去不影响**本次**结论，但下次对比就没有基线，变更检测会显得"什么都没发现"。
        // 这种失败必须留痕——否则用户遇到的是"它怎么不告诉我改了什么"，而我们查不出原因。
        if let Err(e) = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join("watch.json"), t))
        {
            crate::dlog::w("9002", &format!("变更基线写入失败（{}）：{e}", dir.display()));
        }
    }

    let mut d = Digest {
        first_run,
        from: prev.at,
        at,
        added,
        changed,
        removed,
        new_risks,
        fixed_risks,
        ..Default::default()
    };
    d.headline = headline(&d);
    d.lines = lines(&d);
    Ok(d)
}

fn count_kind(names: &[String], kind: &str) -> usize {
    names.iter().filter(|n| kind_of(n) == kind).count()
}

fn headline(d: &Digest) -> String {
    if d.first_run {
        return "从这里开始记账：以后你（或者 AI）每改一次，我都能说清改了哪儿。".into();
    }
    let src = count_kind(&d.changed, "源码") + count_kind(&d.added, "源码");
    let deps = count_kind(&d.changed, "依赖清单") + count_kind(&d.added, "依赖清单");
    let touched = d.changed.len() + d.added.len() + d.removed.len();
    if touched == 0 && d.new_risks.is_empty() && d.fixed_risks.is_empty() {
        return "文件没动过。".into();
    }
    let mut s = if src > 0 {
        format!("改了 {} 个源码文件", src)
    } else {
        format!("动了 {} 个文件", touched)
    };
    if deps > 0 {
        s.push_str("，其中包含依赖清单（改完要重新装一次依赖）");
    }
    if !d.new_risks.is_empty() {
        s.push_str(&format!("；多出 {} 条要注意的", d.new_risks.len()));
    } else if !d.fixed_risks.is_empty() {
        s.push_str(&format!("；顺手排掉了 {} 条隐患", d.fixed_risks.len()));
    }
    s
}

fn lines(d: &Digest) -> Vec<String> {
    if d.first_run {
        return vec!["这条记录放在 .jingwei/watch.json，跟笔记放在一起，不会跟着提交。".into()];
    }
    if d.is_empty() {
        return vec![];
    }
    let mut out: Vec<String> = Vec::new();

    let src_new = d
        .added
        .iter()
        .filter(|n| kind_of(n) == "源码")
        .cloned()
        .collect::<Vec<_>>();
    let src_mod = d
        .changed
        .iter()
        .filter(|n| kind_of(n) == "源码")
        .cloned()
        .collect::<Vec<_>>();
    if !src_mod.is_empty() {
        out.push(format!("改过源码：{}", show(&src_mod)));
    }
    if !src_new.is_empty() {
        out.push(format!("新增源码：{}", show(&src_new)));
    }
    let deps: Vec<String> = d
        .added
        .iter()
        .chain(d.changed.iter())
        .filter(|n| kind_of(n) == "依赖清单")
        .cloned()
        .collect();
    if !deps.is_empty() {
        out.push(format!(
            "依赖清单动了：{} —— 现在跑一次安装才算真装上",
            show(&deps)
        ));
    }
    let cfg: Vec<String> = d
        .added
        .iter()
        .chain(d.changed.iter())
        .filter(|n| kind_of(n) == "配置")
        .cloned()
        .collect();
    if !cfg.is_empty() {
        out.push(format!("配置改了：{}", show(&cfg)));
    }
    if !d.removed.is_empty() {
        out.push(format!("删掉了 {} 个文件：{}", d.removed.len(), show(&d.removed)));
    }
    if !d.new_risks.is_empty() {
        out.push(format!(
            "新多出来的注意点：{}",
            d.new_risks
                .iter()
                .take(2)
                .map(|r| r.title.clone())
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    if !d.fixed_risks.is_empty() {
        out.push(format!(
            "已经解决的：{}",
            d.fixed_risks
                .iter()
                .take(2)
                .map(|r| r.title.clone())
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    out.truncate(MAX_LINES);
    out
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-chg-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_first_look_only_records() {
        let d = scratch("first");
        std::fs::write(d.join("main.py"), "print(1)").unwrap();
        let g = take(&d).unwrap();
        assert!(g.first_run);
        assert!(!g.lines.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_source_edit_shows_up_as_a_source_edit() {
        let d = scratch("edit");
        std::fs::write(d.join("main.py"), "print(1)\n").unwrap();
        take(&d).unwrap();
        // 浅比对认「时间戳 + 长度」：测试里要跨过同一时间戳的物理边界。
        // 长度也变了才稳（真实编辑通常也改长度；只改长度不变的情况由深比对兜住）。
        std::thread::sleep(std::time::Duration::from_millis(5));
        std::fs::write(d.join("main.py"), "print(22)\n").unwrap();
        let g = take(&d).unwrap();
        assert!(!g.first_run);
        assert_eq!(g.changed, vec!["main.py".to_string()]);
        assert!(g.headline.contains("源码"), "{}", g.headline);
        assert!(g.lines[0].contains("main.py"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 同一时间戳 + 同样长度的写入：浅比对认不出来（NTFS 上真会发生），
    /// 深比对认内容，必须报出来。这两条一起钉住这个边界。
    #[test]
    fn same_size_same_tick_edit_is_caught_by_deep_compare() {
        let d = scratch("deepeq");
        std::fs::write(d.join("app.js"), "let a = 1;").unwrap();
        take_deep(&d).unwrap();          // 深比对要先有一次深基线（第一次深扫会把 hash 存下来）
        std::fs::write(d.join("app.js"), "let a = 2;").unwrap();   // 长度相同
        let g = take_deep(&d).unwrap();
        assert!(
            g.changed == vec!["app.js".to_string()],
            "深比对必须认出内容变了：{:?}",
            g.changed
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 没有任何改动时，深比对也要闭嘴。
    #[test]
    fn deep_compare_stays_quiet_when_nothing_changed() {
        let d = scratch("deepquiet");
        std::fs::write(d.join("a.py"), "x = 1\n").unwrap();
        take_deep(&d).unwrap();
        let g = take_deep(&d).unwrap();
        assert!(g.is_empty(), "{:?}", g.lines);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 报过一次就不再念第二遍——否则每轮轮询都会刷同一批改动。
    #[test]
    fn a_change_is_reported_once() {
        let d = scratch("once");
        std::fs::write(d.join("a.js"), "1").unwrap();
        take(&d).unwrap();
        // 解析度是毫秒 + 字节数：同一毫秒内改出同样长度，这里认不出来。
        // 真人手改不会这么巧；写测试得绕开这个物理边界。
        std::thread::sleep(std::time::Duration::from_millis(5));
        std::fs::write(d.join("a.js"), "22").unwrap();
        assert!(!take(&d).unwrap().is_empty());
        assert!(take(&d).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 产物目录里的动静不算「你的改动」。
    #[test]
    fn artifact_churn_is_ignored() {
        let d = scratch("artifacts");
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        take(&d).unwrap();
        std::fs::create_dir_all(d.join("node_modules").join("pkg")).unwrap();
        std::fs::write(d.join("node_modules").join("pkg").join("a.js"), "1").unwrap();
        assert!(take(&d).unwrap().is_empty(), "node_modules 的变化不该报出来");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 依赖清单动了要点名说「得重新装」。
    #[test]
    fn a_dependency_edit_says_to_reinstall() {
        let d = scratch("deps");
        std::fs::write(d.join("package.json"), "{\"name\":\"x\"}").unwrap();
        take(&d).unwrap();
        std::fs::write(d.join("package.json"), "{\"name\":\"x\",\"version\":\"1\"}").unwrap();
        let g = take(&d).unwrap();
        assert!(g.headline.contains("依赖清单"), "{}", g.headline);
        assert!(g.lines.iter().any(|l| l.contains("安装")), "{:?}", g.lines);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 补上 .gitignore 应该被认成「排掉了一条隐患」。
    #[test]
    fn fixing_gitignore_reads_as_a_resolved_risk() {
        let d = scratch("fixed");
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        take(&d).unwrap();
        std::fs::write(d.join(".gitignore"), "node_modules/\n").unwrap();
        let g = take(&d).unwrap();
        assert!(
            g.fixed_risks.iter().any(|r| r.id.starts_with("gitignore")),
            "{:?}",
            g.fixed_risks
        );
        assert!(g.lines.iter().any(|l| l.contains("已经解决")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn at_most_six_lines() {
        let d = scratch("cap");
        std::fs::write(d.join("a.py"), "1").unwrap();
        take(&d).unwrap();
        for n in ["a.py", "b.py", "c.json", ".gitignore", "readme.md", "d.py", "e.py"] {
            std::fs::write(d.join(n), "x").unwrap();
        }
        std::fs::remove_file(d.join("a.py")).unwrap();
        let g = take(&d).unwrap();
        assert!(g.lines.len() <= MAX_LINES, "{:?}", g.lines);
        let _ = std::fs::remove_dir_all(&d);
    }
}
