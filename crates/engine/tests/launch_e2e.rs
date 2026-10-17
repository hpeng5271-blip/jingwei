//! **`launch()` 的端到端**（2026-10-16 加）。
//!
//! ## 为什么需要这一层
//!
//! `launch.rs` 里原来有 **5 条**单元测试，而它们**全是诊断的辅助函数**
//! （`split_quoted` / `diagnose` 的四种形状）。`launch()` 这个**入口本身没人测**：
//! 六阶段怎么走、命令从哪儿来、契约与 override 谁优先、退出码怎么报——
//! 这些错了，那 5 条辅助测试**全绿**。
//!
//! ## 为什么可以真起进程（而且必须真起）
//!
//! `Prepared`（前四阶段的结论，`run_cmd` 就在它上面）是 `pub(crate)`——
//! **没有"只算不跑"的公开口子**，而 `Outcome`/`Event` 都**不带那条命令**。
//! 所以"跑的是哪条命令"这个问题，**只能从可观察的效果上看**：
//! 让命令**打印一个独特字符串**，再看输出里出现的是哪一个。
//! **这比断言"命令字符串等于什么"更硬**——它证明的是"那句话真的被打印了"。
//!
//! 可控的地方：
//!   - 契约里的命令是**我们自己写的一句 Python**（`print` 一句话就退，毫秒级）；
//!   - 项目在**临时目录**里，`launch()` 往 `.jingwei/graph.db` 写的运行记录
//!     落在那儿，**不污染任何真项目**（这条必须守住：这个仓库为"我们自己的数据
//!     污染结论"付过账——`.jingwei` 曾经成了"这个项目最长的路径"）。
//!   - 解释器路径**由夹具自己解析**（不靠 PATH）：PATH 上那个 `python.exe`
//!     可能是 Windows 的应用商店占位符，跑起来会弹商店——**那样红的是环境，不是产品**。
//!
//! ## 这一层管什么
//!
//! 管：**命令真的按契约执行了** / **override 真的压过契约** / **退出码原样带回来** /
//! **"没得跑"时不猜**。不管：措辞、性能、界面上的样子（那是界面门禁）。

// 测试里的临时目录清理失败不影响断言，也不能改变结论——显式豁免，
// 而不是让 lint 在整条测试里静默失声（与 `deps_tests.rs` / `cli_contract.rs` 同一句）。
#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::launch::{launch, Event, LaunchOptions, Outcome, Purpose, StageStatus};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 找一个**真能用的** Python 解释器，并且返回**绝对路径**。
///
/// 两件事都要：
///   - **要真能用**：PATH 上那个 `python.exe` 可能是 `WindowsApps\` 下的
///     应用商店占位符——跑它不会有输出，只会让测试红在一个与产品无关的地方。
///     所以先问 `python -c print(1)` 出不出声。
///   - **要绝对路径**：契约的 `runtime.python_exe` 收的是解释器路径
///     （见演示项目里那条 `C:\Users\…\python.exe`），给个光名字不是它的用法。
fn python_exe() -> Option<String> {
    for cand in ["python", "python3"] {
        let ok = std::process::Command::new(cand)
            .args(["-c", "print(1)"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            continue;
        }
        // 解析成绝对路径（`where`/`which` 那一步由我们自己走，不引依赖）
        let out = std::process::Command::new(cand)
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .ok()?;
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() && std::path::Path::new(&p).is_file() {
            return Some(p);
        }
        return Some(cand.to_string());
    }
    None
}

/// 造一个最小项目：一份**与真契约同形**的 `env.contract.json` + 一句会退出的 Python。
///
/// **形状照抄演示项目**（`tools/make-demos.ps1` 生成的那份），一个字段都不少：
/// 第一版我自己编了一个"最小"契约（只有 schema_version/name/runtime/deps/commands），
/// 结果 `launch()` 报「契约里没有声明 run 命令」——
/// **少的是 `ready_when`**，而它是"什么时候算就绪"的判据。
/// 教训：**夹具要照真东西写，不要照自己的理解写**（SOP §S6 同一课）。
fn fixture(tag: &str, py: &str, run: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jingwei-launch-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // JSON 里反斜杠要转义（Windows 路径）
    let py_json = py.replace('\\', "\\\\");
    let run_json = run.replace('\\', "\\\\").replace('"', "\\\"");
    std::fs::write(
        d.join("env.contract.json"),
        format!(
            r#"{{
  "schema_version": 1,
  "name": "smoke-{tag}",
  "description": "launch_e2e 的夹具",
  "runtime": {{ "python": ">=3.10", "python_exe": "{py_json}" }},
  "deps": {{ "required": [], "optional": [] }},
  "commands": {{ "run": "{run_json}" }},
  "log_files": [],
  "ready_when": {{ "port": null, "log_match": null, "exit_code": 0, "timeout_s": 30 }},
  "manual": []
}}"#
        ),
    )
    .unwrap();
    d
}

fn opts(root: &Path, run_override: Option<&str>, purpose: Purpose) -> LaunchOptions {
    LaunchOptions {
        root: root.to_path_buf(),
        run_override: run_override.map(|s| s.to_string()),
        extra_args: Vec::new(),
        // **硬上限**：夹具的命令是毫秒级的，5 秒还没完就是真出问题了。
        timeout: Duration::from_secs(5),
        purpose,
    }
}

/// 跑一次，把**阶段**与**输出行**都收下来。
fn run_once(o: &LaunchOptions) -> (Outcome, Vec<(usize, StageStatus)>, String) {
    let mut stages: Vec<(usize, StageStatus)> = Vec::new();
    let mut out_lines = String::new();
    let out = launch(o, &mut |e: Event| match e {
        Event::Stage { index, status, .. } => stages.push((index, status)),
        Event::Line { text, .. } => {
            out_lines.push_str(&text);
            out_lines.push('\n');
        }
        _ => {}
    })
    .expect("launch 不该整次失败");
    (out, stages, out_lines)
}

/// **一句话 Python 跑通了**：这是这一层最小的一条真端到端。
///
/// 四个断言分开钉：
///   ① 退出码 0（命令真的执行了、也真的成功了）；
///   ② **契约那句真的被打印出来了**——这是"跑的是哪条命令"最硬的证据；
///   ③ 六阶段都报出来了（少了哪一段，用户就看不到那一步）；
///   ④ 阶段号是**连续**的（跳号会让界面上的进度条缺一格，而那是假象）。
#[test]
fn 契约里的命令会被真的执行() {
    let Some(py) = python_exe() else {
        // **环境缺 python 时跳过，而不是假装通过。**
        // 但也不能无声：打一行说明，不然"绿"会被读成"验过了"。
        eprintln!("跳过：这台机器上没有能用的 python");
        return;
    };
    let d = fixture("ok", &py, &format!("{py} -c \"print('CONTRACT_SAYS_HI')\""));
    let o = opts(&d, None, Purpose::Run);
    let (out, stages, text) = run_once(&o);

    assert_eq!(out.exit_code, 0, "一句 print 该成功退出");
    assert!(
        text.contains("CONTRACT_SAYS_HI"),
        "契约里那句该真的被打印出来——这是'跑的是哪条命令'的证据。实际输出：{text}"
    );
    assert!(
        stages.len() >= 5,
        "六阶段该都报出来（少一段用户就看不到那一步），实际 {} 段",
        stages.len()
    );
    // ④ 阶段号连续：跳号在界面上就是"进度条缺一格"，而那是假象
    let mut idx: Vec<usize> = stages.iter().map(|(i, _)| *i).collect();
    idx.sort_unstable();
    idx.dedup();
    for (want, got) in (1..).zip(idx.iter()) {
        assert_eq!(want, *got, "阶段号该从 1 连续排下来，实际 {idx:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// **`run_override` 优先于契约**——"契约决定跑哪条命令，但你说了算"那条线。
///
/// 判据落在**打印出来的那句话**上：契约说 `CONTRACT`、override 说 `OVERRIDE`，
/// 输出里出现哪一个，就是跑了哪一个。
/// 这条错了的表现很具体：用户在界面上写了要跑什么，跑的却是契约里那条。
#[test]
fn override_优先于契约() {
    let Some(py) = python_exe() else { return };
    let d = fixture("override", &py, &format!("{py} -c \"print('FROM_CONTRACT')\""));
    let o = opts(
        &d,
        Some(&format!("{py} -c \"print('FROM_OVERRIDE')\"")),
        Purpose::Run,
    );
    let (out, _, text) = run_once(&o);
    assert!(
        text.contains("FROM_OVERRIDE"),
        "给了 override 就该跑它，实际输出：{text}"
    );
    assert!(
        !text.contains("FROM_CONTRACT"),
        "契约那条**不该**被跑（跑了说明 override 没生效）：{text}"
    );
    assert_eq!(out.exit_code, 0, "override 那条也是能跑通的");
    let _ = std::fs::remove_dir_all(&d);
}

/// **跑失败要报非 0，而且不许整次 `Err`。**
///
/// 这条是"能不能跑"这个问题的另一半：`launch` 的返回值表示
/// **"这次启动流程走完了没有"**，而"跑失败了"是**结果**，不是流程错误。
/// 混起来的后果：一次编译失败会弹成一个"工具出错"的对话框，
/// 而它其实是一条用户该读的正常结论。
#[test]
fn 命令失败是结论不是流程错误() {
    let Some(py) = python_exe() else { return };
    let d = fixture("fail", &py, &format!("{py} -c \"import sys; sys.exit(3)\""));
    let o = opts(&d, None, Purpose::Run);
    // `run_once` 里已经 `expect("launch 不该整次失败")`——走到这儿就说明没整次失败
    let (out, _, _) = run_once(&o);
    assert_eq!(out.exit_code, 3, "退出码要原样带回来（脚本靠它）");
    let _ = std::fs::remove_dir_all(&d);
}

/// **契约里没写 run、也不是 Node 项目 → 如实说"没得跑"，不许猜一条出来。**
///
/// 这一条守的是三态：**"我们不知道跑什么"不是"这个项目坏了"**。
/// 猜一条（比如硬编 `python main.py`）的后果是用户去查一个不存在的问题。
#[test]
fn 没有可跑的命令时不猜() {
    let d = std::env::temp_dir().join(format!("jingwei-launch-nocmd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // 一个什么都没有的目录：没有契约、没有 package.json
    let o = opts(&d, None, Purpose::Run);
    let mut stages: Vec<(usize, StageStatus)> = Vec::new();
    let r = launch(&o, &mut |e: Event| {
        if let Event::Stage { index, status, .. } = e {
            stages.push((index, status));
        }
    });
    // 允许两种诚实的结果：整次失败（说清缺什么），或走完但报"没得跑"。
    match r {
        Ok(out) => assert_ne!(
            out.exit_code, 0,
            "没有可跑的命令时不该报成功——那是猜了一条出来"
        ),
        Err(e) => assert!(
            !e.to_string().is_empty(),
            "失败也要说清为什么（空错误等于没说）"
        ),
    }
    let _ = std::fs::remove_dir_all(&d);
}
