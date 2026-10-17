//! 端到端：**一个真目录 → 图谱里的一批节点与边**。
//!
//! 这是设计 §12.1 里那一层"集成测试"的第一次落地。之前只有单元测试——
//! 单元测试各自都对，但它们不回答"连起来跑通了吗"这个问题。
//!
//! 用真临时目录、真 SQLite 文件（不是内存库）：内存库漏得掉 WAL、文件锁、
//! 重复打开这些**只在真文件上才出现**的问题。

// 测试里的临时目录清理失败不影响断言，也不能改变结论——显式豁免，
// 而不是让 lint 在整个 crate 里静默失声。
#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::{
    index,
    store::{Dir, Store},
    EdgeKind,
};
use std::path::{Path, PathBuf};

fn make_project(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-e2e-{tag}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).expect("清临时目录失败");
    }
    std::fs::create_dir_all(d.join("src")).expect("建目录失败");
    std::fs::write(
        d.join("src/lib.rs"),
        "pub fn a() {}\nstruct S;\nimpl S { fn m(&self) {} }\n",
    )
    .expect("写文件失败");
    // 故意写坏一个：它必须进图谱（符号是真的），但必须被标成"不完整"。
    std::fs::write(d.join("src/bad.rs"), "fn broken( { }\n").expect("写文件失败");
    d
}

fn open(root: &Path) -> Store {
    Store::open(&root.join(".jingwei").join("graph.db")).expect("开库失败")
}

/// 按给定的文件内容铺一个临时项目。
///
/// 为什么要这个：`make_project` 是固定内容，而"闭包""重导出"这些东西
/// **只有写成具体形状才测得出来**——它们本来就是形状问题。
fn make_tree(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-e2e-{tag}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).expect("清临时目录失败");
    }
    for (rel, body) in files {
        let p = d.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("建目录失败");
        }
        std::fs::write(&p, body).expect("写文件失败");
    }
    d
}

/// **产物目录里的源码一个都不该被索引，而且大小写不敏感**（2026-10-17）。
///
/// ## 拿什么换的
///
/// 「哪些目录不当源码扫描」这份清单原来在 **三处各写一遍**
/// （`detect/lang.rs` / `graph/index.rs` / `state/watch.rs`），而且**判法不同**：
/// `lang.rs` 把名字 `to_lowercase()` 之后比，另外两处**直接比原名**。
///
/// 而 Windows 的文件系统**不区分大小写**——`Target` 与 `target` 是同一个目录。
/// 于是**同一份清单、两种判法，结果就分叉**：
///
///   · `jingwei index` 对一个含 `Target/deep/x.rs` 的目录报 **2 个文件**
///     （真源码只有 1 个）—— 大写 T 漏过去了；
///   · `__pycache__` **压根不在** `index.rs` 那份清单里，
///     而 `parse_python_tests.rs` 的注释写着「这条由 `index.rs` 的目录黑名单管」——
///     **一条假注释，指着一条不存在的判据**。实测 `__pycache__/x.py` 会被索引。
///
/// 现在清单与判法都只有一处（`util::scan::NOT_SOURCE_DIRS` / `is_not_source_dir`），
/// 这个测试钉住**从外面看得见的效果**：那几个目录里的源码一个都不许算。
#[test]
fn 产物目录里的源码一个都不该被索引() {
    let root = make_tree(
        "skip-dirs",
        &[
            ("src/app.rs", "fn main(){}"),
            // 大写 T：Windows 上和 `target/` 是同一个目录，原来的判法漏掉了
            ("Target/deep/x.rs", "fn a(){}"),
            ("NODE_MODULES/pkg/y.rs", "fn b(){}"),
            // 这个最典型：注释说"index.rs 管"，而 index.rs 里没有它
            ("__pycache__/tricky.rs", "fn c(){}"),
            // 我们自己的笔记目录（AGENTS.md：别把自己算进被观察的对象）
            (".jingwei/notes.note", "x"),
            ("Cargo.toml", "[package]\nname=\"t\"\nversion=\"0.1.0\"\n"),
        ],
    );
    let mut s = open(&root);
    let r = index::index_rust(&root, &mut s).expect("索引失败");
    assert_eq!(
        r.files, 1,
        "只有 src/app.rs 是源码；Target/ NODE_MODULES/ __pycache__/ 一律不许算，实际 {}",
        r.files
    );
    assert_eq!(r.symbols, 1, "只该有 main 一个符号，实际 {}", r.symbols);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn 一个目录索引完图谱里就有了东西() {
    let root = make_project("basic");
    let mut s = open(&root);
    let r = index::index_rust(&root, &mut s).expect("索引失败");

    assert_eq!(r.files, 2, "两个 .rs 文件");
    assert!(r.symbols >= 4, "至少 a/S/m/broken 四个符号，实际 {}", r.symbols);
    assert_eq!(r.incomplete, 1, "bad.rs 必须被标成不完整");
    assert!(!r.truncated);

    let (nodes, edges) = s.counts().expect("数不过来");
    assert_eq!(
        nodes as usize,
        3 + r.symbols,
        "项目 1 + 文件 2 + 全部符号"
    );
    assert!(edges >= 3, "项目→文件 两条、文件→符号 至少一条，实际 {edges}");

    // 项目节点在（相对路径的根就是 "."，规范化之后是空的）
    assert!(s.node("project:").expect("读失败").is_some());
    // 文件节点在
    assert!(s.node("file:src/lib.rs").expect("读失败").is_some());
}

/// **幂等**：同一个目录索引两遍，图谱不该变大。
/// 这条是整套设计的核心玩法（反复重跑），所以它必须在集成层也成立。
#[test]
fn 索引两遍图谱不会变大() {
    let root = make_project("idem");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("第一次失败");
    let first = s.counts().expect("数不过来");
    index::index_rust(&root, &mut s).expect("第二次失败");
    let second = s.counts().expect("数不过来");
    assert_eq!(first, second, "重跑不该涨库");
}

/// **`scanned` 与 `files` 是两个数，而且第二遍 `files` 就该是 0**（2026-10-16）。
///
/// 为什么这条值得单独钉：这两个数**混着用会把用户骗了**——
/// `jingwei index` 原来只报 `files`，而它是"这次真正处理了几个"，
/// 于是增量运行会显示「1 个文件 · 10 个符号」，**看起来像"这个项目只有一个文件"**。
/// 实测两处外壳因此差 7 倍：`jingwei index` 报 1，`export_graph`（临时空库）报 91。
///
/// 三个断言分开钉，因为它们会各自坏：
///   ① 第一遍：扫到 N 个，且**全都处理了**（`files == scanned`）；
///   ② 第二遍：扫到的数**不变**（一个都没漏扫），而处理的是 **0**（全被增量跳过）；
///   ③ 第二遍的 `skipped_unchanged + skipped_same_content` **等于 `scanned`**——
///      否则"没变"这个说法就没有下落（可能是被别的分支悄悄丢掉的）。
#[test]
fn 扫到的数与处理了的数是两个数() {
    let root = make_project("scanned-vs-files");
    let mut s = open(&root);

    let first = index::index_rust(&root, &mut s).expect("第一次失败");
    assert!(first.scanned > 0, "空库第一遍必须扫到东西");
    assert_eq!(
        first.files, first.scanned,
        "空库第一遍：扫到的都该被处理"
    );

    let second = index::index_rust(&root, &mut s).expect("第二次失败");
    assert_eq!(
        second.scanned, first.scanned,
        "第二遍扫到的数不该变——变了说明收集不稳定"
    );
    assert_eq!(
        second.files, 0,
        "第二遍什么都没改，处理数必须是 0（这正是'增量在正常工作'）"
    );
    assert_eq!(
        second.skipped_unchanged + second.skipped_same_content,
        second.scanned,
        "没变的数要对得上：否则'跳过了'这个说法没有下落"
    );
}

/// 符号的 id 里必须带容器路径——不然 `Ledger::post` 和别的 `post` 会撞成同一个节点。
#[test]
fn impl_里的方法带容器路径() {
    let root = make_project("container");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("索引失败");
    assert!(
        s.node("symbol:src/lib.rs#S::m").expect("读失败").is_some(),
        "impl 里的方法必须是 S::m，不是光秃秃的 m"
    );
    // 而且大小写要原样保留（Rust 大小写敏感）
    assert!(s.node("symbol:src/lib.rs#S").expect("读失败").is_some());
}

/// 上次索引会留下 `files.incomplete` 那种"没解析完"的痕迹——
/// 这一层还没有把 `files` 表接上（那是下一步），所以这里只确认
/// **坏文件不会让整次索引失败**：一个文件坏，别的照收。
#[test]
fn 一个文件坏掉不影响别的文件() {
    let root = make_project("resilient");
    let mut s = open(&root);
    let r = index::index_rust(&root, &mut s).expect("索引不该因为一个坏文件就整次失败");
    assert_eq!(r.files, 2, "两个文件都进去了");
    assert_eq!(r.incomplete, 1, "但坏的那个被标出来了");
    assert!(
        s.node("symbol:src/bad.rs#broken").expect("读失败").is_some(),
        "坏文件里的符号仍然进图谱——它确实存在，只是不可靠"
    );
}

// ── 增量：M4 的四条行为 ─────────────────────────────────────────

/// **增量真的生效**：一个字节都没改，第二遍就该一个文件都不重解析。
/// 这是"重跑是核心玩法"这句话能不能成立的关键——每次重跑都全量解析的话，
/// 大项目根本等不起。
#[test]
fn 没改东西时第二遍全部跳过() {
    let root = make_project("nochange");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("第一遍失败");
    let r = index::index_rust(&root, &mut s).expect("第二遍失败");
    assert_eq!(r.files, 0, "没有文件该被重解析");
    assert_eq!(r.skipped_unchanged, 2, "两个文件都该被一级检测拦下");
    assert_eq!(r.edges_expired, 0, "没重解析就不该动关系");
}

/// **`touch` 一下不该触发重解析**——这是二级检测存在的唯一理由。
/// 编辑器、检出、复制都会动 mtime，全信它等于每次都在全量重解析。
#[test]
fn 只改了修改时间的不重解析() {
    let root = make_project("touch");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("第一遍失败");
    let f = root.join("src/lib.rs");
    let same = std::fs::read_to_string(&f).expect("读失败");
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&f, same).expect("写失败"); // 内容一样，只更新了 mtime
    let r = index::index_rust(&root, &mut s).expect("第二遍失败");
    assert_eq!(r.files, 0, "内容没变，不该重解析");
    assert_eq!(r.skipped_same_content, 1, "二级检测该拦住它");
}

/// 磁盘上没有了 → 图谱里也要没有了（节点连同它的边）。
#[test]
fn 删掉的文件从图谱里撤掉() {
    let root = make_project("remove");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("第一遍失败");
    std::fs::remove_file(root.join("src/bad.rs")).expect("删失败");
    let r = index::index_rust(&root, &mut s).expect("第二遍失败");
    assert_eq!(r.removed, 1);
    assert!(s.node("file:src/bad.rs").expect("读失败").is_none());
    assert!(
        s.node("symbol:src/bad.rs#broken").expect("读失败").is_none(),
        "它的符号也该跟着没"
    );
    assert!(
        s.node("file:src/lib.rs").expect("读失败").is_some(),
        "别的文件不受影响"
    );
}

/// **符号改名之后，旧符号不许留痕。**
///
/// 这是"图谱反映当前状态"最容易被破坏的一处：UPSERT 只会**新增和复活**，
/// 不会清理"这次没再出现的"。少了那一步清理，改过名的项目会在图谱里
/// 越攒越多已经不存在的符号——而界面上会如实把它们列出来。
#[test]
fn 改名之后旧符号不留痕() {
    let root = make_project("rename");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("第一遍失败");
    assert!(s.node("symbol:src/lib.rs#a").expect("读失败").is_some());

    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn b() {}\nstruct S;\nimpl S { fn m(&self) {} }\n",
    )
    .expect("写失败");
    let r = index::index_rust(&root, &mut s).expect("第二遍失败");
    assert_eq!(r.files, 1, "只有改了的那个文件被重解析");
    assert!(s.node("symbol:src/lib.rs#b").expect("读失败").is_some(), "新符号在");
    assert!(
        s.node("symbol:src/lib.rs#a").expect("读失败").is_none(),
        "旧符号必须撤掉——否则图谱会攒一堆已经不存在的东西"
    );
}

// ── 一条龙：跑过之后要留下痕迹（M5）──────────────────────────────

/// 跑一次之后，图谱里该能回答"这次跑出了哪些问题、判在哪个文件上"。
#[test]
fn 一次运行在图谱里留下痕迹() {
    let root = make_project("run");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("索引失败");

    let r = index::record_run(
        &mut s,
        &root,
        "cargo test",
        101,
        &[("E0308 类型不匹配".to_string(), Some("src/lib.rs".to_string()))],
        5000,
    )
    .expect("记运行失败");

    assert_eq!(r.diagnostics, 1);
    assert_eq!(r.unknown_targets, 0);
    assert!(s.node(&r.run_id).expect("读失败").is_some(), "Run 节点该在");

    // 运行 → 诊断：Q11 补的那条边，图谱靠它回答"上次跑失败是哪些问题"
    let diags = s
        .neighbors(&r.run_id, EdgeKind::Produced, Dir::Out)
        .expect("查失败");
    assert_eq!(diags.len(), 1, "该有一条 produced 边");

    // 诊断 → 文件：判在哪个文件上
    let files = s
        .neighbors(&diags[0], EdgeKind::DiagnosedAt, Dir::Out)
        .expect("查失败");
    assert_eq!(
        files,
        vec!["file:src/lib.rs".to_string()],
        "诊断该挂到那个文件上"
    );
}

/// 诊断指向一个**没索引过**的文件时：挂不上，但要**如实说**，不许造假节点。
#[test]
fn 指向未索引文件的诊断会被如实报出来() {
    let root = make_project("unknown_target");
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("索引失败");

    let r = index::record_run(
        &mut s,
        &root,
        "cargo test",
        101,
        &[
            ("有文件的诊断".to_string(), Some("src/lib.rs".to_string())),
            ("没索引过的诊断".to_string(), Some("src/never_seen.rs".to_string())),
        ],
        6000,
    )
    .expect("记运行失败");

    assert_eq!(r.diagnostics, 2);
    assert_eq!(r.unknown_targets, 1, "挂不上的那条要算出来");
    assert!(
        s.node("file:src/never_seen.rs").expect("读失败").is_none(),
        "不许为了挂上它而凭空造一个文件节点"
    );
}

// ── 解析期最要紧的两条路：局部绑定、跨文件 use ─────────────────────
//
// 这两条在单元测试里各有一段（parse 层抽得到、resolve 层连得上），
// 但**单元测试各自都对、接起来跑不通**这件事，这个项目已经吃过一次亏
// （见 `through_import` 那段注释：路径参照系不同，全等比对永远不命中）。
// 所以它们必须在集成层也有一条断言。

/// **闭包算符号，而且真的连成边**。
///
/// `let inner = |x| ...;` 之后那句 `inner(1)` 是个调用，它指向的东西
/// **本来在图谱里不存在**——于是解析期只能报"拿不准"。收成符号之后，
/// 这里该出现一条 `outer → outer::inner` 的 calls 边。
#[test]
fn 闭包被调用时连成边() {
    let root = make_tree(
        "closure",
        &[(
            "src/a.rs",
            "fn outer() {\n    let inner = |x: i64| x + 1;\n    inner(1);\n}\n",
        )],
    );
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("索引失败");

    assert!(
        s.node("symbol:src/a.rs#outer::inner").expect("读失败").is_some(),
        "闭包要作为一个符号进图谱，id 带它的作用域链"
    );
    assert_eq!(
        s.neighbors("symbol:src/a.rs#outer", EdgeKind::Calls, Dir::Out)
            .expect("查失败"),
        vec!["symbol:src/a.rs#outer::inner".to_string()],
        "调用它的就是那个闭包，不该拿不准也不该指向别处"
    );
}

/// **一句 `pub use` 就能定死一个名字的去向**，跨文件也一样。
///
/// `probe.rs` 里那句 `pub use crate::util::{which};` 等于它自己说清楚了
/// "本文件的 `which` 是 `util.rs` 里那个"——解析期不该拿不准。
///
/// 这条是实测逼出来的：拿不准的名单里一度只剩它，而它明明有 `use`。
/// 根因是 `pub use` 没被当成 use（`strip_prefix("use")` 对 `pub use ` 失败）。
#[test]
fn pub_use_把跨文件的调用连起来() {
    let root = make_tree(
        "pubuse",
        &[
            ("src/util.rs", "pub fn which() {}\n"),
            (
                "src/probe.rs",
                "pub use crate::util::{which};\n\nfn probe() {\n    which();\n}\n",
            ),
        ],
    );
    let mut s = open(&root);
    index::index_rust(&root, &mut s).expect("索引失败");

    assert_eq!(
        s.neighbors("symbol:src/probe.rs#probe", EdgeKind::Calls, Dir::Out)
            .expect("查失败"),
        vec!["symbol:src/util.rs#which".to_string()],
        "use 说得很清楚，跨文件也要连上"
    );
}
