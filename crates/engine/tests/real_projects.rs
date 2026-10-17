//! **拿真夹具问它，然后检查答案的结构。**
//!
//! ## 为什么单独一条集成测试
//!
//! 这个仓库的账本上有一条反复出现的规律：**写新单元测试很少抓到缺陷，
//! 而"拿真东西去问它"每一次都抓到。** 已经抓到过的（都在 `AGENTS.md` §5）：
//!
//! | 拿什么去问 | 抓到什么 |
//! |---|---|
//! | 真窗口截图 | 「装项目的目录」被当成一个项目 |
//! | `audit --json` 扫演示项目 | Rust 项目在检测层**不存在**（`langs=[]`） |
//! | 同一次扫描 | 工具自己的 `.jingwei` 污染了被检项目的结论 |
//! | 五个不同语言的真项目 | Rust 有检测有依赖有诊断，**唯独没有体检规则** |
//!
//! 这些都不是"某个函数算错了"，而是**结构与结构之间的缝**——单元测试盯着一个函数，
//! 缝正好落在两个函数之间。所以这一条的做法跟单元测试相反：
//! **造真的项目，走真的路（`make_facts` → `run_all`），检查答案的形状。**
//!
//! ## 断言为什么是"结构性"的
//!
//! 如果断言"这个项目应该正好 3 条问题"，那每次改文案都会红，门禁会被人绕过去
//! （这个项目的账本上记着这种事的代价）。这里钉的是**不管内容怎么变都必须成立的东西**：
//! 语言认不认得出来、每条问题四要素齐不齐、有没有 id 重复、有没有把内部失败漏到用户面前。
//! 语言那几条是**精确**的——因为"哪门语言"没有解释空间。

// 测试里的临时目录清理失败不影响断言，也不能改变结论——显式豁免，
// 而不是让 lint 在整个 crate 里静默失声。
#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::detect::probe::make_facts;
use jingwei_engine::understand::rules::run_all;
use jingwei_model::Severity;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// **每个目录名必须唯一，不能只靠 PID。**
///
/// 踩过一次：第一版只用了 `process::id()`，而 Rust 的集成测试是**在同一进程里
/// 并行跑**的——每个测试都调 `fixtures()`，于是后调的那个把前一个的临时目录
/// `remove_dir_all` 掉了，断言看到的是空目录（"Python 项目认不出语言"就是这么来的）。
/// 那是**测试自己**的 bug，不是产品的；但它报出来的症状跟真缺陷一模一样，
/// 很值得记一笔：**假警报和真缺陷长得一样，所以脚手架的正确性也得当回事。**
static SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("jw-real-{}-{}-{}", tag, std::process::id(), n));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(p, body).unwrap();
}

/// 五种语言 + 一个空目录 + 一个复合项目。**每种都能被「哪门语言」精确断言。**
///
/// 空目录那一份是故意的：判据是"不知道就不许猜"，所以它必须一个语言都不报。
fn fixtures() -> Vec<(&'static str, PathBuf, Vec<&'static str>)> {
    let mut out = Vec::new();

    let d = scratch("python");
    write(&d, "main.py", "print(1)\n");
    write(&d, "requirements.txt", "requests\n");
    out.push(("python", d, vec!["Python"]));

    let d = scratch("rust");
    write(&d, "Cargo.toml", "[package]\nname=\"x\"\n");
    write(&d, "src/main.rs", "fn main(){}\n");
    out.push(("rust", d, vec!["Rust"]));

    // **只有 `Cargo.toml`、没有 `.rs`。**
    //
    // 这一份是专门为了"测得出走的是哪条路"加的。上面那份带 `src/main.rs`，
    // 而检测里有一条 `has_ext(&["rs"])` 兜底——**把 `Cargo.toml` 那条判据整个删掉，
    // 它照样会被认成 Rust**。于是上面那份测的是"结果对"，测不出"靠什么认出来的"。
    //
    // 这一份没有 `.rs` 可看，只能靠构建文件认。它同时钉住一个真实场景：
    // Cargo 工作区、或者还没写代码的空壳项目——`Cargo.toml` 在，源码还没长出来。
    let d = scratch("rust-manifest-only");
    write(&d, "Cargo.toml", "[package]\nname=\"x\"\n");
    write(&d, "README.md", "# 还没写代码\n");
    out.push(("rust-manifest-only", d, vec!["Rust"]));

    let d = scratch("java");
    write(&d, "pom.xml", "<project/>\n");
    write(&d, "src/main/java/Main.java", "public class Main {}\n");
    out.push(("java", d, vec!["Java"]));

    let d = scratch("cpp");
    write(&d, "CMakeLists.txt", "cmake_minimum_required(VERSION 3.10)\n");
    write(&d, "main.cpp", "int main(){}\n");
    out.push(("cpp", d, vec!["C / C++"]));

    let d = scratch("node");
    write(&d, "package.json", "{\"name\":\"x\",\"version\":\"1.0.0\"}\n");
    write(&d, "index.js", "console.log(1)\n");
    out.push(("node", d, vec!["前端 / Node"]));

    // 复合项目：一个仓库两门语言。**少报一边就等于告诉用户"另一边不存在"。**
    let d = scratch("composite");
    write(&d, "package.json", "{\"name\":\"t\",\"version\":\"1.0.0\"}\n");
    write(&d, "src-tauri/Cargo.toml", "[package]\nname=\"t\"\n");
    write(&d, "src-tauri/src/main.rs", "fn main(){}\n");
    out.push(("composite", d, vec!["前端 / Node", "Rust"]));

    // 空目录：什么都不该报
    let d = scratch("empty");
    out.push(("empty", d, vec![]));

    out
}

fn lang_names(root: &Path) -> Vec<String> {
    let facts = make_facts(root, None);
    facts.langs.iter().map(|l| l.name().to_string()).collect()
}

/// **一、哪门语言，必须认对。**
///
/// 这条单独提出来，是因为它是**已经真出过事**的那一条：Rust 曾经在
/// `detect::lang::Lang` 里**根本没有变体**，于是 `hello_rust` 的 `langs` 是空数组，
/// 而下游把"空 + Unknown"当成"可能是 Python"，**对一个纯 Rust 项目跑了全套 Python 规则**。
#[test]
fn every_fixture_is_read_as_the_right_language() {
    for (tag, root, want) in fixtures() {
        let got = lang_names(&root);
        assert_eq!(
            got, want,
            "「{tag}」的语言认错了：期望 {want:?}，实际 {got:?}。\
             语言认错会让整套规则跑在错的项目上——这比漏报更糟，因为它会变成假警报。"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// **二、复合项目两边都要报。**
///
/// 单独一条是因为它断言的性质与上面不同：上面是"认对"，这里是"**一个都不许落下**"。
/// 只报一边等于告诉用户"另一边不存在"——而用户会信。
#[test]
fn a_composite_project_reports_both_halves() {
    let d = scratch("composite-both");
    write(&d, "package.json", "{\"name\":\"t\",\"version\":\"1.0.0\"}\n");
    write(&d, "src-tauri/Cargo.toml", "[package]\nname=\"t\"\n");
    write(&d, "src-tauri/src/main.rs", "fn main(){}\n");
    let got = lang_names(&d);
    assert!(got.iter().any(|l| l.contains("Node")), "复合项目漏了前端那一半：{got:?}");
    assert!(got.iter().any(|l| l == "Rust"), "复合项目漏了 Rust 那一半：{got:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// **三、每条问题四要素齐全。**
///
/// 这是这个项目四条不许破的线之一（"无自查不报错"）。单元测试里有一条守着
/// `Severity::Error` 那一档，但 **`Warn` / `Info` 没人守**——而用户看到的大部分是那两档。
#[test]
fn every_finding_is_actionable_and_has_a_self_check() {
    for (tag, root, _) in fixtures() {
        let facts = make_facts(&root, None);
        for f in run_all(&facts, None) {
            assert!(!f.id.is_empty(), "「{tag}」有一条问题没有 id");
            assert!(!f.title.is_empty(), "「{tag}」的 {} 没有标题", f.id);
            assert!(
                !f.boundary.is_empty(),
                "「{tag}」的 {} 没写「什么时候不用管」——那一条是防误报的，不能空",
                f.id
            );
            assert!(
                !f.verify.is_empty(),
                "「{tag}」的 {} 没有自查命令；用户没法自己验证的话，这条结论就只能靠信我们",
                f.id
            );
            for v in &f.verify {
                assert!(!v.trim().is_empty(), "「{tag}」的 {} 有一条空的自查命令", f.id);
            }
            if f.severity == Severity::Error {
                assert!(
                    f.title.chars().count() >= 6,
                    "「{tag}」的 {} 报的是「要处理」，标题却短得看不出在说什么：{:?}",
                    f.id,
                    f.title
                );
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// **四、同一条问题不许报两遍。**
///
/// 两个不同层的规则写了同一件事时，用户会以为是两个问题——
/// 2026-10-16 写 `rust-lock-missing` 时当场撞上（`project.rs` 里已经有 `cargo-lock-missing`）。
/// 这条钉住"以后别再撞"。
#[test]
fn the_same_problem_is_never_reported_twice() {
    for (tag, root, _) in fixtures() {
        let facts = make_facts(&root, None);
        let mut ids: Vec<String> = run_all(&facts, None).into_iter().map(|f| f.id).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(
            before,
            ids.len(),
            "「{tag}」有重复的问题 id——同一件事报两遍，用户会以为是两个问题"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// **五、"查不了"不许伪装成"没有问题"。**
///
/// 三态是这套系统的核心，而**最危险的错法是把"我们失败了"写成一条看着正常的结论**。
/// 这条查的是结论正文里有没有混进内部失败的措辞——那种话该走 `Unknown` 通道，
/// 不该变成"这个项目有问题"。
#[test]
fn no_finding_is_an_internal_error_in_disguise() {
    // 这些词出现在**结论正文**里，说明某处把"我们没查成"当成了"这东西有毛病"。
    const SMELLS: &[&str] = &["解析失败", "内部错误", "panic", "not implemented", "todo!"];
    for (tag, root, _) in fixtures() {
        let facts = make_facts(&root, None);
        for f in run_all(&facts, None) {
            let body = format!("{} {} {}", f.title, f.evidence.join(" "), f.next);
            for s in SMELLS {
                assert!(
                    !body.contains(s),
                    "「{tag}」的 {} 把内部失败写进了结论（含「{s}」）：{body}",
                    f.id
                );
            }
            assert!(
                !f.next.trim().is_empty(),
                "「{tag}」的 {} 的「下一步」是空的——说了有问题却不说往哪走，等于把问题丢回给用户",
                f.id
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// **六、有 `Cargo.toml` 却没工具链时，必须报「要处理」。**
///
/// 这一条是"结构性"断言里唯一的例外——它精确断言了一条规则的存在，
/// 因为那条规则的理由是硬的：**没工具链时 `cargo build` 直接起不来**，
/// 而用户看到的是 `'cargo' 不是内部或外部命令`，那句话不会告诉他"这台机器缺工具链"。
///
/// 做法：把 `facts.toolchain["rustc"]` 拿掉再跑规则——**这模拟的是"那台机器没装"，
/// 而不是"这台机器没装"**，所以它在任何机器上结论都一样。
#[test]
fn a_rust_project_without_a_toolchain_is_an_error() {
    let d = scratch("rust-no-toolchain");
    write(&d, "Cargo.toml", "[package]\nname=\"x\"\n");
    write(&d, "src/main.rs", "fn main(){}\n");
    let mut facts = make_facts(&d, None);
    assert!(
        facts.langs.contains(&jingwei_engine::detect::lang::Lang::Rust),
        "夹具没被认成 Rust，这条测试本身不成立"
    );
    facts
        .toolchain
        .insert("rustc".into(), jingwei_engine::facts::ToolFact::default());
    let errs: Vec<String> = run_all(&facts, None)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| f.id)
        .collect();
    assert!(
        errs.contains(&"rust-no-toolchain".to_string()),
        "没工具链的 Rust 项目必须报「要处理」，实际只报了：{errs:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **七、反过来：Python 项目不许收到 Rust 规则。**
///
/// 分派写反了照样能通过只钉一个方向的测试——所以两个方向都要钉。
#[test]
fn a_python_project_gets_no_rust_rule() {
    let d = scratch("py-not-rust");
    write(&d, "main.py", "print(1)\n");
    let mut facts = make_facts(&d, None);
    facts
        .toolchain
        .insert("rustc".into(), jingwei_engine::facts::ToolFact::default());
    let ids: Vec<String> = run_all(&facts, None).into_iter().map(|f| f.id).collect();
    assert!(
        !ids.contains(&"rust-no-toolchain".to_string()),
        "Python 项目收到了 Rust 规则——规则按语言分派这件事写反了：{ids:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}