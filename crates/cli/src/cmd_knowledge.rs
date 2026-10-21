//! `jingwei knowledge check`：**知识条目的严筛**——双头漏斗最细的那道颈（2026-10-20，pro 任务卡 A）。
//!
//! 为什么要有它：采集与二次编制如果只靠文档自觉，条目会越写越像"感觉"，
//! 而这道颈的判据只有一句话：**没有来源、没有等级、没有判据的条目，不算过筛**；
//! **关联符号在图谱里查不到的，只能算"查不了"，不许算"挂上图谱"**（三态，不许压扁）。
//!
//! 条目格式（`docs/knowledge/*.md`，front-matter 用最简单的 `key: value`，不引 YAML 依赖）：
//!
//! ```text
//! ---
//! claim: 一句话主张
//! source: https://… 或 docs/…（仓库内相对路径必须真的存在）
//! level: 一手 | 转写 | 二手 | 查不了
//! check: 怎么验（命令或判据）
//! symbol: symbol:crates/engine/src/graph/store.rs#Store::apply_file_update
//! ---
//! ```
//!
//! 输出一张漏斗表：`采集 N / 过筛 M / 挂图谱 K / 缺口 J`，缺口逐条列名与原因。

use std::collections::BTreeMap;
use std::path::Path;

use jingwei_engine::graph::store::Store;

/// 等级枚举与三态对齐：`查不了` 就是三态里的那个"查不了"，不是"没有"。
pub const LEVELS: [&str; 4] = ["一手", "转写", "二手", "查不了"];
/// 一条条目要"过筛"必须齐全的四项。
const REQUIRED: [&str; 4] = ["claim", "source", "level", "check"];

#[derive(Debug, Default, Clone)]
pub struct Entry {
    pub file: String,
    pub fields: BTreeMap<String, String>,
    pub symbols: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub total: usize,
    /// 四字段齐全（来源/等级/判据/主张）。
    pub passed: usize,
    /// 关联符号全部在图谱里查得到。
    pub linked: usize,
    /// 缺字段（**不许算过筛**）。
    pub problems: Vec<String>,
    /// 三态里的"查不了"：符号查不到、或没有图谱。
    pub gaps: Vec<String>,
    /// **文本态**：过筛了但没挂任何符号（按符号遍历不到）。只列名字，不算红。
    pub unmapped: Vec<String>,
    /// 等级分布（按过筛条目数）：一手 / 转写 / 二手 / 查不了。
    /// 这是"过滤分级"本身的可见性——只报"过筛 16"看不出这 16 条有多硬。
    pub levels: std::collections::BTreeMap<String, usize>,
    pub no_graph: bool,
    /// 待确认区（`_inbox/`）的候选数——**不计入 total**（候选不是条目）。
    pub inbox: usize,
    /// 候选核对失败（来源失效 / claim 与原文不一致）。
    pub bad_candidates: Vec<String>,
}

/// 解析一条条目。front-matter 之外的内容不参与判定（正文可以是笔记）。
pub fn parse_entry(file: &str, text: &str) -> Entry {
    let mut fields = BTreeMap::new();
    let mut symbols = Vec::new();
    // **BOM 不是空白**：PowerShell 写的文件常带它，`trim()` 不认（踩过）。
    let t = text.trim_start_matches('\u{feff}');
    let mut lines = t.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return Entry {
            file: file.to_string(),
            fields,
            symbols,
        };
    }
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        // **只剥"整值被引号包住"的那一层**，不做 `trim_matches('"')`：
        // 后者会把值**内部**的引号也吃掉——实测过：claim 写成 `……显示成"没有"` 时，
        // 结尾那个引号被剥掉，条目文字与原文对不上（而 claim 逐字是候选的硬规矩）。
        let v = v.trim();
        let v = if v.len() > 1
            && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
        {
            v[1..v.len() - 1].trim().to_string()
        } else {
            v.to_string()
        };
        if k == "symbol" || k == "symbols" {
            for s in v.split(',') {
                let s = s.trim();
                if !s.is_empty() {
                    symbols.push(s.to_string());
                }
            }
        } else {
            fields.insert(k, v);
        }
    }
    Entry {
        file: file.to_string(),
        fields,
        symbols,
    }
}

/// 符号能不能在图谱里查出来。`symbol:` 前缀按 id 查，否则按名字查（同名多义也算"有"）。
fn symbol_ok(store: &Store, s: &str) -> bool {
    if s.starts_with("symbol:") {
        store.node(s).map(|n| n.is_some()).unwrap_or(false)
    } else {
        store
            .symbol_ids_by_name(s)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }
}

/// 来源是 URL 就放行；**看起来像仓库内路径的，必须真的存在**（文档承诺的文件不存在 = 典型事故）；
/// 其余的（书名、论文标题、"查不了"）不是路径，放行。
///
/// `docs/research/x.md:12` 这种「文件:行号」也算路径——行号只用于回查原文，不参与存在性判断。
/// **多个来源用 `;` 分隔，逐个都要过**：合并条目时不许把另一条来源悄悄丢掉
/// （2026-10-20 pro 的监督条件：合并后 source 必须覆盖原来两条的并集）。
fn source_ok(root: &Path, v: &str) -> bool {
    let all = v.trim();
    if all.is_empty() {
        return false;
    }
    all.split(';').all(|part| one_source_ok(root, part))
}

fn one_source_ok(root: &Path, v: &str) -> bool {
    let s = v.trim().trim_matches('`').trim();
    if s.is_empty() {
        return false;
    }
    if s.starts_with("http://") || s.starts_with("https://") || s == "查不了" {
        return true;
    }
    let core = strip_line_no(s);
    let looks_path = core.contains('/')
        || [".md", ".rs", ".json", ".txt", ".toml"]
            .iter()
            .any(|e| core.ends_with(e));
    if looks_path {
        return root.join(core).exists();
    }
    true
}

/// 去掉末尾的 `:<行号>`（`docs/a.md:12` → `docs/a.md`）。没有就当原样。
pub fn strip_line_no(s: &str) -> &str {
    match s.rsplit_once(':') {
        Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => s,
    }
}

/// `path:line` → (path, line)。行号不是数字时返回 None。
pub fn split_source(v: &str) -> Option<(&str, u32)> {
    let (head, tail) = v.rsplit_once(':')?;
    let n: u32 = tail.parse().ok()?;
    Some((head, n))
}

/// 待确认区：`docs/knowledge/_inbox/`。
pub fn inbox_dir(root: &Path) -> std::path::PathBuf {
    root.join("docs").join("knowledge").join("_inbox")
}

/// **核对候选**：`claim` 必须是 `source` 那行的原文子串。
/// 源文件删了那行、或者 claim 被人改写 → 这里报「来源失效 / 与原文不一致」。
pub fn verify_inbox(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(inbox_dir(root)) else {
        return out;
    };
    let mut files: Vec<std::path::PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
        .collect();
    files.sort();
    for p in files {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let e = parse_entry(&name, &text);
        let Some(src) = e.fields.get("source") else {
            out.push(format!("_inbox/{name} 缺 source——候选必须写清来自哪一行"));
            continue;
        };
        let Some((path, line)) = split_source(src) else {
            out.push(format!(
                "_inbox/{name} 的 source 应为 `文件:行号`（现在是 `{src}`）"
            ));
            continue;
        };
        let Ok(body) = std::fs::read_to_string(root.join(path)) else {
            out.push(format!(
                "_inbox/{name} 的来源文件不存在：{path}——来源失效"
            ));
            continue;
        };
        let Some(row) = body.lines().nth((line.saturating_sub(1)) as usize) else {
            out.push(format!(
                "_inbox/{name} 的来源行不存在：{path}:{line}——来源失效"
            ));
            continue;
        };
        let claim = e.fields.get("claim").cloned().unwrap_or_default();
        if !row.contains(&claim) {
            out.push(format!(
                "_inbox/{name} 的 claim 与原文不一致（{path}:{line}）——要么改写过了，要么源文件变了"
            ));
        }
    }
    out
}

/// 目录里所有**条目**文件（README 与 `_` 草稿不算，否则一条说明就让漏斗永远红）。
pub fn entry_files(root: &Path) -> Vec<std::path::PathBuf> {
    let dir = root.join("docs").join("knowledge");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<std::path::PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
        .filter(|p| {
            let n = p
                .file_name()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_default();
            n != "README.md" && !n.starts_with('_')
        })
        .collect();
    files.sort();
    files
}

/// 读出所有条目（`learn` 与 `check` 共用同一份解析，别各写一套）。
pub fn load_entries(root: &Path) -> Vec<Entry> {
    entry_files(root)
        .into_iter()
        .map(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            parse_entry(&name, &text)
        })
        .collect()
}

/// 条目名字（去掉 `.md`）——`prereq:` 就是按这个名字引用的。
pub fn entry_stem(name: &str) -> &str {
    name.strip_suffix(".md").unwrap_or(name)
}

/// 一条条目过没过严筛。返回 Err(原因) 时**不许**把它当成"已核实的知识"。
pub fn why_rejected(root: &Path, e: &Entry) -> Option<String> {
    let mut missing: Vec<&str> = Vec::new();
    for k in REQUIRED {
        match e.fields.get(k) {
            Some(v) if !v.trim().is_empty() => {}
            _ => missing.push(k),
        }
    }
    if !missing.is_empty() {
        return Some(format!("缺 {}（没有这些就不算过筛）", missing.join(" / ")));
    }
    if let Some(lv) = e.fields.get("level") {
        if !LEVELS.contains(&lv.as_str()) {
            return Some(format!("level = `{lv}`，只能是 一手/转写/二手/查不了"));
        }
    }
    if let Some(src) = e.fields.get("source") {
        if !source_ok(root, src) {
            return Some("source 指向的仓库内路径不存在".to_string());
        }
    }
    None
}

/// 严筛一遍 `docs/knowledge/`。`store` 为 None = 没有图谱（图谱侧的判定只能"查不了"）。
pub fn evaluate(root: &Path, store: Option<&Store>) -> Report {
    let mut r = Report {
        no_graph: store.is_none(),
        ..Default::default()
    };
    for e in load_entries(root) {
        let name = e.file.clone();
        r.total += 1;

        if let Some(why) = why_rejected(root, &e) {
            r.problems.push(format!("{name} {why}"));
            continue;
        }
        r.passed += 1;
        // 等级分布：只统计**过筛**的条目（红的不算进"分级"里，免得把没资格的分级也算进来）
        if let Some(lv) = e.fields.get("level") {
            *r.levels.entry(lv.clone()).or_insert(0) += 1;
        }

        if e.symbols.is_empty() {
            // **文本态**（没挂符号）的要能看见名字，不能只看到一个数字：
            // 挂不上符号的条目按符号遍历不到，等于只在 Markdown 里睡着。
            r.unmapped.push(crate::cmd_explain::short_name(&e.file));
            continue;
        }
        match store {
            None => r.gaps.push(format!(
                "{} 关联了 {} 个符号，但没有图谱——查不了（先 jingwei index）",
                e.file,
                e.symbols.len()
            )),
            Some(st) => {
                let bad: Vec<&String> = e.symbols.iter().filter(|s| !symbol_ok(st, s)).collect();
                if bad.is_empty() {
                    r.linked += 1;
                } else {
                    r.gaps.push(format!(
                        "{} 关联的符号图谱里查不到：{}——查不了",
                        e.file,
                        bad.iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    ));
                }
            }
        }
    }
    // **候选区单独核**：候选不算条目（不计入 total），但"来源失效/被改写"必须红。
    r.inbox = std::fs::read_dir(inbox_dir(root))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    r.bad_candidates = verify_inbox(root);
    r
}

/// 投影对账：Markdown 是真源，`claims` 表是投影。两边不一致 → 返回一句人话。
///
/// **为什么要有它**（pro 任务卡 H）：`stale`/`symbol` 现在都走库，库一旦落后于 Markdown，
/// 判断就静默错了。所以 `check` 负责把"库与文件漂移"变成可见的红。
/// 比的是 id + 四字段 + **符号条数**：改名/加符号都会让条数变化。
pub fn projection_drift(root: &Path, store: &Store) -> Option<String> {
    use std::collections::BTreeMap;
    // 文件侧的符号也要按**同一规则**解析（裸名字 → 唯一 id），否则会因写法不同误报漂移。
    let want: BTreeMap<String, (String, String, String, String, Vec<String>)> = claim_rows(root)
        .into_iter()
        .map(|r| {
            let mut syms: Vec<String> = r
                .symbols
                .iter()
                .map(|s| store.resolve_symbol_ref(s))
                .collect();
            syms.sort();
            (r.id, (r.claim, r.level, r.source, r.topic, syms))
        })
        .collect();
    let have: BTreeMap<String, (String, String, String, String, Vec<String>)> = store
        .claims_snapshot()
        .ok()?
        .into_iter()
        .map(|r| (r.id, (r.claim, r.level, r.source, r.topic, r.symbols)))
        .collect();
    if want == have {
        return None;
    }
    let only_file: Vec<&String> = want.keys().filter(|k| !have.contains_key(*k)).collect();
    let only_db: Vec<&String> = have.keys().filter(|k| !want.contains_key(*k)).collect();
    let changed: Vec<&String> = want
        .iter()
        .filter(|(k, v)| have.get(*k).map(|h| h != *v).unwrap_or(false))
        .map(|(k, _)| k)
        .collect();
    let mut parts = Vec::new();
    if !only_file.is_empty() {
        parts.push(format!("只在文件里：{}", only_file.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" / ")));
    }
    if !only_db.is_empty() {
        parts.push(format!("只在库里：{}", only_db.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" / ")));
    }
    if !changed.is_empty() {
        parts.push(format!("内容不一致：{}", changed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" / ")));
    }
    Some(format!(
        "图谱主张投影与 Markdown 漂移（{}）——跑 jingwei knowledge sync",
        parts.join("；")
    ))
}

/// 把某条目里一个查不到的符号改挂到新符号（**精确匹配**，不许模糊）。
pub fn fix_symbol(root: &Path, store: Option<&Store>, entry: &str, old: &str, new: &str) -> Result<(), String> {
    let st = store.ok_or_else(|| "没有图谱（先 jingwei index）".to_string())?;
    let path = root.join("docs").join("knowledge").join(entry);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读不了 {entry}：{e}"))?;
    // 旧符号必须真的在文件里、且真的查不到——否则这次"修"没有对象
    if !text.contains(old) {
        return Err(format!("{entry} 里没有 `{old}`"));
    }
    if symbol_ok(st, old) {
        return Err(format!("`{old}` 在图谱里查得到，不需要重编"));
    }
    // 新符号必须精确解析（id 或唯一名字）
    let resolved = if new.starts_with("symbol:") {
        if symbol_ok(st, new) {
            new.to_string()
        } else {
            return Err(format!("新符号 `{new}` 在图谱里查不到（改名/删了？）"));
        }
    } else {
        match st.symbol_ids_by_name(new) {
            Ok(ids) if ids.len() == 1 => ids[0].clone(),
            Ok(ids) if ids.is_empty() => {
                return Err(format!("图谱里没有叫 `{new}` 的符号"));
            }
            Ok(ids) => {
                return Err(format!(
                    "有 {} 个符号叫 `{new}`，用 `--symbol symbol:...` 精确指定",
                    ids.len()
                ))
            }
            Err(e) => return Err(format!("查符号失败：{e}")),
        }
    };
    let updated = text.replace(old, &resolved);
    std::fs::write(&path, updated).map_err(|e| format!("写不回 {entry}：{e}"))?;
    Ok(())
}

/// 把过筛的条目投影成图谱里的 claim 行（真源仍是 `docs/knowledge/*.md`）。
pub fn claim_rows(root: &Path) -> Vec<jingwei_engine::graph::store::ClaimRow> {
    load_entries(root)
        .into_iter()
        .filter(|e| why_rejected(root, e).is_none())
        .map(|e| jingwei_engine::graph::store::ClaimRow {
            id: entry_stem(&e.file).to_string(),
            claim: e.fields.get("claim").cloned().unwrap_or_default(),
            level: e.fields.get("level").cloned().unwrap_or_default(),
            source: e.fields.get("source").cloned().unwrap_or_default(),
            topic: e.fields.get("topic").cloned().unwrap_or_default(),
            symbols: e.symbols.clone(),
        })
        .collect()
}

/// 重建图谱里的主张层。返回 (主张数, 挂上真实符号的条数)；**没索引过**时返回 None
/// （不许悄悄建一个空库——那会在人家目录里留东西，还让三态变糊）。
pub fn sync_claims(root: &Path) -> Option<(usize, usize)> {
    let db = root.join(".jingwei").join("graph.db");
    if !db.exists() {
        return None;
    }
    let mut store = Store::open(&db).ok()?;
    if store.symbol_count().unwrap_or(0) == 0 {
        return None; // 库在但还没索引过：主张无处可挂，别写进去
    }
    let rows = claim_rows(root);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    store.sync_claims(&rows, now).ok()
}

/// **索引收尾时的知识层对账**（2026-10-21）：只在"文档有主张、库里 0 条"时重建。
///
/// 触发场景：删掉 `.jingwei` 重建、换机器、或从 git 拉了别人的仓库（库不在版本控制里）。
/// 不补的话，`explain` 的"知识层"那行会**静默变空**——看着像"这条代码没被任何主张解释过"，
/// 而真相是"投影还没建"。三态纪律：不能让"没建"伪装成"没有"。
///
/// 返回 `Some((主张数, 挂符号数))` 表示做了重建；`None` 表示不需要或做不了（库不存在等）。
pub fn ensure_claims_projection(root: &Path) -> Option<(usize, usize)> {
    let db = root.join(".jingwei").join("graph.db");
    if !db.exists() {
        return None;
    }
    {
        let store = Store::open_readonly(&db).ok()?;
        if store.claim_count().unwrap_or(0) > 0 {
            return None; // 已经有投影：不擅自动
        }
    }
    let rows = claim_rows(root);
    if rows.is_empty() {
        return None; // 文档里也没主张：没什么可建
    }
    sync_claims(root)
}

/// **只读**打开图谱：库不存在或还没索引过 → None（= 查不了，**不是**"没有主张"）。
///
/// 为什么不能用 `Store::open`：它会**建库**。在没索引过的项目里跑一次 `knowledge check`
/// 就在人家目录里留下一个空 `.jingwei/graph.db`，而且随后 `stale` 会把"还没索引"
/// 误报成"条目全该重编"——三态就压扁了（2026-10-20 实测踩到）。
pub fn open_graph(root: &Path) -> Option<Store> {
    let db = root.join(".jingwei").join("graph.db");
    if !db.exists() {
        return None;
    }
    let s = jingwei_engine::graph::query::open_for_query(root).ok()?;
    if s.symbol_count().unwrap_or(0) == 0 {
        return None;
    }
    Some(s)
}

pub fn cmd_knowledge(root: &Path, rest: &[String], json: bool) -> i32 {
    if rest.first().map(|s| s.as_str()) == Some("verify") {
        // **让判据真的跑起来**（pro 任务卡 I）：只手动跑，不进任何门禁。
        let allow_dirty = rest.iter().any(|a| a == "--allow-dirty");
        return crate::cmd_kverify::cmd_knowledge_verify(root, json, allow_dirty);
    }
    if rest.first().map(|s| s.as_str()) == Some("sync") {
        return match sync_claims(root) {
            Some((n, linked)) => {
                if json {
                    println!("{}", serde_json::json!({"claims": n, "linked": linked}));
                } else {
                    println!("已同步：图谱里有 {n} 条主张（其中 {linked} 条挂着真实符号）");
                }
                0
            }
            None => {
                println!("查不了：没有图谱（先 jingwei index）——主张层没法落库");
                0
            }
        };
    } else if rest.first().map(|s| s.as_str()) == Some("add") {
        return add_from_inbox(root, rest.get(1).map(|s| s.as_str()), json);
    }
    if rest.first().map(|s| s.as_str()) == Some("stale") {
        // **走库**（pro 任务卡 H）：过期判断与反查同源，不再读 Markdown。
        // 库落后于 Markdown 的风险由 `knowledge check` 的投影对账兜底（那边会红）。
        let Some(store) = open_graph(root) else {
            println!("查不了：没有图谱（先 jingwei index）——待重编判不出来");
            return 0;
        };
        let list = store.stale_claims().unwrap_or_default();
        if json {
            let v: Vec<_> = list
                .iter()
                .map(|(c, s, src)| serde_json::json!({"entry": c, "symbol": s, "source": src}))
                .collect();
            println!("{}", serde_json::json!({"stale": v, "count": list.len()}));
        } else if list.is_empty() {
            println!("待重编 0 条（条目引用的符号都能在图谱里查到）");
        } else {
            println!(
                "待重编 {} 条（条目引用的符号图谱里查不到——改名/删了？）：",
                list.len()
            );
            for (c, s, src) in &list {
                println!("  ! {c} → {s}");
                println!("     ← {src}");
            }
            println!("改挂：jingwei knowledge fix <条目> --symbol <新符号|symbol:id>");
        }
        return if list.is_empty() { 0 } else { 1 };
    }
    if rest.first().map(|s| s.as_str()) == Some("fix") {
        let Some(entry) = rest.get(1) else {
            eprintln!("用法：jingwei knowledge fix <条目文件名> --symbol <新符号|symbol:id>");
            return 2;
        };
        let pos = rest.iter().position(|a| a == "--symbol");
        let Some(new) = pos.and_then(|i| rest.get(i + 1)) else {
            eprintln!("缺 --symbol <新符号|symbol:id>");
            return 2;
        };
        let store = open_graph(root);
        // 旧符号 = 该条目里所有查不到的符号（通常就一处）
        let old = load_entries(root)
            .into_iter()
            .find(|e| e.file == *entry)
            .and_then(|e| {
                e.symbols
                    .iter()
                    .find(|s| store.as_ref().map(|st| !symbol_ok(st, s)).unwrap_or(true))
                    .cloned()
            });
        let Some(old) = old else {
            eprintln!("{entry} 没有查不到的符号——不需要重编");
            return 1;
        };
        return match fix_symbol(root, store.as_ref(), entry, &old, new) {
            Ok(()) => {
                _ = sync_claims(root); // 改挂也要同步，否则图谱里还是旧边
                println!("已改挂：{entry} 的 {old} → {new}（写回文件了）");
                0
            }
            Err(e) => {
                eprintln!("改挂失败：{e}");
                1
            }
        };
    }
    if rest.first().map(|s| s.as_str()) != Some("check") {
        // **子命令要能自己被发现**（pro 卡：入口可见）：以前这里只列 check/add，
        // sync / stale / fix / verify 只能靠读源码或说明书才知道。
        eprintln!("用法：jingwei knowledge <子命令>");
        eprintln!("  check                     严筛条目 + 核对候选，输出漏斗表（含待重编提示）");
        eprintln!("  add <_inbox 里的文件名>    人确认：补好 check 后把候选移进条目");
        eprintln!("  sync                      把条目投影进图谱（claims + claim_edges）");
        eprintln!("  stale                     列出「引用的符号图谱里查不到」的条目，N>0 时 exit=1");
        eprintln!("  fix <条目> --symbol <新符号|symbol:id>  精确改挂过期条目");
        eprintln!("  verify [--allow-dirty]     让 check-run 真的跑（P/F/U；只手动，不进任何门禁）");
        return 2;
    }
    // 读路径一律**只读**开图（`open_graph` 不建库）：没索引过就说查不了，不制造副作用。
    let store = open_graph(root);
    let mut r = evaluate(root, store.as_ref());

    // **投影对账**（pro 任务卡 G/H）：`claims` 表是 Markdown 的投影。
    // `symbol`/`stale` 现在都走库，库一旦落后于文件，判断就静默错了——所以这里逐条对账并红。
    if let Some(st) = store.as_ref() {
        if let Some(drift) = projection_drift(root, st) {
            r.problems.push(drift);
        }
    }

    if json {
        let v = serde_json::json!({
            "total": r.total, "passed": r.passed, "linked": r.linked,
            "problems": r.problems, "gaps": r.gaps, "no_graph": r.no_graph,
            "inbox": r.inbox, "bad_candidates": r.bad_candidates,
            "unmapped": r.unmapped, "levels": r.levels,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        println!(
            "知识条目：采集 {} / 过筛 {} / 挂图谱 {} / 缺口 {}",
            r.total,
            r.passed,
            r.linked,
            r.problems.len() + r.gaps.len()
        );
        for p in &r.problems {
            println!("  × {p}");
        }
        for g in &r.gaps {
            println!("  ! {g}");
        }
        if !r.unmapped.is_empty() {
            let shown: Vec<&str> = r.unmapped.iter().take(5).map(|s| s.as_str()).collect();
            let more = if r.unmapped.len() > 5 {
                format!(" 等 {} 条", r.unmapped.len())
            } else {
                String::new()
            };
            println!(
                "  · 文本态 {} 条（没挂符号：按符号遍历不到，只能按主题学）：{}{more}",
                r.unmapped.len(),
                shown.join(" / ")
            );
        }
        if r.total == 0 {
            println!("  · docs/knowledge/ 里还没有条目（先写一条再跑）");
        }
        if r.passed > 0 {
            // **过滤分级本身**（2026-10-21）：只报"过筛 16"看不出这 16 条有多硬。
            let order = ["一手", "转写", "二手", "查不了"];
            let parts: Vec<String> = order
                .iter()
                .filter_map(|lv| {
                    r.levels
                        .get(*lv)
                        .map(|n| format!("{lv} {n}"))
                })
                .collect();
            let other: usize = r
                .levels
                .iter()
                .filter(|(k, _)| !order.contains(&k.as_str()))
                .map(|(_, v)| *v)
                .sum();
            let more = if other > 0 {
                format!(" / 其它 {other}")
            } else {
                String::new()
            };
            println!("  · 等级（过筛 {} 条）：{}{more}", r.passed, parts.join(" / "));
            // 硬判据：一手占比低的主题，学的时候要打折——只提示，不置红。
            let first_hand = r.levels.get("一手").copied().unwrap_or(0);
            if first_hand * 2 < r.passed {
                println!("  · 一手不到一半：转写/二手的结论当「线索」看，别当判据");
            }
        }
        if r.inbox > 0 || !r.bad_candidates.is_empty() {
            println!(
                "  · 待确认区 _inbox：{} 条候选（不计入采集；`knowledge add` 确认后才入账）",
                r.inbox
            );
            for b in &r.bad_candidates {
                println!("  × {b}");
            }
        }
        if r.no_graph {
            println!("  · 没有图谱：符号侧一律「查不了」（先 jingwei index）");
        }
    }
    // **待重编提示**（pro 卡：闭环完整性，不是纯 UX）：`stale` 是独立命令，不提示的话
    // 用户几乎不会去跑它，回收动作就隐形了。N **与 stale 同源**（都来自 `stale_claims()`），
    // 且**只提示、不置 exit≠0**——"红"只留给缺字段/来源失效那类真问题。
    if let Some(st) = store.as_ref() {
        let stale = st.stale_claims().map(|v| v.len()).unwrap_or(0);
        if stale > 0 && !json {
            println!("  ⚠ 待重编 {stale} 条 → jingwei knowledge stale");
        }
    }
    // 缺字段 / 等级错 / 来源不存在 / 候选来源失效 = 红；缺口（查不了）只如实报。
    if r.problems.is_empty() && r.bad_candidates.is_empty() {
        0
    } else {
        1
    }
}

/// 人确认一条候选：`_inbox/x.md` → `docs/knowledge/x.md`。
/// 前提：`check` 字段被人补过（不是 `待验`），且四字段齐全——否则不许进库。
fn add_from_inbox(root: &Path, arg: Option<&str>, json: bool) -> i32 {
    let Some(name) = arg else {
        eprintln!("用法：jingwei knowledge add <_inbox 里的文件名，例如 原子能-转写要点-L42.md>");
        return 2;
    };
    let name = if name.ends_with(".md") {
        name.to_string()
    } else {
        format!("{name}.md")
    };
    // 顶层 `_*` 与 `README.md` 是草稿/说明，严筛根本不看它们——移进去等于静默丢失。
    if name.starts_with('_') || name == "README.md" {
        eprintln!("{name} 以 `_` 开头或是 README（那是草稿/说明，不算条目）——改个名字再 add");
        return 1;
    }
    let from = inbox_dir(root).join(&name);
    if !from.exists() {
        eprintln!("_inbox 里没有 {name}（先 jingwei harvest <文件>）");
        return 2;
    }
    let text = std::fs::read_to_string(&from).unwrap_or_default();
    let e = parse_entry(&name, &text);
    let check = e.fields.get("check").cloned().unwrap_or_default();
    if check.trim().is_empty() || check.trim() == "待验" {
        eprintln!("{name} 的 check 还是「待验」——先写清怎么验，再 add（不许把没判据的候选放进库）");
        return 1;
    }
    if let Some(why) = why_rejected(root, &e) {
        eprintln!("{name} 过不了严筛：{why}");
        return 1;
    }
    // **查重**（2026-10-20 实测暴露）：确认候选时发现库里已有一条意思相同的手写条目
    // （"架构 = 取舍"），两条并存的后果是 learn 里出现两个同义步骤、采集数字虚高。
    // 判据：claim 去空白后整串相等，就当同一条——不猜"语义相似"（那会误杀）。
    let norm = |s: &str| s.split_whitespace().collect::<String>();
    let want = norm(e.fields.get("claim").map(|s| s.as_str()).unwrap_or(""));
    if !want.is_empty() {
        for other in load_entries(root) {
            let have = norm(other.fields.get("claim").map(|s| s.as_str()).unwrap_or(""));
            if have == want {
                eprintln!(
                    "已有同主张的条目：{}——同一句话不要两条（要么改这条的措辞，要么就别加）",
                    other.file
                );
                return 1;
            }
        }
    }
    let to = root.join("docs").join("knowledge").join(&name);
    if to.exists() {
        eprintln!("docs/knowledge/{name} 已经存在——不覆盖（自己处理冲突）");
        return 1;
    }
    // 移进库时去掉 `candidate: true` 这一行（它已经不是候选了）
    let cleaned: String = text
        .lines()
        .filter(|l| !l.trim().starts_with("candidate:"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if let Err(err) = std::fs::write(&to, cleaned) {
        eprintln!("写不了 {}：{err}", to.display());
        return 2;
    }
    _ = std::fs::remove_file(&from);
    // **入库即同步**（pro 任务卡 G）：条目是 Markdown 真源，图谱里那张主张表是投影；
    // 不同步的话 `symbol` 会查不到刚入库的这条，等于白写。
    _ = sync_claims(root);
    if json {
        println!("{}", serde_json::json!({"added": name}));
    } else {
        println!("已入库：docs/knowledge/{name}（候选区已清，主张层已同步）");
        println!("接着跑 jingwei knowledge check 看漏斗");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-kn-{tag}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("docs").join("knowledge")).unwrap();
        d
    }

    fn put(root: &Path, name: &str, body: &str) {
        std::fs::write(root.join("docs").join("knowledge").join(name), body).unwrap();
    }

    #[test]
    fn 解析_front_matter_与符号() {
        let e = parse_entry(
            "a.md",
            "\u{feff}---\nclaim: 双源才敢说是\nsource: https://x\nlevel: 一手\ncheck: 命令\nsymbol: symbol:a.rs#f\nsymbols: g, h\n---\n正文",
        );
        assert_eq!(e.fields.get("claim").map(|s| s.as_str()), Some("双源才敢说是"));
        assert_eq!(e.fields.get("level").map(|s| s.as_str()), Some("一手"));
        assert_eq!(e.symbols, vec!["symbol:a.rs#f", "g", "h"]);
    }

    #[test]
    fn 引号只剥包住整值的那一层() {
        // 内部引号必须保留（2026-10-20 实测：claim 结尾的引号被 trim_matches 吃掉，
        // 条目文字与原文对不上，而 claim 逐字是候选的硬规矩）
        let inner = parse_entry(
            "a.md",
            "---\nclaim: 显示成\"没有\"就行\nsource: https://x\nlevel: 一手\ncheck: c\n---\n",
        );
        assert_eq!(
            inner.fields.get("claim").map(|s| s.as_str()),
            Some("显示成\"没有\"就行")
        );
        let wrapped = parse_entry(
            "b.md",
            "---\nclaim: \"整值被引号包住\"\nsource: https://x\nlevel: 一手\ncheck: c\n---\n",
        );
        assert_eq!(
            wrapped.fields.get("claim").map(|s| s.as_str()),
            Some("整值被引号包住")
        );
    }

    #[test]
    fn 缺来源或缺等级就不过筛() {
        let root = tmp("miss");
        put(&root, "ok.md", "---\nclaim: x\nsource: https://x\nlevel: 一手\ncheck: 跑一下\n---\n");
        put(&root, "nosrc.md", "---\nclaim: x\nlevel: 一手\ncheck: 跑一下\n---\n");
        put(&root, "nolevel.md", "---\nclaim: x\nsource: https://x\ncheck: 跑一下\n---\n");
        let r = evaluate(&root, None);
        assert_eq!(r.total, 3);
        assert_eq!(r.passed, 1, "{:?}", r.problems);
        assert_eq!(r.problems.len(), 2, "{:?}", r.problems);
        assert!(r.problems.iter().any(|p| p.contains("nosrc.md") && p.contains("source")));
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 等级只能是四个之一() {
        let root = tmp("level");
        put(&root, "bad.md", "---\nclaim: x\nsource: https://x\nlevel: 听说\ncheck: 跑一下\n---\n");
        let r = evaluate(&root, None);
        assert_eq!(r.passed, 0);
        assert!(r.problems[0].contains("听说"), "{:?}", r.problems);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 来源是仓库内路径时必须存在() {
        let root = tmp("src");
        put(&root, "gone.md", "---\nclaim: x\nsource: docs/nope.md\nlevel: 一手\ncheck: 跑一下\n---\n");
        let r = evaluate(&root, None);
        assert_eq!(r.passed, 0, "{:?}", r.problems);
        assert!(r.problems[0].contains("source"), "{:?}", r.problems);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 说明与草稿不算条目() {
        let root = tmp("skip");
        put(&root, "README.md", "# 说明\n没有 front-matter\n");
        put(&root, "_draft.md", "---\nclaim: 草稿\n---\n");
        put(
            &root,
            "real.md",
            "---\nclaim: x\nsource: https://x\nlevel: 一手\ncheck: 跑一下\n---\n",
        );
        let r = evaluate(&root, None);
        assert_eq!(r.total, 1, "README 与 _ 草稿不该计入采集");
        assert_eq!(r.passed, 1);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 同主张的重复条目要能查出来() {
        // 判据：claim 去空白后整串相等（不猜"语义相似"，那会误杀）
        let norm = |s: &str| s.split_whitespace().collect::<String>();
        assert_eq!(
            norm("架构 = 取舍，且不只在技术层"),
            norm("架构 =  取舍，且不只在技术层")
        );
        assert_ne!(
            norm("架构就是取舍"),
            norm("架构 = 取舍，且不只在技术层"),
            "措辞不同不算重复（由人判断）"
        );
    }

    #[test]
    fn 没有图谱时关联符号算查不了而不是算挂上() {
        let root = tmp("nogph");
        put(
            &root,
            "s.md",
            "---\nclaim: x\nsource: https://x\nlevel: 一手\ncheck: 跑一下\nsymbol: symbol:a.rs#f\n---\n",
        );
        let r = evaluate(&root, None);
        assert_eq!(r.passed, 1);
        assert_eq!(r.linked, 0, "没有图谱就不许算挂上");
        assert_eq!(r.gaps.len(), 1);
        assert!(r.gaps[0].contains("查不了"));
        _ = std::fs::remove_dir_all(&root);
    }
}

