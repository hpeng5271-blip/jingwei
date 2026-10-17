//! **Python 进图谱**的端到端验收（2026-10-16，第二门语言）。
//!
//! ## 为什么单开一个文件、而不是加在单元测试里
//!
//! 这一段要验的东西**跨了四个模块**：`index` 收文件 → `parse_python` 抽符号 →
//! `resolve` 落地跨文件调用 → `store` 写边。单元测试各盯一个函数，
//! 而这一类缺陷**恰恰落在两个函数之间的缝**上——这个仓库的账本上那条规律是：
//! **写新单元测试很少抓到缺陷，而"拿真东西去问"每一次都抓到。**
//!
//! 实际开发这一段时，缝里露出来的是这个：
//! `RefFact.scope` 我写成了短名（`main`）而不是完整 id（`symbol:main.py#main`），
//! 于是索引**直接失败**在「写关系失败：main -calls-> symbol:hello_python/core.py#greet」。
//! 那一条**单元测试看不见**（`parse_python` 单独跑是对的），只有走真库才露出来。
//!
//! ## 断言是结构性的
//!
//! 不钉具体条数——那种断言每次改代码就红，会被当成噪声绕过去
//! （`real_projects.rs` 的教训）。这里钉的是**形状**：
//! 文件进了、符号 id 拼对了、跨文件那条边的**两个端点**是完整 id。

// 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
// 而不是让 lint 在整个 crate 里静默失声（与 `model/src/lib.rs` 的测试模块同一条做法）。
#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::graph::store::{Dir, Store};
use jingwei_model::EdgeKind;
use jingwei_model::NodeKind;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    // 目录名带进程号 + tag：集成测试**在同一进程里并行跑**，
    // 只用 tag 的话两个测试会互相删对方的夹具（`real_projects.rs` 踩过这个）。
    let d = std::env::temp_dir().join(format!("jw-pygraph-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, body).unwrap();
}

/// 造一个**有跨文件调用**的 Python 包。
///
/// 形状刻意做得像真的：包目录 + `__init__.py` + 一个模块 + 一个入口，
/// 入口 `from <包> import <模块>` 之后调 `<模块>.<函数>()`——
/// 这正是"改了那个函数会影响谁"要回答的那种调用。
fn python_project(root: &Path) {
    write(root, "pkg/__init__.py", "");
    write(
        root,
        "pkg/core.py",
        "def greet(who):\n    return f'hi {who}'\n\n\nclass Person:\n    def hello(self):\n        return greet('x')\n",
    );
    write(
        root,
        "main.py",
        "from pkg import core\n\n\ndef main():\n    return core.greet('world')\n",
    );
}

/// 索引一个 Python 项目，返回报告。
fn index(root: &Path) -> (jingwei_engine::graph::index::IndexReport, PathBuf) {
    let db = jingwei_engine::graph_db_path(root);
    if let Some(d) = db.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut store = Store::open(&db).expect("建图谱库");
    let rep = jingwei_engine::graph::index::index_rust(root, &mut store).expect("索引该成功");
    (rep, db)
}

/// **`index_rust` 这个名字现在是骗人的**——它收 `.rs` 和 `.py` 两种。
/// 这条测试不改名，只把事实钉下来：函数名留着（改名要动 5 处调用点，
/// 那是另一件事），但**行为必须是两门语言都收**。
/// 名字与行为不符这件事本身记在 `AGENTS.md` §5 的账上。
#[test]
fn 索引器收python文件不只是rust() {
    let d = scratch("files");
    python_project(&d);
    let (rep, _db) = index(&d);
    assert_eq!(
        rep.files, 3,
        "`pkg/__init__.py` / `pkg/core.py` / `main.py` 三个都该收进来，实际 {}",
        rep.files
    );
    assert_eq!(rep.incomplete, 0, "这三段代码没有语法错");
}

/// **符号 id 的形状**：函数在文件下、方法在类下。
#[test]
fn python的函数与方法都成了符号() {
    let d = scratch("symbols");
    python_project(&d);
    let (_rep, db) = index(&d);
    let store = Store::open_readonly(&db).expect("只读开库");
    let names: Vec<String> = store
        .list_kind(NodeKind::Symbol, 100)
        .expect("列符号")
        .into_iter()
        .map(|n| n.id)
        .collect();
    for want in [
        "symbol:pkg/core.py#greet",
        "symbol:pkg/core.py#Person",
        "symbol:pkg/core.py#Person::hello",
        "symbol:main.py#main",
    ] {
        assert!(
            names.contains(&want.to_string()),
            "少了符号 `{want}`。实际拿到：{names:?}"
        );
    }
}

/// **跨文件调用真的连上了**——这是"改了这里会影响谁"能问出来的前提。
///
/// 断言钉的是**边的两个端点**，而且刻意要求端点是**完整 id**：
/// 短名（`main`）在图上会指向一个不存在的节点，而索引会因此失败。
#[test]
fn python的跨文件调用连成了边() {
    let d = scratch("calls");
    python_project(&d);
    let (_rep, db) = index(&d);
    let store = Store::open_readonly(&db).expect("只读开库");

    // 直接查边：主文件里调了 `core.greet()`，被调者在另一个文件里。
    let got = store
        .neighbors("symbol:main.py#main", EdgeKind::Calls, Dir::Out)
        .expect("查 main 的调用出边");
    assert!(
        got.contains(&"symbol:pkg/core.py#greet".to_string()),
        "`main.py` 里 `core.greet('world')` 该连到 `pkg/core.py#greet`。实际出边：{got:?}"
    );
}

/// **类里的方法调同类方法**（`self.hello()` 那种）。
///
/// 这一条与 Rust 的 `impl` 同义：接收者类型就是所在 class，不需要任何推断。
/// 少了它，Python 的类基本等于没有调用关系。
#[test]
fn python的self调用连到同类的方法() {
    let d = scratch("self");
    write(
        &d,
        "a.py",
        "class C:\n    def a(self):\n        return self.b()\n\n    def b(self):\n        return 1\n",
    );
    let (_rep, db) = index(&d);
    let store = Store::open_readonly(&db).expect("只读开库");
    let got = store
        .neighbors("symbol:a.py#C::a", EdgeKind::Calls, Dir::Out)
        .expect("查 C::a 的调用出边");
    assert!(
        got.contains(&"symbol:a.py#C::b".to_string()),
        "`self.b()` 该连到 `C::b`，实际：{got:?}"
    );
}

/// **两门语言在同一个项目里互不干扰**：一个 Python 文件 + 一个 Rust 文件，
/// 两个都要进图谱，而且各自按自己的语法解析。
///
/// 少这一条，改 `parse.rs` 时把 `.py` 也塞进 Rust 解析器（或反过来）
/// 不会有人发现——而症状是"符号抽出来了但全是错的"。
#[test]
fn 同一个项目里的rust与python各按自己的语法解析() {
    let d = scratch("mixed");
    write(&d, "a.rs", "pub fn from_rust() -> i32 { 1 }\n");
    write(&d, "b.py", "def from_python():\n    return 2\n");
    let (rep, db) = index(&d);
    assert_eq!(rep.files, 2, "两种后缀都该收");
    let store = Store::open_readonly(&db).expect("只读开库");
    let names: Vec<String> = store
        .list_kind(NodeKind::Symbol, 100)
        .expect("列符号")
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert!(
        names.contains(&"symbol:a.rs#from_rust".to_string()),
        "Rust 那个没抽出来：{names:?}"
    );
    assert!(
        names.contains(&"symbol:b.py#from_python".to_string()),
        "Python 那个没抽出来：{names:?}"
    );
}

/// **`.pyc` 之类不该被当成源码收进来**（那是二进制，解析出来全是垃圾）。
#[test]
fn 二进制与缓存文件不进图谱() {
    let d = scratch("junk");
    write(&d, "a.py", "def f():\n    return 1\n");
    write(&d, "a.pyc", "\u{0}\u{1}binary");
    write(&d, "notes.txt", "hello");
    let (rep, _db) = index(&d);
    assert_eq!(rep.files, 1, "只该收 `a.py`，实际 {} 个", rep.files);
}

