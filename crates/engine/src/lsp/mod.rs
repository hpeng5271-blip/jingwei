//! **最小 LSP**（2026-10-18 起步）：只做一件事——**从调用处跳到定义**。
//!
//! 为什么只做这一件：编辑器的 LSP 有几十种能力，但"经纬"的全部价值是
//! **把理解层接进编辑器**。定义跳转是那个"一用就知道有没有用"的能力，
//! 其余（补全、重命名、诊断推送）等它站住了再说。
//!
//! **它不解析代码**：位置来自**已经建好的图谱**（`nodes`/`edges`），
//! 所以我们和别的 LSP 有一处本质区别——**答案的质量取决于索引，不取决于这一次请求**。
//! 查不到时**必须回 `null`**（三态：不适用就是 null，不许瞎指一个位置）。
//!
//! 边界（写清，免得以后误会）：
//! - 只支持 **stdio**（`jingwei lsp` 由编辑器拉起，不监听端口）；
//! - 只支持 `textDocument/definition` 与握手；其余请求回"不支持"而不是装死；
//! - 位置匹配用**行**（`line`），`character` 不参与——图谱里只存了行。

use crate::graph::store::Store;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// 读一条 LSP 消息（`Content-Length` 分帧）。EOF 返回 `Ok(None)`。
pub fn read_message<R: BufRead>(r: &mut R) -> std::io::Result<Option<Value>> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            return Ok(None); // 管道关了
        }
        let t = line.trim_end_matches(['\r', '\n']);
        if t.is_empty() {
            break; // 头结束
        }
        if let Some(rest) = t.strip_prefix("Content-Length:") {
            len = rest.trim().parse::<usize>().ok();
        }
        // 其它头（Content-Type 等）忽略——LSP 允许
    }
    let len = match len {
        Some(l) => l,
        None => return Ok(None), // 没有长度就断开，别猜
    };
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(serde_json::from_slice(&buf)?))
}

/// 写一条消息（自动算长度）。**一次写完再 flush**，否则编辑器会卡在半个包上。
pub fn write_message<W: Write>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(v)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

/// `file:///d%3A/x/y.rs` → `D:\x\y.rs`（只认本地文件，不认 http）。
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let decoded = percent_decode(rest);
    // Windows 的 URI 是 /d:/x/y，去前导斜杠后正好是 d:/x/y
    Some(PathBuf::from(decoded))
}

/// 路径 → `file://` URI（正斜杠、盘符小写、空格等百分号编码）。
pub fn path_to_uri(p: &Path) -> String {
    let mut s = p.to_string_lossy().replace('\\', "/");
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    let mut out = String::from("file://");
    for ch in s.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            _ => out.push(ch),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 图谱里的路径是**相对根、正斜杠**；这里把 URI 换成同一种写法。
fn rel_of(root: &Path, uri: &str) -> Option<String> {
    let abs = uri_to_path(uri)?;
    let rel = abs.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// 在 `file_path` 里找**不超过 `line` 的最近一个符号**。
///
/// 为什么要"最近"而不是"精确等于"：编辑器把光标放在 `foo(` 上时，行号对得上，
/// 但光标也可能停在 `let x = foo(` 这种行——**同一个符号**。图谱只存了符号起始行。
pub fn symbol_near(store: &Store, file: &str, line: u32) -> Option<(String, String, u32)> {
    let conn = &store.conn;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, line FROM nodes \
             WHERE kind='symbol' AND file_path=?1 AND line IS NOT NULL AND line<=?2 \
             ORDER BY line DESC LIMIT 1",
        )
        .ok()?;
    let mut rows = stmt.query(rusqlite::params![file, i64::from(line)]).ok()?;
    let row = rows.next().ok()??;
    let id: String = row.get(0).ok()?;
    let name: String = row.get(1).ok()?;
    let ln: u32 = row.get::<_, i64>(2).ok()? as u32;
    Some((id, name, ln))
}

/// 从 `src` 出发，沿 `calls` 边取出**所有**有位置的候选（名字一起带出来）。
///
/// 为什么不是"第一条就跳"：同一个函数会调很多东西，**随便挑一条等于瞎指**。
/// 候选要靠"**这一行的文本里有没有这个名字**"来筛（见 `definition_at`）。
pub fn call_targets(store: &Store, src: &str) -> Vec<(String, String, u32)> {
    let conn = &store.conn;
    let mut out = Vec::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT n.name, n.file_path, n.line FROM edges e JOIN nodes n ON n.id = e.dst \
         WHERE e.src=?1 AND e.kind='calls' AND e.expires_at IS NULL \
           AND n.file_path IS NOT NULL AND n.line IS NOT NULL \
         ORDER BY n.file_path, n.line",
    ) else {
        return out;
    };
    let Ok(mut rows) = stmt.query(rusqlite::params![src]) else {
        return out;
    };
    while let Ok(Some(row)) = rows.next() {
        let name: String = row.get(0).unwrap_or_default();
        let file: String = row.get(1).unwrap_or_default();
        let line: i64 = row.get(2).unwrap_or(0);
        out.push((name, file, line as u32));
    }
    out
}

/// **谁用到这个符号**（2026-10-19）：沿 `calls` / `references` 边**反向**找。
///
/// 返回 `(文件, 行号, 名字)`；`include_decl` 时把定义自己也算进去（LSP 的 `includeDeclaration`）。
///
/// 三态：查不到就返回空表（**空 ≠ 没被使用**——图谱没索引到这里也是空，
/// 所以调用方要把"空"和"库过期"分开说，见 `up_to_date`）。
pub fn references_at(
    store: &Store,
    root: &Path,
    uri: &str,
    line0: u32,
    include_decl: bool,
) -> Vec<(String, u32)> {
    let Some(rel) = rel_of(root, uri) else {
        return Vec::new();
    };
    // 光标所在这一行最近的符号（含它自己）
    let Some((id, _name, _line)) = symbol_near(store, &rel, line0 + 1) else {
        return Vec::new();
    };
    let mut out: Vec<(String, u32)> = Vec::new();
    if include_decl {
        // 定义自己的位置（用来在编辑器里一起高亮）
        if let Ok(mut st) = store.conn.prepare(
            "SELECT file_path, line FROM nodes WHERE id=?1 AND file_path IS NOT NULL AND line IS NOT NULL",
        ) {
            if let Ok(mut rows) = st.query(rusqlite::params![id]) {
                if let Ok(Some(r)) = rows.next() {
                    let f: String = r.get(0).unwrap_or_default();
                    let l: i64 = r.get(1).unwrap_or(0);
                    out.push((f, l as u32));
                }
            }
        }
    }
    // **先看调用点表**（2026-10-19）：这里存的才是"调用发生在哪一行"。
    // 老库/没重索引时这张表可能是空的 → 退回"被引用符号的定义行"（不如精确，但不是假的）。
    let mut precise = 0usize;
    if let Ok(mut st) = store.conn.prepare(
        "SELECT s.file, s.line FROM edge_sites s \
         JOIN edges e ON e.src=s.src AND e.dst=s.dst AND e.kind=s.kind \
         WHERE s.dst=?1 AND s.kind IN ('calls','references') AND e.expires_at IS NULL \
         ORDER BY s.file, s.line",
    ) {
        if let Ok(mut rows) = st.query(rusqlite::params![id]) {
            while let Ok(Some(r)) = rows.next() {
                let f: String = r.get(0).unwrap_or_default();
                let l: i64 = r.get(1).unwrap_or(0);
                out.push((f, l as u32));
                precise += 1;
            }
        }
    }
    // **退回**：没有调用点记录时，至少给出"哪个符号引用了它"（定义行）
    if precise == 0 {
        if let Ok(mut st) = store.conn.prepare(
            "SELECT n.file_path, n.line FROM edges e JOIN nodes n ON n.id = e.src \
             WHERE e.dst=?1 AND e.kind IN ('calls','references') AND e.expires_at IS NULL \
               AND n.file_path IS NOT NULL AND n.line IS NOT NULL \
             ORDER BY n.file_path, n.line",
        ) {
            if let Ok(mut rows) = st.query(rusqlite::params![id]) {
                while let Ok(Some(r)) = rows.next() {
                    let f: String = r.get(0).unwrap_or_default();
                    let l: i64 = r.get(1).unwrap_or(0);
                    out.push((f, l as u32));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 图谱里这个文件的 `size/mtime_ns` 与磁盘**一致**吗？
///
/// 为什么要问：**LSP 的位置来自索引，不是来自这一次打开的文件**。
/// 文件改过而没重新索引时，行号会漂——那时唯一诚实的答案是 `null`，
/// 否则就是"看起来跳到了、其实指的是旧代码"（比不跳更坏）。
pub fn up_to_date(store: &Store, root: &Path, file: &str) -> bool {
    let conn = &store.conn;
    let Ok(mut stmt) = conn.prepare("SELECT size, mtime_ns FROM files WHERE path=?1") else {
        return false;
    };
    let Ok(mut rows) = stmt.query(rusqlite::params![file]) else {
        return false;
    };
    let Ok(Some(row)) = rows.next() else {
        return false;
    };
    let (size, mtime): (i64, i64) = match (row.get(0), row.get(1)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return false,
    };
    let abs = root.join(file.replace('/', std::path::MAIN_SEPARATOR_STR));
    let Ok(md) = std::fs::metadata(&abs) else {
        return false;
    };
    if md.len() as i64 != size {
        return false;
    }
    matches!(md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()),
        Some(d) if d.as_nanos() as i64 == mtime)
}

/// `definition` 的核心：返回 `(uri, line0)`，**查不到就是 None**。
///
/// 判据分两步，任何一步不成立都回 `None`（三态里的"查不了"）：
///   ① 光标所在文件+行，能找到**最近的符号**（图谱里存的是符号起始行）；
///   ② 该符号的 `calls` 边里，**目标名字出现在这一行的文本里**——
///      否则就说明这一行调的不是它（同一个函数调十样东西，随便挑一条等于瞎指）。
pub fn definition_at(store: &Store, root: &Path, uri: &str, line0: u32) -> Option<(String, u32)> {
    let file = rel_of(root, uri)?;
    if !up_to_date(store, root, &file) {
        if std::env::var("JINGWEI_LSP_DEBUG").is_ok() {
            eprintln!("lsp-debug: {file} 索引与磁盘不一致（改过没重索引）→ 回 null，不许指旧位置");
        }
        return None;
    }
    let (sym_id, _name, _ln) = symbol_near(store, &file, line0 + 1)?;
    let text = std::fs::read_to_string(root.join(file.replace('/', std::path::MAIN_SEPARATOR_STR))).ok()?;
    let this_line = text.lines().nth(line0 as usize)?;
    let cands = call_targets(store, &sym_id);
    if std::env::var("JINGWEI_LSP_DEBUG").is_ok() {
        eprintln!("lsp-debug: file={file} line={line0} sym={sym_id} 该行={:?}", this_line.trim());
        for (n, f, l) in &cands {
            eprintln!("  cand {n} → {f}:{l} 命中={}", mentions(this_line, n));
        }
    }
    for (name, tfile, tline) in cands {
        if name.is_empty() || !mentions(this_line, &name) {
            continue;
        }
        let abs = root.join(tfile.replace('/', std::path::MAIN_SEPARATOR_STR));
        return Some((path_to_uri(&abs), tline.saturating_sub(1)));
    }
    None
}

/// 这一行有没有**以标识符形式**提到 `name`（`cmd` 不该命中 `cmd_guide`）。
fn mentions(line: &str, name: &str) -> bool {
    let b = line.as_bytes();
    let n = name.as_bytes();
    if n.is_empty() || b.len() < n.len() {
        return false;
    }
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i + n.len() <= b.len() {
        if &b[i..i + n.len()] == n {
            let before_ok = i == 0 || !is_ident(b[i - 1]);
            let after_ok = i + n.len() == b.len() || !is_ident(b[i + n.len()]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}


/// **字节列 → UTF-16 code unit 列**（债表 0.26）。
///
/// LSP 的 `Position.character` 单位是 UTF-16 code unit，而我们的列号是**字节列**（tree-sitter 的
/// `Point.column`）。含中文/emoji 的行两者不同：`"中文"` 是 6 字节、2 个 UTF-16 单元。
/// 不换算就会把编辑落到错位置——改用户代码，宁可多一层转换。
fn utf16_col(line: &str, byte_col: usize) -> usize {
    let end = byte_col.min(line.len());
    // 列落在字符中间说明上游算错了：退回行首，别切出半个字符。
    let Some(prefix) = line.get(..end) else {
        return 0;
    };
    prefix.encode_utf16().count()
}
/// 处理一条消息：请求 → 回消息；通知 → `None`。
pub fn handle(store: Option<&Store>, root: &Path, msg: &Value) -> Option<Value> {
    let method = msg.get("method")?.as_str()?;
    let id = msg.get("id").cloned();
    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id?,
            "result": {
                "capabilities": { "definitionProvider": true, "referencesProvider": true, "hoverProvider": true, "renameProvider": true },
                "serverInfo": { "name": "jingwei", "version": env!("CARGO_PKG_VERSION") }
            }
        })),
        "shutdown" => Some(json!({ "jsonrpc": "2.0", "id": id?, "result": Value::Null })),
        // **重命名**（债表 0.25）：把 `plan_rename` 的结果变成 WorkspaceEdit。
        // 判不准的位置（一行多处同名等）→ 返回空 changes + 错误消息，**不发编辑**；
        // 编辑器的规矩是"要么全改要么不改"，半截重命名比不改更糟。
        "textDocument/rename" => {
            let id = id?;
            let params = msg.get("params")?;
            let uri = params.get("textDocument")?.get("uri")?.as_str()?;
            let line = params.get("position")?.get("line")?.as_u64()? as u32;
            let new_name = params.get("newName")?.as_str()?;
            let Some(store) = store else {
                return Some(json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }));
            };
            let Some(file) = rel_of(root, uri) else {
                return Some(json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }));
            };
            let Some((sym_id, _name, _ln)) = symbol_near(store, &file, line + 1) else {
                return Some(json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }));
            };
            match store.plan_rename(root, &sym_id, new_name) {
                Ok(plan) if plan.ambiguous.is_empty() => {
                    let mut changes = serde_json::Map::new();
                    // 同一个文件只读一次，用来把字节列换成 UTF-16 列。
                    let mut line_cache: std::collections::BTreeMap<String, Vec<String>> =
                        std::collections::BTreeMap::new();
                    for e in &plan.edits {
                        let abs = root.join(e.file.replace('/', std::path::MAIN_SEPARATOR_STR));
                        let key = path_to_uri(&abs);
                        let lines = line_cache.entry(e.file.clone()).or_insert_with(|| {
                            std::fs::read_to_string(root.join(&e.file))
                                .map(|t| t.lines().map(|l| l.to_string()).collect())
                                .unwrap_or_default()
                        });
                        let line_text = lines
                            .get((e.line.saturating_sub(1)) as usize)
                            .cloned()
                            .unwrap_or_default();
                        let byte_col = if e.byte > 0 { e.byte as usize } else { e.col.saturating_sub(1) as usize };
                        // 起点用 UTF-16；终点 = 起点 + 旧名字在 UTF-16 下的长度（全是 ASCII 时相同）。
                        let start = utf16_col(&line_text, byte_col) as u64;
                        let end = start + e.old.encode_utf16().count() as u64;
                        let arr = changes.entry(key).or_insert_with(|| json!([]));
                        if let Some(a) = arr.as_array_mut() {
                            a.push(json!({
                                "range": { "start": { "line": e.line - 1, "character": start },
                                            "end": { "line": e.line - 1, "character": end } },
                                "newText": e.new
                            }));
                        }
                    }
                    Some(json!({ "jsonrpc": "2.0", "id": id, "result": { "changes": changes } }))
                }
                Ok(_) => Some(json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": Value::Null,
                    "error": { "code": -32002, "message": "有判不准的位置，未生成编辑（避免半截重命名）" }
                })),
                Err(e) => Some(json!({
                    "jsonrpc": "2.0", "id": id, "result": Value::Null,
                    "error": { "code": -32002, "message": format!("规划失败：{e}") }
                })),
            }
        }
        "textDocument/definition" => {
            let id = id?;
            let params = msg.get("params")?;
            let uri = params.get("textDocument")?.get("uri")?.as_str()?;
            let line = params.get("position")?.get("line")?.as_u64()? as u32;
            let loc = store.and_then(|s| definition_at(s, root, uri, line));
            let result = match loc {
                // **找不到就 null**——这是三态里的"我们查不了"，不是"没有定义"
                None => Value::Null,
                Some((uri, line)) => json!({
                    "uri": uri,
                    "range": { "start": { "line": line, "character": 0 },
                               "end":   { "line": line, "character": 0 } }
                }),
            };
            Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
        }
        "textDocument/hover" => {
            let id = id?;
            let params = msg.get("params")?;
            let uri = params.get("textDocument")?.get("uri")?.as_str()?;
            let line = params.get("position")?.get("line")?.as_u64()? as u32;
            let md = store.and_then(|s| hover_at(s, root, uri, line));
            let result = match md {
                None => Value::Null,
                Some(v) => json!({ "contents": { "kind": "markdown", "value": v } }),
            };
            Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
        }
        "textDocument/references" => {
            let id = id?;
            let params = msg.get("params")?;
            let uri = params.get("textDocument")?.get("uri")?.as_str()?;
            let line = params.get("position")?.get("line")?.as_u64()? as u32;
            let include = params
                .get("context")
                .and_then(|c| c.get("includeDeclaration"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let locs = store
                .map(|s| references_at(s, root, uri, line, include))
                .unwrap_or_default();
            let result: Vec<Value> = locs
                .into_iter()
                .map(|(file, line)| {
                    json!({
                        "uri": path_to_uri(&root.join(&file)),
                        "range": { "start": { "line": line.saturating_sub(1), "character": 0 },
                                   "end":   { "line": line.saturating_sub(1), "character": 0 } }
                    })
                })
                .collect();
            Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
        }

        // 其它请求：如实说不支持，不装死（编辑器会显示成"没有这个能力"）
        _ => id.map(|id| json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("jingwei 还不支持 {method}") }
        })),
    }
}

/// **诊断推送**（2026-10-19）：把这个文件的已知问题翻成 LSP 的 `Diagnostic`。
///
/// 三件事，都**有出处**：
/// 1. 索引比文件旧 → 警告（位置不可信）；
/// 2. 这个文件上次没解析完 → 错误（`files.incomplete = 1`）；
/// 3. 上次 doctor 在**这个文件**上报过的问题（图谱里 `kind='diagnostic'` 的节点）。
///
/// 刻意**不**在这里跑构建/规则：那要几秒钟，编辑器会以为服务端卡死。
/// 诊断推送只反映"库里已经有的结论"，跑新结论仍然由 `jingwei doctor` 干。
pub fn diagnostics_for(store: &Store, root: &Path, rel: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let diag = |line: u32, severity: u8, msg: String| {
        json!({
            "range": { "start": { "line": line.saturating_sub(1), "character": 0 },
                       "end":   { "line": line.saturating_sub(1), "character": 0 } },
            "severity": severity, "source": "jingwei", "message": msg
        })
    };

    // 1) 库旧了：这句话必须出现，否则用户会信旧行号
    if !up_to_date(store, root, rel) {
        out.push(diag(1, 2, "索引比这个文件旧：位置与引用数可能是旧的，先跑 `jingwei index`".to_string()));
    }

    // 2) 上次没解析完
    let incomplete: i64 = store
        .conn
        .query_row("SELECT incomplete FROM files WHERE path=?1", rusqlite::params![rel], |r| r.get(0))
        .unwrap_or(0);
    if incomplete == 1 {
        out.push(diag(1, 1, "这个文件上次没解析完（语法坏了？）：符号与关系可能不全".to_string()));
    }

    // 3) doctor 报过的问题（只取这个文件的）
    if let Ok(mut st) = store.conn.prepare(
        "SELECT name, line FROM nodes WHERE kind='diagnostic' AND file_path=?1 \
         ORDER BY coalesce(line, 0) LIMIT 50",
    ) {
        if let Ok(mut rows) = st.query(rusqlite::params![rel]) {
            while let Ok(Some(r)) = rows.next() {
                let msg: String = r.get(0).unwrap_or_default();
                let line: i64 = r.get(1).unwrap_or(1);
                out.push(diag(line as u32, 1, msg));
            }
        }
    }
    out
}

/// **悬停**（2026-10-19）：这个符号是什么、定义在哪、**被几处调用**。
///
/// 为什么值得做：编辑器里最常问的一句就是"这是啥、谁在用"。
/// 数据全在图谱里（节点 + `edges` + `edge_sites`），不需要重新解析。
///
/// 三态：符号找不到 → `None`；**库比文件旧 → 明写"位置不可信"**，不装作准。
pub fn hover_at(store: &Store, root: &Path, uri: &str, line0: u32) -> Option<String> {
    let rel = rel_of(root, uri)?;
    let (id, name, line) = symbol_near(store, &rel, line0 + 1)?;

    // 类别（`value` / `type` / `local`…）——parse 那边写在 `data.sym` 里
    let class: String = store
        .conn
        .query_row(
            "SELECT coalesce(json_extract(data, '$.sym'), '') FROM nodes WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap_or_default();
    let kind_zh = match class.as_str() {
        "value" => "值（函数/常量）",
        "type" => "类型",
        "local" => "局部绑定（闭包等）",
        _ => "符号",
    };

    // 被调用多少处：边上多少条 + 有没有精确调用点
    let callers: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM edges WHERE dst=?1 AND kind='calls' AND expires_at IS NULL",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let sites: i64 = store
        .conn
        .query_row("SELECT count(*) FROM edge_sites WHERE dst=?1", rusqlite::params![id], |r| r.get(0))
        .unwrap_or(0);

    let mut md = format!("**{name}** — {kind_zh}\n\n定义：`{}:{}`", rel, line);
    if callers > 0 {
        md.push_str(&format!("\n\n被 **{callers}** 个符号调用"));
        if sites > 0 && sites != callers {
            md.push_str(&format!("（{sites} 处调用点）"));
        }
    } else {
        md.push_str("\n\n图谱里没有调用者（`空` ≠ `没人用`：也可能是库旧了）");
    }
    if !up_to_date(store, root, &rel) {
        md.push_str("\n\n> ⚠️ **索引比文件旧**：位置与调用数可能是旧的，先跑 `jingwei index`");
    }
    Some(md)
}

/// 跑 stdio 循环直到 EOF / `exit`。
pub fn serve<R: BufRead, W: Write>(root: &Path, mut r: R, mut w: W) -> std::io::Result<()> {
    // 库可能在也可能不在：不在就"每个 definition 都回 null"，**不许崩**
    let store = Store::open_readonly(&crate::graph_db_path(root)).ok();
    while let Some(msg) = read_message(&mut r)? {
        if msg.get("method").and_then(|m| m.as_str()) == Some("exit") {
            break;
        }
        // **诊断推送**（2026-10-19）：编辑器打开/保存时，把这个文件的已知问题推过去。
        // 用通知（没有 id）——LSP 里 publishDiagnostics 规定就是通知。
        let method_now = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        if matches!(method_now, "textDocument/didOpen" | "textDocument/didSave") {
            if let (Some(store), Some(uri)) = (
                store.as_ref(),
                msg.get("params")
                    .and_then(|p| p.get("textDocument"))
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str()),
            ) {
                if let Some(rel) = rel_of(root, uri) {
                    let diags = diagnostics_for(store, root, &rel);
                    let note = json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/publishDiagnostics",
                        "params": { "uri": uri, "diagnostics": diags }
                    });
                    write_message(&mut w, &note)?;
                }
            }
        }
        if let Some(out) = handle(store.as_ref(), root, &msg) {
            write_message(&mut w, &out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// **分帧要能原样读回来**：写→读一次往返，长度与内容都不能差。
    #[test]
    fn 消息分帧能往返() {
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();
        let mut cur = Cursor::new(buf);
        let back = read_message(&mut cur).unwrap().unwrap();
        assert_eq!(back, msg);
    }

    /// **没有 Content-Length 就断开**（不猜、不阻塞）——否则编辑器会挂在那里。
    #[test]
    fn 没有长度头就断开() {
        let mut cur = Cursor::new(b"Content-Type: x\r\n\r\n{}".to_vec());
        assert!(read_message(&mut cur).unwrap().is_none());
    }

    /// URI ↔ 路径：Windows 盘符、空格编码都要对得上。
    #[test]
    fn uri与路径互相转得回来() {
        let p = PathBuf::from("D:/a b/c.rs");
        let uri = path_to_uri(&p);
        assert!(uri.starts_with("file:///D:/a%20b/c.rs"), "{uri}");
        let back = uri_to_path(&uri).unwrap();
        assert_eq!(back.to_string_lossy().replace('\\', "/"), "D:/a b/c.rs");
    }

    /// **名字要按标识符匹配**：`cmd` 不该命中 `cmd_guide`，否则会跳到隔壁的函数。
    #[test]
    fn 名字按标识符匹配() {
        assert!(mentions("let x = cmd_guide(&root, json)", "cmd_guide"));
        assert!(!mentions("let x = cmd_guide(&root, json)", "cmd"));
        assert!(!mentions("cmd_guide(&root)", "guide_x"));
    }

    /// **查不到必须是 null**：没有库、没有这个文件、没有这个符号——三种都不许瞎指。
    #[test]
    fn 查不到定义就回null() {
        let root = Path::new("D:/none");
        let msg = json!({
            "jsonrpc": "2.0", "id": 7, "method": "textDocument/definition",
            "params": { "textDocument": { "uri": "file:///D:/none/a.rs" },
                        "position": { "line": 3, "character": 1 } }
        });
        let out = handle(None, root, &msg).unwrap();
        assert_eq!(out["result"], Value::Null, "没有库时不许编一个位置出来");
    }

    /// **没有库时 hover 回 null**（不是编一段话说"这是函数"）。
    #[test]
    fn 没有库时hover回null() {
        let msg = json!({
            "jsonrpc": "2.0", "id": 9, "method": "textDocument/hover",
            "params": { "textDocument": { "uri": "file:///D:/none/a.rs" },
                        "position": { "line": 0, "character": 0 } }
        });
        let out = handle(None, Path::new("D:/none"), &msg).unwrap();
        assert_eq!(out["result"], Value::Null, "库都没有，不许编");
    }

    /// 不认识的请求：回 -32601，不是沉默。
    #[test]
    fn 不支持的请求要说出来() {
        // hover 已经有实现（2026-10-19），"不支持"的样例改用补全
        let msg = json!({ "jsonrpc": "2.0", "id": 3, "method": "textDocument/completion" });
        let out = handle(None, Path::new("D:/none"), &msg).unwrap();
        assert_eq!(out["error"]["code"], -32601);
    }
}
