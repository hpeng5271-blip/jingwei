//! **全量 ≡ 增量**：缓存/复用这条线唯一的金标准检验（2026-10-19）。
//!
//! 为什么选它（学习调研的结论）：抽取错了会立刻显形，**缓存错了表现为"看着很对的旧数据"**，
//! 用户永远发现不了。所以把唯一的检验预算押在"全量重算 vs 增量重算逐字节相等"上。
//!
//! 它一条覆盖四类失效：
//!   ① 失效判定（该重算的没重算 / 不该重算的重算了）；
//!   ② 缓存键（内容哈希 / 解析器版本）；
//!   ③ 合并顺序（增量路径与全量路径写库顺序不同）；
//!   ④ 非确定性（HashMap 迭代顺序泄漏进输出）。
//!
//! 三条用例：
//!   A. 同一份内容连跑两次 → 图完全相同（幂等）；
//!   B. **只碰 mtime、内容不变** → 必须仍然命中（专抓"拿 mtime 当真相"）；
//!   C. 改 1 个文件 1 处 → 增量结果必须等于**另起一个干净库**全量重算的结果。

#![allow(clippy::let_underscore_must_use)]

use jingwei_engine::{index, store::Store};
use std::path::{Path, PathBuf};

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-eq-{tag}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).expect("清临时目录失败");
    }
    std::fs::create_dir_all(d.join("src")).expect("建目录失败");
    std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n")
        .expect("写文件失败");
    std::fs::write(d.join("src/a.rs"), "pub fn a() { helper() }\npub fn helper() {}\n")
        .expect("写文件失败");
    std::fs::write(d.join("src/b.rs"), "pub fn b() { crate::a::a() }\n").expect("写文件失败");
    d
}

fn open(dir: &Path, db: &str) -> Store {
    let p = dir.join(db).join("graph.db");
    Store::open(&p).expect("开库失败")
}

/// 图的可比表示：**节点与边都排序后**再比，避免拿存储顺序当等价。
fn snapshot(s: &Store) -> String {
    let v = s.dump(100_000).expect("导出失败");
    let mut nodes: Vec<String> = v
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|a| a.iter().map(|x| x.to_string()).collect())
        .unwrap_or_default();
    let mut edges: Vec<String> = v
        .get("edges")
        .and_then(|n| n.as_array())
        .map(|a| a.iter().map(|x| x.to_string()).collect())
        .unwrap_or_default();
    nodes.sort();
    edges.sort();
    format!("NODES\n{}\nEDGES\n{}\n", nodes.join("\n"), edges.join("\n"))
}

/// A + B：幂等，以及"碰 mtime 不算改"。
#[test]
fn 内容没变时增量结果与第一次完全一致() {
    let root = fixture("idem");
    let mut s = open(&root, ".jingwei");

    index::index_source(&root, &mut s).expect("第一次索引失败");
    let first = snapshot(&s);

    // A：再跑一次（什么都没动）
    let r2 = index::index_source(&root, &mut s).expect("第二次索引失败");
    assert_eq!(r2.files, 0, "一个文件都没变，不该处理任何文件（实际 {}）", r2.files);
    assert_eq!(snapshot(&s), first, "幂等：图必须逐字节相同");

    // B：只碰 mtime（内容一个字节没改）
    let f = root.join("src/a.rs");
    let content = std::fs::read(&f).expect("读失败");
    std::fs::write(&f, &content).expect("重写失败"); // 写回同样内容 → mtime 变
    let r3 = index::index_source(&root, &mut s).expect("第三次索引失败");
    assert_eq!(r3.files, 0, "只碰 mtime 不该重解析（mtime 不是真相，内容哈希才是）");
    assert_eq!(snapshot(&s), first, "碰 mtime 之后图也必须逐字节相同");

    let _ = std::fs::remove_dir_all(&root);
}

/// C：改一个文件 → **增量结果 == 干净库全量结果**（金标准）。
#[test]
fn 增量结果必须等于全量重算() {
    let root = fixture("gold");

    // 增量这一路：建库、索引、改一个文件、再索引
    let mut inc = open(&root, ".jingwei");
    index::index_source(&root, &mut inc).expect("首次索引失败");
    std::fs::write(root.join("src/b.rs"), "pub fn b() { crate::a::helper() }\n").expect("写失败");
    let r = index::index_source(&root, &mut inc).expect("增量索引失败");
    assert_eq!(r.files, 1, "只该重解析 b.rs（实际 {}）", r.files);
    let incremental = snapshot(&inc);

    // 全量这一路：另起一个干净库，从零索引同一份内容
    let mut clean = open(&root, ".clean");
    index::index_source(&root, &mut clean).expect("全量索引失败");
    let full = snapshot(&clean);

    if incremental != full {
        // **把差异点打出来**（不是只报"不等"）：前 8 行不同就够定位方向。
        let a: Vec<&str> = incremental.lines().collect();
        let b: Vec<&str> = full.lines().collect();
        let mut shown = 0;
        for i in 0..a.len().max(b.len()) {
            let x = a.get(i).copied().unwrap_or("<无>");
            let y = b.get(i).copied().unwrap_or("<无>");
            if x != y && shown < 8 {
                eprintln!("差异[{i}]\n  增量: {x}\n  全量: {y}");
                shown += 1;
            }
        }
        panic!("**增量 != 全量**（见上面的差异行）");
    }

    let _ = std::fs::remove_dir_all(&root);
}
