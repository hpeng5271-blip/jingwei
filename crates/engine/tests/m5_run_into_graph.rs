//! M5 的验收：**跑过一次就写进图谱，而且能被反查回来**。
//!
//! 为什么这条测试必须存在：`record_run` 这个函数在 M5 被写出来之后，
//! **整整一段时间里只有测试在调它**——生产路径零调用点。
//! 也就是说"运行结果进图谱"这件事，当时只有函数定义、没有接线，
//! 而**没有任何东西会因此失败**（这正是这个项目最贵的那条教训的形态）。
//! 这条测试盯的就是那根线：跑完之后，图谱里必须真的有东西。
//!
//! 用真目录 + 真 SQLite（不是内存库）：WAL、文件锁、重复打开这些问题
//! 只在真文件上才出现。

// 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
// 而不是让 lint 在这个文件里静默失声（和 crate 里别的测试保持同一套写法）。
#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::graph::store::{Dir, Store};
use jingwei_engine::graph::query;
use jingwei_engine::launch::{self, LaunchOptions, Purpose};
use jingwei_engine::{EdgeKind, NodeKind, Verdict};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-m5-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 造一个**契约里写着会失败的命令**的项目。
///
/// 用 PowerShell 而不是 python：这台机器上两个都有，但 PowerShell 是 Windows 自带的，
/// 测试不该因为"没装 python"而变成跳过——那样它就守不住任何东西了。
/// 命令里**故意带一个像诊断的字符串**，用来验"诊断也进图谱"这一半。
fn failing_project(dir: &Path) {
    // 报错里提到的那个文件要**真的存在**：`record_run` 只给磁盘上真实存在的文件落节点
    // （路径可能只是输出文本里的一段，那种不凭空造节点）。所以这里先把它造出来。
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src").join("main.py"), "raise SystemExit(3)\n").unwrap();

    let contract = serde_json::json!({
        "schema_version": 1,
        "name": "m5-failing",
        "commands": { "run": "powershell -NoProfile -Command \"Write-Output 'src/main.py:3:1: error: boom'; exit 3\"" },
        "ready_when": { "exit_code": 0, "timeout_s": 20 }
    });
    std::fs::write(
        dir.join("env.contract.json"),
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
}

#[test]
fn 跑失败之后图谱里留下了这次运行() {
    let d = scratch("run");
    failing_project(&d);

    let opts = LaunchOptions {
        root: d.clone(),
        run_override: None,
        extra_args: Vec::new(),
        timeout: Duration::from_secs(20),
        purpose: Purpose::Run,
    };
    let out = launch::launch(&opts, &mut |_ev| {}).expect("启动本身不该失败");

    assert_eq!(out.exit_code, 3, "契约里的命令就是退出码 3");
    assert!(
        !out.diagnostics.is_empty(),
        "输出里有 `src/main.py:3:1: error: boom`，该被认出来：{:?}",
        out.diagnostics
    );

    // ── 图谱那一半：真的写进去了吗 ──────────────────────────────
    let db = jingwei_engine::graph_db_path(&d);
    assert!(db.is_file(), "跑完之后图谱库该存在：{}", db.display());
    let store = Store::open_readonly(&db).expect("只读打开图谱库");

    let runs = store.list_kind(NodeKind::Run, 10).expect("列 Run 节点失败");
    assert_eq!(runs.len(), 1, "这次运行该在图谱里留下一个 Run 节点：{runs:?}");
    assert!(runs[0].name.contains("powershell"), "Run 的名字是那条命令");

    let produced = store
        .neighbors(&runs[0].id, EdgeKind::Produced, Dir::Out)
        .expect("查 produced 边失败");
    assert!(
        !produced.is_empty(),
        "Run --produced--> Diagnostic 这条边必须存在——它就是'上次跑失败是哪些问题'的答案"
    );

    // 诊断挂到文件上了没有（认出来的那条带路径）
    let mut attached = false;
    for diag in &produced {
        let files = store
            .neighbors(diag, EdgeKind::DiagnosedAt, Dir::Out)
            .expect("查 diagnosed_at 边失败");
        if !files.is_empty() {
            attached = true;
            assert_eq!(
                files[0], "file:src/main.py",
                "落点必须是报错里那个文件——挂到别的文件上比不挂还糟"
            );
        }
    }
    assert!(
        attached,
        "至少要有一条诊断挂到文件上，否则'改了这里会影响谁'仍然答不出来"
    );

    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 一次次重跑不会把图谱写成垃圾() {
    let d = scratch("twice");
    failing_project(&d);
    let opts = LaunchOptions {
        root: d.clone(),
        run_override: None,
        extra_args: Vec::new(),
        timeout: Duration::from_secs(20),
        purpose: Purpose::Run,
    };
    launch::launch(&opts, &mut |_ev| {}).expect("第一次");
    launch::launch(&opts, &mut |_ev| {}).expect("第二次");

    let store = Store::open_readonly(&jingwei_engine::graph_db_path(&d)).expect("开库");
    let runs = store.list_kind(NodeKind::Run, 10).expect("列 Run");
    assert_eq!(
        runs.len(),
        2,
        "跑两次就是两次运行——它们是不同的事实，不该被合并成一个节点"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **写进去，还要问得出来。**
///
/// 这一条盯的是 M5 的后半截：图谱里躺着"上次跑失败是哪些问题"，但没有一条路能问出来。
/// 只有写没有读的图谱，等于没写——数据在库里，价值没到人手上。
#[test]
fn 跑完之后能问出上次失败是哪些问题() {
    let d = scratch("answer");
    failing_project(&d);
    let opts = LaunchOptions {
        root: d.clone(),
        run_override: None,
        extra_args: Vec::new(),
        timeout: Duration::from_secs(20),
        purpose: Purpose::Run,
    };
    launch::launch(&opts, &mut |_ev| {}).expect("跑一次");

    let store = query::open_for_query(&d).expect("只读打开图谱");

    // ① 列表：界面第一眼要看到的东西（命令、退出码、出了几条）
    let list = query::recent_runs(&store, 10).expect("列最近运行");
    assert_eq!(list.len(), 1, "只有一次运行");
    assert_eq!(list[0].exit_code, 3, "退出码就是契约里那个 3");
    assert!(
        list[0].command.contains("powershell"),
        "列表要带命令，否则看不出这次跑的是什么：{:?}",
        list[0]
    );
    assert_eq!(
        list[0].diagnostics, 1,
        "列表要带条数——它是'点开哪一条'的依据：{:?}",
        list[0]
    );

    // ② 明细：点开这一条之后要看到的
    let a = query::answer_run(&store, &list[0].run_id)
        .expect("查询失败")
        .expect("这条运行该查得到");
    assert_eq!(a.exit_code, 3);
    assert_eq!(a.diagnostics.len(), 1, "{:?}", a.diagnostics);
    let d0 = &a.diagnostics[0];
    assert_eq!(d0.title, "boom", "诊断标题是输出里那句话：{:?}", d0);
    assert_eq!(
        d0.file,
        Verdict::Yes("file:src/main.py".to_string()),
        "落点必须是报错里那个文件——**这是'改了这里会影响谁'的入口**：{:?}",
        d0
    );

    // ③ 查一个不存在的运行 id：该说"没有"，而不是空答案
    assert!(
        query::answer_run(&store, "run:不存在的")
            .expect("查询不该报错")
            .is_none(),
        "查不到就是 None——**'没有这次运行'和'这次运行没有问题'是两件事**"
    );

    let _ = std::fs::remove_dir_all(&d);
}

/// 没有落点的诊断：`No` 而不是 `Unknown`，也绝不能是 `Yes`。
///
/// 这一条守的是三态在查询层的落点："这条问题本来就没说在哪个文件"
/// 与"它说了但那个文件没索引过"必须分得开。
#[test]
fn 没有落点的诊断如实说没有() {
    let d = scratch("nofile");
    // 契约里的命令**只打印一句没有位置的话**——认不出文件，于是落点是 No
    let contract = serde_json::json!({
        "schema_version": 1,
        "name": "m5-nofile",
        "commands": { "run": "powershell -NoProfile -Command \"Write-Output 'ModuleNotFoundError: No module named x'; exit 1\"" },
        "ready_when": { "exit_code": 0, "timeout_s": 20 }
    });
    std::fs::write(
        d.join("env.contract.json"),
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();

    let opts = LaunchOptions {
        root: d.clone(),
        run_override: None,
        extra_args: Vec::new(),
        timeout: Duration::from_secs(20),
        purpose: Purpose::Run,
    };
    launch::launch(&opts, &mut |_ev| {}).expect("跑一次");

    let store = query::open_for_query(&d).expect("只读打开图谱");
    let list = query::recent_runs(&store, 10).expect("列最近运行");
    assert_eq!(list.len(), 1);
    let a = query::answer_run(&store, &list[0].run_id)
        .expect("查询失败")
        .expect("查得到");
    assert!(!a.diagnostics.is_empty(), "至少该记下一条：{:?}", a);
    for diag in &a.diagnostics {
        assert_eq!(
            diag.file,
            Verdict::No,
            "认不出文件就该是 No（本来就没说在哪个文件），既不许编一个、也不许说成查不了：{diag:?}"
        );
        assert!(
            diag.stated_file.is_none(),
            "这条诊断本来就没说位置，stated_file 该是 None：{diag:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// **"它说了，但那个文件没索引到"必须是 `Unknown`，不是 `No`。**
///
/// 这条是 2026-10-16 外部对照复核抓出来的缺陷的验收测试，也是这个项目
/// **招牌不变量的最后一块**：`AGENTS.md` §2 写着"`unknown` 永远不许显示成'没有'"，
/// 而在这次修复之前，查询层**从来没有产出过 `Unknown`**——
/// 于是 CLI / Tauri / 界面里为它写的那三支渲染（「没索引到」）**全是死代码**，
/// 用户对一条明说了文件的诊断，看到的却是「（这条问题没有说是哪个文件）」。
///
/// 构造：让进程吐一个**认得出形状、但指向磁盘上不存在的文件**的 traceback。
/// 写入侧手里有那个路径（存进 `data.target`），但因为 `is_file()` 为假，
/// **不凭空造节点**，于是没有 `diagnosed_at` 边——这正是第 ② 种情形。
#[test]
fn 说了位置但没索引到的诊断是查不了而不是没有() {
    let d = scratch("stated");
    // 走一个 `.ps1` 文件，而不是 `-Command "..."`。
    //
    // **为什么**：PowerShell 的 `-Command` 会把参数里的双引号吃掉，
    // 于是 `File "app/x.py"` 打成 `File app/x.py`——而那不是 traceback 的形状，
    // 解析器会（正确地）认不出来。第一版夹具就是这么错的：
    // **测试红了，红的原因在夹具，不在产品。夹具必须造出真形状。**
    std::fs::write(
        d.join("emit.ps1"),
        "Write-Output 'Traceback (most recent call last):'\n\
         Write-Output '  File \"app/never_indexed.py\", line 7, in boom'\n\
         Write-Output 'KeyError: 1'\n\
         exit 1\n",
    )
    .unwrap();
    let contract = serde_json::json!({
        "schema_version": 1,
        "name": "m5-stated",
        "commands": { "run": "powershell -NoProfile -ExecutionPolicy Bypass -File emit.ps1" },
        "ready_when": { "exit_code": 0, "timeout_s": 20 }
    });
    std::fs::write(
        d.join("env.contract.json"),
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();

    let opts = LaunchOptions {
        root: d.clone(),
        run_override: None,
        extra_args: Vec::new(),
        timeout: Duration::from_secs(20),
        purpose: Purpose::Run,
    };
    launch::launch(&opts, &mut |_ev| {}).expect("跑一次");

    let store = query::open_for_query(&d).expect("只读打开图谱");
    let list = query::recent_runs(&store, 10).expect("列最近运行");
    assert_eq!(list.len(), 1, "该记下一次运行：{list:?}");
    let a = query::answer_run(&store, &list[0].run_id)
        .expect("查询失败")
        .expect("查得到");

    let unknown: Vec<_> = a
        .diagnostics
        .iter()
        .filter(|x| x.file == Verdict::Unknown)
        .collect();
    assert!(
        !unknown.is_empty(),
        "这条诊断说了在 app/never_indexed.py，而那个文件不在图谱里——\
         必须是 Unknown（我们没查到），不许说成 No（它没说）：{:?}",
        a.diagnostics
    );
    // 而且要能说出**是哪个文件**没说上——只说"没索引到"而不说哪个文件，用户没法行动
    assert_eq!(
        unknown[0].stated_file.as_deref(),
        Some("app/never_indexed.py"),
        "要把'它原本说的是哪个文件'带出来，否则用户不知道该去索引什么"
    );
    // **归属要一路带到这里**（2026-10-16 接上）：
    // 走到 `Unknown` 的唯一一条路是"它说了、我们没索引"——**是我们的失败**，
    // 不是用户的空白。外壳（CLI / Tauri / 界面）拿这个字段直接说人话，
    // **不许自己猜**"Unknown 是谁的"（那正是 32 处没接上的原因）。
    assert_eq!(
        unknown[0].unknown_why,
        jingwei_engine::Unknown::OurGap,
        "没索引到那个文件是我们的失败，不是用户的空白"
    );
    // 反向：**挂上了的那条不许被归成失败**（`Yes` 的时候这个问题没有意义）。
    for d in &a.diagnostics {
        if matches!(d.file, Verdict::Yes(_)) {
            assert_eq!(
                d.unknown_why,
                jingwei_engine::Unknown::Unclear,
                "已挂上的诊断不该有'谁造成的未知'——硬归一头就是编"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}
