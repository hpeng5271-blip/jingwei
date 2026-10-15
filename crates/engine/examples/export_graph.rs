//! 把某个目录的图谱导出成 JSON——**给渲染栈验证用的真实数据**。
//!
//! 用法：
//! ```text
//! cargo run -p jingwei-engine --example export_graph -- <目录> [输出.json] [上限]
//! ```
//!
//! 为什么要有这个 example：验证"canvas 能不能画几千个点"这件事，
//! **不能拿假数据**——真数据的形状（长 id、大量 `contains`/`defines` 边、
//! 少数节点连接度高）决定了布局和渲染的真实难度。

use jingwei_engine::{graph::index::index_rust, graph::store::Store};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| ".".to_string());
    let out = args.next().unwrap_or_else(|| "graph.json".to_string());
    let limit: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);

    let root_path = std::path::Path::new(&root);
    if !root_path.is_dir() {
        eprintln!("目录不存在：{root}");
        std::process::exit(1);
    }

    // 库放在临时目录：这是**一次性导出**，不该在项目里留下东西。
    let db = std::env::temp_dir().join(format!("jw-export-{}.db", std::process::id()));
    for p in [&db, &db.with_extension("db-wal"), &db.with_extension("db-shm")] {
        // 临时文件删不掉不影响导出结果——显式忽略。
        #[allow(clippy::let_underscore_must_use)]
        let _ = std::fs::remove_file(p);
    }

    let mut store = match Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("建库失败：{e}");
            std::process::exit(1);
        }
    };
    let t0 = std::time::Instant::now();
    let rep = match index_rust(root_path, &mut store) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("索引失败：{e}");
            std::process::exit(1);
        }
    };
    let index_ms = t0.elapsed().as_millis();

    let dump = match store.dump(limit) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("导出失败：{e}");
            std::process::exit(1);
        }
    };
    let text = serde_json::to_string(&dump).unwrap_or_else(|_| "{}".into());
    if let Err(e) = std::fs::write(&out, text.as_bytes()) {
        eprintln!("写文件失败：{e}");
        std::process::exit(1);
    }
    #[allow(clippy::let_underscore_must_use)]
    let _ = std::fs::remove_file(&db);

    // 报告里给的是**索引器自己说的话**，不是脚本猜的。
    println!(
        "索引 {} 个文件 / {} 个符号（{} 个解析不完整）· 耗时 {}ms",
        rep.files, rep.symbols, rep.incomplete, index_ms
    );
    println!(
        "图谱：{} 个节点 / {} 条边（导出上限 {}，截断={}）",
        dump["total_nodes"], dump["total_edges"], limit, dump["truncated"]
    );
    // **解析期的三个数字分开报**——它们的说法完全不同：
    // 连上了多少 / 有多少拿不准（不猜）/ 有多少是外部的（不归我们管）。
    // 只报一个"关系总数"的话，看的人分不清"图很全"和"图漏了一半"。
    println!(
        "解析：连上 {} 条 · 拿不准 {} 条 · 外部符号 {} 个",
        rep.resolved,
        rep.ambiguous.len(),
        rep.external
    );
    // **拿不准的那几个名字要打出来**——不然"46 条"这句话没法查。
    // （这条是这一轮的教训：计数解释不了任何事，事实才解释得了。）
    if !rep.ambiguous.is_empty() {
        let mut names = rep.ambiguous.clone();
        names.sort();
        names.dedup();
        let head: Vec<String> = names.iter().take(12).cloned().collect();
        println!("  拿不准的名字（去重后 {} 个）：{}", names.len(), head.join("、"));
    }
    println!("已写到 {out}（{} KB）", text.len() / 1024);
}
