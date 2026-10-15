//! 诊断与体检那一摊：`doctor` / `audit` / `fix` / `whatschanged`。
//!
//! 它们回答的是同一个问题族：**这个项目现在有什么毛病、哪一条能代你修掉、
//! 从上次看过算起变了什么**。所以放一个文件里；拆开会让"体检"这件事
//! 在三个地方各说一半。
//!
//! 从 main.rs 拆出来的理由：门禁的行数上限（见《设计》§12.3）。

use super::{c, pad, BOLD, CYAN, DIM, GREEN, MORE, RED, YELLOW};
use jingwei_engine::{changes, fix, probe, project, rules, snapshot, Severity};
use std::path::Path;
use std::sync::atomic::Ordering;

/// 这些是「机器现在的状态」，跟具体某个项目无关。
/// 每次体检都各刷一行，看的人很快就麻木了——收进一行里，想看再加 --more。
pub(crate) const MACHINE_NOTES: &[&str] = &["console-encoding", "path-python-is-alias"];

pub(crate) fn cmd_doctor(root: &Path, json: bool, want_check: bool) -> i32 {
    let prev = snapshot::load_latest(root).map(|(_, f)| f);
    let facts = probe::make_facts(root, None);
    let findings = rules::run_all(&facts, prev.as_ref());
    jingwei_engine::dlog::i(
        "1300",
        &format!(
            "root={} lang={} findings={} contract={} ms={}",
            root.display(),
            facts.lang.name(),
            findings.len(),
            facts.project.contract.is_some(),
            0
        ),
    );

    // 传递依赖图：命令行也要能看到「这个东西一共拉进来多少个包」
    let (edges, total) = jingwei_engine::deps::rust_tree(root);
    // 真实编译器诊断（--check 才跑：第一次要编译依赖，可能久）
    // 和图形界面共用同一个入口（diag::check_all）：两条路各算各的，
    // 迟早会算得不一样——这个坑刚踩过一次，不再踩第二次。
    let (diags, diag_notes) = if want_check {
        jingwei_engine::diag::check_all(root, std::time::Duration::from_secs(300))
    } else {
        (Vec::new(), Vec::new())
    };

    if json {
        let out = serde_json::json!({
            "facts": facts,
            "findings": findings,
            "diagnostics": diags,
            "diagnostics_notes": diag_notes,
            "deps_tree": {
                "edges": edges.iter().map(|e| serde_json::json!({"from": e.from, "to": e.to})).collect::<Vec<_>>(),
                "total": total,
            }
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return has_error(&findings);
    }

    println!();
    println!(
        "  {}  {}  ·  环境体检",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    println!("  {} {}", c(DIM, "时间"), facts.taken_at);
    println!();
    println!("  {}", c(BOLD, "概览"));
    for (k, v) in snapshot::summary_lines(&facts) {
        println!("    {} {}", c(DIM, &pad(&k, 10)), v);
    }

    // 默认只展开「这个项目的事」；机器层面的提示折成一行
    let (notes, shown): (Vec<&rules::Finding>, Vec<&rules::Finding>) = findings
        .iter()
        .partition(|f| MACHINE_NOTES.contains(&f.id.as_str()));
    // 项目卫生是另一条线：它不影响「跑不跑得起来」，所以默认也折起来，
    // 想看细节走 jingwei audit——那边的排版就是按它分组的。
    let (hygiene, shown): (Vec<&rules::Finding>, Vec<&rules::Finding>) =
        shown.into_iter().partition(|f| !f.group.is_empty());
    let shown = if MORE.load(Ordering::Relaxed) {
        findings.iter().collect()
    } else {
        shown
    };

    // 项目卫生不数进这一行：它有自己的折叠行，混在一起会让人以为
    // 「共 18 条」里有一半是挡住运行的问题。
    let chain: Vec<&rules::Finding> = findings.iter().filter(|f| f.group.is_empty()).collect();
    let (mut e, mut w, mut info) = (0, 0, 0);
    for f in &chain {
        match f.severity {
            Severity::Error => e += 1,
            Severity::Warn => w += 1,
            Severity::Info => info += 1,
        }
    }
    println!();
    if chain.is_empty() && hygiene.is_empty() {
        println!("  {} 没有发现问题。", c(GREEN, "✓"));
        println!(
            "    {}",
            c(
                DIM,
                "感觉一样要看一眼「概览」：环境类型和依赖是不是你以为的那样。"
            )
        );
        return 0;
    }

    println!(
        "  {} 共 {} 条（{} 错误 / {} 警告 / {} 提示）",
        c(BOLD, "问题"),
        chain.len(),
        e,
        w,
        info
    );
    for f in shown.iter() {
        let (mark, col) = match f.severity {
            Severity::Error => ("✗", RED),
            Severity::Warn => ("!", YELLOW),
            Severity::Info => ("i", CYAN),
        };
        println!();
        println!("  {} {} {}", c(col, mark), c(BOLD, &f.title), c(DIM, &f.id));
        if !f.evidence.is_empty() {
            println!("      {}", c(DIM, "证据"));
            for x in &f.evidence {
                println!("        · {}", x);
            }
        }
        if !f.verify.is_empty() {
            println!("      {}", c(DIM, "自己验证"));
            for x in &f.verify {
                println!("        $ {}", x);
            }
        }
        if !f.boundary.is_empty() {
            println!("      {} {}", c(DIM, "边界"), f.boundary);
        }
        if !f.next.is_empty() {
            println!("      {} {}", c(DIM, "下一步"), f.next);
        }
        if MORE.load(Ordering::Relaxed) && !f.excluded.is_empty() {
            println!("      {}", c(DIM, "可以排除的"));
            for x in &f.excluded {
                println!("        · {}", x);
            }
        }
    }
    println!();
    if !notes.is_empty() && !MORE.load(Ordering::Relaxed) {
        let ids: Vec<String> = notes
            .iter()
            .map(|f| format!("{}（{}）", f.title, f.id))
            .collect();
        println!(
            "  {} {} 条机器层面的提示，和这个项目无关：{}",
            c(DIM, "·"),
            notes.len(),
            ids.join("；")
        );
        println!("    {}", c(DIM, "想看细节加 --more"));
        println!();
    }
    if !hygiene.is_empty() && !MORE.load(Ordering::Relaxed) {
        let actionable = hygiene
            .iter()
            .filter(|f| f.severity != Severity::Info)
            .count();
        println!(
            "  {} {} 条项目卫生提示（忽略规则、锁文件、体积），其中 {} 条值得处理：jingwei audit 看全部",
            c(DIM, "·"),
            hygiene.len(),
            actionable
        );
        println!();
    }
    println!(
        "  {} 先处理错误，再看警告；提示不用管。",
        c(DIM, "顺序")
    );
    if e > 0 {
        1
    } else {
        0
    }
}

pub(crate) fn has_error(findings: &[rules::Finding]) -> i32 {
    if findings.iter().any(|f| f.severity == Severity::Error) {
        1
    } else {
        0
    }
}

// ------------------------------------------------------------------ audit

/// 一条结论的排版。顺序是刻意的：先给看到的事实，再给能自己验的办法，
/// 然后才是边界和建议。反过来写（先建议）会让人跳过证据直接照做。
pub(crate) fn print_finding_block(f: &rules::Finding) {
    let (mark, col) = match f.severity {
        Severity::Error => ("×", RED),
        Severity::Warn => ("!", YELLOW),
        Severity::Info => ("i", CYAN),
    };
    println!();
    println!("  {} {} {}", c(col, mark), c(BOLD, &f.title), c(DIM, &f.id));
    let block = |label: &str, lines: &[String]| {
        if lines.is_empty() {
            return;
        }
        println!("      {}", c(DIM, label));
        for x in lines {
            for (i, l) in x.lines().enumerate() {
                if i == 0 {
                    println!("        · {}", l);
                } else {
                    println!("          {}", l);
                }
            }
        }
    };
    block("证据", &f.evidence);
    if !f.verify.is_empty() {
        println!("      {}", c(DIM, "自己验证"));
        for x in &f.verify {
            println!("        $ {}", x);
        }
    }
    if !f.boundary.is_empty() {
        println!("      {} {}", c(DIM, "什么时候不用管"), f.boundary);
    }
    if !f.next.is_empty() {
        println!("      {}", c(DIM, "下一步"));
        for (i, l) in f.next.lines().enumerate() {
            if i == 0 {
                println!("        · {}", l);
            } else {
                println!("          {}", l);
            }
        }
    }
}

/// 项目体检：和语言无关的那一层——哪些文件不该进仓库、依赖记在哪、
/// 这个目录有多大。doctor 回答「跑不起来为什么」，它回答「跑得起来之后」。
pub(crate) fn cmd_audit(root: &Path, json: bool, deep: bool) -> i32 {
    if !root.is_dir() {
        eprintln!("这个目录不存在：{}", root.display());
        return 2;
    }
    let s = if deep {
        project::scan_deep(root)
    } else {
        project::scan(root)
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
        return i32::from(
            s.items
                .iter()
                .any(|f| f.severity == Severity::Error),
        );
    }

    let (e, w, info) = (
        s.items.iter().filter(|f| f.severity == Severity::Error).count(),
        s.items.iter().filter(|f| f.severity == Severity::Warn).count(),
        s.items.iter().filter(|f| f.severity == Severity::Info).count(),
    );

    println!();
    println!(
        "  {}  {}  ·  项目体检",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    // 细节先拼好再打印：把 format! 嵌进 println! 的参数里既难读，
    // 也是 clippy 明确会报的写法
    let scan_detail = format!(
        "{} 个文件{}{}{}",
        s.files_scanned,
        if s.langs.is_empty() {
            String::new()
        } else {
            format!("  语言 {}", s.langs.join(" / "))
        },
        if s.has_git { "  Git 仓库" } else { "  还不是 Git 仓库" },
        if deep { "  含体积统计" } else { "  未算体积（--deep）" }
    );
    println!("  {} {}", c(DIM, "扫描"), scan_detail);

    if !s.manifests.is_empty() {
        println!();
        println!("  {}", c(BOLD, "依赖清单"));
        for m in &s.manifests {
            println!(
                "    {} {:<6} {}",
                pad(&m.path, 22),
                c(DIM, &m.kind),
                if m.note.is_empty() { "（没读出内容）" } else { &m.note }
            );
        }
        if !s.lockfiles.is_empty() {
            println!(
                "    {} {}",
                c(DIM, "锁文件"),
                s.lockfiles.join("、")
            );
        }
    }

    if s.items.is_empty() {
        println!();
        println!("  {} 没有发现要处理的问题。", c(GREEN, "✓"));
        println!(
            "    {}",
            c(
                DIM,
                "看过的：忽略规则、依赖清单和锁文件、IDE 配置、路径长度、垃圾文件。"
            )
        );
        println!();
        return 0;
    }

    println!();
    println!(
        "  {} 共 {} 条（{} 要处理 / {} 注意 / {} 提示）",
        c(BOLD, "体检结果"),
        s.items.len(),
        e,
        w,
        info
    );
    for g in [
        project::G_IGNORE,
        project::G_DEPS,
        project::G_HABIT,
        project::G_SIZE,
    ] {
        let group: Vec<&rules::Finding> = s.items.iter().filter(|f| f.group == g).collect();
        if group.is_empty() {
            continue;
        }
        println!();
        println!("  {}  {}", c(BOLD, &format!("— {} —", g)), c(DIM, &format!("{} 条", group.len())));
        for f in group {
            print_finding_block(f);
        }
    }
    println!();
    if !deep {
        println!(
            "  {}",
            c(DIM, "体积和大文件要加 --deep 才算；平时不用。")
        );
    }
    println!("  {}", c(DIM, "先处理「×」，再看「!」，「i」放着不管也不会出事。"));
    if e > 0 {
        1
    } else {
        0
    }
}

// ------------------------------------------------------------------ snap

/// 这个项目刚才变了什么。报一次就记账，不重复念同一批改动。
pub(crate) fn cmd_changes(root: &Path, json: bool) -> i32 {
    if !root.is_dir() {
        eprintln!("这个目录不存在：{}", root.display());
        return 2;
    }
    let d = match changes::take(root) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("读不动项目状态：{}", e);
            return 1;
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&d).unwrap_or_default());
        return 0;
    }
    println!();
    println!(
        "  {}  {}  ·  这个项目变了什么",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    if !d.first_run && !d.from.is_empty() {
        println!("  {} 从 {} 算起", c(DIM, "对比"), d.from);
    }
    println!();
    println!("  {}", c(BOLD, &d.headline));
    for l in &d.lines {
        println!("    {} {}", c(DIM, "·"), l);
    }
    if d.lines.is_empty() {
        println!(
            "    {}",
            c(DIM, "文件没动过：源码、依赖清单、配置都和上次一样。")
        );
    }
    println!();
    if d.first_run {
        println!("  {}", c(DIM, "这是在记账，不是发现问题。下次再看就有对比了。"));
    } else {
        println!(
            "  {}",
            c(DIM, "改动列表在 .jingwei/watch.json，和笔记放一起；报过一次就不再重复。")
        );
    }
    0
}

/// 体检里能安全代做的那几件事。默认只预演——改别人仓库里的文件，
/// 先让他看清要加哪几行，比事后解释便宜得多。
pub(crate) fn cmd_fix(root: &Path, json: bool, yes: bool, only: &[String]) -> i32 {
    if !root.is_dir() {
        eprintln!("这个目录不存在：{}", root.display());
        return 2;
    }
    let s = project::scan(root);
    let plan = fix::plan(&s);

    if json {
        let out = if yes {
            match fix::apply(root, &s, only) {
                Ok(r) => serde_json::json!({ "dryRun": false, "report": r }),
                Err(e) => serde_json::json!({ "dryRun": false, "error": e.to_string() }),
            }
        } else {
            serde_json::json!({ "dryRun": true, "plan": plan })
        };
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    let wanted: Vec<fix::Fix> = if only.is_empty() {
        plan.fixes.clone()
    } else {
        plan.fixes
            .iter()
            .filter(|f| only.contains(&f.id))
            .cloned()
            .collect()
    };

    println!();
    println!(
        "  {}  {}  ·  {}",
        c(BOLD, "jingwei"),
        env!("CARGO_PKG_VERSION"),
        if yes { "动手改" } else { "预演" }
    );
    println!("  {} {}", c(DIM, "项目"), root.display());
    println!();

    if wanted.is_empty() {
        println!("  {} 没有工具该替你改的东西。", c(GREEN, "✓"));
    } else {
        println!("  {}", c(BOLD, "会做的事"));
        for f in &wanted {
            println!();
            println!("  {} {}", c(BOLD, &f.title), c(DIM, &f.id));
            println!("      改这个文件  {}", f.target);
            for l in &f.lines {
                println!("        + {}", l);
            }
            println!("      {} {}", c(DIM, "说明"), f.note);
        }
    }
    if !plan.skipped.is_empty() {
        println!();
        println!(
            "  {}",
            c(
                BOLD,
                "看得见但不代做的（工具不该越过你的决定）"
            )
        );
        for sk in &plan.skipped {
            if !only.is_empty() && !only.contains(&sk.id) {
                continue;
            }
            println!("    {} {}", c(DIM, &sk.id), sk.title);
            println!("        {}", c(DIM, &sk.reason));
        }
    }

    if !yes {
        println!();
        println!(
            "  {}",
            c(DIM, "这是预演。确认无误再加 --yes 动手；只想做其中一条用 --only <id>。")
        );
        return 0;
    }

    match fix::apply(root, &s, only) {
        Ok(r) => {
            println!();
            if r.nothing_to_do {
                println!("  {} 没有需要新增的行，文件没动。", c(GREEN, "✓"));
                return 0;
            }
            for l in &r.done {
                println!("  {} {}", c(GREEN, "✓"), l);
            }
            println!();
            println!(
                "  {}",
                c(DIM, "改动只在工作区里，git diff 能看到；不满意就 git checkout 那两个文件。")
            );
            0
        }
        Err(e) => {
            eprintln!("  改不动：{}", e);
            1
        }
    }
}
