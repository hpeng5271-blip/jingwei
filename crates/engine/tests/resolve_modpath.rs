#![allow(clippy::let_underscore_must_use)]

//! **模块路径调用 `mod::item(...)` 的解析**（2026-10-21）。
//!
//! 由来：跨项目验证时在 tauri 里发现 `lib.rs:283` 的 `info::command(options)?` 一条边都没有
//! （`command` 在这个 crate 里有多个同名，末段唯一命中规则拒绝连）；于是 `impact` 对整个
//! 模块只能报"查不了"。这里钉住窄修法：`a::b` 里 `a` 是小写模块名时，先落到同目录的
//! `a.rs` / `a/mod.rs`，再看那文件里有没有 `b`。

use jingwei_engine::graph::{index, query, store::Store};

fn fixture(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jw-modpath-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src/info")).unwrap();
    std::fs::create_dir_all(d.join("src/other")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    // 两个模块里都叫 command —— 末段唯一命中规则会拒绝，模块路径规则应该连上
    std::fs::write(d.join("src/info/mod.rs"), "pub fn command() {}\n").unwrap();
    std::fs::write(d.join("src/other/mod.rs"), "pub fn command() {}\n").unwrap();
    std::fs::write(
        d.join("src/lib.rs"),
        "mod info;\nmod other;\npub fn run_it() {\n    info::command();\n    other::command();\n}\n",
    )
    .unwrap();
    d
}

#[test]
fn 模块路径调用能连到那个文件里的同名函数() {
    let root = fixture("ok");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let a = query::explain_symbol(&s, "symbol:src/info/mod.rs#command")
        .expect("查询失败")
        .expect("info::command 应该在图谱里");
    let b = query::explain_symbol(&s, "symbol:src/other/mod.rs#command")
        .expect("查询失败")
        .expect("other::command 应该在图谱里");
    assert_eq!(a.calls, 1, "info::command 应有 1 个调用者，实际 {:?}", a.callers);
    assert_eq!(b.calls, 1, "other::command 应有 1 个调用者，实际 {:?}", b.callers);
    assert!(
        a.callers.iter().all(|(_, _, s)| s.contains("run_it")),
        "调用者应是 run_it，实际 {:?}",
        a.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `crate::a::b` 现在也能连（2026-10-21 第二步：带前缀的路径）。
#[test]
fn crate_前缀的路径也能连到模块文件() {
    let root = fixture("prefix");
    std::fs::write(
        root.join("src/lib.rs"),
        "mod info;\nmod other;\npub fn run_it() {\n    crate::info::command();\n}\n",
    )
    .unwrap();
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");
    let a = query::explain_symbol(&s, "symbol:src/info/mod.rs#command")
        .expect("查询失败")
        .expect("符号应在图谱里");
    assert_eq!(
        a.calls, 1,
        "crate::info::command 应连到 src/info/mod.rs#command，实际 {:?}",
        a.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **负向**：`crate::a::b` 的同后缀候选有多个时，必须放弃（宁少勿错）。
#[test]
fn crate_前缀遇到多个同后缀候选时不连() {
    let d = std::env::temp_dir().join(format!("jw-modpath-amb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src/x/util")).unwrap();
    std::fs::create_dir_all(d.join("src/y/util")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    // 两个 util.rs 里都有 do_it —— 后缀相同、全图两个候选
    std::fs::write(d.join("src/x/util.rs"), "pub fn do_it() {}\n").unwrap();
    std::fs::write(d.join("src/y/util.rs"), "pub fn do_it() {}\n").unwrap();
    std::fs::write(
        d.join("src/lib.rs"),
        "mod x;\nmod y;\npub fn main_() {\n    crate::x::util::do_it();\n}\n",
    )
    .unwrap();
    let mut s = Store::open(&d.join("g.db")).expect("开库失败");
    index::index_source(&d, &mut s).expect("索引失败");
    for f in ["src/x/util.rs", "src/y/util.rs"] {
        let e = query::explain_symbol(&s, &format!("symbol:{f}#do_it"))
            .expect("查询失败")
            .expect("符号应在图谱里");
        assert_eq!(e.calls, 0, "两个候选时不许乱连，实际 {:?}", e.callers);
    }
    let _ = std::fs::remove_dir_all(&d);
}
