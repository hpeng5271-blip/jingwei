//! **LSP 找引用**（2026-10-19，第二阶段第 3 件）。
//!
//! ⚠️ **边界（当前数据模型的实话）**：`edges` 表**没有存调用点的行号**，
//! 所以 references 返回的是"**哪个符号**引用了它、那个符号定义在哪一行"，
//! 不是"调用点那一行"。编辑器跳过去会落在函数定义上。
//!
//! 要精确到调用点，得在解析期把 `RefFact.line` 落到边上（schema 改动），
//! 那是下一步，不在这一条测试里假装已经做到。

#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::{index, lsp, store::Store};
use std::path::PathBuf;

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-lspref-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn helper() {}\n").unwrap();
    // **调用点在第 2 行**（不是 caller 的定义行第 1 行）——这样才分得出精确与退回
    std::fs::write(
        d.join("src/b.rs"),
        "pub fn caller() {\n    crate::a::helper();\n}\n",
    )
    .unwrap();
    d
}

/// 定义了 `helper` 的文件里问 references → 必须返回 `b.rs`（调用者所在文件）。
#[test]
fn 找得到谁引用了这个符号() {
    let root = fixture("basic");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let uri = lsp::path_to_uri(&root.join("src/a.rs"));
    let locs = lsp::references_at(&s, &root, &uri, 0, true);
    assert!(!locs.is_empty(), "helper 明明被 b.rs 调用了，不该是空的");

    let files: Vec<&str> = locs.iter().map(|(f, _)| f.as_str()).collect();
    assert!(
        files.iter().any(|f| f.contains("b.rs")),
        "必须包含调用者 b.rs，实际 {files:?}"
    );
    // **精确到调用点**：b.rs 的第 2 行（caller 定义在第 1 行）
    let hit = locs.iter().find(|(f, _)| f.contains("b.rs")).expect("应有 b.rs");
    assert_eq!(hit.1, 2, "要跳到**调用那一行**（第 2 行），不是定义行；实际 {:?}", locs);

    let _ = std::fs::remove_dir_all(&root);
}

/// **没被用到的符号 → 空表**（而且空表要能和"库过期"分开：
/// 这里文件刚索引过，所以空一定意味着"真的没人引用"）。
#[test]
fn 没人引用就是空表() {
    let root = fixture("unused");
    std::fs::write(root.join("src/c.rs"), "pub fn lonely() {}\n").unwrap();
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let uri = lsp::path_to_uri(&root.join("src/c.rs"));
    let locs = lsp::references_at(&s, &root, &uri, 0, false);
    assert!(locs.is_empty(), "lonely 没人用，应当空表，实际 {locs:?}");

    let _ = std::fs::remove_dir_all(&root);
}
