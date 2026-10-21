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

/// **负向**：类型看不出来时不连（`let x = make()` 里的 `make` 不是类型）。
#[test]
fn 类型看不出来时不乱连() {
    let root = fixture(
        "unknown",
        "pub struct S;\nimpl S {\n    pub fn touch(&self) {}\n}\nfn make() -> S { S }\npub fn run() {\n    let x = make();\n    x.touch();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::touch")
        .expect("查询失败")
        .expect("S::touch 应在图谱里");
    assert_eq!(
        e.calls, 0,
        "`let x = make()` 里 make 不是类型，不该连，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}
