#![allow(clippy::let_underscore_must_use)]

//! **`x.m()` 里 x 的类型字面可见时**，把方法调用连到 `T::m`（2026-10-21）。
//!
//! 只认三种"字面写着"的类型：`let x: T = …`、`let x = T::new(…)` / `let x = T { … }`、形参 `x: T`。
//! 看不懂的一律退回"引用"（会被统计成没连上），**绝不猜**。

use jingwei_engine::graph::{index, query, store::Store};

fn fixture(tag: &str, body: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jw-method-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    std::fs::write(d.join("src/a.rs"), body).unwrap();
    d
}

#[test]
fn let_推出来的类型能连上方法() {
    let root = fixture(
        "let",
        "pub struct S;\nimpl S {\n    pub fn open() -> S { S }\n    pub fn save(&self) {}\n}\npub fn use_it() {\n    let s = S::open();\n    s.save();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::save")
        .expect("查询失败")
        .expect("S::save 应在图谱里");
    assert_eq!(e.calls, 1, "应连上 1 个调用者，实际 {:?}", e.callers);
    assert!(
        e.callers.iter().all(|(_, _, s)| s.contains("use_it")),
        "调用者应是 use_it，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn 形参标注的类型也能连上方法() {
    let root = fixture(
        "param",
        "pub struct S;\nimpl S {\n    pub fn touch(&self) {}\n}\npub fn run(s: &S) {\n    s.touch();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::touch")
        .expect("查询失败")
        .expect("S::touch 应在图谱里");
    assert_eq!(e.calls, 1, "形参类型应让它连上，实际 {:?}", e.callers);
    let _ = std::fs::remove_dir_all(&root);
}

/// **负向**：类型看不出来时不连。
///
/// 注：`let x = make()` 现在**能**推出来了（2026-10-21 加了返回值推导，见 `从函数返回值推出类型`）；
/// 这里换成"函数不在本文件里、也没有标注"的形状——那是真的查不了，宁可少一条边。
#[test]
fn 类型看不出来时不乱连() {
    let root = fixture(
        "unknown",
        "pub struct S;\nimpl S {\n    pub fn touch(&self) {}\n}\npub fn run() {\n    let x = unknown_factory();\n    x.touch();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::touch")
        .expect("查询失败")
        .expect("S::touch 应在图谱里");
    assert_eq!(
        e.calls, 0,
        "`unknown_factory()` 的类型查不了，不该连，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_字段的方法调用能连上() {
    let root = fixture(
        "field",
        "pub struct Inner;\nimpl Inner {\n    pub fn touch(&self) {}\n}\npub struct Outer {\n    inner: Inner,\n}\nimpl Outer {\n    pub fn go(&self) {\n        self.inner.touch();\n    }\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#Inner::touch")
        .expect("查询失败")
        .expect("Inner::touch 应在图谱里");
    assert_eq!(e.calls, 1, "self.inner.touch() 应连上，实际 {:?}", e.callers);
    assert!(
        e.callers.iter().all(|(_, _, s)| s.contains("Outer::go")),
        "调用者应是 Outer::go，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **负向**：字段类型没在本文件 `struct` 里写着 → 不连。
#[test]
fn 字段类型看不出来时不乱连() {
    let root = fixture(
        "field-unknown",
        "pub struct Inner;\nimpl Inner {\n    pub fn touch(&self) {}\n}\npub struct Outer {\n    inner: Inner,\n}\nimpl Outer {\n    pub fn go(&self, other: &Outer2) {\n        other.inner.touch();\n    }\n}\npub struct Outer2 { inner: Inner }\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#Inner::touch")
        .expect("查询失败")
        .expect("Inner::touch 应在图谱里");
    assert_eq!(
        e.calls, 0,
        "`other.inner.touch()` 里 other 是形参，字段类型没在本文件 struct 里写着，不该连，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `let s = Store::open(p).ok()?` 这种**带 `?`/`unwrap` 包装**的也要认出类型。
#[test]
fn 带问号或_unwrap_的初始化也能认出类型() {
    let root = fixture(
        "wrapped",
        "pub struct S;\nimpl S {\n    pub fn open() -> Option<S> { Some(S) }\n    pub fn save(&self) {}\n}\npub fn use_it() -> Option<()> {\n    let s = S::open()?;\n    s.save();\n    let t = S::open().unwrap();\n    t.save();\n    Some(())\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::save")
        .expect("查询失败")
        .expect("S::save 应在图谱里");
    assert!(
        e.calls >= 1,
        "`S::open()?` / `.unwrap()` 之后的方法调用应连上，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **跨文件**方法调用：类型在本文件可见、方法定义在另一个文件（2026-10-21）。
#[test]
fn 跨文件的方法调用也能连上() {
    let d = std::env::temp_dir().join(format!("jw-method-x-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    std::fs::write(
        d.join("src/a.rs"),
        "pub struct S;\nimpl S {\n    pub fn new() -> S { S }\n    pub fn save(&self) {}\n}\n",
    )
    .unwrap();
    std::fs::write(
        d.join("src/b.rs"),
        "use crate::a::S;\npub fn run() {\n    let s = S::new();\n    s.save();\n}\n",
    )
    .unwrap();
    let mut st = Store::open(&d.join("g.db")).expect("开库失败");
    index::index_source(&d, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::save")
        .expect("查询失败")
        .expect("S::save 应在图谱里");
    assert_eq!(
        e.calls, 1,
        "跨文件的 `s.save()` 应连到 src/a.rs#S::save，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **嵌套调用里的方法调用也要被看见**（2026-10-21 实测抓到的真缺口）。
///
/// `store.save().ok()` / `foo(store.save())`：外层 `call_expression` 先被处理，
/// 而匹配臂之间不会"落下继续"，于是内层调用**整条丢**。
#[test]
fn 被_ok_包起来的方法调用也能连上() {
    let root = fixture(
        "nested",
        "pub struct S;\nimpl S {\n    pub fn new() -> S { S }\n    pub fn save(&self) -> Option<()> { Some(()) }\n}\npub fn use_it() {\n    let s = S::new();\n    s.save().ok();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::save")
        .expect("查询失败")
        .expect("S::save 应在图谱里");
    assert_eq!(
        e.calls, 1,
        "`s.save().ok()` 里内层调用应被看见，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **返回值推导**：`let x = make(); x.m()`——类型来自本文件那个函数的返回类型（2026-10-21）。
#[test]
fn 从函数返回值推出类型() {
    let root = fixture(
        "ret",
        "pub struct S;\nimpl S {\n    pub fn save(&self) {}\n}\nfn make() -> S { S }\npub fn use_it() {\n    let s = make();\n    s.save();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::save")
        .expect("查询失败")
        .expect("S::save 应在图谱里");
    assert_eq!(e.calls, 1, "`let s = make();` 应推出 S，实际 {:?}", e.callers);
    let _ = std::fs::remove_dir_all(&root);
}

/// **负向**：返回类型是泛型（`-> Option<S>`）时不猜。
#[test]
fn 泛型返回类型不猜() {
    let root = fixture(
        "ret-generic",
        "pub struct S;\nimpl S {\n    pub fn save(&self) {}\n}\nfn make() -> Option<S> { Some(S) }\npub fn use_it() {\n    let s = make();\n    s.save();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::save")
        .expect("查询失败")
        .expect("S::save 应在图谱里");
    assert_eq!(
        e.calls, 0,
        "`-> Option<S>` 没拆包装，不该硬推成 S，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

