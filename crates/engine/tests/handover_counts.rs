use jingwei_engine::graph::{index, query, store::Store};

fn fixture() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jw-handover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn helper() {}\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "pub fn caller() {\n    crate::a::helper();\n}\n").unwrap();
    d
}

/// 交接待办要的三个数：文件/符号/调用边，以及"被调用最多的文件"。
#[test]
fn handover_的底账能算出来() {
    let root = fixture();
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");
    let (files, symbols, calls, refs) = query::basic_counts(&s).expect("计数失败");
    assert!(files >= 2 && symbols >= 2 && calls >= 1, "{files}/{symbols}/{calls}");
    assert_eq!(refs, 0);
    let top = query::top_incoming_files(&s, 5).expect("热点失败");
    assert!(!top.is_empty());
    assert_eq!(top[0].0, "src/a.rs", "helper 在 a.rs，应排第一：{top:?}");
    let _ = std::fs::remove_dir_all(&root);
}
