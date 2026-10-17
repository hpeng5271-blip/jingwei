//! 一次体检的全部材料，打包成前端直接能用的形状。
//!
//! 为什么单独一个文件：它**只负责组装一份 JSON**——把引擎那边十几个数据源
//! （事实、诊断、运行记录、依赖、图谱、Node 工具链）收成一趟给前端，
//! 省得界面来回问十几次。和"注册命令""管理窗口"那些是真两回事，
//! 而且它是首屏最热的一段，单独放好读也好量。
//!
//! 从 main.rs 拆出来的理由：门禁管着"单文件不许超 1000 行、
//! 登记在案的长文件只许降不许升"（《设计》§12.3）。

use jingwei_engine::{notes, probe, project, rules, runs, snapshot, Contract};
use serde_json::{json, Value};
use std::path::Path;

use crate::fix;

/// 一次体检的全部材料，一次给前端，省得来回问
pub(crate) fn inspect_payload(root: &Path) -> Value {
    // 老版本把记录存在一个 jsonl 里；第一次打开时搬成笔记。
    // 不搬的话，老项目在界面上会显示「一篇都没有」——那是假象。
    let _ = notes::migrate_legacy(root);
    let prev = snapshot::load_latest(root).map(|(_, f)| f);
    let facts = probe::make_facts(root, None);
    // 传递依赖图：读取本身很便宜（一份 lock 文件），就跟着 inspect 一起回来
    let deps_tree = jingwei_engine::deps::rust_tree(root);
    // Node 那边同理——**只做核过官方文档的那一种**（npm 的 package-lock.json）。
    // 别的包管理器如实返回"还没做"：一张空图会被画成"这个项目没有依赖"，
    // 那是最坏的一种撒谎。
    let web_tree = jingwei_engine::deps::web_tree(root);
    let findings = rules::run_all(&facts, prev.as_ref());
    let (contract, contract_err) = Contract::load(root);
    // **一句话指南**（2026-10-16）：项目是什么 / 能不能跑 / 卡在哪 / 下一步敲什么。
    //
    // 数据与 `jingwei guide` **完全同源**：同一条 `make_guide` 取数路径 +
    // 同一个 `Guide::to_json()`。这样界面与命令行**不可能各说各的**——
    // 两处各写一遍是这个仓库付过账的错（`--help` 漏列命令、说明书承诺不存在的入口）。
    //
    // 界面拿它当**概览页主位**：人打开一个项目时问的是一个**问题**（"怎么把它跑起来"），
    // 而不是一份清单。清单在下面的「诊断」里。
    let guide = jingwei_engine::understand::guide::make_guide(&facts, &[]);

    let summary: Vec<Value> = snapshot::summary_lines(&facts)
        .into_iter()
        .map(|(k, v)| json!({ "k": k, "v": v }))
        .collect();

    let counts = json!({
        // 只数排查链上的结论：项目卫生有自己的卡片，
        // 混进来会让首屏那句「有 N 件事值得看」和下面列出来的对不上。
        "error": findings.iter().filter(|f| f.severity == rules::Severity::Error && f.group.is_empty()).count(),
        "warn": findings.iter().filter(|f| f.severity == rules::Severity::Warn && f.group.is_empty()).count(),
        "info": findings.iter().filter(|f| f.severity == rules::Severity::Info && f.group.is_empty()).count(),
    });

    json!({
        "root": root.to_string_lossy(),
        "exists": root.is_dir(),
        "summary": summary,
        "findings": findings,
        "counts": counts,
        "notes": notes::list(root),
        "runs": runs::list(root, 8),
        "venvs": probe::scan_venvs(root),
        "bound": contract.as_ref().and_then(|c| c.runtime.python_exe.clone()),
        "contractError": contract_err,
        "contractName": contract.as_ref().and_then(|c| c.name.clone()),
        "runCommand": contract.as_ref().and_then(|c| c.run_command().map(String::from)),
        "installCommand": contract.as_ref().and_then(|c| c.commands.install.clone()),
        "buildCommand": contract.as_ref().and_then(|c| c.commands.build.clone()),
        "testCommand": contract.as_ref().and_then(|c| c.commands.test.clone()),
        "hasTest": contract.as_ref().and_then(|c| c.commands.test.clone()).is_some(),
        "hasInstall": contract.as_ref().and_then(|c| c.commands.install.clone()).is_some(),
        // 体检的快扫结果跟着 facts 一起回来了：打开项目的那一下就已经有了，
        // 界面不用再问一次。深扫（算体积）才需要单独叫 audit。
        "hygiene": hygiene_value(facts.project.hygiene.as_ref()),
        // 右侧面板要显示"这个项目声明了什么依赖、装了没装"。
        //
        // **这里原来还有一项 `"deps"`（`BTreeMap<String, bool>` 的布尔图），2026-10-16 删了。**
        // 它是 `depsDetail` 的**弱化副本**：同一批数据，丢掉三态（`Verdict::Unknown`
        // 在布尔图里就是 `false`，与"确认没装"无法区分）。留着它只有一个后果——
        // **谁都可以再写一个只读布尔图的新消费者，把"查不了"说成"没有"**。
        // 这个仓库为那个错修过两次（规则层一次、`snapshot.rs` 一次），所以第三次直接掐掉来源。
        "depsDetail": facts.project.deps_detail,
        // 一句话指南（形状见 `Guide::to_json`）。**与 `jingwei guide --json` 同源。**
        "guide": guide.to_json(),
        // 传递依赖图（Rust：Cargo.lock 里写着）
        "depsTree": {
            "edges": deps_tree.0.iter().map(|e| json!({"from": e.from, "to": e.to})).collect::<Vec<_>>(),
            "total": deps_tree.1,
        },
        // Node 的传递依赖图 + **来源**（或"为什么没有图"）。
        // 与 Rust 那张分开给：一个 Tauri 项目两边都有，混在一起画出来的树
        // 会把两个生态的名字串起来。
        "depsTreeWeb": {
            "edges": web_tree.edges.iter().map(|e| json!({"from": e.from, "to": e.to})).collect::<Vec<_>>(),
            "total": web_tree.total,
            "source": web_tree.source,
            "note": web_tree.note,
        },
        "declared": facts.project.declared_deps,
        // Node / 前端那一摊：**用什么包管理器、依赖装到哪去了、版本要求写在哪**。
        // 非 Node 项目是 `null`（不是空对象）——界面上"没有"和"没查"必须分得清。
        "web": jingwei_engine::web::probe_web(root),
        // JVM 那一摊：**这次会用哪个 JDK、凭据是什么、项目钉了哪个构建工具**。
        // 事实早就算过了（`facts.jvm`，诊断规则也在用它），这里只是把它露出来——
        // 一台机器上装了三个 JDK 的时候，"用的是哪个"比"有没有"重要得多。
        "jvm": facts.jvm,
        "jvmBuild": jingwei_engine::jvm::build_tool(root)
            .map(|(name, wrapper)| json!({ "name": name, "wrapper": wrapper })),
    })
}

/// 体检结果打包成前端直接能用的形状：扫描本身 + 能代做的修改清单。
/// 两样一起给，界面上「这条能不能点一下修掉」就不用再问一次后端。
pub(crate) fn hygiene_value(s: Option<&project::ProjectScan>) -> Value {
    let Some(s) = s else { return Value::Null };
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    if let Value::Object(ref mut m) = v {
        let fixes = fix::plan(s).fixes;
        m.insert("fixes".into(), serde_json::to_value(fixes).unwrap_or(Value::Null));
    }
    v
}
