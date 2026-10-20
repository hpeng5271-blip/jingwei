//! `jingwei learn <主题>`：把知识条目按**显式前置**排成一条能照着走的路径（2026-10-20，pro 任务卡 B）。
//!
//! 这是双头漏斗第一个**可见出口**：严筛过的条目（`docs/knowledge/*.md`）按 `prereq:` 排顺序，
//! 每一步都带「凭什么」——claim / level / source，有符号或决策的再带落点。
//!
//! 三条不许破的规矩（pro 定的翻车点）：
//! 1. **顺序只能来自 `prereq:`**（或将来图谱的依赖边），**不许按文件名/时间戳拍脑袋**；
//! 2. 每步必须引用 ≥1 条真条目；引用不到就**不生成**那一步，只报"查不了"；
//! 3. 被引用的条目过不了严筛（缺来源/等级）→ 该步标 `查不了`，整条命令 exit≠0，不许当绿。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::cmd_knowledge::{entry_stem, load_entries, why_rejected, Entry};

/// 一条路径步骤：**每步都必须带"凭什么"**（claim / level / source），否则不许生成。
#[derive(Debug, Clone)]
pub struct Step {
    pub entry: String,
    pub claim: String,
    pub level: String,
    pub source: String,
    pub symbols: Vec<String>,
    /// 该条目的前置（已解析成条目名）。
    pub prereq: Vec<String>,
    /// 过不了严筛时的原因；None = 这条可以当"已核实"。
    pub rejected: Option<String>,
}

/// 按主题取条目，再按 `prereq:` 排成 DAG 的拓扑序。
pub fn plan(root: &Path, topic: &str) -> Result<Vec<Step>, String> {
    let all: Vec<Entry> = load_entries(root);
    let want = topic.trim();

    // **前置可以跨主题**：知识本身是一张图，主题只是取子图。
    // 所以"前置存不存在"按**全部条目**判，排序只在本主题内部看（外部前置按"已学"处理）。
    let all_names: BTreeSet<String> = all.iter().map(|e| entry_stem(&e.file).to_string()).collect();

    // 主题匹配：`topic:` 可写多个，逗号分隔，精确匹配（不做模糊，模糊会把不相干的条目卷进来）。
    let picked: Vec<Entry> = all
        .into_iter()
        .filter(|e| {
            e.fields
                .get("topic")
                .map(|t| t.split(',').any(|x| x.trim() == want))
                .unwrap_or(false)
        })
        .collect();
    if picked.is_empty() {
        return Err(format!(
            "还没有 `topic: {want}` 的条目（先采集：格式见 docs/knowledge/README.md）"
        ));
    }

    let names: BTreeSet<String> = picked
        .iter()
        .map(|e| entry_stem(&e.file).to_string())
        .collect();
    let mut steps: BTreeMap<String, Step> = BTreeMap::new();
    for e in &picked {
        let stem = entry_stem(&e.file).to_string();
        let prereq: Vec<String> = e
            .fields
            .get("prereq")
            .map(|v| {
                v.split(',')
                    .map(|x| x.trim().trim_end_matches(".md").to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        for p in &prereq {
            if !all_names.contains(p) {
                // 前置指向了根本不存在的条目：这一条**不能**猜顺序
                return Err(format!(
                    "{stem} 的前置 `{p}` 在 docs/knowledge 里找不到——顺序查不了（别猜）"
                ));
            }
            let _ = &names; // 主题内的名字只用于排序，见下面的 Kahn
        }        steps.insert(
            stem.clone(),
            Step {
                entry: stem,
                claim: e.fields.get("claim").cloned().unwrap_or_default(),
                level: e.fields.get("level").cloned().unwrap_or_default(),
                source: e.fields.get("source").cloned().unwrap_or_default(),
                symbols: e.symbols.clone(),
                prereq,
                rejected: why_rejected(root, e),
            },
        );
    }

    // Kahn：入度为 0 的先出；同一个入度的按名字排（只用来**稳定**输出，不用来决定先后）。
    let mut order: Vec<Step> = Vec::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    let total = steps.len();
    while order.len() < total {
        let ready: Vec<String> = steps
            .values()
            .filter(|s| !done.contains(&s.entry))
            // 本主题内的前置要等它先出；**跨主题的前置按"已学"处理**（它不在这个子图里）。
            .filter(|s| s.prereq.iter().all(|p| !names.contains(p) || done.contains(p)))
            .map(|s| s.entry.clone())
            .collect();
        if ready.is_empty() {
            let left: Vec<String> = steps
                .keys()
                .filter(|k| !done.contains(*k))
                .cloned()
                .collect();
            return Err(format!(
                "顺序查不了：{} 互相前置成环（{}）",
                left.len(),
                left.join(" / ")
            ));
        }
        for name in ready {
            if let Some(s) = steps.get(&name) {
                order.push(s.clone());
                done.insert(name);
            }
        }
    }
    Ok(order)
}

/// 进度落在**本地状态**里（`%LOCALAPPDATA%\jingwei\learn-progress.json`），不进仓库：
/// 它是"我这个人的学到哪了"，不是项目事实；写进 git 会让每次学习产生脏提交与冲突。
pub fn progress_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("jingwei").join("learn-progress.json")
}

/// 读进度：一组**步骤 id**（= 条目名去 .md）。跨主题共用同一份，不许各存一份。
pub fn load_progress(path: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    serde_json::from_str::<Vec<String>>(&text)
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

pub fn save_progress(path: &Path, done: &BTreeSet<String>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let v: Vec<&String> = done.iter().collect();
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_else(|_| "[]".into()))
}

/// `jingwei learn <主题> [--mark <步骤>]`：打勾 + 打路径。
/// `jingwei learn --symbol <符号>`：**从代码出发**——围绕这个符号，按等级列出解释过它的主张。
///
/// 这是"以项目为来源"的正门：读代码时先看这块被哪些主张解释过，而不是先背方法论。
/// 走图边（`claims_for_symbol`），不读 Markdown——与 `explain`/`symbol` 同一份事实。
fn learn_by_symbol(root: &Path, query: &str, json: bool) -> i32 {
    let Some(store) = crate::cmd_knowledge::open_graph(root) else {
        println!("查不了：没有图谱（先 jingwei index）");
        return 1;
    };
    let key = if query.starts_with("symbol:") {
        if store.node(query).map(|n| n.is_some()).unwrap_or(false) {
            query.to_string()
        } else {
            println!("查不了：`{query}` 不在图谱里（可能改名/删了）");
            return 1;
        }
    } else {
        match store.symbol_ids_by_name(query) {
            Ok(ids) if ids.len() == 1 => ids[0].clone(),
            Ok(ids) if ids.is_empty() => {
                println!("查不了：图谱里没有叫 `{query}` 的符号（先 index，或名字拼错了）");
                return 1;
            }
            Ok(ids) => {
                println!("有 {} 个符号叫 `{query}`，用 id 指定：", ids.len());
                for id in ids {
                    println!("  {id}");
                }
                return 1;
            }
            Err(e) => {
                eprintln!("查符号失败：{e}");
                return 1;
            }
        }
    };

    let mut claims = store.claims_for_symbol(&key).unwrap_or_default();
    claims.sort_by(|a, b| {
        crate::cmd_symbol::level_rank(&a.level)
            .cmp(&crate::cmd_symbol::level_rank(&b.level))
            .then_with(|| a.id.cmp(&b.id))
    });
    if claims.is_empty() {
        // 查不了不是失败：这块代码还没有人（或还没采集）写过主张。
        println!("查不了：这块代码还没有主张解释过它（先 harvest / knowledge add）");
        return 0;
    }
    if json {
        let v: Vec<_> = claims
            .iter()
            .map(|c| {
                serde_json::json!({
                    "entry": c.id,
                    "claim": c.claim,
                    "level": c.level,
                    "source": c.source,
                })
            })
            .collect();
        println!("{}", serde_json::json!({"symbol": key, "claims": v}));
        return 0;
    }
    println!(
        "围绕符号：{key} —— {} 条主张（按等级：一手 → 转写 → 二手 → 查不了）",
        claims.len()
    );
    // 条目 → 主题与位置（用 plan() 排一次，指出"这条在主题里的第几步、前面还有几步"）：
    // 光列主张不给上下文，用户不知道要不要先补前面的。
    let entries = crate::cmd_knowledge::load_entries(root);
    for (i, c) in claims.iter().enumerate() {
        println!(
            "  {i}. [{lv}] {claim}（条目：{name}）",
            i = i + 1,
            lv = c.level,
            claim = c.claim,
            name = crate::cmd_explain::short_name(&c.id)
        );
        println!("     ← {}", c.source);
        if let Some(where_) = topic_position(root, &entries, &c.id) {
            println!("     {where_}");
        }
    }
    println!("（想看整条路径：jingwei learn <主题>；打勾：--mark <序号>）");
    0
}

/// 这条条目在它所属主题里的位置：`主题：判据 第 2/3 步（前面还有 1 步）`。
/// 判不出来（没主题 / 顺序有环）就返回 None——不编位置。
fn topic_position(root: &Path, entries: &[crate::cmd_knowledge::Entry], id: &str) -> Option<String> {
    // 图里的 claim id 是**去 `.md` 的条目名**，`load_entries` 的 `file` 带 `.md`——按 stem 比。
    let e = entries
        .iter()
        .find(|e| e.file.trim_end_matches(".md") == id || e.file == id)?;
    let raw = e.fields.get("topic")?;
    let topic = raw
        .split([',', '，', '、', ';', '；'])
        .map(|t| t.trim())
        .find(|t| !t.is_empty())?;
    let plan = plan(root, topic).ok()?;
    let idx = plan.iter().position(|s| s.entry == id)?;
    let before = idx;
    if before == 0 {
        Some(format!("主题：{topic} 第 1/{} 步", plan.len()))
    } else {
        Some(format!(
            "主题：{topic} 第 {}/{} 步（前面还有 {before} 步）",
            idx + 1,
            plan.len()
        ))
    }
}

/// 符号 id 的短形态：`symbol:crates/cli/src/main.rs#cmd_positions` → `cmd_positions（crates/cli/src/main.rs）`。
/// 不是 `symbol:` 形态就原样返回（裸名字本来就是短的）。
fn short_symbol(id: &str) -> String {
    let Some(rest) = id.strip_prefix("symbol:") else {
        return id.to_string();
    };
    match rest.split_once('#') {
        Some((path, name)) => format!("{name}（{path}）"),
        None => rest.to_string(),
    }
}

pub fn cmd_learn(root: &Path, rest: &[String], json: bool) -> i32 {
    // `--symbol <符号>`：从代码出发的入口，优先于"主题"解析。
    if let Some(pos) = rest.iter().position(|a| a == "--symbol") {
        match rest.get(pos + 1) {
            Some(q) => return learn_by_symbol(root, q, json),
            None => {
                eprintln!("--symbol 后面要跟符号名或 symbol:id");
                return 2;
            }
        }
    }
    let Some(topic) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("用法：jingwei learn <主题> [--mark <步骤id>]   （按 docs/knowledge 条目的 prereq 排顺序）");
        return 2;
    };

    let plan = match plan(root, topic) {
        Ok(p) => p,
        Err(e) => {
            // **查不了不是失败**：还没有条目是"还没采集"，exit 0 如实说。
            if e.contains("还没有") {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"topic": topic, "steps": [], "unknown": e})
                    );
                } else {
                    println!("查不了：{e}");
                }
                return 0;
            }
            // 顺序判不出来（环 / 前置不存在）才算失败：这时候任何顺序都是编的。
            eprintln!("{e}");
            return 1;
        }
    };

    // `--mark <步骤>`：可以给**序号**（`--mark 3`，就是 learn 输出里那个 3.）或条目名。
    // 无论哪种，只能标 learn **真的输出过**的步骤，别的 id 一律拒绝（防止伪进度）。
    if let Some(pos) = rest.iter().position(|a| a == "--mark") {
        let Some(raw) = rest.get(pos + 1) else {
            eprintln!("--mark 后面要跟步骤序号（如 3）或条目名");
            return 2;
        };
        // 序号：省得手抄长文件名（pro 卡：`--mark 3` 也认）
        let step: String = match raw.parse::<usize>() {
            Ok(n) if n >= 1 && n <= plan.len() => plan[n - 1].entry.clone(),
            Ok(n) => {
                eprintln!("没有第 {n} 步（这个主题共 {} 步）", plan.len());
                return 1;
            }
            Err(_) => raw.clone(),
        };
        if !plan.iter().any(|s| s.entry == step) {
            eprintln!("`{step}` 不在主题 `{topic}` 的步骤里——只能标 learn 真的输出过的步骤");
            for (i, s) in plan.iter().enumerate() {
                eprintln!("  {}. {}", i + 1, s.entry);
            }
            return 1;
        }
        let path = progress_path();
        let mut done = load_progress(&path);
        done.insert(step.clone());
        if let Err(e) = save_progress(&path, &done) {
            eprintln!("写进度失败：{e}");
            return 2;
        }
        let learned = plan.iter().filter(|s| done.contains(&s.entry)).count();
        if json {
            println!(
                "{}",
                serde_json::json!({"marked": step, "learned": learned, "total": plan.len()})
            );
        } else {
            println!("已记：{step}（{topic} 进度 {learned}/{}）", plan.len());
        }
        return 0;
    }

    let bad = plan.iter().filter(|s| s.rejected.is_some()).count();
    if json {
        let v: Vec<_> = plan
            .iter()
            .map(|s| {
                serde_json::json!({
                    "entry": s.entry,
                    "claim": s.claim,
                    "level": s.level,
                    "source": s.source,
                    "symbols": s.symbols,
                    "prereq": s.prereq,
                    "rejected": s.rejected,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({"topic": topic, "steps": v, "rejected": bad})
        );
    } else {
        let done = load_progress(&progress_path());
        let learned = plan.iter().filter(|s| done.contains(&s.entry)).count();
        println!(
            "主题：{topic} —— {} 步（已学 {learned}/{}；顺序来自条目的 prereq，不是文件名）",
            plan.len(),
            plan.len()
        );
        for (i, s) in plan.iter().enumerate() {
            let mark = if done.contains(&s.entry) { "●" } else { "○" };
            match &s.rejected {
                Some(why) => {
                    println!("  {mark} {}. ? {} —— 查不了：{why}", i + 1, s.claim);
                    println!("     ← {} · {}", s.level, s.source);
                }
                None => {
                    println!("  {mark} {}. {}", i + 1, s.claim);
                    let mut basis = format!("     ← {} · {}", s.level, s.source);
                    if !s.prereq.is_empty() {
                        basis.push_str(&format!("（前置：{}）", s.prereq.join(" / ")));
                    }
                    println!("{basis}");
                    if !s.symbols.is_empty() {
                        // 只打 `symbol:path#名字` 里的名字 + 文件：全 id 太长，一行放不下也读不出重点
                        let shown: Vec<String> =
                            s.symbols.iter().map(|x| short_symbol(x)).collect();
                        println!("     挂：{}", shown.join(" / "));
                    }
                }
            }
        }
        if learned < plan.len() {
            println!("打勾：jingwei learn {topic} --mark <序号>（也可以给条目名）");
        }
    }
    if bad > 0 {
        eprintln!("有 {bad} 步的条目过不了严筛（缺来源/等级）——这些步骤只能算查不了");
        return 1;
    }
    0
}

/// `jingwei progress`（不带主题）：**所有主题的鸟瞰**——已学 m/n、下一步是哪条。
///
/// 为什么要有它：只有 `progress <主题>` 时，用户得先记得有哪些主题；而"今天学什么"
/// 恰恰是最常问的一句。这里只读本地进度 + 条目，不写任何东西。
/// 主题一览的一行：主题、已学、总数、下一步、顺序判不了的原因。
struct TopicRow {
    topic: String,
    learned: usize,
    total: usize,
    next: Option<String>,
    error: Option<String>,
}

pub fn cmd_progress(root: &Path, rest: &[String], json: bool) -> i32 {
    let done = load_progress(&progress_path());
    if let Some(topic) = rest.iter().find(|a| !a.starts_with("--")) {
        return cmd_learn(root, std::slice::from_ref(topic), json);
    }

    // 收集主题（去重、稳定排序）：`topic:` 允许写多个（逗号分隔），这里要拆开——
    // 否则 `判据, 存储` 会整串变成一个假主题（2026-10-21 实跑时抓到）。
    let mut topics: Vec<String> = Vec::new();
    for e in crate::cmd_knowledge::load_entries(root) {
        let Some(raw) = e.fields.get("topic") else {
            continue;
        };
        for t in raw.split([',', '，', '、', ';', '；']) {
            let t = t.trim();
            if !t.is_empty() {
                topics.push(t.to_string());
            }
        }
    }
    topics.sort();
    topics.dedup();
    if topics.is_empty() {
        println!("查不了：docs/knowledge/ 里还没有带 topic 的条目（先写一条再跑）");
        return 0;
    }

    // (主题, 已学, 总数, 下一步, 顺序判不了的原因)
    let mut rows: Vec<TopicRow> = Vec::new();
    for t in &topics {
        match plan(root, t) {
            Ok(plan) => {
                let learned = plan.iter().filter(|s| done.contains(&s.entry)).count();
                let next = plan
                    .iter()
                    .find(|s| !done.contains(&s.entry))
                    .map(|s| s.claim.clone());
                rows.push(TopicRow {
                    topic: t.clone(),
                    learned,
                    total: plan.len(),
                    next,
                    error: None,
                });
            }
            Err(e) => rows.push(TopicRow {
                topic: t.clone(),
                learned: 0,
                total: 0,
                next: None,
                error: Some(e),
            }),
        }
    }
    if json {
        let v: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "topic": r.topic, "learned": r.learned, "total": r.total,
                    "next": r.next, "error": r.error,
                })
            })
            .collect();
        println!("{}", serde_json::json!({"topics": v}));
        return 0;
    }
    println!("主题一览（已学/总步数；下一步 = 第一条没打勾的）：");
    for r in &rows {
        if let Some(e) = &r.error {
            println!("  {}  ? 查不了：{e}", r.topic);
        } else if let Some(claim) = &r.next {
            println!("  {}  {}/{}  下一步：{claim}", r.topic, r.learned, r.total);
        } else {
            println!("  {}  {}/{}  全部学完", r.topic, r.learned, r.total);
        }
    }
    println!("看某条路径：jingwei learn <主题>；打勾：jingwei learn <主题> --mark <序号>");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 进度落盘再读回来一致() {
        let p = std::env::temp_dir().join(format!("jw-prog-{}.json", std::process::id()));
        _ = std::fs::remove_file(&p);
        let mut s = BTreeSet::new();
        s.insert("原子能-01-先问为什么".to_string());
        save_progress(&p, &s).unwrap();
        let back = load_progress(&p);
        assert_eq!(back, s);
        _ = std::fs::remove_file(&p);
    }

    #[test]
    fn 进度文件坏了就当没学() {
        let p = std::env::temp_dir().join(format!("jw-prog-bad-{}.json", std::process::id()));
        std::fs::write(&p, "不是 json").unwrap();
        assert!(load_progress(&p).is_empty());
        _ = std::fs::remove_file(&p);
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-learn-{tag}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("docs").join("knowledge")).unwrap();
        d
    }

    /// `prereq` 传 `""` 或 `"prereq: x\n"`。
    fn entry(topic: &str, prereq: &str) -> String {
        format!(
            "---\nclaim: 主张\ntopic: {topic}\n{prereq}source: https://x\nlevel: 一手\ncheck: 跑一下\n---\n"
        )
    }

    fn put(root: &Path, name: &str, body: &str) {
        std::fs::write(root.join("docs").join("knowledge").join(name), body).unwrap();
    }

    #[test]
    fn 顺序跟着_prereq_走而不是文件名() {
        let root = tmp("order");
        // 文件名故意与正确顺序相反：z 在最前，a 在最后
        put(&root, "z-first.md", &entry("t", ""));
        put(&root, "a-second.md", &entry("t", "prereq: z-first\n"));
        put(&root, "m-third.md", &entry("t", "prereq: a-second\n"));
        let plan = plan(&root, "t").unwrap();
        let order: Vec<&str> = plan.iter().map(|s| s.entry.as_str()).collect();
        assert_eq!(order, vec!["z-first", "a-second", "m-third"]);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 跨主题的前置按已学处理() {
        let root = tmp("cross");
        put(&root, "base.md", &entry("other", ""));
        put(&root, "in-topic.md", &entry("t", "prereq: base\n"));
        let plan = plan(&root, "t").unwrap();
        // 子图里只有 in-topic；外部前置 base 不在子图里，按"已学"处理，不报错也不排进去
        let order: Vec<&str> = plan.iter().map(|s| s.entry.as_str()).collect();
        assert_eq!(order, vec!["in-topic"]);
        assert_eq!(plan[0].prereq, vec!["base"]);
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 前置根本不存在就拒绝给顺序() {
        let root = tmp("ghost");
        put(&root, "a.md", &entry("t", "prereq: 根本没有这条\n"));
        let e = plan(&root, "t").unwrap_err();
        assert!(e.contains("找不到"), "{e}");
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 没有这个主题的条目就是查不了() {
        let root = tmp("unknown");
        put(&root, "a.md", &entry("other", ""));
        let e = plan(&root, "没有这个主题").unwrap_err();
        assert!(e.contains("还没有"), "{e}");
        _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 前置成环就拒绝给顺序() {
        let root = tmp("cycle");
        put(&root, "a.md", &entry("t", "prereq: b\n"));
        put(&root, "b.md", &entry("t", "prereq: a\n"));
        let e = plan(&root, "t").unwrap_err();
        assert!(e.contains("环"), "{e}");
        _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod short_symbol_tests {
    use super::short_symbol;

    #[test]
    fn 符号id显示成名字加文件_裸名字原样() {
        assert_eq!(short_symbol("symbol:crates/a/b.rs#f"), "f（crates/a/b.rs）");
        assert_eq!(short_symbol("cmd_handover"), "cmd_handover");
        assert_eq!(short_symbol("symbol:weird"), "weird");
    }
}
