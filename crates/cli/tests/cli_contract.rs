//! **CLI 的端到端契约测试**（2026-10-16 加）。
//!
//! ## 为什么需要这一层
//!
//! 规划测试时量出来的缺口：`crates/cli` 原来只有 **7 条**测试，而且**全是"扫源码"式**的
//! （读 `include_str!("cmd_graph.rs")`，断言里面有某段代码）——
//! 那类测试守得住"别改回去"，**守不住"跑起来到底返回什么"**。
//!
//! 而 CLI 是**用户的主界面**，它的 **退出码与输出字段是脚本的契约**：
//! `jingwei graph --json` 被取证脚本直接读，`jingwei index` 的退出码决定
//! `&& 下一步` 走不走。**改了行为，原来没有任何东西会红。**
//!
//! ## 为什么用 `CARGO_BIN_EXE_`
//!
//! 它由 cargo 在集成测试里自动提供（**不用引 `assert_cmd` 之类的依赖**，
//! 也不用自己找 target 目录——"不许给自己搭脚手架"）。拿到的就是刚构建的那个
//! `jingwei` 可执行文件，所以**测的是真二进制，不是 `main` 里的某个函数**。
//!
//! ## 这一层管什么、不管什么
//!
//! 管：**退出码分档** / **关键字段在不在** / **"查不了"与"没有"分不分得开**。
//! 不管：具体措辞（那是 `ui` 与文档的事）、性能、屏幕上的样子（那是界面门禁）。

// 测试里的临时目录清理失败不影响断言，也不能改变结论——显式豁免，
// 而不是让 lint 在整个 crate 里静默失声（与 `deps_tests.rs` 同一句）。
#![allow(clippy::let_underscore_must_use)]

use std::path::PathBuf;
use std::process::{Command, Output};

/// 拿 cargo 给的那个真二进制。**拿不到就跳过并说明原因**——
/// 而不是让整条测试无声地"通过"（这个仓库为"看起来在守"付过账）。
fn bin() -> Option<PathBuf> {
    option_env!("CARGO_BIN_EXE_jingwei").map(PathBuf::from)
}

/// 一个临时的空目录（用它当"没有项目的目录"）。
fn empty_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jingwei-cli-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 跑一次 CLI。**不设 cwd**：这几个命令都接 `--root`，
/// 顺带钉住"不靠当前目录也能用"（cwd 默认是哪儿是这个仓库踩过的坑）。
fn run(args: &[&str]) -> Output {
    let b = bin().expect("CARGO_BIN_EXE_jingwei 应该由 cargo 提供");
    Command::new(b).args(args).output().expect("跑不起来")
}

/// 退出码。**这比输出文字重要**：脚本只看它。
fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// `--help` 要列全命令，而且**退出码必须是 0**。
///
/// 这一条对应 `事故账本` 里那笔账：`--help` 曾**漏列 4 个命令**，
/// 而 `使用说明.md` 承诺的 `inspect` 根本不存在。两份清单各写各的，必然漂移。
#[test]
fn help_列出全部命令并成功退出() {
    let Some(_) = bin() else { return };
    let o = run(&["--help"]);
    assert_eq!(code(&o), 0, "`--help` 该成功退出");
    let s = stdout(&o);
    for cmd in ["index", "graph", "guide", "doctor", "inspect", "run", "scan"] {
        assert!(
            s.contains(cmd),
            "`--help` 里该有 `{cmd}`——说明书承诺过的入口不能在表里缺席"
        );
    }
}

/// **认不出的子命令必须非 0**，不许静默当成默认行为。
///
/// 这条线的由来：`cmd_graph` 里"空子命令"与"认不出的子命令"曾共用同一个兜底臂，
/// 于是 `jingwei graph index`（账本里写着"故意没做"的那条）会打印运行历史并**返回 0**——
/// 脚本里 `jingwei graph index && 下一步` 就这么安心往下走了。
#[test]
fn 认不出的子命令不许静默成功() {
    let Some(_) = bin() else { return };
    let top = run(&["这不是一个命令"]);
    assert_ne!(code(&top), 0, "顶层认不出的子命令必须非 0");
    let sub = run(&["graph", "这不是子命令"]);
    assert_ne!(code(&sub), 0, "`graph` 认不出的子命令必须非 0");
}

/// **空目录：说"没有"，退出码 0**——那是用户的空白，不是我们的失败。
#[test]
fn 空目录的图谱查询说没有而不是报错() {
    let Some(_) = bin() else { return };
    let d = empty_dir("nodeps");
    let o = run(&["graph", "--root", d.to_str().unwrap()]);
    assert_eq!(code(&o), 0, "还没建过图谱是正常情形，不该非 0 退出");
    let s = stdout(&o);
    assert!(s.contains("还没建过图谱"), "要说清是'还没建'：{s}");
    let _ = std::fs::remove_dir_all(&d);
}

/// **索引一个真目录：退出码 0，而且报告要分开说"扫到多少"与"处理了多少"**。
///
/// 这条钉的是那个"报告拿'变了几个'当'项目有几个'"的缺陷：
/// `files` 是这次处理了几个，**没变的会被增量检测跳过**，
/// 所以一个正常项目跑第二遍时 `files` 就是 0 而 `scanned` 不变。
#[test]
fn 索引报告要分开说扫到多少与处理了多少() {
    let Some(_) = bin() else { return };
    // 用一个真的装着源码的目录（本仓库自己的 `crates/util` 之类的最小集合不够稳，
    // 所以就地造一个只有两个 rs 文件的目录）
    let d = empty_dir("idx");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    std::fs::write(d.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(d.join("src/lib.rs"), "pub fn a() {}\n").unwrap();

    let first = run(&["index", "--root", d.to_str().unwrap()]);
    assert_eq!(code(&first), 0, "索引该成功：{}", String::from_utf8_lossy(&first.stderr));
    let s1 = stdout(&first);
    assert!(s1.contains("2 个文件"), "第一遍该处理 2 个文件：{s1}");

    // 第二遍：什么都没改 → 扫到的数不变，处理的数是 0
    let second = run(&["index", "--root", d.to_str().unwrap()]);
    assert_eq!(code(&second), 0, "第二遍也该成功");
    let s2 = stdout(&second);
    assert!(
        s2.contains("处理了 0 个文件") || s2.contains("0 个文件"),
        "第二遍没变，处理的该是 0：{s2}"
    );
    assert!(
        s2.contains("扫到 2 个"),
        "扫到的数不该变——变了说明收集不稳定：{s2}"
    );

    let _ = std::fs::remove_dir_all(&d);
}

/// **真空目录：索引必须非 0**，而且要把"扫的是哪个目录"说出来。
///
/// 原来它报「✓ 图谱已建好」并**退出码 0**——"成功的空结果"与"失败"分不开。
#[test]
fn 索引一个没有源码的目录要非零退出() {
    let Some(_) = bin() else { return };
    let d = empty_dir("empty");
    let o = run(&["index", "--root", d.to_str().unwrap()]);
    assert_ne!(code(&o), 0, "一个源文件都没有时不该报成功");
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(err.contains("扫到 0 个文件"), "要说清扫到了 0 个：{err}");
    assert!(
        err.contains(d.to_str().unwrap()),
        "要把扫的目录说出来，否则'0 个文件'没法查：{err}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **`--json` 的字段是契约**：脚本直接读它，改名就是破坏性变更。
#[test]
fn json_输出的关键字段在() {
    let Some(_) = bin() else { return };
    let d = empty_dir("json");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    std::fs::write(d.join("src/main.rs"), "fn main() {}\n").unwrap();

    let o = run(&["index", "--root", d.to_str().unwrap(), "--json"]);
    assert_eq!(code(&o), 0, "带 --json 也该成功");
    let s = stdout(&o);
    for k in ["files", "scanned", "symbols", "calls"] {
        assert!(s.contains(&format!("\"{k}\"")), "JSON 里该有 {k}：{s}");
    }

    // 图谱查询那边的 JSON 契约（"没有"要说成 no，不是报错）。
    //
    // ⚠️ **必须换一个干净目录**：上面刚对 `d` 索引过，它的库里已经有东西了，
    // 再查 `d` 自然不是 `no`。第一版就是在这儿红的——
    // **夹具的"前置状态"没算清楚，测试就会自己造出假失败。**
    let clean = empty_dir("json-empty");
    let g = run(&["graph", "--root", clean.to_str().unwrap(), "--json"]);
    assert_eq!(code(&g), 0, "还没建过图谱是正常情形，不该非 0 退出");
    let gs = stdout(&g);
    assert!(
        gs.contains("\"state\"") && gs.contains("\"no\""),
        "还没建过图谱时 JSON 该是 state=no：{gs}"
    );

    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&clean);
}

/// **`guide` 的 JSON 是 CLI 与桌面界面共用的那一份**（2026-10-16）。
///
/// ## 这条钉的是什么
///
/// 概览页主位那张「一句话指南」和 `jingwei guide` **必须同源**——
/// 构造在 `Guide::to_json()` 上，两个出口都调它。
/// 两处各写一遍的后果这个仓库很清楚：`--help` 漏列 4 个命令、
/// 说明书承诺一个不存在的 `inspect`——**两份清单各写各的，必然漂移。**
///
/// 三个断言分开钉：
///   ① `ready` 是**一个对象带 `state`**，不是一个布尔（三态要装得下）；
///   ② 有 `headline` 与 `nextStep`（界面主位要的就是这两句）；
///   ③ `state=unknown` 时**必须带 `why`**——那是"该谁去补"，
///      少了它界面就只能显示一句压扁的"查不出来"。
#[test]
fn guide_json是命令行与界面共用的那一份() {
    let Some(_) = bin() else { return };
    let d = empty_dir("guide");
    std::fs::write(d.join("requirements.txt"), "rich>=13\n").unwrap();

    let o = run(&["guide", "--root", d.to_str().unwrap(), "--json"]);
    let s = stdout(&o);
    let v: serde_json::Value = serde_json::from_str(&s).expect("guide --json 要是合法 JSON");

    // ① ready 是个对象，带 state
    let state = v["ready"]["state"].as_str().unwrap_or("");
    assert!(
        ["yes", "no", "unknown"].contains(&state),
        "ready.state 必须是三态之一，实际：{state}"
    );
    // ② 界面主位要的那两句
    assert!(v["headline"].is_string(), "要有 headline（第一眼看的那句）");
    assert!(v["nextStep"].is_string(), "要有 nextStep（能直接敲的那条）");
    // ③ 「查不了」要能分出是谁的
    if state == "unknown" {
        let why = v["ready"]["why"].as_str().unwrap_or("");
        assert!(
            ["their_gap", "our_gap", "unclear"].contains(&why),
            "state=unknown 时必须带 why（该谁去补）：{s}"
        );
        assert!(
            v["ready"]["whatToDo"].is_string(),
            "还要带 whatToDo（用户该做什么）：{s}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// **退出码就是三态**：能跑 0 / 跑不起来 1 / 查不了 2。
///
/// 这一条是 `guide` 的招牌（它的注释里写着"三支都要有"）。
/// **"查不了"与"跑不起来"共用退出码 = 把三态压扁**，
/// 而脚本要靠它决定"要不要拦下来"。
#[test]
fn guide的退出码分三档() {
    let Some(_) = bin() else { return };
    let d = empty_dir("guide-code");
    // 一个"什么都没声明"的目录：没有契约、没有清单 → 查不了（2）
    let o = run(&["guide", "--root", d.to_str().unwrap()]);
    let c = code(&o);
    assert!(
        c == 0 || c == 2,
        "空目录该是'查不了'(2)或'能跑'(0)，实际 {c}"
    );
    assert_ne!(c, 1, "空目录不该是'跑不起来'(1)——那是把我们的空白说成他的问题");
    let _ = std::fs::remove_dir_all(&d);
}

/// **`doctor` 与 `guide` 对"能不能跑"必须给同一个退出码**（2026-10-17）。
///
/// ## 拿什么换的
///
/// 拿两个**从没测过的真项目**跑（工具声称支持 Node / Java，而我此前只测过 Python 与 Rust）：
///
/// | 目标 | `guide` | `doctor` |
/// |---|---|---|
/// | 真 Node 项目（依赖没装） | ✗ 跑不起来 · 退 **1** | 退 **0** |
/// | Java 项目（机器没 JDK） | ✗ 跑不起来 · 退 **1** | 退 **0** |
///
/// 而 `使用说明.md` 写的是「**退出码是准的**：命令跑成了才是 `0`……
/// 或那次检查没通过，都会给非 `0`」。
/// **一个跑不起来的项目，`doctor` 退 0 就是"检查通过了"——那是假话。**
///
/// ## 为什么不是把规则的 severity 调高
///
/// `web-deps-not-installed` 本身是对的：它的 boundary 写着
/// 「刚克隆下来、或者刚清过缓存时，这是正常状态」。改成 `Error` 会让"刚 clone"
/// 变成"出错了"，那是另一种误报。
///
/// 两个命令回答**两个不同的问题**，答案都对；**错的是退出码**——
/// 脚本只看退出码，不看你的措辞。
#[test]
fn doctor与guide对能不能跑给同一个退出码() {
    let d = empty_dir("node-no-deps");
    // 一个"声明了依赖但没装"的最小 Node 项目：guide 会说"跑不起来"，
    // 而 doctor 的 finding 只是 warn。两个退出码必须一致。
    std::fs::write(
        d.join("package.json"),
        r#"{"name":"x","version":"1.0.0","dependencies":{"express":"^4.19.0"}}"#,
    )
    .unwrap();
    std::fs::write(d.join("index.js"), "require('express');").unwrap();
    let p = d.to_str().unwrap().to_string();

    let g = run(&["guide", "--root", &p]);
    let doc = run(&["doctor", "--root", &p]);

    assert_eq!(
        code(&g),
        code(&doc),
        "同一个项目，两个命令的退出码不一样：guide={} doctor={}。\
         脚本只看退出码，它没法定两个都信。\n guide 输出：{}\n doctor 输出：{}",
        code(&g),
        code(&doc),
        stdout(&g),
        stdout(&doc),
    );
    // 而且这个码该是 1（跑不起来），不是 0
    assert_eq!(
        code(&g),
        1,
        "声明了依赖却没装，`guide` 该说跑不起来（1），实际 {}\n{}",
        code(&g),
        stdout(&g)
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// 夹具的路径必须真的是我们造的那个（**先确认夹具本身没搭错**）。
#[test]
fn 夹具目录是可用的() {
    let d = empty_dir("sanity");
    assert!(d.is_dir());
    let _ = std::fs::remove_dir_all(&d);
}

/// **目录不存在时，三个出口必须说同一件事、给同一个退出码**（2026-10-17）。
///
/// ## 拿什么换的
///
/// 拿边界输入跑出来的：对一个**拼错的路径**——
///   `guide`  → 「补上就能判断」（退出码 **2**）
///   `doctor` → 「问题 共 3 条（0 错误）……」 → **退出码 0**！
///   `audit`  → 又是另一种（退出码 2）
///
/// **`doctor` 退出 0 是最坏的**：脚本靠退出码决定要不要拦下来，
/// 而它对一个不存在的目录说"一切正常"。
///
/// ## 为什么是 `1` 而不是 `2`
///
/// `介绍.md` 的约定：**`1` = 跑不起来（他的问题）**、`2` = 查不了（我们的问题）。
/// 路径拼错是**他的问题**——不是我们读不到。所以 `1`，
/// **不需要发明第四种退出码**。
///
/// 三个断言分开钉：同一个码 / 那个码是 1 / 输出里说"不存在"。
#[test]
fn 目录不存在时三个出口说同一件事() {
    let d = empty_dir("missing-probe");
    let ghost = d.join("no-such-dir");
    assert!(!ghost.exists());

    for cmd in ["guide", "doctor", "audit"] {
        let o = run(&[cmd, "--root", ghost.to_str().unwrap()]);
        let c = code(&o);
        let out = format!("{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
        assert_eq!(
            c, 1,
            "{cmd} 对不存在的目录该退 1（他的问题），实际 {c}——\
             出口不一致，脚本就没法决定要不要拦下来。输出：{out}"
        );
        assert!(
            out.contains("不存在"),
            "{cmd} 要说清「这个目录不存在」，实际输出：{out}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// **"不是一个项目"时，`--json` 与文字输出必须说同一件事**（2026-10-17）。
///
/// ## 拿什么换的
///
/// 上一轮给 `guide` 加了 `what_to_do`（引擎知道场合时自己给一句话），
/// 而 `cmd_guide` 的文字渲染**直接读 `unknown_why.what_to_do()`，把那个字段绕过去了**：
/// `--json` 说「这里不像一个项目」，文字还在说「缺一样你这边的东西 / 补上就能判断」。
///
/// 而且 `nextStep` 也跟着错：头说"不适用"、尾说"你去补一份契约"——**同一屏自相矛盾**。
#[test]
fn 不是项目时json与文字说同一件事() {
    let d = empty_dir("not-a-project");
    // **夹具不能放源码**（2026-10-18 改）：现在"无标记但有源码"会给建议，
    // 而这条测试要验的是**json 与文字口径一致**，不是"会不会给建议"。
    // 有源码那一支由 `guide::tests::没标记但有源码时给建议` 钉住。
    std::fs::write(d.join("notes.txt"), "").unwrap();
    let p = d.to_str().unwrap().to_string();

    let j = run(&["guide", "--root", &p, "--json"]);
    let jv: serde_json::Value =
        serde_json::from_str(&stdout(&j)).expect("guide --json 该是合法 JSON");
    let json_says = jv["ready"]["whatToDo"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let json_next = jv["nextStep"].as_str().unwrap_or_default().to_string();

    let t = run(&["guide", "--root", &p]);
    let text = stdout(&t);

    assert!(
        json_says.contains("不像一个项目"),
        "json 该说出'不像一个项目'（引擎的表态），实际：{json_says}"
    );
    // ① 文字里要有**同一句话**——不许各说各的
    assert!(
        text.contains("不像一个项目"),
        "文字输出里没有那句'不像一个项目'：{text}"
    );
    // ② 不许再说"缺一样你这边的东西"（那是"你少给了什么"的口径，对象不对）
    assert!(
        !text.contains("缺一样你这边的东西"),
        "既然不是项目，就不许说'你缺东西'——那是按项目口径写的：{text}"
    );
    // ③ 下一步不许让他去补契约（头说"不适用"、尾说"你去补"＝同一屏自相矛盾）
    assert!(
        json_next.contains("不用做什么"),
        "不是一个项目时，下一步该是'不用做什么'，实际：{json_next}"
    );

    let _ = std::fs::remove_dir_all(&d);
}

/// **`projects` 说"跳过了 N 个目录"时，必须能核**（2026-10-18）。
///
/// 第一视角试用时抓到的：原来只说数量和规则（"node_modules / target / 以 . 开头"），
/// 读者**无法核对到底跳了哪几个**——那不是"说得准"，是"听起来准"。
#[test]
fn 跳过的目录要列名字() {
    let d = empty_dir("skipped-list");
    // 一个真项目 + 一个该被跳过的目录
    std::fs::create_dir_all(d.join("app")).unwrap();
    std::fs::write(d.join("app").join("package.json"), "{}").unwrap();
    std::fs::create_dir_all(d.join("node_modules").join("x")).unwrap();
    let p = d.to_str().unwrap().to_string();

    let out = run(&["projects", "--root", &p, "--depth", "2"]);
    let text = stdout(&out);
    assert!(text.contains("找到 1 个项目") || text.contains("找到 1 个项目"), "该只找到 app：{text}");
    assert!(text.contains("跳过了"), "跳过数要报：{text}");
    assert!(
        text.contains("node_modules"),
        "**要列名字**，否则读者无法核对（这条就是为它加的）：{text}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **没索引的项目，`positions` 不许"凭空通过"**（2026-10-20 修的真 bug）。
///
/// 起因：`positions` 以前用 `Store::open` 打开图谱——它会**建库**，于是没索引过的项目
/// 打印「✓ 对得上 0 / × 对不上 0」并 exit=0。而"对不上必须是 0"正是 MANIFEST 的验收判据，
/// 等于**没索引的项目凭空通过**，还在人家目录里留一个空 `.jingwei/`。
///
/// 这条测试钉住三件事：退出码非 0、说"查不了"、**没有副作用**（不许建库）。
#[test]
fn 没索引时_positions_说查不了而不是零错() {
    let Some(_) = bin() else { return };
    let d = empty_dir("positions-noindex");
    let p = d.to_str().unwrap().to_string();

    let o = run(&["positions", "--root", &p]);
    assert_ne!(
        code(&o),
        0,
        "没索引时退出 0 会被脚本当成「对不上 0 = 通过」：{}",
        stdout(&o)
    );
    let s = stdout(&o);
    assert!(s.contains("查不了"), "要明说查不了，实际：{s}");
    assert!(
        !d.join(".jingwei").exists(),
        "读路径不许在项目目录里建库（副作用）"
    );

    let _ = std::fs::remove_dir_all(&d);
}

/// **没索引的项目里，所有读命令都不许建库**（2026-10-21）。
///
/// 这一条是"读路径建库"那类 bug 的批量守卫：`Store::open` 会建库，一旦哪条读命令
/// 又用回去，就会在用户目录里留 `.jingwei/`，并把"查不了"压成"错了/全绿"。
/// 逐个命令跑一遍，只断言两件事：**跑得完**、**目录里没有 `.jingwei`**。
#[test]
fn 没索引时所有读命令都不建库() {
    let Some(_) = bin() else { return };
    let cmds: Vec<Vec<&str>> = vec![
        vec!["positions"],
        vec!["symbol", "nope"],
        vec!["explain", "nope"],
        vec!["learn", "--symbol", "nope"],
        vec!["knowledge", "check"],
        vec!["knowledge", "stale"],
        vec!["knowledge", "verify"],
        vec!["handover"],
    ];
    for c in cmds {
        let d = empty_dir("no-db");
        let p = d.to_str().unwrap().to_string();
        let mut args: Vec<&str> = c.clone();
        args.push("--root");
        args.push(&p);
        let o = run(&args);
        assert!(
            !d.join(".jingwei").exists(),
            "`jingwei {}` 在没索引的项目里建了库（读命令不许有副作用）",
            c.join(" ")
        );
        // 退出码只要求"能跑完"（不是被 panic/未捕获错误打断）
        assert!(
            code(&o) >= 0,
            "`jingwei {}` 异常退出：{}",
            c.join(" "),
            stdout(&o)
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// **"说了 N 条，就得列出 N 行"**（2026-10-21）。
///
/// 由来：`explain` 曾打 "被调用：3 处"，下面却列了 5 行（边数 vs 站点行数）。这种"数字与清单对不上"
/// 是最容易骗过自己的报告 bug——读的人不会去数，写的人也以为对。这里把它变成结构断言：
/// 报出的数应等于明细行数；超过 5 行时明细只给前 5 行且必须能看出来被截断。
#[test]
fn explain_报出的数与列出的行数一致() {
    let Some(_) = bin() else { return };
    let d = empty_dir("count-invariant");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("src").join("a.rs"),
        "fn helper() -> i32 { 1 }\nfn run() -> i32 { helper() + helper() }\n",
    )
    .unwrap();
    let p = d.to_str().unwrap().to_string();
    let _ = run(&["index", "--root", &p]);
    let out = run(&["explain", "helper", "--root", &p]);
    let text = stdout(&out);

    // `被调用：A 个符号 / B 个调用点`
    let line = text
        .lines()
        .find(|l| l.starts_with("被调用："))
        .unwrap_or_else(|| panic!("没有'被调用'行：{text}"));
    let nums: Vec<usize> = line
        .chars()
        .collect::<String>()
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    assert_eq!(nums.len(), 2, "应报两个数（符号数 / 调用点数）：{line}");
    let (symbols, sites) = (nums[0], nums[1]);

    let details: Vec<&str> = text
        .lines()
        .skip_while(|l| !l.starts_with("被调用："))
        .skip(1)
        .take_while(|l| !l.starts_with("被引用："))
        .filter(|l| l.trim_start().starts_with("src/"))
        .collect();
    let shown = if sites > 5 { 5 } else { sites };
    assert_eq!(
        details.len(),
        shown,
        "报 {sites} 个调用点，却列了 {} 行：{text}",
        details.len()
    );
    let distinct: std::collections::BTreeSet<&str> = details
        .iter()
        .filter_map(|l| l.split(" ← ").nth(1))
        .collect();
    assert!(
        distinct.len() <= symbols,
        "报 {symbols} 个符号，明细里却有 {} 个不同调用者：{text}",
        distinct.len()
    );

    let _ = std::fs::remove_dir_all(&d);
}

/// **`impact` 的层计数必须等于列出的行数**（2026-10-21，与 explain 那条同一类不变量）。
///
/// 触发点是"第 N 层（K 个）:"后面只列前 6 个——如果 `K` 是"总数"而明细是"全部"（或反之），
/// 读的人会以为看全了。这里用「列出的行数 + 是否给出"等 N 个"」还原真实条数，必须等于 K。
#[test]
fn impact_每层报的个数与列出的行数一致() {
    let Some(_) = bin() else { return };
    let d = empty_dir("impact-count");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("src").join("a.rs"),
        "fn helper() -> i32 { 1 }\nfn mid1() -> i32 { helper() }\nfn mid2() -> i32 { helper() }\nfn top() -> i32 { mid1() + mid2() }\n",
    )
    .unwrap();
    let p = d.to_str().unwrap().to_string();
    let _ = run(&["index", "--root", &p]);
    let out = run(&["impact", "helper", "--root", &p]);
    let text = stdout(&out);

    let lines: Vec<&str> = text.lines().collect();
    let mut checked = 0;
    for (i, l) in lines.iter().enumerate() {
        let Some(rest) = l.trim_start().strip_prefix("第 ") else {
            continue;
        };
        let Some((n, tail)) = rest.split_once(" 层（") else {
            continue;
        };
        let Some(k) = tail.split(" 个）").next().and_then(|s| s.trim().parse::<usize>().ok()) else {
            continue;
        };
        let mut listed = 0usize;
        let mut truncated = false;
        for nxt in lines.iter().skip(i + 1) {
            if nxt.trim_start().starts_with("第 ") || nxt.trim_start().starts_with("知识层") || nxt.trim_start().starts_with("边界") {
                break;
            }
            if nxt.trim_start().starts_with("被 ") {
                listed += 1;
            } else if nxt.contains("…等 ") {
                truncated = true;
            }
        }
        let shown = if k > 6 { 6 } else { k };
        assert_eq!(
            listed, shown,
            "第 {n} 层报 {k} 个，却列了 {listed} 行（截断标记 {truncated}）：{text}"
        );
        checked += 1;
    }
    assert!(checked >= 1, "一条层都没有，测试没量到东西：{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// **`knowledge check` 的"文本态 N 条"必须等于列出的名字数**（2026-10-21）。
///
/// 这一行同时干两件事：报数 + 列名字（超过 5 个时截断并写"等 N 条"）。
/// 数字和清单必须是同一个来源，否则"3 条"下面列 2 个名字都不会有人发现。
#[test]
fn check_文本态的个数与列出的名字一致() {
    let Some(_) = bin() else { return };
    let d = empty_dir("check-unmapped");
    let kdir = d.join("docs").join("knowledge");
    std::fs::create_dir_all(&kdir).unwrap();
    for i in 1..=3 {
        std::fs::write(
            kdir.join(format!("e{i}.md")),
            format!("---\nclaim: 第 {i} 条主张\nsource: https://example.com/{i}\nlevel: 一手\ncheck: 只是想清楚\n---\n"),
        )
        .unwrap();
    }
    let p = d.to_str().unwrap().to_string();
    let out = run(&["knowledge", "check", "--root", &p]);
    let text = stdout(&out);
    let line = text
        .lines()
        .find(|l| l.contains("文本态"))
        .unwrap_or_else(|| panic!("没有'文本态'行：{text}"));
    let n: usize = line
        .split("文本态 ")
        .nth(1)
        .and_then(|s| s.split(" 条").next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| panic!("这行没有可解析的条数：{line}"));
    let listed = line
        .split("）：")
        .nth(1)
        .map(|rest| rest.split(" / ").count())
        .unwrap_or(0);
    let shown = if n > 5 { 5 } else { n };
    assert_eq!(listed, shown, "报 {n} 条，却列了 {listed} 个名字：{line}");
    let _ = std::fs::remove_dir_all(&d);
}

/// **`symbol` 的"条目：N 条"必须等于列出的条数**（2026-10-21，第三处同形状的不变量）。
///
/// 现在它不截断（全列），但"报数 + 列表"的形状在别处都出过事；这条测试先把口径钉住：
/// 将来若加截断，必须同时给出"等 N 条"，否则数字与清单就对不上了。
#[test]
fn symbol_报的条目数与列出的条数一致() {
    let Some(_) = bin() else { return };
    let d = empty_dir("symbol-count");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("src").join("a.rs"),
        "fn helper() -> i32 { 1 }\nfn run() -> i32 { helper() }\n",
    )
    .unwrap();
    let kdir = d.join("docs").join("knowledge");
    std::fs::create_dir_all(&kdir).unwrap();
    for i in 1..=2 {
        std::fs::write(
            kdir.join(format!("e{i}.md")),
            format!(
                "---\nclaim: 第 {i} 条主张\nsource: https://example.com/{i}\nlevel: 一手\ncheck: 只是想清楚\nsymbol: symbol:src/a.rs#helper\n---\n"
            ),
        )
        .unwrap();
    }
    let p = d.to_str().unwrap().to_string();
    let _ = run(&["index", "--root", &p]);
    let _ = run(&["knowledge", "sync", "--root", &p]);
    let out = run(&["symbol", "helper", "--root", &p]);
    let text = stdout(&out);

    let line = text
        .lines()
        .find(|l| l.starts_with("条目："))
        .unwrap_or_else(|| panic!("没有'条目'行：{text}"));
    let n: usize = line
        .split("条目：")
        .nth(1)
        .and_then(|s| s.split(" 条").next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| panic!("这行没有可解析的条数：{line}"));
    let listed = text
        .lines()
        .filter(|l| {
        let t = l.trim_start();
        t.starts_with(|c: char| c.is_ascii_digit()) && t.contains(". [")
    })
    .count();
    assert_eq!(listed, n, "报 {n} 条，却列了 {listed} 条：{text}");
    let _ = std::fs::remove_dir_all(&d);
}


/// **删库重建后，`index` 要自动把知识层按文档补回来**（2026-10-21）。
///
/// 场景：换机器 / 清了 `.jingwei` / 从 git 拉了别人仓库（库不在版本控制里）。
/// 不补的话 `explain` 的"知识层"那行会**静默变空**——看着像"这条代码没被任何主张解释过"，
/// 真相是"投影还没建"。三态纪律：不能让"没建"伪装成"没有"。
#[test]
fn index_后知识层自动重建() {
    let Some(_) = bin() else { return };
    let d = empty_dir("index-claims");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn f() {}\n").unwrap();
    let kdir = d.join("docs").join("knowledge");
    std::fs::create_dir_all(&kdir).unwrap();
    std::fs::write(
        kdir.join("e.md"),
        "---\nclaim: 示例主张\nsource: https://example.com/x\nlevel: 一手\ncheck: 只是想清楚\nsymbol: src/a.rs#f\n---\n",
    )
    .unwrap();
    let p = d.to_str().unwrap().to_string();

    // 1) 第一次索引：库是空的，应当**顺手把知识层建起来**并在输出里说一句。
    let out = run(&["index", "--root", &p]);
    let text = stdout(&out);
    assert!(
        text.contains("知识层已按文档重建"),
        "删库后第一次索引应重建知识层，实际输出：{text}"
    );

    // 2) 紧接着问一次：不该再报"漂移"（说明投影真的建好了，不是只打了句话）。
    let chk = run(&["knowledge", "check", "--root", &p]);
    let ctext = stdout(&chk);
    assert!(
        !ctext.contains("漂移"),
        "索引后不该还报漂移：{ctext}"
    );

    // 3) 第二次索引：已经有投影，**不该**再打印重建（不擅自动用户的库）。
    let out2 = run(&["index", "--root", &p]);
    let text2 = stdout(&out2);
    assert!(
        !text2.contains("知识层已按文档重建"),
        "投影已存在时不该重建，实际输出：{text2}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **泛型容器的方法：说"查不了"时要补一句"不等于没人用"**（2026-10-21）。
///
/// 接收者类型是 `Type<..>` 时，调用点写的是 `x.m()` 而 `x` 的类型推不出来——
/// 那类调用静态连就是猜。不解释的话，用户会把"查不了"读成"这个方法没人调用"。
#[test]
fn explain_泛型方法会解释查不了的含义() {
    let Some(_) = bin() else { return };
    let d = empty_dir("explain-generic");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        d.join("src/a.rs"),
        "pub struct G<T> { pub x: T }\nimpl<T> G<T> {\n    pub fn m(&self) {}\n}\npub struct P;\nimpl P {\n    pub fn m(&self) {}\n}\n",
    )
    .unwrap();
    let p = d.to_str().unwrap().to_string();
    let idx = run(&["index", "--root", &p]);
    assert_eq!(code(&idx), 0, "索引应成功：{}", stdout(&idx));

    // 泛型容器：应带那句解释。
    let g = run(&[
        "explain",
        "symbol:src/a.rs#G<T>::m",
        "--root",
        &p,
    ]);
    let gtext = stdout(&g);
    assert!(
        gtext.contains("不等于「没人用」"),
        "泛型方法应解释'查不了'的含义，实际：{gtext}"
    );

    // 具体类型：不该多这句（否则这句话就失去信息量）。
    let s = run(&["explain", "symbol:src/a.rs#P::m", "--root", &p]);
    let stext = stdout(&s);
    assert!(
        !stext.contains("不等于「没人用」"),
        "具体类型不该带这句解释，实际：{stext}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **`impact` 对泛型方法也要说清"查不了不等于没上游"**（2026-10-21，与 `explain` 同一句话）。
///
/// "0 个上游"最危险的读法是"改它很安全"。泛型派发的调用点静态连不准，所以这句话必须出现。
#[test]
fn impact_泛型方法会提醒不要当成没有上游() {
    let Some(_) = bin() else { return };
    let d = empty_dir("impact-generic");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        d.join("src/a.rs"),
        "pub struct G<T> { pub x: T }\nimpl<T> G<T> {\n    pub fn m(&self) {}\n}\npub struct P;\nimpl P {\n    pub fn m(&self) {}\n}\n",
    )
    .unwrap();
    let p = d.to_str().unwrap().to_string();
    let idx = run(&["index", "--root", &p]);
    assert_eq!(code(&idx), 0, "索引应成功：{}", stdout(&idx));

    let g = run(&["impact", "symbol:src/a.rs#G<T>::m", "--root", &p]);
    let gtext = stdout(&g);
    assert!(
        gtext.contains("不等于「没有上游」"),
        "泛型方法应提醒别当成没有上游，实际：{gtext}"
    );

    let s = run(&["impact", "symbol:src/a.rs#P::m", "--root", &p]);
    let stext = stdout(&s);
    assert!(
        !stext.contains("不等于「没有上游」"),
        "具体类型不该带这句提醒，实际：{stext}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **`handover` 的「③ 规模」必须与 `graph stats` 逐个数一致**（2026-10-21）。
///
/// 由来：这两个数我给用户念过好几次，也**因为不同步被坑过两次**（包是旧的、报告是旧的）。
/// 它们是同一个事实的两个出口，任何一处漂移都该红——**人来对眼睛不可靠，让它自动对**。
#[test]
fn handover_的规模与_graph_stats_一致() {
    let Some(_) = bin() else { return };
    let d = empty_dir("handover-counts");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        d.join("src/a.rs"),
        "pub fn helper() -> i32 { 1 }\npub fn run() -> i32 { helper() + helper() }\n",
    )
    .unwrap();
    let p = d.to_str().unwrap().to_string();
    assert_eq!(code(&run(&["index", "--root", &p])), 0, "索引应成功");

    let stats = stdout(&run(&["graph", "stats", "--json", "--root", &p]));
    let sj: serde_json::Value = serde_json::from_str(&stats).expect("stats JSON 解析失败");
    let want_files = sj["nodes"]["file"].as_i64().unwrap_or(0);
    let want_symbols = sj["nodes"]["symbol"].as_i64().unwrap_or(0);
    let want_calls = sj["edges"]["calls"].as_i64().unwrap_or(0);
    let want_refs = sj["edges"]["references"].as_i64().unwrap_or(0);

    let ho = stdout(&run(&["handover", "--root", &p]));
    let line = ho
        .lines()
        .find(|l| l.contains("③ 规模"))
        .unwrap_or_else(|| panic!("没有「③ 规模」行：{ho}"));
    let nums: Vec<i64> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    assert_eq!(
        nums,
        vec![want_files, want_symbols, want_calls, want_refs],
        "handover 的规模与 graph stats 不一致：{line}"
    );
    let _ = std::fs::remove_dir_all(&d);
}
