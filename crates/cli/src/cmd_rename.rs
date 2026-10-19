//! `jingwei rename`：**重命名的预演**（2026-10-19，债表 0.25）。
//!
//! 默认**只打印**（dry-run）：把每一处要改的 `file:line:col` 与"判不准所以不改"的位置都列出来。
//! 加 `--write` 才真的写文件——重命名是改用户代码，**默认不动手**。
//!
//! 判据：一处位置若一行里有多个同名标识符、或那一行根本找不到名字，就**不猜**，
//! 进"不敢改"清单并让退出码非零（有没改完的地方，不许假报成功）。

use std::collections::BTreeMap;
use std::path::Path;

pub fn cmd_rename(root: &Path, name: &str, to: &str, write: bool, json: bool) -> i32 {
    let db = root.join(".jingwei").join("graph.db");
    let store = match jingwei_engine::graph::store::Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("打不开图谱（先跑一次 jingwei index）：{e}");
            return 2;
        }
    };

    // 名字 → 符号 id。**同名多义就停下报出来**，不替用户挑。
    let ids = match store.symbol_ids_by_name(name) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("查符号失败：{e}");
            return 2;
        }
    };
    if ids.is_empty() {
        eprintln!("图谱里没有叫 `{name}` 的符号（先 index，或名字拼错了）");
        return 2;
    }
    if ids.len() > 1 {
        eprintln!("有 {} 个符号叫 `{name}`，用 id 指定：", ids.len());
        for id in &ids {
            eprintln!("  {id}");
        }
        return 2;
    }

    let plan = match store.plan_rename(root, &ids[0], to) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("规划失败：{e}");
            return 2;
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&plan).unwrap_or_default());
    } else {
        println!("符号：{}  →  {to}", ids[0]);
        println!("能改：{} 处（{} 个文件）", plan.edits.len(), plan.files().len());
        for e in &plan.edits {
            println!("  {}:{}:{}  {} → {}", e.file, e.line, e.col, e.old, e.new);
        }
        if !plan.ambiguous.is_empty() {
            println!("不敢改：{} 处（判不准，没动）", plan.ambiguous.len());
            for a in &plan.ambiguous {
                println!("  {}:{}  {}", a.file, a.line, a.reason);
            }
        }
    }

    // **有判不准的位置就不许写**：半截重命名比不改更糟。
    if !plan.ambiguous.is_empty() {
        eprintln!("有 {} 处判不准，本次不写入", plan.ambiguous.len());
        return 1;
    }

    if !write {
        eprintln!("（预演，未写入；确认后加 --write）");
        return 0;
    }

    // 按文件分组改：同一文件只读一次、写一次
    let mut by_file: BTreeMap<String, Vec<(u32, u32, u32, String)>> = BTreeMap::new();
    for e in &plan.edits {
        by_file
            .entry(e.file.clone())
            .or_default()
            .push((e.line, e.col, e.byte, e.new.clone()));
    }
    for (file, mut edits) in by_file {
        let path = root.join(&file);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("读不了 {file}：{e}");
                return 2;
            }
        };
        // 列号来自解析器（**行内字节列**）；同一行内**从右往左**改，避免列号漂移。
        edits.sort_by_key(|e| std::cmp::Reverse(e.1));
        let mut by_col: Vec<(u32, u32, String)> = Vec::new();
        for (line, col, _byte, new) in &edits {
            by_col.push((*line, *col, new.clone()));
        }
        let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        by_col.sort_by_key(|e| std::cmp::Reverse(e.1));
        for (line, col, new) in by_col {
            let idx = (line.saturating_sub(1)) as usize;
            if let Some(l) = lines.get_mut(idx) {
                let start = (col.saturating_sub(1)) as usize;
                let bytes = l.as_bytes();
                let mut n = 0;
                while start + n < bytes.len()
                    && (bytes[start + n].is_ascii_alphanumeric() || bytes[start + n] == b'_')
                {
                    n += 1;
                }
                if n == 0 {
                    eprintln!("警告：{file}:{line}:{col} 不在词边界，跳过");
                    continue;
                }
                l.replace_range(start..start + n, &new);
            }
        }
        let out = lines.join("\n") + "\n";
        if let Err(e) = std::fs::write(&path, out) {
            eprintln!("写不回 {file}：{e}");
            return 2;
        }
        println!("已改：{file}");
    }
    if plan.ambiguous.is_empty() { 0 } else { 1 }
}
