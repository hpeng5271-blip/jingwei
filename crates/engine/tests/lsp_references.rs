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

/// **hover**：悬停要说清三件事——是什么、定义在哪、被几处调用。
#[test]
fn hover_说清是什么和在哪() {
    let root = fixture("hover");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let uri = lsp::path_to_uri(&root.join("src/a.rs"));
    let md = lsp::hover_at(&s, &root, &uri, 0).expect("helper 该有 hover");
    assert!(md.contains("helper"), "要写出名字：{md}");
    assert!(md.contains("定义："), "要写出定义位置：{md}");
    assert!(md.contains("调用"), "要说被调用情况：{md}");

    // **查不了就是查不了**：没索引过的文件不该假装知道
    let ghost = lsp::path_to_uri(&root.join("src/ghost.rs"));
    assert!(lsp::hover_at(&s, &root, &ghost, 0).is_none(), "没索引过的文件要回 None");

    let _ = std::fs::remove_dir_all(&root);
}

/// **诊断推送**：坏文件（上次没解析完）必须推一条 error；干净文件不推噪音。
#[test]
fn 坏文件会推一条错误诊断() {
    let root = fixture("diag");
    std::fs::write(root.join("src/broken.rs"), "fn broken( { }\n").unwrap();
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let bad = lsp::diagnostics_for(&s, &root, "src/broken.rs");
    assert!(!bad.is_empty(), "坏文件该有诊断");
    assert!(
        bad.iter().any(|d| d.get("severity").and_then(|v| v.as_u64()) == Some(1)),
        "至少一条是 error：{bad:?}"
    );

    let ok = lsp::diagnostics_for(&s, &root, "src/a.rs");
    assert!(ok.is_empty(), "干净文件不该有诊断，实际 {ok:?}");

    let _ = std::fs::remove_dir_all(&root);
}

/// **重命名**（债表 0.25）：定义 + 跨文件调用点都要出编辑；判不准时不发编辑。
#[test]
fn 重命名返回跨文件编辑() {
    let root = fixture("rename");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let uri = lsp::path_to_uri(&root.join("src/a.rs"));
    let msg = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "textDocument/rename",
        "params": {
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 11 },
            "newName": "renamed"
        }
    });
    let resp = lsp::handle(Some(&s), &root, &msg).expect("rename 要有响应");
    let changes = resp
        .get("result")
        .and_then(|r| r.get("changes"))
        .expect("应当返回 changes");
    let files = changes.as_object().expect("changes 是对象");
    assert!(
        files.len() >= 2,
        "定义文件与调用文件都该有编辑，实际 {files:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}