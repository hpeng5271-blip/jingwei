use jingwei_engine::graph::{index, query, store::Store};

fn fixture(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jw-explain-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn helper() {}\n").unwrap();
    std::fs::write(
        d.join("src/b.rs"),
        "pub fn caller() {\n    crate::a::helper();\n}\n",
    )
    .unwrap();
    d
}

/// \`explain_symbol\` 要说得出"定义在哪、被谁调用"。
#[test]
fn explain_给出定义与被调用点() {
    let root = fixture("ok");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");
    let ex = query::explain_symbol(&s, "symbol:src/a.rs#helper")
        .expect("查询失败")
        .expect("应有这个符号");
    assert_eq!(ex.file.as_deref(), Some("src/a.rs"));
    assert_eq!(ex.line, Some(1));
    assert!(ex.calls >= 1, "应至少有一条调用边，实际 {}", ex.calls);
    let hit = ex.callers.iter().any(|(f, l)| f == "src/b.rs" && *l == 2);
    assert!(hit, "调用点应在 src/b.rs:2，实际 {:?}", ex.callers);
    let _ = std::fs::remove_dir_all(&root);
}

/// 查不存在的符号 → \`None\`（不是空结构）。
#[test]
fn explain_不存在的符号返回_none() {
    let root = fixture("none");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");
    assert!(query::explain_symbol(&s, "symbol:src/a.rs#nope").unwrap().is_none());
    let _ = std::fs::remove_dir_all(&root);
}
