// 外壳不做决定，只做转接：所有判断都在 jingwei-engine 里。
// 这里唯一的职责是把引擎的返回值摊平成前端好用的形状，外加把启动事件推出去。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use jingwei_engine::{
    changes, clean, editors, envsetup, fix, launch, machine, native, notes, probe, profiles,
    project, projects, runs, scaffold, workcopy, Contract,
};
// 统一信封：那些"查不到"的结果必须能说出来（Q7 查出的 12 个命令没有这个通道）
use jingwei_engine::envelope::Envelope;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;

mod payload;
mod channel;
mod cli;
mod notify;
mod tasks;
use payload::{hygiene_value, inspect_payload};

// 开发工具：`--report <路径>` 把界面状态写进文件（见模块头）。
// 单独一个文件，是因为 main.rs 本来就贴着 1000 行上限，
// 而这一段与"窗口、对话框、编辑器、笔记"不是一类东西。
mod scan;
mod selfcheck;
mod verify;
use selfcheck::start_report;

/// 启动时如果带了个目录参数，就直接把它当项目打开。
/// 用起来很顺手：`jingwei.exe D:\projects\myapp`，或者把文件夹拖到 exe 上。
struct InitialProject(std::sync::Mutex<Option<String>>);

/// 启动时想直接落在哪一页（`--tab graph`）。空 = 停在「概览」。
///
/// **为什么要这个参数**：这个项目的界面只能靠人拉开窗口看，
/// 而"拉开窗口看"没法自动化——想给某一页做截图对比，先得有办法让窗口**直接开在那一页**。
/// 顺带它也是个真需求：`--tab graph <目录>` 一把跳到图谱页。
///
/// 名字由前端校验：这里只把字符串原样交出去，认不出来的页名由前端当"没有"处理
/// （**不在这里猜**——页名清单是前端的事，两处各写一份迟早对不上）。
struct InitialTab(std::sync::Mutex<Option<String>>);

#[tauri::command]
fn initial_tab(state: tauri::State<InitialTab>) -> Value {
    // 没给参数 = 正常状态（走 yes + null），不是"查不了"。
    let t = state.0.lock().ok().and_then(|g| g.clone());
    Envelope::yes(t).to_value()
}


/// 窗口在不在前台。不在就不采样——后台不该占用资源。
static WINDOW_FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 状态栏的采样器。CPU 占用要两次采样求差才准，所以必须留着上一次的计数。
struct SysSampler(std::sync::Mutex<machine::Sampler>);

/// 前端主动拉一帧。**这是主路径**——事件推送是加分项，拉取才是保证：
/// 只要这个命令能调通，状态栏就一定有数字，不会出现"一直挂个 —"。
#[tauri::command]
fn sample_now(state: tauri::State<SysSampler>) -> Value {
    // 状态栏每秒问一次。拿不到锁只是"这一拍跳过"，但**不能回一个 null**——
    // 前端拿 null 会显示成"没有数据"，而真相是"这次没采到"。
    let Ok(mut s) = state.0.lock() else {
        return Envelope::<Value>::unknown("采样器正被上一次采样占着，这一拍跳过").to_value();
    };
    Envelope::yes(s.sample()).to_value()
}

#[tauri::command]
fn hardware_fast() -> Value {
    // 几十毫秒，读注册表和系统接口。这是主路径。
    let v = machine::probe_hardware_fast();
    // **空对象是这里最危险的返回值**：前端拿到 `{}` 会显示成"没有硬件"，
    // 而真相是"一个字段都没读到"（多半是权限、驱动，或者被安全软件拦了）。
    if v.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return Envelope::<Value>::unknown("硬件接口一个字段都没读到（多半是权限或驱动）").to_value();
    }
    Envelope::yes(v).to_value()
}

#[tauri::command]
fn machine_report() -> Value {
    let m = machine::probe_machine();
    let env = machine::env_snapshot();
    let tools = probe::probe_toolchain();
    let v = json!({
        "machine": m,
        "env": env,
        "tools": tools,
        "pythonCount": probe::find_pythons(Path::new("."), None, false).len(),
    });
    // 整体没读到就如实说；**字段级的"查不了"还没做**（记在待办里，带触发条件：
    // 有用户报"某某一项显示成空"时按字段补）。
    let empty = v
        .as_object()
        .map(|o| o.values().all(|x| x.is_null() || x.as_object().map(|i| i.is_empty()).unwrap_or(false)))
        .unwrap_or(true);
    if empty {
        return Envelope::<Value>::unknown("机器清单的各路来源都没读到东西").to_value();
    }
    Envelope::yes(v).to_value()
}

/// 深度检查：往底下钻一层，看原生扩展和编译能力。
/// 界面上的「机器」页和项目的「底层」一节都吃这个。
#[tauri::command]
async fn deep_check(project: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = project.unwrap_or_default();
        let (contract, _) = Contract::load(Path::new(&root));
        let py = probe::first_working(Path::new(&root), contract.as_ref(), 5);
        let Some((exe, f)) = py else {
            return Ok(json!({ "ok": false, "why": "这台机器上没找到能跑起来的 Python" }));
        };
        let site: Vec<PathBuf> = f.site_dirs.iter().map(PathBuf::from).collect();
        let issues = native::scan_native(
            &site,
            Path::new(&f.prefix),
            Path::new(&f.base_prefix),
            &f.arch,
            40,
        );
        // 几十个同类问题收成一条：界面只该看到"几条、都是什么、怎么办"
        let native_summary = native::summarize(&issues, &f.arch);
        let has_setuptools = f
            .installed
            .iter()
            .any(|s| s.to_lowercase().starts_with("setuptools"));
        let has_wheel = f
            .installed
            .iter()
            .any(|s| s.to_lowercase().starts_with("wheel"));
        Ok(json!({
            "ok": true,
            "exe": exe.to_string_lossy(),
            "bits": f.bits,
            "arch": f.arch,
            "abi": f.abi,
            "isolated": f.isolated,
            "pthFiles": f.pth_files,
            "siteDirs": f.site_dirs,
            "installedCount": f.installed.len(),
            "native": native_summary,
            "issues": native_summary.samples,
            "build": native::build_tools(has_setuptools, has_wheel),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn initial_project(state: tauri::State<InitialProject>) -> Value {
    // `null` = 第一次用，还没有"上次打开的项目"——正常状态，走 `yes`。
    let last = state.0.lock().ok().and_then(|g| g.clone());
    Envelope::yes(last).to_value()
}


// 图谱那几条命令（index / stats / dump / runs）搬进了 `graph.rs`：
// main.rs 加了 `graph_runs` 之后撞了门禁的 1000 行上限，而"这四条是同一件事的四个深度"
// 是一条真实的接缝。这里只留模块声明。
mod graph;
// 这里只 import 真正**直接调用**的那个（watch_start 要用它）：注册表里用模块路径，
// 多余的 import 会变成 clippy 的 unused 警告——零警告是门禁里的一道。
use graph::graph_db;

/// 实时层：正在盯哪个项目。
///
/// **为什么是"同时只盯一个"**：用户同一时刻只在一个项目里干活，
/// 而每个监视器都要周期性扫磁盘。开十个监视器等于让机器替十个项目做无用功。
/// 切项目时旧的自动停。
#[derive(Default)]
struct WatchState(std::sync::Arc<std::sync::Mutex<Option<(String, u64)>>>);

/// 开始盯一个项目：文件一改就增量索引，并把结果推给界面。
///
/// **为什么用轮询而不是系统事件**（见 `state::watch` 的说明）：
/// Windows 的事件在网络盘 / WSL / 同步盘上会**静默失效**——
/// 界面什么都不动，而用户以为"没变化"。轮询最笨，但它在哪儿都工作，
/// 而且失败看得见（没变就是真的没变）。
///
/// `interval_ms` 是**两次实际扫描之间的最小间隔**（debounce）：
/// `git checkout` 一个分支会连续产生几千个事件，每一拍都扫一遍纯属浪费。
#[tauri::command]
fn watch_start(
    app: AppHandle,
    state: tauri::State<WatchState>,
    root: String,
    interval_ms: Option<u64>,
) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{root}"));
    }
    let gap = std::time::Duration::from_millis(interval_ms.unwrap_or(1500));

    // 记下"现在盯的是谁"。**先写状态再起任务**——
    // 任务自己会检查"我还是不是当前那个"，被换掉就自己退出去。
    let mut guard = state.0.lock().map_err(|_| "监视状态锁坏了".to_string())?;
    let token = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    *guard = Some((root.clone(), token));
    drop(guard);

    let shared = state.0.clone();
    let app2 = app.clone();
    let root2 = root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = jingwei_engine::state::watch::Watcher::new(Path::new(&root2), gap);
        loop {
            std::thread::sleep(gap);
            // 被换掉了就退出（用户切了项目，这个任务没用了）
            let still_mine = shared
                .lock()
                .map(|g| g.as_ref().map(|(_, t)| *t == token).unwrap_or(false))
                .unwrap_or(false);
            if !still_mine {
                break;
            }
            let Some(cs) = w.poll() else { continue };
            if cs.is_empty() {
                continue;
            }
            // 有变化：增量索引。失败也要说话——**静默的实时层比没有还糟**：
            // 界面上的数字会一直停在旧值，而用户以为那是新的。
            let msg = match jingwei_engine::graph::store::Store::open(&graph_db(Path::new(&root2)))
                .and_then(|mut s| jingwei_engine::graph::index::index_source(Path::new(&root2), &mut s))
            {
                Ok(rep) => json!({
                    "ok": true,
                    "changed": cs.changed.len(),
                    "removed": cs.removed.len(),
                    "symbols": rep.symbols,
                    "resolved": rep.resolved,
                }),
                Err(e) => json!({ "ok": false, "reason": e.to_string() }),
            };
            // 推送失败只可能是窗口已经关了——那时没人接这个事件。
            // 记日志只会把热路径刷满，所以显式忽略。
            #[allow(clippy::let_underscore_must_use)]
            let _ = app2.emit("graph://changed", msg);
        }
    });

    Ok(Envelope::yes(json!({ "watching": root, "interval_ms": gap.as_millis() as u64 })).to_value())
}

/// 停掉实时层。
#[tauri::command]
fn watch_stop(state: tauri::State<WatchState>) -> Value {
    if let Ok(mut g) = state.0.lock() {
        *g = None;
    }
    Envelope::<Value>::yes(json!({ "watching": null })).to_value()
}


/// 跑一次真实的编译器检查。
///
/// 为什么是「按需跑」而不是常驻：`cargo check` 第一次要编译依赖，
/// 慢是正常的；挂在打字上会让人以为编辑器卡死了。手动触发一次、
/// 结果落进「问题」栏，这个节奏更像"我点了一下，它告诉我哪里不对"。
#[tauri::command]
async fn check_project(root: String) -> Value {
    let dir = std::path::PathBuf::from(&root);
    // 按项目里有的东西决定跑哪些：Rust 走 cargo，Python 走 ruff，TS 走 tsc。
    // 哪一样没装，就回一句"装它才能查"——绝不用弱替代品冒充检查。
    let (list, notes) = tauri::async_runtime::spawn_blocking(move || {
        jingwei_engine::diag::check_all(&dir, std::time::Duration::from_secs(300))
    })
    .await
    .unwrap_or((Vec::new(), vec!["检查没能启动".to_string()]));
    // 检查**跑成了**（哪怕一条诊断都没有）就是 `yes`——
    // 真正的"没跑成"写在 `notes` 里（比如"没装 cargo，装它才能查"），
    // 那是**检查器给出的结论**，不是我们自己的失败。两者别混。
    Envelope::yes(serde_json::json!({ "diagnostics": list, "notes": notes })).to_value()
}

#[tauri::command]
fn inspect(root: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    Ok(inspect_payload(&p))
}

/// 项目体检。deep=false 只读清单和目录名（毫秒级）；deep=true 才算体积。
#[tauri::command]
fn audit(root: String, deep: bool) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    let s = if deep {
        project::scan_deep(&p)
    } else {
        project::scan(&p)
    };
    Ok(hygiene_value(Some(&s)))
}

/// 让工具替用户改掉体检里能安全代做的那几件（补 .gitignore、建 .dockerignore）。
/// 前端只传 id；具体改什么由引擎重新扫一遍决定，不信任界面传来的内容。
#[tauri::command]
fn apply_fix(root: String, ids: Vec<String>) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    let before = project::scan(&p);
    let report = fix::apply(&p, &before, &ids).map_err(|e| e.to_string())?;
    let after = project::scan(&p);
    Ok(json!({ "report": report, "scan": hygiene_value(Some(&after)) }))
}

/// 「这个项目刚才变了什么」。界面每隔一阵子问一次，有变化就弹一张卡。
#[tauri::command]
fn changes_take(root: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    let d = changes::take(&p).map_err(|e| e.to_string())?;
    serde_json::to_value(&d).map_err(|e| e.to_string())
}

/// 只看这个项目的记录和笔记——「笔记」页要能把每个项目的留存翻出来，
/// 但不该因此把环境体检整套都跑一遍（那是「打开项目」才做的事）。
#[tauri::command]
fn project_log(root: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    Ok(json!({
        "root": root,
        "notes": serde_json::to_value(notes::list(&p)).unwrap_or(Value::Null),
        "runs": serde_json::to_value(runs::list(&p, 40)).unwrap_or(Value::Null),
    }))
}

// ── 项目生命周期里「还没进去」的那一段 ──────────────────────────

// ── 编辑器：工作副本（改动不直接落盘，应用才写）──────────────────

/// 列出可编辑的文件（跳过产物、缓存、git 内部）
#[tauri::command]
fn list_files(root: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    Ok(json!({ "files": workcopy::files(&p, 800) }))
}

#[tauri::command]
fn open_file(root: String, path: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    let o = workcopy::read(&p, &path).map_err(|e| e.to_string())?;
    serde_json::to_value(&o).map_err(|e| e.to_string())
}

/// 应用一次编辑：先留快照，再原子写。
#[tauri::command]
fn apply_edit(
    root: String,
    path: String,
    text: String,
    encoding: Option<String>,
    eol: Option<String>,
) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    // 按打开时记下的编码与换行写回——不认识就按 UTF-8/LF，至少不会比原来更糟
    let enc = encoding.unwrap_or_else(|| "utf-8".into());
    let eol = eol.unwrap_or_else(|| "lf".into());
    let d = workcopy::apply_as(&p, &path, &text, &enc, &eol).map_err(|e| e.to_string())?;
    let after = workcopy::read(&p, &path).map_err(|e| e.to_string())?;
    Ok(json!({ "draft": d, "opened": after }))
}

/// 手动留一版（不改文件）
#[tauri::command]
fn snapshot_file(root: String, path: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    let d = workcopy::snapshot(&p, &path).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(&d).unwrap_or(Value::Null))
}

/// 回到某一版（回滚本身也会先留一版）
#[tauri::command]
fn restore_draft(root: String, path: String, id: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    workcopy::restore(&p, &path, &id).map_err(|e| e.to_string())?;
    let after = workcopy::read(&p, &path).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(&after).unwrap_or(Value::Null))
}

/// 深比对：连内容一起对（补上"同一时间戳同样长度"的盲区）
#[tauri::command]
fn deep_changes(root: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    let d = changes::take_deep(&p).map_err(|e| e.to_string())?;
    serde_json::to_value(&d).map_err(|e| e.to_string())
}

/// 扫一个父目录，把下面的项目找出来。深度别太大：这不是文件管理器，
/// 翻三层以上只会把别人的仓库、备份、下载目录一起捞进来。
#[tauri::command]
fn list_projects(parent: String, depth: usize) -> Result<Value, String> {
    let p = PathBuf::from(&parent);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", parent));
    }
    Ok(json!({
        "parent": parent,
        "projects": projects::discover(&p, depth.clamp(1, 4)),
    }))
}


/// 第一次打开时给个像样的默认值：常见放代码的地方，存在哪个用哪个。
#[tauri::command]
fn default_parent() -> Value {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let mut cands: Vec<PathBuf> = vec![
        PathBuf::from(r"D:\projects"),
        // **GitHub 克隆常放这**（2026-10-18 用户反馈"扫不到拉下来的项目"）。
        PathBuf::from(r"D:\research"),
        PathBuf::from(r"D:\code"),
        PathBuf::from(r"D:\dev"),
        PathBuf::from(r"E:\projects"),
    ];
    if !home.is_empty() {
        cands.push(PathBuf::from(&home).join("Desktop"));
        cands.push(PathBuf::from(&home).join("Documents"));
        cands.push(PathBuf::from(&home));
    }
    cands.push(PathBuf::from(r"D:\"));
    let hit = cands.into_iter().find(|p| p.is_dir());
    match hit {
        Some(p) => Envelope::yes(json!({ "parent": p.to_string_lossy() })).to_value(),
        // 一个候选目录都不存在 = 异常（连各盘根都在候选里）。
        // 用"空"当"用户还没选过"的信号是错的：界面会静默地什么都不给。
        None => Envelope::<Value>::unknown("几个常见开发目录都不存在（含各盘根），请手动选一次")
            .to_value(),
    }
}

/// 起草契约。save=false 只看它打算写什么（界面先说一句「它会这样跑」），
/// save=true 才真的落盘。
#[tauri::command]
fn contract_draft(root: String, save: bool) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    let (c, why) = if save {
        projects::write_draft(&p).map_err(|e| e.to_string())?
    } else {
        projects::draft(&p)
    };
    Ok(json!({ "contract": c, "why": why, "saved": save }))
}

/// 存回人手改过的契约。界面只传字段，不传整个文件——少一个写坏的机会。
#[tauri::command]
fn contract_save(root: String, contract: Value) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    let mut c: Contract = serde_json::from_value(contract).map_err(|e| e.to_string())?;
    // 空字符串在契约里等于「没有这一步」，别把空命令存进去让启动直接失败
    for f in [
        &mut c.commands.install,
        &mut c.commands.run,
        &mut c.commands.test,
    ] {
        if f.as_deref().map(|s| s.trim().is_empty()).unwrap_or(false) {
            *f = None;
        }
    }
    // 和人手改过的旧版本比一比：变了的就是"人定的"，记下来。
    // 以后自动起草不许覆盖它们——覆盖一次，用户就再也不敢改了。
    if let (Some(old), _) = jingwei_engine::contract::Contract::load(&p) {
        c.mark_manual_against(&old);
    }
    c.save(&p).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(&c).unwrap_or(Value::Null))
}

#[tauri::command]
async fn pick_folder(app: AppHandle, title: String) -> Option<String> {
    app.dialog()
        .file()
        .set_title(&title)
        .blocking_pick_folder()
        .map(|p| p.to_string())
}

#[tauri::command]
fn scan_clean(root: Option<String>) -> Result<Value, String> {
    // 不打开项目也要能清：那些全局缓存（pip / npm / cargo / 浏览器…）
    // 本来就跟项目无关，只清它们是完全成立的一件事。
    let p = root.map(PathBuf::from).unwrap_or_default();
    let has_project = p.is_dir();
    let items = clean::scan(&p);
    let a: Vec<&clean::Item> = items.iter().filter(|i| i.zone == clean::Zone::A).collect();
    let (bytes, n) = clean::total(&items, clean::Zone::A);
    Ok(json!({
        "items": items,
        "zoneA": { "bytes": bytes, "count": n },
        "recommendedBytes": a.iter()
            .filter(|i| i.action == clean::Action::Delete && i.recommended)
            .map(|i| i.size_bytes).sum::<u64>(),
        "recommendedCount": a.iter()
            .filter(|i| i.action == clean::Action::Delete && i.recommended).count(),
        "human": clean::human_size(bytes),
        "humanRecommended": clean::human_size(
            a.iter().filter(|i| i.action == clean::Action::Delete && i.recommended)
                .map(|i| i.size_bytes).sum::<u64>()
        ),
        "hasProject": has_project,
    }))
}

/// 前端只传它勾中的路径。这里重新扫一遍再删——
/// 绝不相信界面传来的任意路径，也不绕过 clean 自己的安全闸。
#[tauri::command]
fn clean_items(root: String, paths: Vec<String>, permanent: bool) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    let wanted: Vec<String> = paths.iter().map(|s| s.to_lowercase()).collect();
    let items: Vec<clean::Item> = clean::scan(&p)
        .into_iter()
        .filter(|i| i.action == clean::Action::Delete && wanted.contains(&i.path.to_lowercase()))
        .collect();
    if items.is_empty() {
        return Err("没有选中任何可清理的项".into());
    }
    let rep = clean::delete(&items, permanent);
    Ok(json!({
        "freed": rep.freed,
        "human": clean::human_size(rep.freed),
        "removed": rep.removed,
        "failed": rep.failed,
        "skipped": rep.skipped,
        "permanent": permanent,
    }))
}

#[tauri::command]
fn save_note(root: String, note: notes::NoteInput) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    notes::save(&p, &note)
        .map(|n| json!({ "saved": n, "all": notes::list(&p) }))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_note(root: String, slug: String) -> Result<Value, String> {
    let p = PathBuf::from(&root);
    notes::delete(&p, &slug)
        .map(|_| json!({ "all": notes::list(&p) }))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn note_kinds() -> Value {
    // 静态常量，不是"查来的"——永远不会空、不会失败，所以恒为 `yes`。
    // 改成信封的唯一理由是**统一**：前端按同一套逻辑解包所有命令。
    let list: Vec<Value> = notes::KINDS
        .iter()
        .map(|(k, l)| json!({ "id": k, "label": l }))
        .collect();
    Envelope::yes(list).to_value()
}

/// 这台机器上装了什么可以用来打开项目
#[tauri::command]
fn openers() -> Value {
    let list = editors::detect();
    if list.is_empty() {
        // 空列表在这条命令上几乎只可能是检测本身出了问题（连系统默认都没有），
        // 所以它是 `unknown`，不是"这台机器没有编辑器"。
        return Envelope::<Vec<editors::App>>::unknown(
            "没检测到任何编辑器（注册表、常见安装目录、PATH 都看过了）",
        )
        .to_value();
    }
    Envelope::yes(list).to_value()
}

#[tauri::command]
fn default_opener() -> Value {
    // 用户自己设的东西：`null` = "你还没设"——**正常状态**，走 `yes`。
    // （判据见 envelope.rs：这是"用户的空白"，不是"我们的失败"。）
    Envelope::yes(editors::default_open()).to_value()
}

#[tauri::command]
fn set_default_opener(id: Option<String>) -> Result<(), String> {
    editors::set_default_open(id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_in(root: String, id: String) -> Result<(), String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("这个目录不存在：{}", root));
    }
    editors::open(&id, &p).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_pythons(project: Option<String>) -> Value {
    let root = project.unwrap_or_default();
    let (contract, _) = Contract::load(Path::new(&root));
    let list: Vec<Value> = probe::find_pythons(Path::new(&root), contract.as_ref(), true)
        .into_iter()
        .map(|c| {
            json!({
                "path": c.path.to_string_lossy(),
                "source": c.source,
                "version": c.version,
                "explicit": c.explicit,
            })
        })
        .collect();
    if list.is_empty() {
        // **这条是那 12 个命令里最典型的一个。**
        // 返回空数组时前端只能显示"一个都没有"，可我们真正知道的是
        // "在环境变量、PATH、py 启动器、uv / pyenv / conda、几个常见目录里都没找到"——
        // 那是 `unknown`，不是 `no`。**"没找到"和"没装"不是一回事。**
        return Envelope::<Value>::unknown(
            "环境变量、PATH、py 启动器、uv / pyenv / conda、几个常见安装目录都找过了，没有发现 Python。\
             如果确定装了，可能是装在了我们自己没想到的地方——把它指给项目契约就行。",
        )
        .to_value();
    }
    Envelope::yes(list).to_value()
}

#[tauri::command]
fn profiles_list() -> Value {
    // 配置档是用户自己建的：空 = "你还没建过"，正常，走 `yes`。
    Envelope::yes(profiles::list()).to_value()
}

#[tauri::command]
fn profile_save(profile: profiles::Profile) -> Result<Vec<profiles::Profile>, String> {
    profiles::save(&profile).map_err(|e| e.to_string())
}

#[tauri::command]
fn profile_remove(name: String) -> Result<Vec<profiles::Profile>, String> {
    profiles::remove(&name).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_project(
    parent: String,
    name: String,
    template: String,
    lang: String,
    purpose: Option<String>,
    python_exe: Option<String>,
    make_venv: bool,
) -> Result<Value, String> {
    let tpl = scaffold::Template::parse(&template).ok_or_else(|| format!("不认识的模板：{}", template))?;
    let safe = scaffold::safe_name(&name);
    let dir = PathBuf::from(&parent).join(&safe);
    // 指定了就用指定的；没指定才去猜。用户的选择不该被启发式规则盖掉。
    let py = python_exe
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| probe::first_working(Path::new(&parent), None, 3).map(|(p, _)| p));
    let rep = scaffold::create(&scaffold::NewOptions {
        dir: dir.clone(),
        name: safe.clone(),
        template: tpl,
        lang: scaffold::Lang::parse(&lang).ok_or_else(|| format!("不认识的语言：{}", lang))?,
        python_exe: py.clone(),
        purpose,
    })
    .map_err(|e| e.to_string())?;

    // 建完顺手把虚拟环境也造上，并绑进契约——这一步不做，新手还是不知道下一步干嘛
    let mut venv = String::from("按你的选择跳过了虚拟环境");
    if make_venv {
        if let Some(px) = &py {
        match envsetup::create_venv(&dir, px, ".venv") {
            Ok(cr) => match envsetup::bind(&dir, &cr.python_exe) {
                Ok(_) => venv = format!("已建好 .venv 并把项目绑过去了（{}）", cr.method.id()),
                Err(e) => venv = format!("虚拟环境建好了，但绑进契约失败：{}", e),
            },
            Err(e) => venv = e.to_string(),
        }
        } else {
            venv = "没找到可用解释器，虚拟环境先跳过".into();
        }
    }
    Ok(json!({
        "dir": dir.to_string_lossy(),
        "created": rep.created,
        "note": rep.note,
        "venv": venv,
    }))
}

/// 启动项目（`Purpose::Run`）。它是 `run_with_stream_purpose` 最常用的那个入口。
fn run_with_stream(
    app: &AppHandle,
    root: &str,
    run_override: Option<String>,
    extra: Vec<String>,
) -> Result<Value, String> {
    run_with_stream_purpose(app, root, run_override, extra, launch::Purpose::Run)
}

/// 启动、跑测试、装依赖、构建——同一套 6 阶段流程，只是**用途**不同。
/// 事件一条条推给前端，前端一条条画。
///
/// **用途必须显式传**：在一门语言里，"装依赖"和"跑测试"是两条不同的命令。
/// 拿一个布尔量 `task: true/false` 来分，就等于把这两件事混成一类——
/// Node 项目上会让"补装依赖"那个按钮去跑 test 脚本。
fn run_with_stream_purpose(
    app: &AppHandle,
    root: &str,
    run_override: Option<String>,
    extra: Vec<String>,
    purpose: launch::Purpose,
) -> Result<Value, String> {
    let opts = launch::LaunchOptions {
        root: PathBuf::from(root),
        run_override,
        extra_args: extra,
        timeout: Duration::from_secs(30),
        purpose,
    };
    let handle = app.clone();
    // 顺手把过程录下来：闭包借用它，跑完再取出来写进 runs.jsonl
    // 记录由引擎的 launch() 自己写，这里只管把事件推给前端。
    // 两个壳各记一份就是两份真相，迟早对不上。
    let out = launch::launch(&opts, &mut |ev| {
        // 每输出一行都会调一次。失败只可能是窗口已经关了——那时没人接这些事件，
        // 记日志只会把热路径刷满。
        #[allow(clippy::let_underscore_must_use)]
        let _ = handle.emit("run://event", &ev);
    })
    .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(&out).unwrap_or(Value::Null))
}

#[tauri::command]
async fn launch_project(app: AppHandle, root: String, extra: Vec<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || run_with_stream(&app, &root, None, extra))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn run_task(app: AppHandle, root: String, task: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (contract, err) = Contract::load(Path::new(&root));
        if let Some(e) = err {
            return Err(format!("契约读不了：{}", e));
        }
        let cmds = contract.as_ref().map(|c| c.commands.clone()).unwrap_or_default();
        let cmd = match task.as_str() {
            "test" => cmds.test,
            "build" => cmds.build,
            "run" => cmds.run,
            _ => cmds.install,
        };
        // **契约没写、但这是个 Node 项目**：交给 `launch` 按 package.json 的
        // scripts 去挑（dev / build / test / lint…）。在这里就报"契约里没声明"
        // 等于告诉一个 Vue 项目"你缺个契约"，而它压根不需要契约。
        let node_project = jingwei_engine::web::is_node_project(Path::new(&root));
        let cmd = match cmd {
            Some(c) => Some(c),
            None if node_project => None,
            None => {
                return Err(format!(
                    "契约里没声明 {} 命令。在 env.contract.json 的 commands 里补一条吧",
                    task
                ))
            }
        };
        // 用途跟着传下去：Node 项目的 install / build / test 各自落到
        // 包管理器的 install 子命令、`run build`、或 `run test`。
        let purpose = match task.as_str() {
            "run" => launch::Purpose::Run,
            "install" => launch::Purpose::Install,
            "build" => launch::Purpose::Build,
            _ => launch::Purpose::Task,
        };
        run_with_stream_purpose(&app, &root, cmd, Vec::new(), purpose)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    jingwei_engine::dlog::install_panic_hook();
    // 参数分两类，**与顺序无关**：
    //   一个存在的目录  → 直接打开它（也支持把文件夹拖到 exe 上）
    //   `--tab <页名>`  → 启动就落在那一页
    // 这样 `jingwei-gui.exe D:\proj --tab graph` 和反过来写都一样能用——
    // 用户不该去记参数顺序。
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // **取一个开关的值，但不许把下一个开关当值**（2026-10-16 修，真缺陷）。
    //
    // 原来的写法是 `.position(|a| a == "--tab").and_then(|i| argv.get(i + 1))`——
    // 它只看"下一个参数在不在"，**不看它是不是另一个开关**。
    //
    // 实际后果（拿界面门禁的取证命令跑出来的）：
    //   `jingwei-gui.exe <目录> --tab graph --report <path>`
    //   解析 `--sweep` 时它后面**紧跟着 `--report`** → 于是 `--report` 被当成
    //   一个页签名，`tabs` 变成非空 → 走"挨页点一遍"那条路，
    //   **本该只报当前这一页的报告，变成了七页**。
    // 而读报告的那侧（`uitest-window.mjs`）期望单页 → 断言全落在错的地方。
    //
    // 这是**通用的参数解析错**：任何"开关后面漏了值"都会把下一个开关吃掉。
    // 所以这里一并收紧三处（`--tab` / `--report` / `--sweep`）。
    let flag_value = |name: &str| -> Option<String> {
        argv.iter()
            .position(|a| a == name)
            .and_then(|i| argv.get(i + 1))
            // 下一个参数以 `--` 开头 = 它不是值，是另一个开关。
            .filter(|v| !v.starts_with("--"))
            .cloned()
    };
    let start = argv
        .iter()
        .find(|a| Path::new(a.as_str()).is_dir())
        .cloned();
    let tab = flag_value("--tab");
    // `--report <路径>`：把界面状态写进文件（开发工具，见 `start_report`）。
    let report = flag_value("--report");
    // **`--shot`（截图）已撤掉（2026-10-16）。**
    //
    // 我加过它：JS 里 `canvas.toDataURL()` → Rust 落盘。代码进了 exe，
    // **但从来没产出过文件，而且没有任何诊断**——因为这个 crate 是
    // **Windows GUI 子系统**，`eprintln!` 的 stderr 不存在（输出被静默丢掉），
    // 而 `dlog` 在这个进程里也不落盘。于是"看不见"叠着"看不见"。
    //
    // 试了 7 次没收敛 → 按 SOP「失败 2 次就停」撤掉。
    // **一条"加了没生效"的能力比没有更糟**：它会让人以为有截图这条路，
    // 而实际上没有（这个项目为"看起来有、其实没有"付过太多次账）。
    //
    // 要它的时候怎么做（留给下一次，附条件的记录）：
    //   ① **先确认诊断通道**——GUI 子系统里 stderr 与 dlog 都可能没有；
    //      唯一验证过能用的是 `save_report` 那条 invoke 通道，
    //      所以**第一件事是让截图状态写进报告**，而不是先写截图。
    //   ② 或者走 Tauri 自己的 window API 截图（不依赖页面）。
    let report_only = true;
    let _ = report_only;
    // `--sweep a,b,c`：启动后**把这几页挨个点一遍**再交报告（开发工具）。
    // 用它而不是"一页开一次窗口"：外壳有六页，一页一次就是六次冷启动；
    // 而"挨个点一遍"本来就是用户的动作，更真也更便宜。
        // 同一个坑：`--sweep` 的值也**不许是另一个开关**（见上面 `flag_value` 的注释）。
        let sweep: Vec<String> = flag_value("--sweep")
            .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
            .unwrap_or_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(InitialProject(std::sync::Mutex::new(start)))
        .manage(InitialTab(std::sync::Mutex::new(tab)))
        .manage(SysSampler(std::sync::Mutex::new(machine::Sampler::new())))
        // 实时层：同时只盯一个项目（见 `WatchState` 的说明）
        .manage(WatchState::default())
        // 状态栏的数字由这里推，不由前端拉：
        // 采样要问系统，前端问不到；每秒来回一次 IPC 也不划算。
        .setup(move |app| {
            jingwei_engine::dlog::i("1000", "app=gui");
            let handle: AppHandle = app.handle().clone();
            // 自检：只有显式传了 `--report` 才跑。`report` 直接 move 进来，
            // 不绕 `app.state()`——那要 `Manager` trait，而且没必要为一次性参数上 state。
            if let Some(p) = report.clone() {
                start_report(handle.clone(), p, sweep.clone());
            }
            std::thread::spawn(move || {
                let mut sampler = machine::Sampler::new();
                // 先立刻推一帧（CPU 占用要两次采样才有值，先给内存/磁盘/进程数）。
                // 之后 5 秒一帧就够——状态栏不是秒表，2 秒一帧是白烧 CPU。
                let mut first = true;
                let mut errs = 0u32;
                loop {
                    // 窗口不在前台就不采样：没人看的时候不该占资源
                    if first || WINDOW_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
                        let s = sampler.sample();
                        if let Err(e) = handle.emit("sys://sample", &s) {
                            errs += 1;
                            // 推不出去要说出来（状态栏空白的原因就在这里）
                            jingwei_engine::dlog::w("1501", &format!("emit sys://sample failed: {}", e));
                        } else if first {
                            first = false;
                            jingwei_engine::dlog::i(
                                "1500",
                                &format!(
                                    "first sample cpu={:.1} mem={}/{}MB disk={:.0}GB py={} errs={}",
                                    s.cpu_percent,
                                    s.mem_used_mb,
                                    s.mem_total_mb,
                                    s.disk_free_gb,
                                    s.python_procs,
                                    errs
                                ),
                            );
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5000));
                }
            });
            Ok(())
        })
        // 日志只在关窗时落盘：编译的时候不能还去抢磁盘。
        // 缓冲区满了会自己落一次，所以被强杀也不会全丢。
        .on_window_event(|_w, e| {
            match e {
                tauri::WindowEvent::Focused(on) => {
                    WINDOW_FOCUSED.store(*on, std::sync::atomic::Ordering::Relaxed);
                }
                tauri::WindowEvent::Destroyed => {
                    jingwei_engine::dlog::i("1001", "window_closed");
                    jingwei_engine::dlog::flush();
                }
                _ => {}
            }
        })
        
.invoke_handler(tauri::generate_handler![
            watch_start,
            watch_stop,
            graph::graph_index,
            graph::graph_stats,
            graph::graph_dump,
            graph::graph_runs,
            cli::cli_json,
            tasks::tasks_state,
    data_dir,
    open_path,
    event_note,
    channel::channel_start,
    channel::channel_request,
    channel::channel_stop,
            notify::notify_gate,
            notify::msg_ingest,
            notify::msg_mark,
            initial_project,
            scan::scan_start,
            scan::scan_status,
            scan::scan_cancel,
            scan::scan_roots,
            scan::scan_similar,
            initial_tab,
            selfcheck::save_report,
            sample_now,
            inspect,
            audit,
            apply_fix,
            changes_take,
            deep_changes,
            project_log,
            list_files,
            open_file,
            apply_edit,
            snapshot_file,
            restore_draft,
            list_projects,
            default_parent,
            contract_draft,
            contract_save,
            hardware_fast,
            machine_report,
            deep_check,
            pick_folder,
            scan_clean,
            clean_items,
            save_note,
            delete_note,
            note_kinds,
            openers,
            default_opener,
            set_default_opener,
            open_in,
            list_pythons,
            profiles_list,
            profile_save,
            profile_remove,
            check_project,
            create_project,
            verify::verify_project,
            launch_project,
            run_task,
        ])
        .run(tauri::generate_context!())
        .expect("jingwei 外壳起不来");
}

/// **数据目录**（2026-10-22，D-20261022-03 Q1）：界面要显示它、并能一键打开。
/// 为什么不放前端拼：路径约定属于后端（`%LOCALAPPDATA%\jingwei`），界面只显示不计算。
#[tauri::command]
fn data_dir() -> String {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("jingwei").display().to_string()
}

/// 在资源管理器里打开一个目录（只接受已存在的目录；不存在就报错，不猜）。
#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.is_dir() {
        return Err(format!("不是目录：{path}"));
    }
    std::process::Command::new("explorer")
        .arg(p)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// **写一条使用事件**（2026-10-22，D-20261022-03 Q3）。
///
/// 只由**界面**调用（人点了什么），所以来源固定为 Human；GUI 为了显示而 spawn 的取数
/// **不走这里**——那类调用是 `cli_json`，pro 明确列为"绝不记"。
#[tauri::command]
fn event_note(kind: String, key: String, detail: Option<String>) -> Result<bool, String> {
    use jingwei_engine::understand::events::{Event, Recorder, Source};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let rec = Recorder::with_default_path();
    let ev = Event { at: now, kind: kind.clone(), key: key.clone(), detail };
    match kind.as_str() {
        "open" => Ok(rec.open_file(&key, now, Source::Human)),
        "revisit" => Ok(rec.revisit(&key, now)),
        "abandon" => Ok(rec.abandon(&key, now)),
        _ => Ok(rec.record(ev, Source::Human)),
    }
}
