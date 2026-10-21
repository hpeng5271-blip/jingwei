#![allow(clippy::let_underscore_must_use)]

//! **跨文件返回值推导的守卫测试**（2026-10-21）。
//!
//! 上一轮带"全项目签名提示"重解析时，删库重建量到 **符号 2167 → 2148**（少 19）。
//! 那是回归，整块回滚。这里先钉住"最小两文件工程里它该连上"的判据，
//! 再谈重新打开——**没有这条测试，重开就是盲改**。

use jingwei_engine::graph::{index, query, store::Store};

fn fixture(tag: &str, a: &str, b: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jw-xfile-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(d.join("src/a.rs"), a).unwrap();
    std::fs::write(d.join("src/b.rs"), b).unwrap();
    d
}

/// **跨文件返回值推导**（2026-10-21 做成）：`make` 定义在 a.rs，b.rs 只 `use` 了它。
///
/// 机制：符号节点上存 `data.ret`，索引时把全项目的 `ret` 当提示，只对"有接收者看不出来的
/// 方法调用"的文件带提示重解析；**符号数一变就不采用**（守卫见 index 里的重解析代码）。
#[test]
fn 跨文件返回值能连上() {
    let root = fixture(
        "basic",
        "pub struct S;\nimpl S {\n    pub fn save(&self) {}\n}\npub fn make() -> S { S }\n",
        "use crate::a::make;\npub fn run() {\n    let x = make();\n    x.save();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let e = query::explain_symbol(&st, "symbol:src/a.rs#S::save")
        .expect("查询失败")
        .expect("S::save 应在图谱里");
    assert_eq!(
        e.calls, 1,
        "`let x = make()` 里 make 在 a.rs，类型应推出来并连上，实际 {:?}",
        e.callers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 同一工程里，**符号数必须稳定**：任何"推出更多边"的改动都不许改动符号总量。
#[test]
fn 重解析不许改变符号总数() {
    let root = fixture(
        "stable",
        "pub struct S;\nimpl S {\n    pub fn save(&self) {}\n}\npub fn make() -> S { S }\n",
        "use crate::a::make;\npub fn run() {\n    let x = make();\n    x.save();\n}\n",
    );
    let mut st = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut st).expect("索引失败");
    let before = st.symbol_count().unwrap_or(0);
    // 再索引一次（全量重解析路径）
    index::index_source(&root, &mut st).expect("第二次索引失败");
    let after = st.symbol_count().unwrap_or(0);
    assert_eq!(before, after, "重解析后符号数变了：{before} → {after}");
    let _ = std::fs::remove_dir_all(&root);
}
