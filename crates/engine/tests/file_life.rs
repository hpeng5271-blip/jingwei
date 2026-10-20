//! 文件生命周期状态机的**端到端**验证（2026-10-20）。
//!
//! 单测只证明转移表自洽；这里证明它真的接到了索引上：
//! 新建 → 再扫 → 改 → 删 → 又出现，五步的 `file_life` 都要对。
use jingwei_engine::graph::file_life::FileLife;
use jingwei_engine::graph::{index, store::Store};

fn project(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jw-life-{tag}-{}", std::process::id()));
    // 上次跑挂掉可能留下同名目录；删不掉不影响本次（临时目录）
    _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
    d
}

#[test]
fn 文件的五个阶段都落在_file_life_上() {
    let root = project("ok");
    let src = root.join("src/a.rs");
    std::fs::write(&src, "pub fn a() {}\n").unwrap();
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");

    // ① 新建 → modified
    index::index_source(&root, &mut s).expect("首次索引失败");
    assert_eq!(s.file_life("src/a.rs").unwrap(), Some(FileLife::Modified));

    // ② 没动 → unchanged
    index::index_source(&root, &mut s).expect("二次索引失败");
    assert_eq!(s.file_life("src/a.rs").unwrap(), Some(FileLife::Unchanged));

    // ③ 改了 → modified
    std::fs::write(&src, "pub fn a() {}\npub fn b() {}\n").unwrap();
    index::index_source(&root, &mut s).expect("改后索引失败");
    assert_eq!(s.file_life("src/a.rs").unwrap(), Some(FileLife::Modified));

    // ④ 删掉 → deleted（墓碑）
    std::fs::remove_file(&src).unwrap();
    index::index_source(&root, &mut s).expect("删后索引失败");
    assert_eq!(s.file_life("src/a.rs").unwrap(), Some(FileLife::Deleted));

    // ⑤ 又出现 → modified（删除前的 sha 已清空，按"变了"处理）
    std::fs::write(&src, "pub fn a() {}\n").unwrap();
    index::index_source(&root, &mut s).expect("重现后索引失败");
    assert_eq!(s.file_life("src/a.rs").unwrap(), Some(FileLife::Modified));

    _ = std::fs::remove_dir_all(&root);
}
