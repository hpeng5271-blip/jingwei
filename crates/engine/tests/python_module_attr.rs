//! **跨文件模块属性**（债 0.28 / D-20261020-04 A 路线）：`mod.CONST` 要变成 `references` 边。
//!
//! 判据不是"猜"：`from pkg import cfg` 写在文件里，`DEBUG = True` 写在 `cfg.py` 里，
//! 于是这条边**有来源**。做不到的情形（星号导入）必须落 unknown，不建边。

#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::{index, store::Store};
use std::path::PathBuf;

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-pymodattr-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("pkg")).expect("建目录失败");
    d
}

/// 正例：`from pkg import cfg` + `cfg.DEBUG` → references 边落在 `pkg/cfg.py#DEBUG`。
#[test]
fn 跨文件模块属性建_references_边() {
    let root = fixture("ok");
    std::fs::write(root.join("pkg/cfg.py"), "DEBUG = True\n\n\ndef helper():\n    return 1\n").unwrap();
    std::fs::write(root.join("pkg/__init__.py"), "").unwrap();
    std::fs::write(
        root.join("main.py"),
        "from pkg import cfg\n\n\ndef run():\n    return cfg.DEBUG\n",
    )
    .unwrap();

    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let v = s.dump(200_000).expect("导出失败");
    let edges = v["edges"].as_array().expect("edges");
    let want_from = "symbol:main.py#run";
    let want_to = "symbol:pkg/cfg.py#DEBUG";
    let hit = edges
        .iter()
        .any(|e| e["kind"] == "references" && e["src"] == want_from && e["dst"] == want_to);
    assert!(hit, "应有 {want_from} → {want_to} 的 references 边；实际 edges={edges:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// 反例：`from pkg.cfg import *` 之后 `DEBUG` 是裸名，**不许**被连成模块属性。
#[test]
fn 星号导入不许猜成模块属性() {
    let root = fixture("star");
    std::fs::write(root.join("pkg/cfg.py"), "DEBUG = True\n").unwrap();
    std::fs::write(root.join("pkg/__init__.py"), "").unwrap();
    std::fs::write(root.join("main.py"), "from pkg.cfg import *\n\n\ndef run():\n    return DEBUG\n")
        .unwrap();

    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let v = s.dump(200_000).expect("导出失败");
    let edges = v["edges"].as_array().expect("edges");
    let bad = edges
        .iter()
        .filter(|e| e["kind"] == "references" && e["dst"] == "symbol:pkg/cfg.py#DEBUG")
        .count();
    assert_eq!(bad, 0, "星号导入不该建出确定边（宁可 unknown）");
    let _ = std::fs::remove_dir_all(&root);
}

/// 反例：**小写模块级变量不是常量符号**，`cfg.debug` 不该连上 `debug = True`。
#[test]
fn 小写变量不收成常量() {
    let root = fixture("lower");
    std::fs::write(root.join("pkg/cfg.py"), "debug = True\n").unwrap();
    std::fs::write(root.join("pkg/__init__.py"), "").unwrap();
    std::fs::write(
        root.join("main.py"),
        "from pkg import cfg\n\n\ndef run():\n    return cfg.debug\n",
    )
    .unwrap();

    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");
    let v = s.dump(200_000).expect("导出失败");
    let edges = v["edges"].as_array().expect("edges");
    let bad = edges
        .iter()
        .filter(|e| e["kind"] == "references" && e["dst"] == "symbol:pkg/cfg.py#debug")
        .count();
    assert_eq!(bad, 0, "小写变量没进符号表，不该出现这条边");
    let _ = std::fs::remove_dir_all(&root);
}
