//! **Python 的"使用"进图谱**（2026-10-19，债表 0.17 的收尾）。
//!
//! 判据来自文件本身：`import flask` / `from flask import Flask` 就写在文件里，
//! 于是 `File → Dependency(flask)` 这条 `DependsOn` 边**不是猜的**。

#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::{index, store::Store};
use std::path::PathBuf;

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-pydeps-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建目录失败");
    std::fs::write(
        d.join("app.py"),
        "from flask import Flask\nimport requests\n\napp = Flask(__name__)\n",
    )
    .unwrap();
    d
}

/// **两个 import 都要变成依赖节点**，并且文件到它们有边。
#[test]
fn import_变成文件到依赖的边() {
    let root = fixture("edges");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let v = s.dump(100_000).expect("导出失败");
    let nodes = v.get("nodes").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let deps: Vec<String> = nodes
        .iter()
        .filter(|n| n.get("kind").and_then(|k| k.as_str()) == Some("dependency"))
        .map(|n| n.get("name").and_then(|k| k.as_str()).unwrap_or("").to_string())
        .collect();
    assert!(deps.contains(&"flask".to_string()), "flask 该成为依赖节点，实际 {deps:?}");
    assert!(
        deps.contains(&"requests".to_string()),
        "requests 该成为依赖节点，实际 {deps:?}"
    );

    let edges = v.get("edges").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let n_dep_edges = edges
        .iter()
        .filter(|e| e.get("kind").and_then(|k| k.as_str()) == Some("depends_on"))
        .count();
    assert!(n_dep_edges >= 2, "至少两条 depends_on 边，实际 {n_dep_edges}");

    let _ = std::fs::remove_dir_all(&root);
}
