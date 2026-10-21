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
    // **只读打开**（2026-10-20 修，与 positions 同一类）：`Store::open` 会建库，
    // 没索引过的项目敲一次 rename 就在人家目录里留一个空 `.jingwei/`。
    if !db.exists() {
        eprintln!("查不了：还没索引（先跑一次 jingwei index）——没有图谱就不知道该改哪几处");
        return 2;
    }
    let store = match jingwei_engine::graph::query::open_for_query(root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("打不开图谱（先跑一次 jingwei index）：{e}");
            return 2;
        }
    };

    // 名字 → 符号 id；也接受直接给 `symbol:...` 形式的 id。
    // **同名多义就停下报出来**，不替用户挑。
    let ids = if name.starts_with("symbol:") {
        vec![name.to_string()]
    } else {
        match store.symbol_ids_by_name(name) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("查符号失败：{e}");
                return 2;
            }
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
        // **沙箱边界**（2026-10-21）：写只许落在项目内、`.git` 不碰——图库里的路径也可能过期/脏
        let path = match crate::sandbox::ensure_within(root, &root.join(&file)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        };
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
        // 原子写：失败时原文件不动（不留半个文件）；回读仍然是最终裁判。
        if let Err(e) = crate::sandbox::write_atomic(&path, out.as_bytes()) {
            eprintln!("写不回 {file}：{e}");
            return 2;
        }
        println!("已改：{file}");
    }

    // **写完回读**：每一处都要真的以新名字开头——写入不等于改对（编码/换行都可能坑）。
    let mut bad = 0;
    for e in &plan.edits {
        let Ok(text) = std::fs::read_to_string(root.join(&e.file)) else {
            bad += 1;
            continue;
        };
        let Some(line) = text.lines().nth((e.line.saturating_sub(1)) as usize) else {
            bad += 1;
            continue;
        };
        let start = (e.col.saturating_sub(1)) as usize;
        if !line.get(start..).map(|s| s.starts_with(&e.new)).unwrap_or(false) {
            eprintln!("回读不符：{}:{} 期望以 `{}` 开头", e.file, e.line, e.new);
            bad += 1;
        }
    }
    if bad > 0 {
        eprintln!("有 {bad} 处回读不符——已写入但需要人工看一眼");
        return 1;
    }
    println!("回读通过：{} 处都已是 `{}`", plan.edits.len(), to);

    if plan.ambiguous.is_empty() { 0 } else { 1 }
}
